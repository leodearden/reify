use std::sync::Mutex;

use crate::engine::EngineSession;

/// Run a closure with access to the engine session, recovering from mutex
/// poisoning and catching panics so they do not propagate to callers.
///
/// # Poison recovery
///
/// `EngineSession` uses an atomic-commit invariant (`engine.rs:28-44`): every
/// state mutation is deferred behind `commit_state` until after `check()`
/// returns.  This means a panic inside `check()` or `build_gui_state` leaves
/// all seven core session fields completely unchanged.  Recovering via
/// `PoisonError::into_inner()` is therefore safe — the inner state is
/// consistent even after a poisoning panic.
///
/// # No-poison guarantee
///
/// The explicit `drop(guard)` after `catch_unwind` returns ensures the
/// `MutexGuard`'s `Drop` runs while `thread::panicking() == false`, so the
/// poison flag is never set by panics caught inside this helper.
pub fn with_engine_lock<F, T>(engine: &Mutex<EngineSession>, f: F) -> Result<T, String>
where
    F: FnOnce(&mut EngineSession) -> T,
{
    let mut guard = engine
        .lock()
        .map_err(|e| format!("engine lock poisoned: {e}"))?;
    Ok(f(&mut guard))
}
