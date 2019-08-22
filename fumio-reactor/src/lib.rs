//! A reactor implementation compatible with `std` futures based on mio.
//!
//! Compared to romio this implementation gives more control over the reactor.
//!
//! The reactor needs to be polled explicitly and can therefor be integrated with other components
//! (pool of futures, timer) to build a single threaded runtime (instead of running each component
//! in a separate thread).
//!
//! While the main reactor struct can only be used by a single thread, the handles to it are
//! thread-safe, and all threads can register for events with it.

#![doc(html_root_url = "https://docs.rs/fumio-reactor/0.1.0")]
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

mod helper;
pub mod net;
pub mod reactor;
