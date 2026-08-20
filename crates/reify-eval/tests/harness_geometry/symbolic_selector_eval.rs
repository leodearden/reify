//! R2b symbolic selector eval integration tests (task #4653).
//!
//! Pins the user-observable signal and §7.1 two-way boundary contract
//! for `Engine::eval` minting kernel-free symbolic `Value::Selector` cells
//! instead of falling through to `Value::Undef`.
//!
//! ## TDD arc
//!
//! **Step-5 (RED):** `top_eval_yields_symbolic_selector` — asserts that
//! `Engine::eval` (no build, no kernel) produces a `Value::Selector(Face)` with
//! `ByNormal { dir: +z, tol_rad > 0 }` and `target.kernel_handle = None` for a
//! `faces_by_normal(body, dir, tol)` cell.  FAILS until step-6 wires
//! `mint_symbolic_topology_selectors_into_values`.
//!
//! **Step-5 (RED):** `eval_and_build_selectors_are_content_hash_equal` and
//! `eval_selector_content_hash_is_cross_run_stable` — pin §7.1 two-way boundary
//! and cross-run byte-stability.  Added when step-5 runs; FAIL until step-6.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, compile_source_with_stdlib};

/// Fixture: Widget with a body (box), a let-bound direction + tolerance, and a
/// `faces_by_normal` selector on top.
///
/// `let`-binding `dir` and `tol` avoids the out-of-scope inline-arg dispatcher
/// issue (PRD §5): inline `vec3(0,0,1)` / `1deg` args in `faces_by_normal`
/// would need the eval-path dispatcher to evaluate inline function-call args,
/// which is not part of R2b scope.  The `let`-bound form pre-resolves the args
/// into `values` before the selector-mint pass runs.
const WIDGET_SRC: &str = r#"structure def Widget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
    let dir = vec3(0.0, 0.0, 1.0)
    let tol = 1deg
    let top = faces_by_normal(body, dir, tol)
}"#;

/// Assert a `Some(Value)` holds a `Value::Selector` whose node is a `Leaf` of
/// `kind`, then invoke `check_query` on the leaf's `LeafQuery`.  Mirrors the
/// helper in `kernel_queries_directional_selectors.rs`.
fn assert_selector_leaf(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    check_query: impl FnOnce(&LeafQuery),
) {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!(
            "{label}: expected Value::Selector, got {other:?}; \
             (RED until mint_symbolic_topology_selectors_into_values is wired in step-6)"
        ),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    match &sv.node {
        SelectorNode::Leaf { query, .. } => check_query(query),
        other => panic!("{label}: must be a Leaf node, got {other:?}"),
    }
}

/// SIGNAL — `Engine::eval` (kernel-free, no build) must yield a
/// `Value::Selector(Face)` for `Widget.top` with:
/// - `SelectorNode::Leaf { ByNormal { dir: +z, tol_rad > 0 } }`
/// - `target.kernel_handle == None` (symbolic)
///
/// **RED** until step-6 adds `mint_symbolic_topology_selectors_into_values` and
/// wires it after the handle-mint in `eval()`/`eval_cached()`/`engine_edit.rs`.
/// Currently `Widget.top` stays at `Value::Undef` because the eval path has no
/// selector-dispatch pass.
#[test]
fn top_eval_yields_symbolic_selector() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time errors; got: {:#?}",
        compile_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Engine::eval — kernel-free.
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("Widget", "top");
    let value = result.values.get(&cell_id);

    // Assert Selector(Face) with ByNormal leaf and symbolic target.
    assert_selector_leaf(value, "Widget.top", SelectorKind::Face, |query| {
        match query {
            LeafQuery::ByNormal { dir, tol_rad } => {
                assert_eq!(*dir, [0.0, 0.0, 1.0], "Widget.top ByNormal dir must be +z");
                assert!(
                    *tol_rad > 0.0,
                    "Widget.top ByNormal tol_rad must be positive (1°); got {tol_rad}"
                );
            }
            other => panic!("Widget.top must be a ByNormal leaf; got {other:?}"),
        }
    });

    // Also pin that the target is symbolic (kernel_handle == None).
    let sv = match value {
        Some(Value::Selector(sv)) => sv,
        _ => unreachable!("already checked above"),
    };
    match &sv.node {
        SelectorNode::Leaf { target, .. } => {
            assert_eq!(
                target.kernel_handle,
                None,
                "symbolic eval must yield target.kernel_handle == None"
            );
        }
        _ => unreachable!(),
    }
}

/// §7.1 two-way boundary: `Engine::eval` (symbolic, `kernel_handle=None`) and
/// `Engine::build` (realized, `kernel_handle=Some(...)`) on the SAME source
/// must produce `content_hash`-equal AND `PartialEq`-equal `Value::Selector`
/// values for `Widget.top` (DD-6: `SelectorValue.content_hash` excludes
/// `kernel_handle`; the leaf target's `realization_ref` + `upstream_values_hash`
/// must match because R2a guarantees the handle fold is byte-identical).
///
/// **RED** until step-6 wires the selector-mint pass.
#[test]
fn eval_and_build_selectors_are_content_hash_equal() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);
    let cell_id = ValueCellId::new("Widget", "top");

    // Path A: pure eval (no kernel) — symbolic selector.
    let mut eval_engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let eval_result = eval_engine.eval(&compiled);
    let eval_value = eval_result.values.get_or_undef(&cell_id);

    // Confirm eval path gives a Selector (assert_selector_leaf handles the
    // Undef → panic message).
    assert_selector_leaf(
        Some(&eval_value),
        "eval Widget.top",
        SelectorKind::Face,
        |_| {},
    );

    // Path B: build with mock kernel — realized selector.
    let kernel = MockGeometryKernel::new();
    let mut build_engine =
        Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(kernel)));
    let build_result = build_engine.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        build_errors.is_empty(),
        "build must succeed with MockGeometryKernel; got: {build_errors:?}"
    );
    let build_value = build_result.values.get_or_undef(&cell_id);

    // Build path must also give a Selector.
    assert_selector_leaf(
        Some(&build_value),
        "build Widget.top",
        SelectorKind::Face,
        |_| {},
    );

    // §7.1 / DD-6: content_hash must be equal (kernel_handle excluded from hash).
    assert_eq!(
        eval_value.content_hash(),
        build_value.content_hash(),
        "content_hash must be equal between symbolic (eval) and realized (build) selectors \
         (DD-6: kernel_handle excluded from SelectorValue.content_hash)"
    );

    // PartialEq also excludes kernel_handle (via GeometryHandleRef::eq).
    assert_eq!(
        eval_value,
        build_value,
        "PartialEq must hold between symbolic (eval) and realized (build) selectors (GHR-β §DD)"
    );
}

/// Cross-run stability: two independent `Engine::eval` runs on the same compiled
/// source yield byte-identical `content_hash` for `Widget.top`.
///
/// **RED** until step-6 wires the selector-mint pass.
#[test]
fn eval_selector_content_hash_is_cross_run_stable() {
    let compiled = compile_source_with_stdlib(WIDGET_SRC);
    let cell_id = ValueCellId::new("Widget", "top");

    // Run 1 — fresh Engine.
    let mut engine1 = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result1 = engine1.eval(&compiled);
    let value1 = result1.values.get_or_undef(&cell_id);
    let ch1 = match &value1 {
        Value::Selector(_) => value1.content_hash(),
        other => panic!("run1: expected Value::Selector for Widget.top, got {other:?}"),
    };

    // Run 2 — separate Engine instance, same compiled module.
    let mut engine2 = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result2 = engine2.eval(&compiled);
    let value2 = result2.values.get_or_undef(&cell_id);
    let ch2 = match &value2 {
        Value::Selector(_) => value2.content_hash(),
        other => panic!("run2: expected Value::Selector for Widget.top, got {other:?}"),
    };

    assert_eq!(
        ch1,
        ch2,
        "content_hash must be byte-identical across independent Engine::eval runs"
    );
}
