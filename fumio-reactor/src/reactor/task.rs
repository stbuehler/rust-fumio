use super::Handle;
use futures_util::task::AtomicWaker;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};

fumio_utils::mpsc! {
	mod mpsc_task_list {
		link TaskListLink;
		head TaskListHead;
		member next of InnerTask;
	}
}

fumio_utils::local_dl_list! {
	mod loc_dl_list {
		link LocalTaskListLink;
		head LocalTaskListHead;
		member local_link of InnerTask;
	}
}

#[derive(Debug)]
pub(super) struct Tasks {
	// tasks to process (new, deregister)
	list: TaskListHead,
	// make sure to keep Tasks alive until they are deregistered, so we can reinterpret tokens as
	// pointers to tasks.
	local_list: LocalTaskListHead,
}

unsafe impl Send for Tasks {}
unsafe impl Sync for Tasks {}

impl Tasks {
	pub(super) fn new() -> Self {
		Self {
			list: TaskListHead::new(),
			local_list: LocalTaskListHead::new(),
		}
	}

	pub(super) fn add_task(&self, task: ReactorTask) {
		let prev = task.inner.state.compare_and_swap(0, STATE_QUEUED, Ordering::Release);
		if 0 != prev {
			// someone else queued it... wtf. anyway, no need to continue
			return;
		}

		self.list.push(task.inner);
	}

	pub(super) fn deregister_task(&self, task: ReactorTask) {
		let prev = task.inner.state.swap(STATE_QUEUED | STATE_DEREGISTERED, Ordering::Release);
		if 0 != prev {
			// either already deregistered or already queued
			// not queuing again
			return;
		}

		self.list.push(task.inner);
	}

	// call after an event loop iteration is done
	pub(super) fn cleanup_tasks(&self) {
		for task_inner in unsafe { self.list.start_pop() } {
			let prev = task_inner.state.fetch_and(!STATE_QUEUED, Ordering::Acquire);
			assert!(0 != STATE_QUEUED & prev, "invalid internal state");
			if 0 != STATE_DEREGISTERED & prev {
				self.local_remove(task_inner)
			} else {
				self.local_add(task_inner)
			}
		}
	}

	fn local_add(&self, task_inner: Arc<InnerTask>) {
		if task_inner.local_link.is_unlinked() {
			// move reference to local list
			let task_inner = ManuallyDrop::new(task_inner);
			unsafe { self.local_list.append(&task_inner); }
		}
	}

	#[allow(clippy::needless_pass_by_value)]
	fn local_remove(&self, task_inner: Arc<InnerTask>) {
		if !task_inner.local_link.is_unlinked() {
			unsafe { task_inner.local_link.unlink(); }
			unsafe { Arc::from_raw(&*task_inner); } // drop refcount hold by list
		}
	}
}

impl Drop for Tasks {
	fn drop(&mut self) {
		self.cleanup_tasks();

		while let Some(task_inner) = unsafe { self.local_list.pop_front() } {
			let _task_inner = unsafe { Arc::from_raw(task_inner) };
		}
	}
}

const STATE_DEREGISTERED: u8 = 0b01;
const STATE_QUEUED: u8 = 0b10;

#[derive(Debug)]
struct InnerTask {
	state: AtomicU8,
	next: TaskListLink,
	local_link: LocalTaskListLink,
	reactor: Handle,
	read_mask: usize,
	write_mask: usize,
	read_readiness: AtomicUsize,
	read_waker: AtomicWaker,
	write_readiness: AtomicUsize,
	write_waker: AtomicWaker,
}

#[derive(Debug, Clone)]
pub(super) struct ReactorTask {
	inner: Arc<InnerTask>,
}

impl ReactorTask {
	pub(super) fn new(reactor: Handle, read_mask: mio::Ready, write_mask: mio::Ready) -> Self {
		let inner = Arc::new(InnerTask {
			state: AtomicU8::new(0),
			next: TaskListLink::new(),
			local_link: LocalTaskListLink::new(),
			reactor,
			read_mask: read_mask.as_usize(),
			write_mask: write_mask.as_usize(),
			read_readiness: AtomicUsize::new(0),
			read_waker: AtomicWaker::new(),
			write_readiness: AtomicUsize::new(0),
			write_waker: AtomicWaker::new(),
		});
		Self { inner }
	}

	pub(super) fn reactor(&self) -> &Handle {
		&self.inner.reactor
	}

	fn take_read_ready(&self) -> mio::Ready {
		mio::Ready::from_usize(self.inner.read_readiness.swap(0, Ordering::Relaxed))
	}

	pub(super) fn clear_read_ready(&self) -> io::Result<mio::Ready> {
		Ok(self.take_read_ready())
	}

	pub(super) fn poll_read_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		let ready = self.take_read_ready();
		if !ready.is_empty() {
			return Poll::Ready(Ok(ready));
		}
		self.inner.read_waker.register(context.waker());
		let ready = self.take_read_ready();
		if !ready.is_empty() {
			return Poll::Ready(Ok(ready));
		}
		self.inner.reactor.expect_upgrade()?; // make sure reactor still lives
		Poll::Pending
	}

	fn take_write_ready(&self) -> mio::Ready {
		mio::Ready::from_usize(self.inner.write_readiness.swap(0, Ordering::Relaxed))
	}

	pub(super) fn clear_write_ready(&self) -> io::Result<mio::Ready> {
		Ok(self.take_write_ready())
	}

	pub(super) fn poll_write_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		let ready = self.take_write_ready();
		if !ready.is_empty() {
			return Poll::Ready(Ok(ready));
		}
		self.inner.write_waker.register(context.waker());
		let ready = self.take_write_ready();
		if !ready.is_empty() {
			return Poll::Ready(Ok(ready));
		}
		self.inner.reactor.expect_upgrade()?; // make sure reactor still lives
		Poll::Pending
	}

	// token doesn't own a reference!
	pub(super) fn as_token(&self) -> mio::Token {
		let raw: *const InnerTask = &*self.inner;
		mio::Token(raw as usize)
	}

	// use only in event loop between poll and cleanup_tasks
	pub(super) fn from_token(token: mio::Token) -> Self {
		let raw: *const InnerTask = token.0 as _;
		let inner = unsafe { Arc::from_raw(raw) };
		// increase ref count by 1
		std::mem::forget(inner.clone());
		Self { inner }
	}

	pub(super) fn update_ready(&self, readiness: mio::Ready) {
		let read_bits = self.inner.read_mask & readiness.as_usize();
		if 0 != read_bits {
			self.inner.read_readiness.fetch_or(read_bits, Ordering::Relaxed);
			self.inner.read_waker.wake();
		}
		let write_bits = self.inner.write_mask & readiness.as_usize();
		if 0 != write_bits {
			self.inner.write_readiness.fetch_or(write_bits, Ordering::Relaxed);
			self.inner.write_waker.wake();
		}
	}
}

impl std::cmp::PartialEq for ReactorTask {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.inner, &other.inner)
	}
}
impl std::cmp::Eq for ReactorTask { }
impl std::hash::Hash for ReactorTask {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.as_token().hash(state)
	}
}
