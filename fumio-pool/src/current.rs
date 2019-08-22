use crate::LocalSpawner;
use fumio_utils::current::Current;
use futures_executor::Enter;

thread_local! {
	static CURRENT: Current<LocalSpawner> = Current::new();
}

pub(crate) fn enter_local<F, T>(spawner: LocalSpawner, enter: &mut Enter, f: F) -> T
where
	F: FnOnce(&mut Enter) -> T
{
	Current::enter(&CURRENT, enter, spawner, f)
}

/// Retrieve the current handle.
pub fn current_local() -> Option<LocalSpawner> {
	#[allow(clippy::redundant_closure_for_method_calls)] // sadly the suggestion doesn't compile
	Current::with(&CURRENT, |h| h.cloned())
}
