use crate::reactor;
use tokio_timer::Timer;
use fumio_utils::park::Park;
use futures_executor::Enter;
use std::io;
use std::ptr::NonNull;
use std::task::Waker;
use std::time::Duration;

#[derive(Debug)]
struct ParkReactor(reactor::Reactor, Option<NonNull<Enter>>);

#[derive(Debug)]
struct Unpark(Waker);

impl tokio_executor::park::Park for ParkReactor {
	type Unpark = Unpark;
	type Error = futures_core::Never;

	fn unpark(&self) -> Self::Unpark {
		Unpark(self.0.waker())
	}

	fn park(&mut self) -> Result<(), Self::Error> {
		let enter = unsafe { self.1.as_mut().expect("not entered").as_mut() };
		self.0.park(enter, None);
		Ok(())
	}

	fn park_timeout(&mut self, timeout: Duration) -> Result<(), Self::Error> {
		let enter = unsafe { self.1.as_mut().expect("not entered").as_mut() };
		self.0.park(enter, Some(timeout));
		Ok(())
	}
}

impl tokio_executor::park::Unpark for Unpark {
	fn unpark(&self) {
		self.0.wake_by_ref()
	}
}

#[derive(Debug)]
pub(crate) struct TimerReactor {
	timer: Timer<ParkReactor>,
}

impl TimerReactor {
	pub(crate) fn new() -> io::Result<Self> {
		let reactor = ParkReactor(reactor::Reactor::new()?, None);
		Ok(Self {
			timer: Timer::new(reactor),
		})
	}

	pub(crate) fn timer_handle(&self) -> tokio_timer::timer::Handle {
		self.timer.handle()
	}

	pub(crate) fn reactor_handle(&self) -> reactor::Handle {
		self.timer.get_park().0.handle()
	}
}

impl Park for TimerReactor {
	fn waker(&self) -> std::task::Waker {
		self.timer.get_park().0.waker()
	}

	fn park(&mut self, enter: &mut Enter, duration: Option<Duration>) {
		self.timer.get_park_mut().1 = Some(NonNull::from(enter));
		let r = self.timer.turn(duration);
		self.timer.get_park_mut().1 = None;
		r.unwrap();
	}
}
