use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

#[derive(Debug)]
struct Inner {
	state: AtomicUsize,
	set_readiness: mio::SetReadiness,
}

const STATE_POLLING: usize = 0b01;
const STATE_PENDING: usize = 0b10;

impl futures_util::task::ArcWake for Inner {
	fn wake_by_ref(arc_self: &Arc<Self>) {
		let prev = arc_self.state.fetch_or(STATE_PENDING, Ordering::Release);
		if 0 != prev & STATE_PENDING {
			// a previous pending flag wasn't reset yet, nothing to do
			return;
		}
		if 0 == prev & STATE_POLLING {
			// not currently polling, will see pending flag before polling, nothing to do
			return;
		}

		// wakeup poll
		let _ = arc_self.set_readiness.set_readiness(mio::Ready::readable());
	}
}

#[derive(Debug)]
pub(super) struct ReactorWaker {
	inner: Arc<Inner>,
}

impl ReactorWaker {
	pub fn new() -> (mio::Registration, Self) {
		let (reg, set) = mio::Registration::new2();
		let inner = Arc::new(Inner {
			state: AtomicUsize::new(0),
			set_readiness: set,
		});
		(reg, Self { inner })
	}

	pub fn waker(&self) -> std::task::Waker {
		futures_util::task::waker(self.inner.clone())
	}

	pub fn start_poll(&mut self) -> (bool, ReactorWakerPollling<'_>) {
		// optimization
		if 0 != self.inner.state.load(Ordering::Acquire) & STATE_PENDING {
			// musn't block in polling anyway, so we don't set STATE_POLLING
			return (true, ReactorWakerPollling { waker: self });
		}
		let pending = 0 != (self.inner.state.fetch_or(STATE_POLLING, Ordering::Acquire) & STATE_PENDING);
		if pending {
			// we're not blocking, unset STATE_POLLING
			self.inner.state.fetch_and(!STATE_POLLING, Ordering::Relaxed);
		}
		(pending, ReactorWakerPollling { waker: self })
	}
}

pub(super) struct ReactorWakerPollling<'a> {
	waker: &'a mut ReactorWaker,
}

impl Drop for ReactorWakerPollling<'_> {
	fn drop(&mut self) {
		// reset pending/polling flags
		self.waker.inner.state.swap(0, Ordering::Acquire);
	}
}
