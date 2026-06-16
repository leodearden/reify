//! Engine-level observed-demand API (selective-demand precondition, task 4532).
//!
//! The observed-demand registry is a PASSIVE side-channel: it mirrors
//! `DemandRegistry`'s cone semantics but is NEVER fed into `compute_eval_set`.
//! These tests pin (a) the registry's add/remove/rebuild/reset/inspect API and
//! (b) the zero-behavior-change contract — registering observed demand must not
//! change the production eval-set produced by `edit_param`.

use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::WouldPruneByKind;
use reify_ir::Value;
// `sorted_values` / `bracket_engine` are shared from reify-test-support (a
// single definition shared with `selective_demand_measurement.rs`) so the
// byte-identity comparison logic cannot drift between the two test files.
use reify_test_support::{bracket_engine, cnid, sorted_values, vcid};

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
    // R0(Realization)} (dirty.rs::dirty_cone_bracket_change_thickness); since
    // production demand is total, eval_set is exactly those 5.
    engine
        .edit_param(ValueCellId::new("Bracket", "thickness"), Value::length(0.004))
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
    assert_eq!(m.eval_set_size, 5, "eval_set = {{volume,C0,C1,C2,R0}}");

    // Only C0 of the eval-set is in the observed cone {C0, thickness}.
    assert_eq!(m.observed_retained, 1, "C0 retained");

    // The other four would be pruned, split by kind.
    assert_eq!(
        m.would_prune,
        WouldPruneByKind {
            value: 1,       // volume
            constraint: 2,  // C1, C2
            realization: 1, // R0
            resolution: 0,
            compute: 0,
        },
        "would-prune split: volume / C1,C2 / R0"
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
            // thickness}; the thickness edit's eval-set {volume,C0,C1,C2,R0}
            // retains R0 and would-prune the rest.
            assert!(
                mb.would_prune.total() > 0,
                "edit {i}: thickness edit must show real pruning"
            );
            assert_eq!(
                mb.would_prune.realization, 0,
                "edit {i}: R0 is registered/retained, never pruned"
            );
            assert!(
                mb.observed_retained >= 1,
                "edit {i}: at least R0 retained"
            );
            saw_real_pruning = true;
        }
    }

    assert!(
        saw_real_pruning,
        "the scripted session must include the thickness edit"
    );
}
