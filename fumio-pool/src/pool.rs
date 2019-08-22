
mod task;

use fumio_utils::park::Park;
use futures_core::future::{Future, FutureObj, LocalFutureObj};
use futures_core::task::{Spawn, LocalSpawn, SpawnError};
use futures_executor::Enter;
use futures_util::pin_mut;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

// Set up and run a basic single-threaded spawner loop, invoking `f` on each
// turn.
fn run_executor<P: Park, T, F: FnMut(&mut Context<'_>) -> Poll<T>>(park: &mut P, enter: &mut Enter, mut f: F) -> T {
	let waker = park.waker();
	let mut cx = Context::from_waker(&waker);

	loop {
		if let Poll::Ready(t) = f(&mut cx) {
			return t;
		}
		park.park(enter, None);
	}
}

/// A single-threaded task pool for polling futures to completion.
///
/// This executor allows you to multiplex any number of tasks onto a single
/// thread. It's appropriate to poll strictly I/O-bound futures that do very
/// little work in between I/O actions.
///
/// To get a handle to the pool that implements
/// [`Spawn`](futures_core::task::Spawn), use the
/// [`spawner()`](LocalPool::spawner) method. Because the executor is
/// single-threaded, it supports a special form of task spawning for non-`Send`
/// futures, via [`spawn_local_obj`](futures_core::task::LocalSpawn::spawn_local_obj).
#[derive(Debug)]
pub struct LocalPool {
	task_list: Rc<task::LocalTaskList>,
}

impl LocalPool {
	/// Create a new, empty pool of tasks.
	pub fn new() -> Self {
		Self {
			task_list: Rc::new(task::LocalTaskList::new()),
		}
	}

	/// Get a clonable handle to the pool as a [`Spawn`].
	pub fn spawner(&self) -> LocalSpawner {
		LocalSpawner {
			task_list: Rc::downgrade(&self.task_list)
		}
	}

	/// Run all tasks in the pool to completion.
	///
	/// The function will block the calling thread until *all* tasks in the pool
	/// completed, including any spawned while running existing tasks.
	pub fn run<P: Park>(&mut self, park: &mut P, enter: &mut Enter) {
		run_executor(park, enter, |cx| self.poll_pool(cx))
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
	pub fn run_until<P: Park, F: Future>(&mut self, park: &mut P, enter: &mut Enter, future: F) -> F::Output {
		pin_mut!(future);

		run_executor(park, enter, |cx| {
			{
				// if our main task is done, so are we
				let result = future.as_mut().poll(cx);
				if let Poll::Ready(output) = result {
					return Poll::Ready(output);
				}
			}

			let _ = self.poll_pool(cx);
			Poll::Pending
		})
	}

	/// Make progress on entire pool, polling each spawend task at most once.
	///
	/// Becomes `Ready` when all tasks are completed.
	pub fn poll_pool(&mut self, cx: &mut Context<'_>) -> Poll<()> {
		self.task_list.poll(cx)
	}

	/// Spawn future on pool
	pub fn spawn(&self, future: LocalFutureObj<'static, ()>) {
		self.task_list.add_task(future);
	}
}

impl Default for LocalPool {
	fn default() -> Self {
		Self::new()
	}
}

impl Spawn for LocalPool {
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

impl LocalSpawn for LocalPool {
	fn spawn_local_obj(
		&mut self,
		future: LocalFutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		self.spawn(future);
		Ok(())
	}

	fn status_local(&self) -> Result<(), SpawnError> {
		Ok(())
	}
}

/// A handle to a [`LocalPool`](LocalPool) that implements [`Spawn`](futures_core::task::Spawn) and
/// [`LocalSpawn`](futures_core::task::LocalSpawn).
#[derive(Clone, Debug)]
pub struct LocalSpawner {
	task_list: Weak<task::LocalTaskList>,
}

impl LocalSpawner {
	/// Enter a spawner.
	///
	/// Clears the current spawner when `f` returns.
	///
	/// [`current_local`](fn.current_local.html) needs this to work.
	///
	/// A runtime (combining reactor, pool, ...) should enter a spawner handle (in
	/// each thread it runs tasks from the pool) so all tasks have access to the
	/// spawner.
	///
	/// # Panics
	///
	/// Panics if a spawner is already entered.
	pub fn enter<F, T>(self, enter: &mut Enter, f: F) -> T
	where
		F: FnOnce(&mut Enter) -> T
	{
		crate::current::enter_local(self, enter, f)
	}
}

impl Spawn for LocalSpawner {
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

impl LocalSpawn for LocalSpawner {
	fn spawn_local_obj(
		&mut self,
		future: LocalFutureObj<'static, ()>,
	) -> Result<(), SpawnError> {
		if let Some(task_list) = self.task_list.upgrade() {
			task_list.add_task(future);
			Ok(())
		} else {
			Err(SpawnError::shutdown())
		}
	}

	fn status_local(&self) -> Result<(), SpawnError> {
		if self.task_list.upgrade().is_some() {
			Ok(())
		} else {
			Err(SpawnError::shutdown())
		}
	}
}
