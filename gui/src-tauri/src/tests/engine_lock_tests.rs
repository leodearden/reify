use std::sync::Arc;

use crate::engine::EngineSession;
use crate::engine_lock;

fn make_engine() -> Arc<std::sync::Mutex<EngineSession>> {
    super::make_test_engine()
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

#[test]
fn pre_poisoned_mutex_is_recovered() {
    let engine = make_engine();

    // Manually poison the mutex using the canonical std-lib pattern:
    // spawn a thread, acquire the lock, and panic while holding it.
    let m = Arc::clone(&engine);
    let _ = std::thread::spawn(move || {
        let _g = m.lock().unwrap();
        panic!("intentional poison");
    })
    .join();

    // Confirm the mutex is actually poisoned now.
    assert!(
        engine.lock().is_err(),
        "mutex should be poisoned after the spawned thread panicked"
    );

    // with_engine_lock must recover from pre-existing poisoning via into_inner().
    let result = engine_lock::with_engine_lock(&engine, |s| s.is_idle());
    assert_eq!(
        result,
        Ok(true),
        "with_engine_lock must recover from a pre-poisoned mutex"
    );
}

#[test]
fn panic_payload_string_appears_in_error_message() {
    let engine = make_engine();
    let result =
        engine_lock::with_engine_lock(&engine, |_s: &mut EngineSession| -> bool {
            panic!("my-marker-7e9c")
        });
    let err = result.expect_err("panicking closure must return Err");
    assert!(
        err.contains("panic in engine"),
        "error must contain 'panic in engine', got: {err:?}"
    );
    assert!(
        err.contains("my-marker-7e9c"),
        "error must contain the panic message 'my-marker-7e9c', got: {err:?}"
    );
}

#[test]
fn panic_payload_owned_string_appears_in_error_message() {
    // Covers the String downcast branch in panic_message:
    // panic!("{}", x) produces a String payload (not &'static str).
    let engine = make_engine();
    let marker = "string-arm-marker".to_string();
    let result =
        engine_lock::with_engine_lock(&engine, |_s: &mut EngineSession| -> bool {
            panic!("{}", marker)
        });
    let err = result.expect_err("panicking closure must return Err");
    assert!(
        err.contains("panic in engine"),
        "error must contain 'panic in engine', got: {err:?}"
    );
    assert!(
        err.contains("string-arm-marker"),
        "error must contain the formatted panic message, got: {err:?}"
    );
}

#[test]
fn panic_payload_non_string_falls_back_to_placeholder() {
    // Covers the fallback branch in panic_message:
    // panic_any(42_i32) produces an i32 payload that neither downcast branch handles.
    let engine = make_engine();
    let result =
        engine_lock::with_engine_lock(&engine, |_s: &mut EngineSession| -> bool {
            std::panic::panic_any(42_i32)
        });
    let err = result.expect_err("panicking closure must return Err");
    assert!(
        err.contains("panic in engine"),
        "error must contain 'panic in engine', got: {err:?}"
    );
    assert!(
        err.contains("<non-string payload>"),
        "error must contain fallback text for non-string payloads, got: {err:?}"
    );
}
