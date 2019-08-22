use super::*;
use std::io;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

// spinlock for ReactorTask
#[derive(Debug)]
struct TaskState {
	// low bit is lock bit
	task: AtomicUsize,
}

impl TaskState {
	const fn new() -> Self {
		Self {
			task: AtomicUsize::new(0),
		}
	}

	fn lock(&self) -> TaskStateLock<'_> {
		let mut state;
		loop {
			state = self.task.fetch_or(1, Ordering::Acquire);
			if 0 == state & 1 {
				break; // no lock before, so we acquired the lock
			}
			core::sync::atomic::spin_loop_hint();
		}
		TaskStateLock {
			state: self,
			task: Self::_task_from_state(state),
		}
	}

	fn _task_from_state(state: usize) -> ManuallyDrop<Option<ReactorTask>> {
		let task = state & !1;
		ManuallyDrop::new(if task != 0 {
			Some(ReactorTask::from_token(mio::Token(task)))
		} else {
			None
		})
	}
}

impl Drop for TaskState {
	fn drop(&mut self) {
		ManuallyDrop::into_inner(Self::_task_from_state(*self.task.get_mut()));
	}
}

struct TaskStateLock<'a> {
	state: &'a TaskState,
	task: ManuallyDrop<Option<ReactorTask>>,
}

impl TaskStateLock<'_> {
	fn as_ref(&self) -> Option<&ReactorTask> {
		self.task.as_ref()
	}

	fn take(&mut self) -> Option<ReactorTask> {
		if self.task.is_some() {
			// clear state
			self.state.task.store(1, Ordering::Relaxed);
			// extract task
			ManuallyDrop::into_inner(std::mem::replace(&mut self.task, ManuallyDrop::new(None)))
		} else {
			None
		}
	}

	fn set(&mut self, task: ReactorTask) {
		let token = task.as_token();
		if self.task.is_some() {
			// drop old task
			ManuallyDrop::into_inner(std::mem::replace(&mut self.task, ManuallyDrop::new(None)));
		}
		self.task = ManuallyDrop::new(Some(task)); // steal refcount
		self.state.task.store(token.0 | 1, Ordering::Relaxed);
	}
}

impl Drop for TaskStateLock<'_> {
	fn drop(&mut self) {
		// clear lock
		self.state.task.fetch_and(!1, Ordering::Release);
	}
}

/// Low-level registration of an event source with the `Reactor`.
///
/// One `mio::Evented` source can only be registered once; this abstraction allows two "parallel"
/// sets of ready events to be polled.  For convenience one is called "read" and the other "write".
/// On construction the set of "read" and "write" bits is given; everything else is ignored.
#[derive(Debug)]
pub struct Registration<E>
where
	E: mio::Evented,
{
	read_mask: mio::Ready,
	write_mask: mio::Ready,
	task: TaskState,
	io: Option<E>, // only becomes None on `into_inner`
}

impl<E> Registration<E>
where
	E: mio::Evented,
{
	/// Create new registration (but don't register it yet).
	pub fn new(io: E, read_mask: mio::Ready, write_mask: mio::Ready) -> Self {
		Self {
			read_mask,
			write_mask,
			task: TaskState::new(),
			io: Some(io),
		}
	}

	/// Return and clear current read events.
	pub fn clear_read_ready(&self) -> io::Result<mio::Ready> {
		let taskl = self.task.lock();
		let task = taskl.as_ref().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Other, "clear_read_ready: not registered")
		})?;
		task.clear_read_ready()
	}

	/// Check for new read events and register context to be woken on new read events if no read
	/// events were pending.
	pub fn poll_read_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		let taskl = self.task.lock();
		let task = taskl.as_ref().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Other, "poll_read_ready: not registered")
		})?;
		task.poll_read_ready(context)
	}

	/// Return and clear current write events.
	pub fn clear_write_ready(&self) -> io::Result<mio::Ready> {
		let taskl = self.task.lock();
		let task = taskl.as_ref().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Other, "clear_write_ready: not registered")
		})?;
		task.clear_write_ready()
	}

	/// Check for new (and clear) write events and register context to be woken on new write events
	/// if no write events were pending.
	pub fn poll_write_ready(&self, context: &mut Context<'_>) -> Poll<io::Result<mio::Ready>> {
		let taskl = self.task.lock();
		let task = taskl.as_ref().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Other, "poll_write_ready: not registered")
		}).unwrap();
		task.poll_write_ready(context)
	}

	/// Register event.
	///
	/// Deregisters automatically if it was registered before.
	pub fn register(&self, handle: &Handle, interest: mio::Ready, opts: mio::PollOpt) -> io::Result<()> {
		let io = self.io.as_ref().expect("missing io");
		self.deregister()?;
		let mut taskl = self.task.lock();
		let reactor = handle.expect_upgrade()?;
		let task = ReactorTask::new(handle.clone(), self.read_mask, self.write_mask);;
		reactor.register(io, task.clone(), interest, opts)?;
		taskl.set(task);
		Ok(())
	}

	/// Only allowed while registered
	pub fn reregister(&self, interest: mio::Ready, opts: mio::PollOpt) -> io::Result<()> {
		let io = self.io.as_ref().expect("missing io");
		let taskl = self.task.lock();
		let task = taskl.as_ref().expect("reregister: not registered");
		let reactor = task.reactor().expect_upgrade()?;
		reactor.reregister(io, task, interest, opts)?;
		Ok(())
	}

	/// Deregister event.
	///
	/// Only fails if mio itself fails.  If it wasn't registered or reactor is gone nothing
	/// happens.
	pub fn deregister(&self) -> io::Result<()> {
		let io = self.io.as_ref().expect("missing io");
		let mut task = self.task.lock();
		if let Some(task) = task.take() {
			if let Some(reactor) = task.reactor().upgrade() {
				reactor.deregister(io, task)?;
			}
		}
		Ok(())
	}

	/// Retrieve reference to the contained IO
	pub fn io_ref(&self) -> &E {
		self.io.as_ref().expect("missing io")
	}

	/// Retrieve mutable reference to the contained IO
	pub fn io_mut(&mut self) -> &mut E {
		self.io.as_mut().expect("missing io")
	}

	/// Handle of registration or unbound `LazyHandle`.
	pub fn handle(&self) -> LazyHandle {
		let taskl = self.task.lock();
		match taskl.as_ref() {
			Some(task) => task.reactor().clone().into(),
			None => LazyHandle::new(),
		}
	}

	/// Extract inner io from Registration (deregisters the io from the reactor).
	pub fn into_inner(mut self) -> E {
		let _ = self.deregister(); // so dropping later doesn't panic
		self.io.take().expect("missing io")
	}
}

impl<E> Drop for Registration<E>
where
	E: mio::Evented,
{
	fn drop(&mut self) {
		let _ = self.deregister();
	}
}
