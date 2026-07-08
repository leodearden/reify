//! Debug-gate integration suite for the no-stale-Undef invariant checker over
//! the EDIT path (task ε, PRD
//! docs/prds/v0_6/eval-uniform-dependency-handling.md §4 ε / §6.1 "extends it
//! graph-natively to edit_param/edit_source").
//!
//! α (task 4952/4953, `no_stale_undef_invariant_gate.rs`) proved the
//! invariant holds post-`eval()`/`eval_cached()` over the fixture corpus.
//! This suite extends that proof to the two edit entry points, `edit_param`
//! and `edit_source`, over the geometry-let -> selector -> consumer shape
//! that γ (task 4954) closed miss #5 for: a top-level geometry LET now
//! carries its own value cell (paired with its `RealizationDecl`), so it
//! rides the dirty cone and is minted in-walk on the edit path
//! (`engine_edit.rs`'s `mint_symbolic_geometry_handle_for_cell_from_graph` +
//! `try_eval_symbolic_topology_selector`), rather than requiring a fresh
//! `build()`.
//!
//! Both `Engine::check_no_stale_undef()` and
//! `invariants::check_no_stale_undef` are reused UNCHANGED here — no
//! edit-path-specific checker variant, no `TopologyTemplate` dependency. Both
//! `edit_param` (engine_edit.rs:2255-2258) and `edit_source` (:3805-3809)
//! install the post-edit snapshot/reverse_index/trace_map into
//! `self.eval_state` immediately before returning, so the checker (which
//! reads `eval_state()`) inspects exactly the state each edit path returns —
//! already graph-native, matching the `_from_graph` shape
//! (`engine_eval.rs`'s `re_eval_consumers_of_in_walk_mints_from_graph`).

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockGeometryKernel, collect_errors, compile_source_with_stdlib};

/// The ε edit-path premise fixture: param-augmented sibling of the α/δ
/// premise fixture `geometry_let_selector_consumer.ri`. See its header for
/// the full rationale.
const EDIT_FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/prd-gate/fixtures/geometry_let_selector_consumer_edit.ri"
);

fn read_edit_fixture() -> String {
    std::fs::read_to_string(EDIT_FIXTURE_PATH)
        .unwrap_or_else(|e| panic!("reading {EDIT_FIXTURE_PATH}: {e}"))
}

/// Deliverable 2 + deliverable 1 core: an `edit_param` upstream of a
/// geometry let whose selector feeds a consumer must re-resolve the consumer
/// (non-Undef) after the edit, AND the edit-path invariant must stay green.
#[test]
fn edit_param_let_backed_selector_consumer_reresolves_and_invariant_green() {
    let source = read_edit_fixture();
    let compiled = compile_source_with_stdlib(&source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "geometry_let_selector_consumer_edit.ri should compile without errors: {errors:#?}"
    );

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let probe_id = ValueCellId::new("GeometryLetSelectorConsumerEdit", "probe");
    let w_id = ValueCellId::new("GeometryLetSelectorConsumerEdit", "w");

    // Baseline: prove the chain resolves BEFORE the edit too, so the
    // post-edit assertion below proves RE-resolution, not just initial
    // resolution.
    let baseline = engine.eval(&compiled);
    let baseline_w = baseline
        .values
        .get(&w_id)
        .cloned()
        .expect("w should be in baseline values");
    let baseline_probe = baseline
        .values
        .get(&probe_id)
        .cloned()
        .expect("probe should be in baseline values");
    assert!(
        !matches!(baseline_probe, Value::Undef),
        "baseline: GeometryLetSelectorConsumerEdit.probe must NOT be \
         Value::Undef before the edit — got {baseline_probe:?}"
    );
    assert!(
        matches!(baseline_probe, Value::Selector(_)),
        "baseline: GeometryLetSelectorConsumerEdit.probe must resolve to a \
         Value::Selector (faces_by_normal is a kernel-free symbolic-eval-wired \
         leaf ctor) — got {baseline_probe:?}"
    );

    let edited_w = Value::length(0.060);
    assert_ne!(
        baseline_w, edited_w,
        "the edited param value must actually differ from baseline, or the \
         re-resolution this test proves would be vacuous"
    );
    let result = engine
        .edit_param(w_id, edited_w)
        .expect("edit_param should succeed after a prior eval()");

    let post_edit_probe = result
        .values
        .get(&probe_id)
        .expect("probe should be in post-edit values");
    assert!(
        !matches!(post_edit_probe, Value::Undef),
        "miss #5 closure: GeometryLetSelectorConsumerEdit.probe must NOT be \
         Value::Undef after edit_param — the geometry-let cell must ride the \
         dirty cone and mint in-walk (γ, task 4954) so the selector consumer \
         re-resolves — got {post_edit_probe:?}"
    );
    assert!(
        matches!(post_edit_probe, Value::Selector(_)),
        "GeometryLetSelectorConsumerEdit.probe must re-resolve to a \
         Value::Selector after edit_param — got {post_edit_probe:?}"
    );

    // Non-vacuity guard (anti-silent-accept): the checker below must be
    // inspecting REAL post-edit state, not an empty/absent one.
    let state = engine
        .eval_state()
        .expect("eval_state must be populated after edit_param");
    for member in ["loc_box", "loc", "probe"] {
        let id = ValueCellId::new("GeometryLetSelectorConsumerEdit", member);
        assert!(
            state.snapshot.graph.value_cells.contains_key(&id),
            "post-edit graph must retain the {member} value cell"
        );
    }

    let violations = engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "edit-path invariant must be green over the ε geometry-let/selector \
         edit-closure scenario after edit_param, got {violations:?}"
    );
}

/// Modified sibling of the ε fixture, used only by the `edit_source` test
/// below: same `loc_box -> loc -> probe` chain, but `loc_box`'s `box()`
/// dimension changes (30mm -> 40mm) so the edit is observable — a
/// byte-identical module would make the re-resolution assertion vacuous.
const EDIT_FIXTURE_SOURCE_MODIFIED: &str = r#"structure def GeometryLetSelectorConsumerEdit {
    param w : Length = 50mm
    let loc_box = box(w, 40mm, 10mm)
    let loc = faces_by_normal(loc_box, vec3(0.0, 0.0, 1.0), 1deg)
    let probe = loc
}"#;

/// Deliverable 1 (§6.1 "ε extends it graph-natively to edit_param/
/// edit_source"): the same geometry-let -> selector -> consumer chain must
/// re-resolve (non-Undef) after `edit_source` too, AND the edit-path
/// invariant must stay green — proving the graph-native checker is reused
/// unchanged over edit_source's `eval_state` install (engine_edit.rs:3805-3809),
/// not just edit_param's.
#[test]
fn edit_source_geometry_let_selector_chain_invariant_green() {
    let source = read_edit_fixture();
    let compiled = compile_source_with_stdlib(&source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "geometry_let_selector_consumer_edit.ri should compile without errors: {errors:#?}"
    );

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let probe_id = ValueCellId::new("GeometryLetSelectorConsumerEdit", "probe");
    let loc_box_id = ValueCellId::new("GeometryLetSelectorConsumerEdit", "loc_box");

    // Baseline: prove the chain resolves BEFORE the edit too, so the
    // post-edit assertion below proves RE-resolution, not just initial
    // resolution.
    let baseline = engine.eval(&compiled);
    let baseline_probe = baseline
        .values
        .get(&probe_id)
        .cloned()
        .expect("probe should be in baseline values");
    assert!(
        matches!(baseline_probe, Value::Selector(_)),
        "baseline: GeometryLetSelectorConsumerEdit.probe must resolve to a \
         Value::Selector before the edit — got {baseline_probe:?}"
    );
    let baseline_loc_box_hash = match baseline.values.get(&loc_box_id) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => *upstream_values_hash,
        other => panic!(
            "baseline: GeometryLetSelectorConsumerEdit.loc_box must hydrate to a \
             Value::GeometryHandle — got {other:?}"
        ),
    };

    let modified_compiled = compile_source_with_stdlib(EDIT_FIXTURE_SOURCE_MODIFIED);
    let modified_errors = collect_errors(&modified_compiled.diagnostics);
    assert!(
        modified_errors.is_empty(),
        "the modified edit_source module should compile without errors: {modified_errors:#?}"
    );

    let result = engine
        .edit_source(&modified_compiled)
        .expect("edit_source should succeed after a prior eval()");

    // Non-vacuity guard (anti-silent-accept): loc_box's minted upstream hash
    // must actually change, or the re-resolution proved below would be
    // vacuous (the same values recomputed from a byte-identical module).
    let post_edit_loc_box_hash = match result.values.get(&loc_box_id) {
        Some(Value::GeometryHandle {
            upstream_values_hash,
            ..
        }) => *upstream_values_hash,
        other => panic!(
            "post-edit_source: GeometryLetSelectorConsumerEdit.loc_box must \
             still hydrate to a Value::GeometryHandle — got {other:?}"
        ),
    };
    assert_ne!(
        baseline_loc_box_hash, post_edit_loc_box_hash,
        "the edited box() dimension must change loc_box's minted upstream \
         hash, or the re-resolution this test proves would be vacuous"
    );

    let post_edit_probe = result
        .values
        .get(&probe_id)
        .expect("probe should be in post-edit values");
    assert!(
        !matches!(post_edit_probe, Value::Undef),
        "miss #5 closure: GeometryLetSelectorConsumerEdit.probe must NOT be \
         Value::Undef after edit_source — got {post_edit_probe:?}"
    );
    assert!(
        matches!(post_edit_probe, Value::Selector(_)),
        "GeometryLetSelectorConsumerEdit.probe must re-resolve to a \
         Value::Selector after edit_source — got {post_edit_probe:?}"
    );

    // Non-vacuity guard (anti-silent-accept): the checker below must be
    // inspecting REAL post-edit state, not an empty/absent one.
    let state = engine
        .eval_state()
        .expect("eval_state must be populated after edit_source");
    for member in ["loc_box", "loc", "probe"] {
        let id = ValueCellId::new("GeometryLetSelectorConsumerEdit", member);
        assert!(
            state.snapshot.graph.value_cells.contains_key(&id),
            "post-edit graph must retain the {member} value cell"
        );
    }

    let violations = engine.check_no_stale_undef();
    assert!(
        violations.is_empty(),
        "edit-path invariant must be green over the ε geometry-let/selector \
         edit-closure scenario after edit_source, got {violations:?}"
    );
}
