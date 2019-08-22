//! Single-threaded pool of (non-`Send`) futures

#![doc(html_root_url = "https://docs.rs/fumio-pool/0.1.0")]
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

mod pool;
pub use pool::{LocalPool, LocalSpawner};

mod current;
pub use current::{current_local};
