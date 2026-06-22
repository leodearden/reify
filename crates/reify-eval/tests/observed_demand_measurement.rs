//! Engine-level observed-demand API (selective-demand precondition, task 4532).
//!
//! The observed-demand registry is a PASSIVE side-channel: it mirrors
//! `DemandRegistry`'s cone semantics but is NEVER fed into `compute_eval_set`.
//! These tests pin (a) the registry's add/remove/rebuild/reset/inspect API and
//! (b) the zero-behavior-change contract — registering observed demand must not
//! change the production eval-set produced by `edit_param`.

use reify_compiler::CompiledModule;
use reify_core::{ModulePath, RealizationNodeId, Type, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{Engine, WouldPruneByKind};
use reify_ir::{CompiledExpr, Value};
// `sorted_values` / `bracket_engine` are shared from reify-test-support (a
// single definition shared with `selective_demand_measurement.rs`) so the
// byte-identity comparison logic cannot drift between the two test files. The
// builder/mocks symbols (re-exported at the crate root) drive the collection
// fixture used by the structural-mutation coverage tests below.
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, bracket_engine, cnid,
    sorted_values, value_ref_typed, vcid,
};

#[test]
fn observed_demand_cone_tracks_registered_roots_only() {
    let mut engine = bracket_engine();

    // C0 is `thickness > 2mm` — reads `thickness` only, NOT `width`.
    let c0 = NodeId::Constraint(cnid("Bracket", 0));
    let width = NodeId::Value(vcid("Bracket", "width"));
    let thickness = NodeId::Value(vcid("Bracket", "thickness"));

    // Register C0 as observed demand and rebuild the observed cone.
    engine.add_observed_demand(c0.clone());
    engine.rebuild_observed_cone();

    assert!(
        engine.observed_demand_is_demanded(&c0),
        "C0 (registered root) must be in the observed cone"
    );
    assert!(
        engine.observed_demand_is_demanded(&thickness),
        "thickness (read by C0) must be pulled into the observed cone"
    );
    assert!(
        !engine.observed_demand_is_demanded(&width),
        "width is not reachable from C0 and must NOT be in the observed cone"
    );
    assert_eq!(
        engine.observed_demand_cone_size(),
        2,
        "observed cone of {{C0}} is exactly {{C0, thickness}}"
    );

    // Removing C0 and rebuilding clears the cone.
    engine.remove_observed_demand(&c0);
    engine.rebuild_observed_cone();
    assert!(
        !engine.observed_demand_is_demanded(&c0),
        "after remove + rebuild, C0 is no longer demanded"
    );
    assert_eq!(
        engine.observed_demand_cone_size(),
        0,
        "after remove + rebuild, the observed cone is empty"
    );

    // reset_observed_demand() empties the cone even after re-registration.
    engine.add_observed_demand(c0.clone());
    engine.rebuild_observed_cone();
    assert_eq!(engine.observed_demand_cone_size(), 2, "re-registered cone");
    engine.reset_observed_demand();
    assert_eq!(
        engine.observed_demand_cone_size(),
        0,
        "reset_observed_demand empties the observed cone"
    );
    assert!(
        !engine.observed_demand_is_demanded(&c0),
        "reset clears registered roots too"
    );
}

#[test]
fn observed_demand_registration_does_not_change_production_eval_set() {
    // Two independent engines, identical edit. engine_b additionally registers
    // observed demand BEFORE the edit. The production eval-set (last_eval_set)
    // must be byte-identical — proving observed_* never feed compute_eval_set.
    let thickness_id = || ValueCellId::new("Bracket", "thickness");

    let mut engine_a = bracket_engine();
    engine_a
        .edit_param(thickness_id(), Value::length(0.004))
        .expect("edit_param(thickness) on engine_a");
    let eval_set_a: Vec<NodeId> = engine_a.last_eval_set().to_vec();

    let mut engine_b = bracket_engine();
    // Register observed demand on a DIFFERENT-shaped cone than the edit's dirty
    // cone (just C0) so any accidental coupling would perturb the eval-set.
    engine_b.add_observed_demand(NodeId::Constraint(cnid("Bracket", 0)));
    engine_b.rebuild_observed_cone();
    engine_b
        .edit_param(thickness_id(), Value::length(0.004))
        .expect("edit_param(thickness) on engine_b");
    let eval_set_b: Vec<NodeId> = engine_b.last_eval_set().to_vec();

    assert_eq!(
        eval_set_a, eval_set_b,
        "observed-demand registration must not change the production eval-set"
    );
}

#[test]
fn edit_param_records_exact_would_prune_measurement() {
    let mut engine = bracket_engine();

    // Constraint panel shows only C0 (`thickness > 2mm`). Observed cone after
    // rebuild is {C0, thickness}.
    engine.add_observed_demand(NodeId::Constraint(cnid("Bracket", 0)));
    engine.rebuild_observed_cone();

    // Edit thickness. Dirty cone = {volume(Value), C0,C1,C2(Constraint),
    // R0(Realization)} (dirty.rs::dirty_cone_bracket_change_thickness). The
    // driver value-loop (θ2 #4713) excludes Realization nodes from
    // last_eval_set (R0 is deferred entirely to build()), so the measured
    // eval-set is {volume,C0,C1,C2} — exactly 4 nodes.
    engine
        .edit_param(
            ValueCellId::new("Bracket", "thickness"),
            Value::length(0.004),
        )
        .expect("edit_param(thickness)");

    let m = engine
        .last_demand_prune_measurement()
        .expect("measurement recorded after edit_param");

    // Invariant: measurement counts the FINAL production eval-set.
    assert_eq!(
        m.eval_set_size,
        engine.last_eval_set().len(),
        "eval_set_size mirrors last_eval_set().len()"
    );
    assert_eq!(m.eval_set_size, 4, "eval_set = {{volume,C0,C1,C2}} (R0 excluded by driver value-loop)");

    // Only C0 of the eval-set is in the observed cone {C0, thickness}.
    assert_eq!(m.observed_retained, 1, "C0 retained");

    // The other three would be pruned, split by kind.
    assert_eq!(
        m.would_prune,
        WouldPruneByKind {
            value: 1,       // volume
            constraint: 2,  // C1, C2
            realization: 0, // R0 excluded from last_eval_set (deferred to build())
            resolution: 0,
            compute: 0,
        },
        "would-prune split: volume / C1,C2"
    );

    // Conservation law (held by construction at the measurement site).
    assert_eq!(
        m.observed_retained + m.would_prune.total(),
        m.eval_set_size,
        "observed_retained + Σwould_prune == eval_set_size"
    );
}

#[test]
fn observed_registration_is_zero_behavior_change_leaf_lock() {
    // LEAF regression lock: a scripted bracket session run on two engines —
    // engine_a (no observed registration) and engine_b (observed demand =
    // {R0, thickness} registered BEFORE the script). For EVERY edit the
    // production results must be byte-identical; engine_b additionally records a
    // real would-prune measurement. Pins the task's zero-behavior-change signal.
    let edits: Vec<(ValueCellId, Value)> = vec![
        (vcid("Bracket", "width"), Value::length(0.1)),
        (vcid("Bracket", "thickness"), Value::length(0.02)),
        (vcid("Bracket", "hole_diameter"), Value::length(0.006)),
        (vcid("Bracket", "width"), Value::length(0.09)),
    ];

    let mut engine_a = bracket_engine();

    let mut engine_b = bracket_engine();
    engine_b.add_observed_demand(NodeId::Realization(RealizationNodeId::new("Bracket", 0)));
    engine_b.add_observed_demand(NodeId::Value(vcid("Bracket", "thickness")));
    engine_b.rebuild_observed_cone();

    let thickness_id = vcid("Bracket", "thickness");
    let mut saw_real_pruning = false;

    for (i, (id, val)) in edits.iter().enumerate() {
        let ra = engine_a
            .edit_param(id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("engine_a edit {i} failed: {e:?}"));
        let rb = engine_b
            .edit_param(id.clone(), val.clone())
            .unwrap_or_else(|e| panic!("engine_b edit {i} failed: {e:?}"));

        // (1) values byte-identical.
        assert_eq!(
            sorted_values(&ra),
            sorted_values(&rb),
            "edit {i}: EvalResult.values must match"
        );
        // (2) resolved_params equal.
        assert_eq!(
            ra.resolved_params, rb.resolved_params,
            "edit {i}: resolved_params must match"
        );
        // (3) diagnostics length equal.
        assert_eq!(
            ra.diagnostics.len(),
            rb.diagnostics.len(),
            "edit {i}: diagnostics length must match"
        );
        // (4) last_eval_set byte-identical.
        assert_eq!(
            engine_a.last_eval_set(),
            engine_b.last_eval_set(),
            "edit {i}: last_eval_set must be byte-identical"
        );

        // engine_b records a measurement on every edit.
        let mb = engine_b
            .last_demand_prune_measurement()
            .unwrap_or_else(|| panic!("edit {i}: engine_b measurement must be Some"));
        assert_eq!(
            mb.observed_retained + mb.would_prune.total(),
            mb.eval_set_size,
            "edit {i}: measurement conservation law"
        );

        if id == &thickness_id {
            // Observed cone is closure of {R0, thickness} = {R0, width, height,
            // thickness}. The driver value-loop (θ2 #4713) excludes Realization
            // nodes from last_eval_set (R0 deferred to build()), so the measured
            // eval-set is {volume,C0,C1,C2}. None of those are in the observed
            // cone, so observed_retained == 0 and all four are would-prune.
            assert!(
                mb.would_prune.total() > 0,
                "edit {i}: thickness edit must show real pruning"
            );
            assert_eq!(
                mb.would_prune.realization, 0,
                "edit {i}: R0 absent from last_eval_set (excluded by driver value-loop)"
            );
            assert_eq!(
                mb.observed_retained, 0,
                "edit {i}: no node in {{volume,C0,C1,C2}} is in observed cone {{R0,width,height,thickness}}"
            );
            saw_real_pruning = true;
        }
    }

    assert!(
        saw_real_pruning,
        "the scripted session must include the thickness edit"
    );
}

// ---------------------------------------------------------------------------
// Structural-mutation coverage (collection grow / shrink).
//
// `edit_param` rebuilds the OBSERVED cone against the post-mutation graph when a
// collection grows or shrinks (`structural_mutation && observed cone non-empty`,
// engine_edit.rs). The bracket / two-body fixtures only ever do plain `param`
// edits, so that branch is otherwise never exercised. These two tests drive a
// `Bolt`/`Parent` collection fixture through count-changing edits to pin the
// measurement across a structural mutation.
// ---------------------------------------------------------------------------

/// Build a `Bolt`/`Parent` collection fixture where `Bolt.diameter` defaults to
/// `Parent.bolt_d`, so a later `edit_param(bolt_d, …)` dirties EVERY bolt
/// instance (the task-4530 reverse_index rebuild makes grown instances track
/// upstream param edits). `n_default` seeds the initial instance count. The
/// returned engine is NOT yet eval'd — the caller calls `eval` so the count
/// cell and instances exist before registering observed demand.
fn bolt_parent_collection_engine(n_default: i64) -> (CompiledModule, Engine) {
    let bolt = TopologyTemplateBuilder::new("Bolt")
        .param(
            "Bolt",
            "diameter",
            Type::length(),
            Some(value_ref_typed("Parent", "bolt_d", Type::length())),
        )
        .build();

    let parent = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "bolt_d",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.01), Type::length())),
        )
        .param(
            "Parent",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(n_default), Type::Int)),
        )
        .let_binding(
            "Parent",
            "__count_bolts",
            Type::Int,
            value_ref_typed("Parent", "n", Type::Int),
        )
        .structure_controlling_cell(ValueCellId::new("Parent", "__count_bolts"))
        .collection_sub_component("bolts", "Bolt", ValueCellId::new("Parent", "__count_bolts"))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(bolt)
        .build();
    let engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    (module, engine)
}

#[test]
fn structural_grow_with_observed_root_measures_grown_instances() {
    // Coverage for the rebuild-on-structural-mutation branch on a GROW, with the
    // reviewer's three assertions: newly-grown UNregistered instances appear in
    // would_prune, the registered instance is retained, and conservation holds.
    let (module, mut engine) = bolt_parent_collection_engine(2);
    engine.eval(&module);

    // Observe one EXISTING instance (mimics the GUI marking body[0] visible).
    // Backward closure of bolts[0].diameter is {bolts[0].diameter, bolt_d}.
    let bolt0_d = NodeId::Value(vcid("Parent.bolts[0]", "diameter"));
    engine.add_observed_demand(bolt0_d.clone());
    engine.rebuild_observed_cone();
    assert!(
        engine.observed_demand_is_demanded(&bolt0_d),
        "registered instance is in the observed cone before the grow"
    );

    // GROW 2 -> 4. structural_mutation == true AND the observed cone is
    // non-empty, so the branch refreshes the observed cone against the grown
    // graph before recording the measurement.
    engine
        .edit_param(vcid("Parent", "n"), Value::Int(4))
        .expect("grow n 2->4");
    let m_grow = engine
        .last_demand_prune_measurement()
        .expect("measurement recorded on the structural grow edit")
        .clone();
    assert_eq!(
        m_grow.observed_retained + m_grow.would_prune.total(),
        m_grow.eval_set_size,
        "conservation holds across the structural grow edit"
    );
    assert_eq!(
        m_grow.eval_set_size,
        engine.last_eval_set().len(),
        "grow measurement counts the FINAL production eval-set"
    );

    // The registered instance survives the grow and stays in the cone.
    assert!(
        engine.observed_demand_is_demanded(&bolt0_d),
        "registered instance remains demanded after the rebuild against the grown graph"
    );

    // Edit the shared upstream param. The task-4530 reverse_index rebuild makes
    // ALL four instances (incl. the newly-grown bolts[2], bolts[3]) dirty, so
    // last_eval_set carries bolts[0..3].diameter.
    engine
        .edit_param(vcid("Parent", "bolt_d"), Value::length(0.02))
        .expect("edit bolt_d after grow");
    let m = engine
        .last_demand_prune_measurement()
        .expect("measurement recorded on the bolt_d edit")
        .clone();

    // (a) The registered instance bolts[0].diameter is RETAINED.
    assert!(
        m.observed_retained >= 1,
        "registered bolts[0].diameter must be retained (observed_retained >= 1), got {}",
        m.observed_retained
    );

    // (b) The newly-grown, UNregistered instances are NOT in the observed cone,
    //     so they are reported as would_prune. bolts[1], bolts[2], bolts[3] are
    //     all unregistered Value nodes in the eval-set => would_prune.value >= 3.
    let bolt2_d = NodeId::Value(vcid("Parent.bolts[2]", "diameter"));
    let bolt3_d = NodeId::Value(vcid("Parent.bolts[3]", "diameter"));
    assert!(
        !engine.observed_demand_is_demanded(&bolt2_d),
        "grown bolts[2].diameter is not a registered root => not observed-demanded"
    );
    assert!(
        !engine.observed_demand_is_demanded(&bolt3_d),
        "grown bolts[3].diameter is not a registered root => not observed-demanded"
    );
    assert!(
        m.would_prune.value >= 3,
        "the unregistered (incl. newly-grown) instances must appear in would_prune.value, got {}",
        m.would_prune.value
    );

    // (c) Conservation still holds against the post-grow eval-set.
    assert_eq!(
        m.observed_retained + m.would_prune.total(),
        m.eval_set_size,
        "conservation holds on the post-grow upstream edit"
    );
    assert_eq!(
        m.eval_set_size,
        engine.last_eval_set().len(),
        "measurement counts the FINAL production eval-set"
    );
}

#[test]
fn structural_shrink_rebuilds_observed_cone_against_shrunk_graph() {
    // Tight regression guard: the rebuild-on-structural-mutation branch must run
    // against the MUTATED graph, not a stale pre-mutation one. Register an
    // instance that a later shrink REMOVES; after the shrink that root is
    // dangling (absent from the graph) and so transitively demands nothing — in
    // particular `bolt_d` drops out of the observed cone. Without the rebuild the
    // cone stays stale and keeps claiming bolt_d is observed-demanded, so this
    // test fails if the branch is removed.
    let (module, mut engine) = bolt_parent_collection_engine(4);
    engine.eval(&module);

    let bolt3_d = NodeId::Value(vcid("Parent.bolts[3]", "diameter"));
    let bolt_d = NodeId::Value(vcid("Parent", "bolt_d"));
    engine.add_observed_demand(bolt3_d.clone());
    engine.rebuild_observed_cone();
    // Backward closure: bolts[3].diameter reads bolt_d => cone == {both}.
    assert_eq!(
        engine.observed_demand_cone_size(),
        2,
        "pre-shrink observed cone is {{bolts[3].diameter, bolt_d}}"
    );
    assert!(
        engine.observed_demand_is_demanded(&bolt_d),
        "bolt_d is reachable from bolts[3].diameter before the shrink"
    );

    // SHRINK 4 -> 2: bolts[2], bolts[3] are removed from the graph. The branch
    // rebuilds the observed cone against the SHRUNK graph; bolts[3].diameter is
    // now a dangling registered root with an empty dependency set.
    engine
        .edit_param(vcid("Parent", "n"), Value::Int(2))
        .expect("shrink n 4->2");

    assert!(
        engine.observed_demand_is_demanded(&bolt3_d),
        "the dangling registered root is still itself in the cone"
    );
    assert!(
        !engine.observed_demand_is_demanded(&bolt_d),
        "bolt_d must drop out: the rebuild ran against the SHRUNK graph \
         (without the structural-mutation rebuild this would stay stale-true)"
    );
    assert_eq!(
        engine.observed_demand_cone_size(),
        1,
        "post-shrink observed cone holds only the dangling root"
    );

    // The measurement is still recorded and conserved on the structural edit.
    let m = engine
        .last_demand_prune_measurement()
        .expect("measurement recorded on the structural shrink edit");
    assert_eq!(
        m.observed_retained + m.would_prune.total(),
        m.eval_set_size,
        "conservation holds across the structural shrink edit"
    );
    assert_eq!(
        m.eval_set_size,
        engine.last_eval_set().len(),
        "shrink measurement counts the FINAL production eval-set"
    );
}
