use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, bracket_source};

use crate::engine::EngineSession;
use crate::engine_lock;

fn make_engine() -> Arc<Mutex<EngineSession>> {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load should succeed");
    Arc::new(Mutex::new(session))
}

#[test]
fn with_engine_lock_returns_ok_for_successful_closure() {
    let engine = make_engine();
    let result = engine_lock::with_engine_lock(&engine, |s| s.is_idle());
    assert_eq!(result, Ok(true), "successful closure should return Ok(true)");
}

#[test]
fn with_engine_lock_returns_err_when_closure_panics() {
    let engine = make_engine();
    // Wrap in catch_unwind so the test harness doesn't abort on the panic.
    // The IMPORTANT invariant being tested: with_engine_lock itself should
    // return Err (i.e., catch_unwind should NOT fire — if it does, the helper
    // let the panic escape, which is the regression we're detecting).
    let outer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        engine_lock::with_engine_lock(&engine, |_s: &mut EngineSession| -> bool {
            panic!("boom")
        })
    }));
    // If the helper worked correctly, the inner with_engine_lock returned Err
    // and catch_unwind saw Ok(Err(...)).
    // If the helper let the panic escape, catch_unwind sees Err and the helper
    // is broken.
    match outer {
        Ok(inner_result) => {
            assert!(
                inner_result.is_err(),
                "helper must return Err when closure panics, got Ok"
            );
        }
        Err(_) => {
            panic!("with_engine_lock let the closure panic escape to the caller — expected Err return instead");
        }
    }
}

#[test]
fn panicking_closure_does_not_poison_mutex() {
    let engine = make_engine();
    // First call: closure panics — must return Err
    let first = engine_lock::with_engine_lock(&engine, |_s: &mut EngineSession| -> bool {
        panic!("boom")
    });
    assert!(first.is_err(), "panicking closure must return Err");

    // Second call: mutex must still be usable (not poisoned)
    let second = engine_lock::with_engine_lock(&engine, |s| s.is_idle());
    assert_eq!(
        second,
        Ok(true),
        "mutex must be usable after a panicking closure (not poisoned)"
    );
}
