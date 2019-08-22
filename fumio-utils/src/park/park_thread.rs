use futures_executor::Enter;
use futures_util::task::{ArcWake, waker};
use std::sync::Arc;
use std::task::Waker;
use std::thread::{self, Thread};
use std::time::Duration;

/// `ParkThread` implements `Park` by suspending the local thread until interrupted.
///
/// Only available with the `park-thread` feature for `fumio-utils` (enabled by default).
#[derive(Debug)]
pub struct ParkThread(());

impl ParkThread {
	/// Create new `ParkThread` instance.
	#[allow(clippy::missing_const_for_fn)] // perhaps one day the impl needs non-const
	pub fn new() -> Self {
		Self(())
	}
}

impl Default for ParkThread {
	fn default() -> Self {
		Self::new()
	}
}

impl crate::park::Park for ParkThread {
	fn waker(&self) -> Waker {
		waker(CURRENT_THREAD_NOTIFY.with(Arc::clone))
	}

	fn park(&mut self, _enter: &mut Enter, duration: Option<Duration>) {
		if let Some(duration) = duration {
			if duration == Duration::new(0, 0) {
				return; // don't park at all
			}
			thread::park_timeout(duration);
		} else {
			thread::park();
		}
	}
}

struct ThreadNotify {
	thread: Thread,
}

thread_local! {
	// allocate only once per thread
	static CURRENT_THREAD_NOTIFY: Arc<ThreadNotify> = Arc::new(ThreadNotify {
		thread: thread::current(),
	});
}

impl ArcWake for ThreadNotify {
	fn wake_by_ref(arc_self: &Arc<Self>) {
		arc_self.thread.unpark();
	}
}
