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
/// `EngineSession` uses an atomic-commit invariant: `commit_state` is the
/// only mutation point for core fields (`engine`, `compiled`, `source_map`,
/// `file_path`, `last_check`, `module_name`).  A panic inside `check()` or
/// `build_gui_state` leaves those core fields unchanged; other fields are
/// caches that tolerate partial state after a panic.  Recovering via
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
    // Recover from any pre-existing poisoning via into_inner().
    // Safety: EngineSession's atomic-commit invariant — commit_state is the
    // only mutation point for core fields; other fields are caches that
    // tolerate partial state after a panic.  The inner state is therefore
    // consistent even if the mutex was poisoned by an external panic.
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
