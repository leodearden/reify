use std::any::Any;
use std::sync::Mutex;

use crate::engine::EngineSession;

/// Extract a human-readable string from a panic payload.
///
/// Rust panic payloads are `Box<dyn Any + Send>`.  The two common cases are:
/// - `&'static str` — from `panic!("literal")`
/// - `String` — from `panic!("{}", value)`
///
/// Falls back to `"<non-string payload>"` for anything else.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return s.to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string payload>".to_string()
}

/// Run a closure with access to the engine session, recovering from mutex
/// poisoning and catching panics so they do not propagate to callers.
///
/// # Poison recovery
///
/// `EngineSession`'s six core fields (`engine`, `compiled`, `source_map`,
/// `file_path`, `last_check`, `module_name`) live inside a private `CoreState`
/// struct whose fields are strictly private — direct field assignment outside
/// `CoreState`'s impl fails to compile.  The only commit points that touch the
/// five invariant-bearing fields (`compiled`, `source_map`, `module_name`,
/// `last_check`, `file_path`) are:
/// - `commit_state` — five-field atomic commit after a successful compile cycle
///   (`file_path` is updated when `Some` is passed; `None` preserves the existing value)
/// - `commit_check` — single-field commit for `last_check` (used by `set_parameter`)
///
/// `engine_mut()` does not touch those fields; the `#[cfg(test)]` mutators are
/// intentional invariant-breakers absent from production builds — the
/// poison-recovery property holds in production.
///
/// A panic inside `check()` or `build_gui_state` therefore leaves core fields
/// at a consistent committed state; other fields are caches that tolerate
/// partial state after a panic.  Recovering via `PoisonError::into_inner()` is
/// therefore safe — the inner state is consistent even after a poisoning panic.
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
    // Recover from any pre-existing poisoning via into_inner().
    // Safety: CoreState's fields are strictly private; the only commit points for
    // the five invariant-bearing fields are commit_state (five-field atomic commit,
    // file_path included via Option) and commit_check (single-field last_check),
    // each atomic with respect to the fields it owns.
    // engine_mut() does not touch those fields; the #[cfg(test)] mutators are
    // intentional invariant-breakers absent from production builds — the
    // poison-recovery property holds in production.
    // Other fields on EngineSession are caches that tolerate partial state.
    // The inner state is therefore consistent even if the mutex was poisoned.
    let mut guard = engine.lock().unwrap_or_else(|p| p.into_inner());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut guard)));
    // Explicit drop BEFORE the match: releases the lock as soon as possible
    // and makes the no-poison guarantee load-bearing and obvious. After
    // catch_unwind returns, thread::panicking() is false, so MutexGuard::drop
    // does NOT set the poison flag regardless of whether f panicked.
    drop(guard);
    match result {
        Ok(v) => Ok(v),
        Err(payload) => Err(format!("panic in engine: {}", panic_message(&*payload))),
    }
}
