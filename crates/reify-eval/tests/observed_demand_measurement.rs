//! Engine-level observed-demand API (selective-demand precondition, task 4532).
//!
//! The observed-demand registry is a PASSIVE side-channel: it mirrors
//! `DemandRegistry`'s cone semantics but is NEVER fed into `compute_eval_set`.
//! These tests pin (a) the registry's add/remove/rebuild/reset/inspect API and
//! (b) the zero-behavior-change contract — registering observed demand must not
//! change the production eval-set produced by `edit_param`.

use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{bracket_compiled_module, cnid, vcid};

/// Build a freshly-eval'd bracket engine.
fn bracket_engine() -> Engine {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module);
    engine
}

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
