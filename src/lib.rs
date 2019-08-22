//! fumio runtime: combining IO, timers and futures in a single thread

#![doc(html_root_url = "https://docs.rs/fumio/0.1.0")]
#![warn(
	missing_debug_implementations,
	missing_docs,
	nonstandard_style,
	rust_2018_idioms,
	clippy::pedantic,
	clippy::nursery,
	clippy::cargo,
)]
#![allow(
	clippy::module_name_repetitions, // often hidden modules and reexported
	clippy::if_not_else, // `... != 0` is a positive condition
	clippy::multiple_crate_versions, // not useful
)]

pub use fumio_reactor::reactor as reactor;
pub use fumio_reactor::net as net;

pub mod pool {
	//! Single-threaded pool of (non-`Send`) futures
	
	pub use fumio_pool::{
		LocalPool,
		LocalSpawner,
		current_local,
	};
}

pub mod timer {
	//! Time based events

	pub use tokio_timer::{
		Delay,
		DelayQueue,
		Interval,
		Timeout,
	};
}

mod runtime;
pub use self::runtime::{Handle, Runtime};
mod timer_reactor;

use std::future::Future;

/// Runs a future until completion with IO reactor and timer in a local pool
///
/// Available environment:
/// - [`fumio::reactor::current()`](reactor/fn.current.html), also automatically used by
///   [`fumio::reactor::LazyHandle`](reactor/struct.LazyHandle.html)
/// - [`fumio::pool::current_local()`](fumio/pool/fn.current_local.html)
/// - [`tokio_timer::timer::TimerHandle::current()`](https://docs.rs/tokio-timer/0.3.0-alpha.2/tokio_timer/timer/struct.Handle.html#method.current)
pub fn run<F, T>(future: F) -> T
where
	F: Future<Output = T>,
{
	let mut runtime = Runtime::new().unwrap();
	runtime.run_until(future)
}
