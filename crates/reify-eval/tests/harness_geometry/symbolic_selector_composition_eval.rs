//! R2c symbolic selector-composition eval integration tests (task #5120).
//!
//! Sibling to `symbolic_selector_eval.rs` (R2b, task #4653): pins the
//! user-observable signal that `Engine::eval` (kernel-free, no build) mints
//! `Value::Selector` COMPOSITION cells (`union`/`intersect`/`difference`,
//! step-2) and NAMED-LEAF cells (`face`/`edge`/`solid_body`/`vertex`,
//! step-4) over SYMBOLIC (`kernel_handle=None`) leaf operands, instead of
//! leaving them at `Value::Undef`.
//!
//! ## TDD arc
//!
//! **Step-1/2 (composition):** wired via `eval_variadic_composition_symbolic`
//! / `reconstruct_selector_value_symbolic`.
//!
//! **Step-3 (RED):** the named-leaf test below FAILS until step-4 wires
//! `face`/`edge`/`solid_body`/`vertex` into `symbolic_eval_helper_for_name` +
//! `try_eval_symbolic_topology_selector` (via the new kernel-free
//! `eval_named_leaf_selector_ctor_symbolic` / `resolve_named_leaf_target_symbolic`
//! siblings).

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
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

/// BT8 — `Engine::eval` must mint `BT8NamedLeaf.s` (`face(b, "nope")`) as a
/// symbolic `Value::Selector(Face)` with `LeafQuery::Named("nope")`, and
/// `check_no_stale_undef` must report zero violations for this fixture.
#[test]
fn bt8_named_leaf_eval_yields_symbolic_named_selector() {
    let source = std::fs::read_to_string(fixture_path("bt8_named_leaf_interim.ri"))
        .expect("fixture bt8_named_leaf_interim.ri must exist");
    let compiled = reify_test_support::compile_source_with_stdlib(&source);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "bt8 fixture must compile without errors: {errors:#?}"
    );

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let value = result.values.get(&ValueCellId::new("BT8NamedLeaf", "s"));
    let sv = assert_selector(value, "BT8NamedLeaf.s", SelectorKind::Face);
    match &sv.node {
        SelectorNode::Leaf { target, query } => {
            assert_eq!(
                target.kernel_handle, None,
                "BT8: symbolic target must have kernel_handle == None"
            );
            assert_eq!(
                query,
                &LeafQuery::Named("nope".to_string()),
                "BT8: Named(\"nope\") leaf"
            );
        }
        other => panic!("BT8NamedLeaf.s must be SelectorNode::Leaf, got {other:?}"),
    }

    let violations = engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "BT8: expected zero stale-Undef violations post-eval; got {violations:?}"
    );
}

/// Review amendment (task #5120 R2c, round-2 review): the unit-level tests in
/// `geometry_ops/tests.rs` pin that `try_eval_symbolic_topology_selector`
/// returns `None` for the solid-CSG-boolean overload of `union`/`intersect`/
/// `difference` (operands are `Value::GeometryHandle`, not `Value::Selector`
/// — e.g. `manifold_boolean.ri`'s `union(box_a,box_b):Solid` or
/// `m5_geometry_flange`'s `difference(body,holes):Solid`). But that `None`
/// only proves the SELECTOR mint declines the cell; it says nothing about
/// what the cell's value actually ends up being, nor whether it is correctly
/// kept out of the α no-stale-Undef net end-to-end. This test closes that
/// gap empirically rather than by assumption.
///
/// Verified fact (NOT `Value::Undef`, contra a first draft of this test):
/// `Engine::eval`'s `mint_symbolic_geometry_handles_into_values` pass
/// (task #4652 R2a, `engine_build.rs`) mints a symbolic
/// `Value::GeometryHandle { kernel_handle: None, .. }` placeholder for EVERY
/// named `Type::Geometry` cell, regardless of whether its `default_expr` is a
/// leaf ctor (`box(..)`) or a composed op like this solid-boolean `union` —
/// this is a pre-existing, R2c-independent mechanism. Consequently the cell
/// is not even a *candidate* stale-Undef violation: `invariants.rs`'s clause
/// 3 ("only a currently-Undef cell can be stale") already filters it out
/// before clause 7's `Type::Geometry` carve-out would need to fire. Either
/// way, the OBSERVABLE contract the reviewer asked for — zero violations on
/// eval, then a real resolution on `build()` — is what this test pins.
///
/// Complements, rather than duplicates,
/// `no_stale_undef_invariant_gate.rs`'s
/// `seeded_solid_boolean_union_undef_is_exempted_by_geometry_clause`: that
/// test FABRICATES a graph/values state with the union cell held at literal
/// `Value::Undef` to pin clause 7 in isolation at the checker level. This
/// test instead drives the REAL `Engine::eval`/`Engine::build` pipeline
/// end-to-end over compiled source — which is how the gap in the discovery
/// above was actually found: the real pipeline never reaches clause 7 for
/// this shape at all, because the placeholder mint gets there first.
///
/// Only `union` is exercised here (not a per-operator triplication like the
/// unit-level None-return tests): the mechanism above is keyed SOLELY on the
/// cell's declared `Type::Geometry`, not on which of the three overloaded
/// names produced it, so a second/third copy of this test under
/// `intersect`/`difference` would walk the identical code path with no added
/// branch coverage — unlike `symbolic_eval_helper_for_name`'s dispatch, which
/// IS per-name and is exactly why that unit-level coverage is tripled.
#[test]
fn solid_csg_boolean_union_is_not_stale_undef_on_eval_and_resolves_on_build() {
    const SRC: &str = r#"
structure def R2cSolidBooleanUnion {
    let box_a = box(10mm, 10mm, 10mm)
    let box_b = box(10mm, 10mm, 10mm)
    let body  = union(box_a, box_b)
}
"#;
    let compiled = reify_test_support::compile_source_with_stdlib(SRC);
    let errors = reify_test_support::collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "solid-CSG-boolean union fixture must compile without errors: {errors:#?}"
    );

    let cell_id = ValueCellId::new("R2cSolidBooleanUnion", "body");

    // Path A: pure eval (no kernel). `union(Geometry, Geometry)` is the
    // solid-CSG-boolean overload, not selector composition, so it must NOT
    // resolve via the selector mint — but the cell is not left at
    // `Value::Undef` either: the general symbolic-geometry-handle mint
    // (task #4652 R2a) stamps a `kernel_handle: None` placeholder for every
    // named Type::Geometry cell.
    let mut eval_engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let eval_result = eval_engine.eval(&compiled);
    let eval_value = eval_result.values.get_or_undef(&cell_id);
    assert!(
        matches!(
            eval_value,
            Value::GeometryHandle {
                kernel_handle: None,
                ..
            }
        ),
        "solid-CSG-boolean union must mint a SYMBOLIC (kernel_handle=None) \
         GeometryHandle placeholder on the kernel-free eval surface — not a \
         Value::Selector (this is the solid-CSG-boolean overload, not \
         selector composition) and not literally Value::Undef; got {eval_value:?}"
    );

    // Whatever the precise exempting clause, the cell must NOT be reported
    // as a stale-Undef violation — this is the end-to-end contract the
    // review flagged as unverified.
    let violations = eval_engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "solid-CSG-boolean union cell must not be flagged as a stale-Undef \
         violation; got {violations:?}"
    );

    // Path B: build with a mock kernel — the solid boolean DOES resolve on
    // the build() path, to a realized geometry handle.
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
    assert!(
        matches!(
            build_value,
            Value::GeometryHandle {
                kernel_handle: Some(_),
                ..
            }
        ),
        "solid-CSG-boolean union must resolve to a realized GeometryHandle on \
         the build() path; got {build_value:?}"
    );
}
