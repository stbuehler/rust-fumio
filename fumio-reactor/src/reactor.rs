//! The reactor implementation and various low-level tools to use it.

mod evented;
mod executor;
mod lazy_handle;
mod registration;
mod task;
mod waker;

pub use self::evented::PollEvented;
pub use self::executor::current;
pub use self::lazy_handle::LazyHandle;
pub use self::registration::Registration;
use self::task::{ReactorTask, Tasks};

use futures_executor::Enter;
use std::io;
use std::sync::{Arc, Weak};
use std::time::Duration;

#[derive(Debug)]
struct Inner {
	poll: mio::Poll,
	waker: std::task::Waker,
	tasks: Tasks,
}

/// A reactor to drive asynchronous IO in context of async/await futures.
#[derive(Debug)]
pub struct Reactor {
	handlep: HandlePriv,
	events: mio::Events,
	wake_target: mio::Registration,
	reactor_waker: waker::ReactorWaker,
}

impl Reactor {
	/// Create a new reactor
	pub fn new() -> io::Result<Self> {
		let poll = mio::Poll::new()?;
		let (wake_target, reactor_waker) = waker::ReactorWaker::new();
		poll.register(&wake_target, mio::Token(0), mio::Ready::readable(), mio::PollOpt::edge())?;

		Ok(Self {
			handlep: HandlePriv {
				inner: Arc::new(Inner {
					poll,
					waker: reactor_waker.waker(),
					tasks: Tasks::new(),
				}),
			},
			events: mio::Events::with_capacity(1024),
			wake_target,
			reactor_waker,
		})
	}

	/// A waker to interrupt the eventloop.
	///
	/// When "awoken" when the reactor isn't polled at the moment the next poll won't block.  When
	/// "awoken" during polling the poll will be interrupted.
	pub fn waker(&self) -> std::task::Waker {
		self.handlep.waker()
	}

	/// Returns a handle to the reactor
	///
	/// The handle is used to register new IO events (i.e. sockets to be polled).
	pub fn handle(&self) -> Handle {
		self.handlep.downgrade()
	}

	/// Poll for event and wait up to `timeout` for at least one event.
	///
	/// Waits "forever" if `timeout` is None, and doesn't block at all if `timeout` is Some(0).
	///
	/// See [`waker`](#method.waker) for another way to interrupt poll.
	pub fn poll(&mut self, mut timeout: Option<Duration>) -> io::Result<()> {
		let (pending, _poll) = self.reactor_waker.start_poll();
		if pending {
			timeout = Some(Duration::new(0, 0));
		}

		self.handlep.inner.poll.poll(&mut self.events, timeout)?;

		for event in &self.events {
			if event.token().0 == 0 { continue; }
			let task = ReactorTask::from_token(event.token());
			task.update_ready(event.readiness());
		}

		self.handlep.inner.tasks.cleanup_tasks();

		Ok(())
	}
}

impl fumio_utils::park::Park for Reactor {
	fn waker(&self) -> std::task::Waker {
		self.handlep.waker()
	}

	fn park(&mut self, _enter: &mut futures_executor::Enter, timeout: Option<Duration>) {
		self.poll(timeout).unwrap();
	}
}

/// A (shared) handle to the reactor.
///
/// The handle is used to register new IO events (i.e. sockets to be polled).
#[derive(Clone, Debug)]
pub struct Handle {
	inner: Weak<Inner>,
}

impl Handle {
	/// A waker to interrupt the eventloop.
	///
	/// Also see [`Reactor::waker`](struct.Reactor.html#method.waker).
	pub fn waker(&self) -> std::task::Waker {
		match self.upgrade() {
			Some(handlep) => handlep.waker(),
			None => futures_util::task::noop_waker(),
		}
	}

	/// Enter a reactor handle.
	///
	/// Clears the current handle when `f` returns.
	///
	/// [`current`](fn.current.html) and
	/// [`LazyHandle`](../reactor/struct.LazyHandle.html) need this to work.
	///
	/// A runtime (combining reactor, pool, ...) should enter a reactor handle (in
	/// each thread it runs tasks from the pool) so all tasks have access to the
	/// reactor.
	///
	/// # Panics
	///
	/// Panics if a handle is already entered.
	pub fn enter<F, T>(self, enter: &mut Enter, f: F) -> T
	where
		F: FnOnce(&mut Enter) -> T
	{
		self::executor::enter(self, enter, f)
	}

	pub(crate) fn upgrade(&self) -> Option<HandlePriv> {
		let inner = self.inner.upgrade()?;
		Some(HandlePriv { inner })
	}

	pub(crate) fn expect_upgrade(&self) -> io::Result<HandlePriv> {
		self.upgrade().ok_or_else(|| {
			io::Error::new(io::ErrorKind::Other, "reactor not running anymore")
		})
	}
}

#[derive(Debug, Clone)]
pub(crate) struct HandlePriv {
	inner: Arc<Inner>,
}

impl HandlePriv {
	fn downgrade(&self) -> Handle {
		let inner = Arc::downgrade(&self.inner);
		Handle { inner }
	}

	fn register<E>(&self, io: &E, task: ReactorTask, interest: mio::Ready, opts: mio::PollOpt) -> io::Result<()>
	where
		E: mio::Evented,
	{
		let token = ReactorTask::as_token(&task);
		self.inner.tasks.add_task(task);
		self.inner.poll.register(io, token, interest, opts)?;
		self.inner.waker.wake_by_ref();
		Ok(())
	}

	fn reregister<E>(&self, io: &E, task: &ReactorTask, interest: mio::Ready, opts: mio::PollOpt) -> io::Result<()>
	where
		E: mio::Evented,
	{
		let token = ReactorTask::as_token(task);
		self.inner.poll.reregister(io, token, interest, opts)?;
		Ok(())
	}

	fn deregister<E>(&self, io: &E, task: ReactorTask) -> io::Result<()>
	where
		E: mio::Evented,
	{
		self.inner.poll.deregister(io)?;
		self.inner.tasks.deregister_task(task);
		self.inner.waker.wake_by_ref();
		Ok(())
	}

	fn waker(&self) -> std::task::Waker {
		self.inner.waker.clone()
	}
}
