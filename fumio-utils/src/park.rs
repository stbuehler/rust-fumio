//! `Park` trait

use futures_executor::Enter;
use std::task::Waker;
use std::time::Duration;

#[cfg(feature = "park-thread")]
mod park_thread;
#[cfg(feature = "park-thread")]
pub use self::park_thread::ParkThread;

/// A trait to allow combining (nesting) of runtime components (IO reactor, timers, pool of
/// futures)
///
/// The most inner component is the one that blocks the thread until there is more work, this is
/// usually the IO reactor or something based on `std::thread::park{_timeout}`.
pub trait Park {
	/// Return a `Waker` that is used to interrupt `park`
	///
	/// Specific semantic:
	/// - When leaving `park` the "wakeup" notification is reset
	/// - When called before `park` is called (or before `park` suspends), `park` mustn't suspend
	///   the thread at all
	/// - When called while `park` suspends the thread it must interrupt it, and `park` must return "soon"
	fn waker(&self) -> Waker;

	/// Park thread
	///
	/// When `duration` is `None` blocks "forever" (until interrupted by a `Waker`).
	///
	/// Even when `duration` is `Some(0 seconds)` you want to call `park`: it might need to do
	/// routine work (like fetching pending IO events) even when not actually suspending the
	/// thread.
	fn park(&mut self, enter: &mut Enter, duration: Option<Duration>);
}
