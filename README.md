# fumio

`fumio` is a runtime designed for single-threaded applications based on the `std` futures API.

It consists of:
- [`fumio-reactor`](https://crates.io/crates/fumio-reactor): [`mio`](https://crates.io/crates/mio)-based asynchronous IO
- [`fumio-pool`](https://crates.io/crates/fumio-pool): single-threaded pool of futures
- [`tokio-timer`](https://crates.io/crates/tokio-timer): time-related events
