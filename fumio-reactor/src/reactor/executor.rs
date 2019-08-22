use fumio_utils::current::Current;
use crate::reactor::Handle;
use futures_executor::Enter;

thread_local! {
	static CURRENT: Current<Handle> = Current::new();
}

pub(crate) fn enter<F, T>(handle: Handle, enter: &mut Enter, f: F) -> T
where
	F: FnOnce(&mut Enter) -> T
{
	Current::enter(&CURRENT, enter, handle, f)
}

/// Retrieve the current handle.
pub fn current() -> Option<Handle> {
	#[allow(clippy::redundant_closure_for_method_calls)] // sadly the suggestion doesn't compile
	Current::with(&CURRENT, |h| h.cloned())
}
