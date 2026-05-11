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
