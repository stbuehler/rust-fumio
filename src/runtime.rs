use crate::timer_reactor::TimerReactor;
use crate::pool::{LocalPool, LocalSpawner};
use futures_core::future::{FutureObj, LocalFutureObj};
use futures_core::task::{Spawn, LocalSpawn, SpawnError};
use futures_executor::Enter;
use std::future::Future;
use std::io;

/// Runtime
///
/// When running/entered it supports the following subsystems:
/// - [`fumio::reactor::current()`](reactor/fn.current.html), also automatically used by
///   [`fumio::reactor::LazyHandle`](reactor/struct.LazyHandle.html)
/// - [`fumio::pool::current_local()`](fumio/pool/fn.current_local.html)
/// - [`tokio_timer::timer::TimerHandle::current()`](https://docs.rs/tokio-timer/0.3.0-alpha.2/tokio_timer/timer/struct.Handle.html#method.current)
#[derive(Debug)]
pub struct Runtime {
	timer_reactor: TimerReactor,
	local_pool: LocalPool,
}

impl Runtime {
	/// Create new runtime
	pub fn new() -> io::Result<Self> {
		Ok(Self {
			timer_reactor: TimerReactor::new()?,
			local_pool: LocalPool::new(),
		})
	}

	/// Handle to the runtime
	pub fn handle(&self) -> Handle {
		Handle {
			reactor_handle: self.timer_reactor.reactor_handle(),
			timer_handle: self.timer_reactor.timer_handle(),
			local_spawner: self.local_pool.spawner(),
		}
	}

	fn enter<F, T>(&mut self, enter: &mut Enter, f: F) -> T
	where
		F: FnOnce(&mut Self, &mut Enter) -> T,
	{
		self.timer_reactor.reactor_handle().enter(enter, move |enter| {
			let timer_handle = self.timer_reactor.timer_handle();
			let _scoped_timer = tokio_timer::timer::set_default(&timer_handle);

			self.local_pool.spawner().enter(enter, move |enter| {
				f(self, enter)
			})
		})
	}

	/// Spawn future on runtime
	pub fn spawn<F>(&self, future: F)
	where
		F: Future<Output=()> + 'static,
	{
		self.local_pool.spawn(Box::pin(future).into())
	}

	/// Spawn future object on runtime
	pub fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) {
		self.local_pool.spawn(future)
	}

	/// Runs all the tasks in the pool until the given future completes.
	///
	/// The given spawner, `spawn`, is used as the default spawner for any
	/// *newly*-spawned tasks. You can route these additional tasks back into
	/// the `LocalPool` by using its spawner handle:
	///
	/// The function will block the calling thread *only* until the future `f`
	/// completes; there may still be incomplete tasks in the pool, which will
	/// be inert after the call completes, but can continue with further use of
	/// one of the pool's run or poll methods. While the function is running,
	/// however, all tasks in the pool will try to make progress.
	pub fn enter_run_until<F, T>(&mut self, enter: &mut Enter, future: F) -> T
	where
		F: Future<Output = T>,
	{
		self.enter(enter, |this, enter| {
			this.local_pool.run_until(&mut this.timer_reactor, enter, future)
		})
	}

	/// Runs all the tasks in the pool until the given future completes.
	///
	/// The given spawner, `spawn`, is used as the default spawner for any
	/// *newly*-spawned tasks. You can route these additional tasks back into
	/// the `LocalPool` by using its spawner handle:
	///
	/// The function will block the calling thread *only* until the future `f`
	/// completes; there may still be incomplete tasks in the pool, which will
	/// be inert after the call completes, but can continue with further use of
	/// one of the pool's run or poll methods. While the function is running,
	/// however, all tasks in the pool will try to make progress.
	pub fn run_until<F, T>(&mut self, future: F) -> T
	where
		F: Future<Output = T>,
	{
		let mut enter = futures_executor::enter().unwrap();
		self.enter_run_until(&mut enter, future)
	}

	/// Run all tasks in the pool to completion.
	///
	/// The function will block the calling thread until *all* tasks in the pool
	/// completed, including any spawned while running existing tasks.
	pub fn enter_run(&mut self, enter: &mut Enter) {
		self.enter(enter, |this, enter| {
			this.local_pool.run(&mut this.timer_reactor, enter)
		})
	}

	/// Run all tasks in the pool to completion.
	///
	/// The function will block the calling thread until *all* tasks in the pool
	/// completed, including any spawned while running existing tasks.
	pub fn run<F, T>(&mut self) {
		let mut enter = futures_executor::enter().unwrap();
		self.enter_run(&mut enter)
	}
}

impl Spawn for Runtime {
	fn spawn_obj(
		&mut self,
		future: FutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		self.spawn_local_obj(future.into())
	}

	fn status(&self) -> Result<(), SpawnError> {
		self.status_local()
	}
}

impl LocalSpawn for Runtime {
	fn spawn_local_obj(
		&mut self,
		future: LocalFutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		self.local_pool.spawn_local_obj(future)
	}

	fn status_local(&self) -> Result<(), SpawnError> {
		self.local_pool.status_local()
	}
}

/// Handle to runtime
///
/// Contains handles for the subsystems.
#[derive(Clone, Debug)]
pub struct Handle {
	reactor_handle: crate::reactor::Handle,
	timer_handle: tokio_timer::timer::Handle,
	local_spawner: LocalSpawner,
}

impl Handle {
	/// Set thread-local "current" handles for reactor, timer and spawner while executing `f`.
	pub fn enter<F, T>(&self, enter: &mut Enter, f: F) -> T
	where
		F: FnOnce(&mut Enter) -> T,
	{
		self.reactor_handle.clone().enter(enter, move |enter| {
			let _scoped_timer = tokio_timer::timer::set_default(&self.timer_handle);

			self.local_spawner.clone().enter(enter, move |enter| {
				f(enter)
			})
		})
	}

	/// Retrieve handle to reactor
	pub fn reactor(&self) -> crate::reactor::Handle {
		self.reactor_handle.clone()
	}

	/// Retrieve handle to timer
	pub fn timer(&self) -> tokio_timer::timer::Handle {
		self.timer_handle.clone()
	}

	/// Retrieve handle to spawner
	pub fn spawner(&self) -> LocalSpawner {
		self.local_spawner.clone()
	}
}

impl Spawn for Handle {
	fn spawn_obj(
		&mut self,
		future: FutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		self.spawn_local_obj(future.into())
	}

	fn status(&self) -> Result<(), SpawnError> {
		self.status_local()
	}
}

impl LocalSpawn for Handle {
	fn spawn_local_obj(
		&mut self,
		future: LocalFutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		self.local_spawner.spawn_local_obj(future)
	}

	fn status_local(&self) -> Result<(), SpawnError> {
		self.local_spawner.status_local()
	}
}
