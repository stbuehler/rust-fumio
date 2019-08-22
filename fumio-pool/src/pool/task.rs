use futures_core::future::{Future, LocalFutureObj};
use futures_util::task::AtomicWaker;
use std::cell::{Cell, UnsafeCell};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::thread::{self, ThreadId};

fumio_utils::local_dl_list! {
	mod loc_pending_list {
		link TaskPendingLink;
		head TaskPendingHead;
		member local_pending_link of Task;
	}
}

fumio_utils::local_dl_list! {
	mod loc_list {
		link TaskLink;
		head TaskHead;
		member local_link of Task;
	}
}

fumio_utils::mpsc! {
	mod mpsc_list {
		link GlobalTaskListLink;
		head GlobalTaskListHead;
		member global_pending_next of Task;
	}
}

#[derive(Debug)]
struct TaskList {
	// local state:

	// list of all alive tasks, owns a refcount on each task
	// all alive tasks need to be on this list
	local_all: TaskHead,
	// list of pending (and alive!) tasks, doesn't own a refcount
	local_pending: TaskPendingHead,
	// head of the single-linked global pending task queue; only the
	// owning thread advances the head, therefore local state.
	//
	// usually points to the stub task: as it can never be null, the head
	// can only be removed from the list if there is another item, and a
	// task can only be added to the local_pending queue once it has been
	// popped.
	//
	// the stub task is repushed as soon as it is popped.
	//
	// this queue keeps a refcount on each task (but not for the stub task).
	global_pending: GlobalTaskListHead, // local state!

	// thread-safe:
	local_thread: ThreadId,
	// waker to notify when a task becomes pending
	waker: AtomicWaker,
}

unsafe impl Send for TaskList {}
unsafe impl Sync for TaskList {}

impl TaskList {
	fn new() -> Self {
		Self {
			local_all: TaskHead::new(),
			local_pending: TaskPendingHead::new(),
			global_pending: GlobalTaskListHead::new(),
			local_thread: thread::current().id(),
			waker: AtomicWaker::new(),
		}
	}

	fn local_notify(&self, task: &Arc<Task>) {
		// local_pending doesn't keep a reference, but only still active tasks
		// are allowed (as they are kept on local_all too)
		if task.alive.get() && task.local_pending_link.is_unlinked() {
			unsafe { self.local_pending.append(task); }
			self.waker.wake();
		}
	}

	fn global_notify(&self, task: &Arc<Task>) {
		// sync on `queued`
		if !task.queued.swap(true, Ordering::Release) {
			self.global_pending.push(task.clone());
			self.waker.wake();
		} // else was still queued when we released the store above
	}

	fn fetch_global_notifies(&self) {
		debug_assert_eq!(thread::current().id(), self.local_thread);

		for task in unsafe { self.global_pending.start_pop() } {
			task.queued.swap(false, Ordering::Acquire); // sync with Release in global_notify
			// move to local queue
			if task.alive.get() && task.local_pending_link.is_unlinked() {
				unsafe { self.local_pending.append(&task); }
			}
		}
	}

	fn poll(&self) -> Poll<()> {
		struct PollList {
			pending: TaskPendingHead,
		}
		impl Drop for PollList {
			fn drop(&mut self) {
				// pop all to readd them on panic
				while let Some(task) = unsafe { self.pending.pop_back() } {
					let task = unsafe { &*task };
					if task.alive.get() && task.local_pending_link.is_unlinked() {
						unsafe { task.task_list().local_pending.prepend(task); }
					}
				}
			}
		}

		let mut poll_list = PollList {
			pending: TaskPendingHead::new(),
		};

		unsafe {
			poll_list.pending.take_from(&self.local_pending);
			while let Some(task) = poll_list.pending.pop_front() {
				/* unsafe */ { &*task }.local_poll();
			}
		}
		if self.local_all.is_empty() {
			Poll::Ready(())
		} else {
			Poll::Pending
		}
	}
}

impl Drop for TaskList {
	fn drop(&mut self) {
		assert!(self.local_all.is_empty());
		assert!(self.local_pending.is_empty());
	}
}

#[derive(Debug)]
pub(super) struct LocalTaskList {
	task_list: Arc<TaskList>,
	_marker: PhantomData<*mut ()>, // don't send
}

impl LocalTaskList {
	pub fn new() -> Self {
		Self {
			task_list: Arc::new(TaskList::new()),
			_marker: PhantomData,
		}
	}

	// poll one round; completes when all tasks completed
	pub fn poll(&self, cx: &mut Context<'_>) -> Poll<()> {
		self.task_list.waker.register(cx.waker());
		self.task_list.fetch_global_notifies();
		self.task_list.poll()
	}

	pub fn add_task(&self, future: LocalFutureObj<'static, ()>) {
		let task = Arc::new(Task::new(self.task_list.clone(), future));
		unsafe { self.task_list.local_all.append(&task); }
		let task = ManuallyDrop::new(task); // now owned by `local_all`
		// trigger initial poll
		self.task_list.local_notify(&task);
	}
}

impl Drop for LocalTaskList {
	fn drop(&mut self) {
		while let Some(task) = unsafe { self.task_list.local_all.pop_front() } {
			// local_clear will drop the refcount from local_all
			unsafe { &*task }.local_clear();
		}
	}
}

#[derive(Debug)]
// unless marked fields are not thread-safe and only for the thread owning the
// corresponding `LocalTaskList`
pub(super) struct Task {
	task_list: Option<Arc<TaskList>>, // thread-safe
	local_link: TaskLink,
	local_pending_link: TaskPendingLink,
	global_pending_next: GlobalTaskListLink, // thread-safe
	queued: AtomicBool, // thread-safe: queued in global_pending
	alive: Cell<bool>,
	future: ManuallyDrop<UnsafeCell<Option<LocalFutureObj<'static, ()>>>>,
}

unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Task {
	fn new(task_list: Arc<TaskList>, future: LocalFutureObj<'static, ()>) -> Self {
		Self {
			task_list: Some(task_list),
			local_link: TaskLink::new(),
			local_pending_link: TaskPendingLink::new(),
			global_pending_next: GlobalTaskListLink::new(),
			queued: AtomicBool::new(false),
			alive: Cell::new(true),
			future: ManuallyDrop::new(UnsafeCell::new(Some(future))),
		}
	}

	fn task_list(&self) -> &TaskList {
		self.task_list.as_ref().expect("not stub task")
	}

	#[allow(clippy::mut_from_ref)] // unsafe anyway
	unsafe fn local_future(&self) -> &mut Option<LocalFutureObj<'static, ()>> {
		debug_assert_eq!(thread::current().id(), self.task_list().local_thread);
		&mut *self.future.get()
	}

	fn local_poll(&self) {
		struct ClearOnPanic<'a> {
			task: Option<&'a Task>,
		}
		impl Drop for ClearOnPanic<'_> {
			fn drop(&mut self) {
				if let Some(t) = self.task.take() {
					if t.alive.get() {
						t.local_clear();
					}
				}
			}
		}

		debug_assert!(self.alive.get());
		let arc_self = ManuallyDrop::new(unsafe { Arc::from_raw(self) }); // no refcount
		let waker = futures_util::task::waker_ref(&arc_self);
		let mut cx = Context::from_waker(&waker);

		let fut = unsafe { self.local_future() }.as_mut().expect("pending futures must be alive");
		let fut = unsafe { Pin::new_unchecked(fut) };

		let mut cop = ClearOnPanic { task: Some(self) };
		if let Poll::Ready(()) = fut.poll(&mut cx) {
			self.local_clear();
		}
		cop.task.take(); // no panic, undo clear on panic
	}

	// consumes one reference (for the one kept by `local_link`)
	fn local_clear(&self) {
		let this = unsafe { Arc::from_raw(self) };
		// mark as queued: won't poll ever again though, no need to queue anymore
		this.queued.store(true, Ordering::Relaxed);
		this.alive.set(false);
		unsafe {
			this.local_pending_link.unlink();
			this.local_link.unlink();
			*this.local_future() = None;
		}
	}
}

impl futures_util::task::ArcWake for Task {
	fn wake_by_ref(arc_self: &Arc<Self>) {
		let id = thread::current().id();
		let task_list = arc_self.task_list();
		if id == task_list.local_thread {
			task_list.local_notify(arc_self);
		} else {
			task_list.global_notify(arc_self);
		}
	}
}

impl Drop for Task {
	fn drop(&mut self) {
		// can't drop future in a thread-safe manner; needs to be dropped manually before
		assert!(!self.alive.get());
		debug_assert!(self.task_list.is_none() || unsafe { self.local_future() }.is_none());
	}
}
