//! Integration tests for the materialization-time annotation eval driver.
//!
//! Covers the `annotation-args ε` (#3556) eval driver (PRD §4, Phase 2, LEAF):
//! for every `AnnotationArgValue::Expr` arg whose schema declares
//! `eval_time = AtMaterialization`, on an instance-shaped host, the driver
//! evaluates the expression at structure-instance materialization and attaches
//! a per-instance `materialized_args` overlay.
//!
//! Step-5 RED: the success-signal integration test.
//!   - `@test_eval(2.0 * 1.5)` on `Foo` → overlay `annotation("test_eval").arg_value("value")`
//!     equals `Value::Real(3.0)` on the `EvalAnnoHarness.foo` instance.
//!   - Compiles (steps 2+4 landed) but is RED on the overlay assertion because
//!     the eval driver is not yet wired into engine_eval.rs (step-6 GREEN).
//!
//! Step-7 RED (added below): failure signals.
//!   - `@test_eval(undefined_ident * 1.0)` on `Bad` → AnnotationEvalFailed diagnostic
//!     AND `EvalAnnoFailHarness.bad` cell is `Value::Undef`.
//!   - `@test_eval(1.0 > 0.0)` → Bool result vs expected Real → same.

use reify_core::ValueCellId;
use reify_ir::{StructureInstanceData, Value};
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

// ── Fixture paths ─────────────────────────────────────────────────────────────

/// Workspace root derived from CARGO_MANIFEST_DIR (two levels above crates/reify-eval).
fn workspace_root() -> std::path::PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf()
}

// ── Step-5 RED: success-signal (overlay attaches for `2.0 * 1.5`) ──────────

/// `@test_eval(2.0 * 1.5)` on Foo evaluates to Real(3.0) and attaches as the
/// `annotation("test_eval").arg_value("value")` overlay on the `EvalAnnoHarness.foo`
/// instance.
///
/// RED on base (step-5): the eval driver is not wired — no overlay is attached —
/// so the `arg_value("value") == Some(&Value::Real(3.0))` assertion fails.
/// GREEN after step-6 wires the driver in engine_eval.rs.
#[test]
fn eval_annotation_smoke_attaches_overlay() {
    let fixture_path = workspace_root()
        .join("crates/reify-compiler/tests/fixtures/eval_annotation_smoke.ri");
    let source = std::fs::read_to_string(&fixture_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {:?}: {e}\n\
             (check CARGO_MANIFEST_DIR resolution and workspace layout)",
            fixture_path
        )
    });

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "unexpected compile errors: {:?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No compile-level errors should appear.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "unexpected eval errors: {:?}",
        eval_errors
    );

    // The EvalAnnoHarness.foo cell must be a Value::StructureInstance of type Foo.
    let foo_id = ValueCellId::new("EvalAnnoHarness", "foo");
    let foo_val = result.values.get(&foo_id).unwrap_or_else(|| {
        panic!(
            "EvalAnnoHarness.foo cell not found; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        )
    });

    let data: &StructureInstanceData = match foo_val {
        Value::StructureInstance(d) => d,
        other => panic!(
            "EvalAnnoHarness.foo expected Value::StructureInstance, got {:?}",
            other
        ),
    };
    assert_eq!(
        data.type_name, "Foo",
        "instance type_name must be Foo, got {:?}",
        data.type_name
    );

    // The materialized overlay must be present and carry Real(3.0).
    //
    // RED (step-5): driver not wired → annotation() returns None.
    // GREEN (step-6): driver attaches the overlay → Some(&Value::Real(3.0)).
    let value = data
        .annotation("test_eval")
        .and_then(|a| a.arg_value("value"))
        .cloned();
    assert_eq!(
        value,
        Some(Value::Real(3.0)),
        "annotation(\"test_eval\").arg_value(\"value\") should be Some(Real(3.0)) \
         after the materialization driver evaluates 2.0 * 1.5; got {:?}",
        value
    );
}
