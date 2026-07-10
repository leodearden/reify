//! R2c symbolic selector-composition eval integration tests (task #5120).
//!
//! Sibling to `symbolic_selector_eval.rs` (R2b, task #4653): pins the
//! user-observable signal that `Engine::eval` (kernel-free, no build) mints
//! `Value::Selector` COMPOSITION cells (`union`/`intersect`/`difference`)
//! over SYMBOLIC (`kernel_handle=None`) leaf operands, instead of leaving
//! them at `Value::Undef`.
//!
//! ## TDD arc
//!
//! **Step-1 (RED):** every test below FAILS until step-2 wires the
//! `union`/`intersect`/`difference` composition arms into
//! `symbolic_eval_helper_for_name` + `try_eval_symbolic_topology_selector`
//! (via the new kernel-free `eval_variadic_composition_symbolic` /
//! `reconstruct_selector_value_symbolic` siblings).

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::SelectorNode;
use reify_ir::{ExportFormat, Value};
use reify_test_support::MockGeometryKernel;

/// Absolute path to a fixture file in `tests/fixtures/selectors/`.
fn fixture_path(name: &str) -> String {
    format!(
        "{}/tests/fixtures/selectors/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

/// Assert `cell_value` holds `Value::Selector(kind)` and return a clone of
/// it, panicking with a RED-until-wired message otherwise. Mirrors
/// `symbolic_selector_eval.rs::assert_selector_leaf`, generalized to any
/// `SelectorNode` shape (not just `Leaf`) since composition cells mint
/// `Union`/`Intersect`/`Difference` nodes.
fn assert_selector(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
) -> reify_ir::value::SelectorValue {
    match cell_value {
        Some(Value::Selector(sv)) => {
            assert_eq!(sv.kind, kind, "{label}: selector kind");
            sv.clone()
        }
        other => panic!(
            "{label}: expected Value::Selector, got {other:?}; \
             (RED until the composition arms are wired in step-2)"
        ),
    }
}

/// Assert `node` is a `Leaf` with a symbolic (`kernel_handle == None`) target.
fn assert_symbolic_leaf(node: &SelectorNode, label: &str) {
    match node {
        SelectorNode::Leaf { target, .. } => {
            assert_eq!(
                target.kernel_handle, None,
                "{label}: symbolic eval must yield target.kernel_handle == None"
            );
        }
        other => panic!("{label}: expected a Leaf node, got {other:?}"),
    }
}

/// BT2 — `Engine::eval` must mint `BT2SameKindUnion.u`
/// (`union(faces_by_normal(b,up,tol), faces_by_normal(b,down,tol))`) as a
/// symbolic `Value::Selector(Union([..]))`, and `check_no_stale_undef` must
/// report zero violations for this fixture.
#[test]
fn bt2_union_eval_yields_symbolic_union_selector() {
    let source = std::fs::read_to_string(fixture_path("bt2_same_kind_union.ri"))
        .expect("fixture bt2_same_kind_union.ri must exist");
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "bt2 fixture must compile without errors: {errors:#?}"
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("BT2SameKindUnion", "u");
    let value = result.values.get(&cell_id);
    let sv = assert_selector(value, "BT2SameKindUnion.u", SelectorKind::Face);

    match &sv.node {
        SelectorNode::Union(children) => {
            assert_eq!(children.len(), 2, "BT2: union must have 2 children");
            for (i, child) in children.iter().enumerate() {
                assert_symbolic_leaf(&child.node, &format!("BT2 union child[{i}]"));
            }
        }
        other => panic!("BT2SameKindUnion.u must be SelectorNode::Union, got {other:?}"),
    }

    let violations = engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "BT2: expected zero stale-Undef violations post-eval; got {violations:?}"
    );
}

/// BT3 — `Engine::eval` must mint both `BT3SetOps.d` (difference) and
/// `BT3SetOps.i` (intersect) as symbolic `Value::Selector` composition
/// cells, and `check_no_stale_undef` must report zero violations for this
/// fixture.
#[test]
fn bt3_difference_and_intersect_eval_yield_symbolic_composition_selectors() {
    let source = std::fs::read_to_string(fixture_path("bt3_difference_intersect.ri"))
        .expect("fixture bt3_difference_intersect.ri must exist");
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "bt3 fixture must compile without errors: {errors:#?}"
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let d_value = result.values.get(&ValueCellId::new("BT3SetOps", "d"));
    let sv_d = assert_selector(d_value, "BT3SetOps.d", SelectorKind::Face);
    match &sv_d.node {
        SelectorNode::Difference(a, b) => {
            assert_symbolic_leaf(&a.node, "BT3 difference minuend");
            assert_symbolic_leaf(&b.node, "BT3 difference subtrahend");
        }
        other => panic!("BT3SetOps.d must be SelectorNode::Difference, got {other:?}"),
    }

    let i_value = result.values.get(&ValueCellId::new("BT3SetOps", "i"));
    let sv_i = assert_selector(i_value, "BT3SetOps.i", SelectorKind::Face);
    match &sv_i.node {
        SelectorNode::Intersect(children) => {
            assert_eq!(children.len(), 2, "BT3: intersect must have 2 children");
            for (idx, child) in children.iter().enumerate() {
                assert_symbolic_leaf(&child.node, &format!("BT3 intersect child[{idx}]"));
            }
        }
        other => panic!("BT3SetOps.i must be SelectorNode::Intersect, got {other:?}"),
    }

    let violations = engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "BT3: expected zero stale-Undef violations post-eval; got {violations:?}"
    );
}

/// §7.1 two-way boundary: eval (symbolic) and build (realized) must produce
/// `content_hash`-equal AND `PartialEq`-equal `Value::Selector` values for
/// BT2's union cell — mirrors
/// `symbolic_selector_eval.rs::eval_and_build_selectors_are_content_hash_equal`.
#[test]
fn bt2_union_eval_and_build_selectors_are_content_hash_equal() {
    let source = std::fs::read_to_string(fixture_path("bt2_same_kind_union.ri"))
        .expect("fixture bt2_same_kind_union.ri must exist");
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let cell_id = ValueCellId::new("BT2SameKindUnion", "u");

    // Path A: pure eval (no kernel) — symbolic selector.
    let mut eval_engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let eval_result = eval_engine.eval(&compiled);
    let eval_value = eval_result.values.get_or_undef(&cell_id);
    assert_selector(
        Some(&eval_value),
        "eval BT2SameKindUnion.u",
        SelectorKind::Face,
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
    assert_selector(
        Some(&build_value),
        "build BT2SameKindUnion.u",
        SelectorKind::Face,
    );

    assert_eq!(
        eval_value.content_hash(),
        build_value.content_hash(),
        "content_hash must be equal between symbolic (eval) and realized (build) union \
         selectors (DD-6: kernel_handle excluded from SelectorValue.content_hash)"
    );
    assert_eq!(
        eval_value, build_value,
        "PartialEq must hold between symbolic (eval) and realized (build) union selectors \
         (GHR-β §DD)"
    );
}
