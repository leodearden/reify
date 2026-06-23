//! α (task 4737) selective-demand integration tests.
//!
//! These pin the PRODUCTION-demand population API on `Engine`: promoting the 4532
//! observed-demand side-channel from a passive measurement into a registry that
//! actually drives `compute_eval_set` (`self.demand`) selectively from the set of
//! viewport-visible `Realization` roots — while a cold `eval()`/`check()`/`build()`
//! keeps the deterministic full scope (the `full_scope` override).
//!
//! step-3 (this file's first test) covers selective POPULATION; step-5 adds the
//! hidden-body + cold-override signal, and step-7 the all-visible byte-identity
//! differential (which reuses the `common/differential.rs` harness — hence the
//! `#[path]` include below).
//!
//! Fixture: [`SELECTIVE_DEMAND_MULTIBODY_SRC`] — a constraint-free, param-driven
//! two-body source (`param w` → `sa = w*3` → box `a`; `sb = w*2` → box `b`) whose
//! dependency spine is `a → sa → w` and `b → sb → w`, so every value cell sits in
//! some realization's backward closure.

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::Engine;
use reify_test_support::{compile_source, MockGeometryKernel};

/// step-3 (RED until step-4): `set_demand_selective` populates the PRODUCTION
/// demand registry from a single visible realization root.
///
/// Cold-eval the two-body fixture, then demand ONLY `body_a` (treat `body_b` as
/// hidden). The selective production cone must be exactly `body_a`'s backward
/// closure `{Realization(a), sa, w}` — strictly under the all-visible cone — with
/// `body_b`'s exclusive value cell `sb` pruned out and the full-scope override OFF.
#[test]
fn set_demand_selective_populates_production_demand_from_one_visible_realization() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    // Cold eval populates eval_state (the snapshot graph the cone rebuilds over).
    engine.eval(&compiled);

    // Realization roots in source order: `let a = box(..)` → realization[0],
    // `let b = box(..)` → realization[1] (verified against the GeometryHandle
    // `realization_ref`s the cold eval mints for cells `a`/`b`).
    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));

    // ALL-VISIBLE baseline: demanding BOTH realizations covers every value cell
    // ({a→sa→w} ∪ {b→sb→w} ⇒ cone {R0,R1,sa,sb,w}). This is the "full" cone the
    // single-body selective cone must come in UNDER.
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    let full_cone = engine.demand_cone_size();

    // SELECTIVE: only `body_a` visible — `body_b` hidden.
    engine.set_demand_selective([body_a.clone()]);

    // `body_a`'s backward closure is exactly {Realization(a), sa, w} = 3 nodes …
    assert_eq!(
        engine.demand_cone_size(),
        3,
        "selective cone must equal body_a's backward closure {{R(a), sa, w}}"
    );
    // … and strictly smaller than the all-visible cone (selectivity is real).
    assert!(
        engine.demand_cone_size() < full_cone,
        "selective cone ({}) must be < the all-visible cone ({full_cone})",
        engine.demand_cone_size()
    );

    // `body_a`'s realization IS demanded; the hidden `body_b`'s is NOT.
    assert!(
        engine.demand_is_demanded(&body_a),
        "visible body_a's realization must be demanded"
    );
    assert!(
        !engine.demand_is_demanded(&body_b),
        "hidden body_b's realization must NOT be demanded"
    );

    // `sa` (read by body_a) is demanded; `sb` (read ONLY by hidden body_b) is pruned.
    assert!(
        engine.demand_is_demanded(&NodeId::Value(ValueCellId::new(e, "sa"))),
        "`sa` feeds visible body_a ⇒ demanded"
    );
    assert!(
        !engine.demand_is_demanded(&NodeId::Value(ValueCellId::new(e, "sb"))),
        "`sb` feeds ONLY hidden body_b ⇒ pruned from the selective cone"
    );

    // Selective population must NOT trip the cold full-scope override.
    assert!(
        !engine.demand_is_full_scope(),
        "selective demand must leave the cold full-scope override OFF"
    );
}

/// step-5 (RED until step-6): the COLD full-scope override.
///
/// A selectively-populated production demand (one visible body, the other hidden)
/// must be OVERRIDDEN to FULL scope by a cold `check()`/`eval()`/`build()` — so CI
/// and the deterministic build path always evaluate the WHOLE graph, INCLUDING the
/// hidden body, regardless of whatever the GUI's current viewport selection is.
///
/// Per PRD D2 the override is a flag flip, NOT a registry clobber: the selective
/// roots survive underneath (only `full_scope` flips on), so a later GUI re-sync
/// can restore selectivity without re-deriving the selection.
///
/// Cold-eval the two-body fixture, demand ONLY `body_a` (hide `body_b`), confirm the
/// selective cone is strictly under the all-visible total and `body_b`'s exclusive
/// cell `sb` is pruned, then run a cold `check()` and assert it flipped the
/// production demand to full scope, re-demanded the hidden body, and re-evaluated
/// `sb` (the whole graph). RED until the cold-eval demand-seed site flips
/// `full_scope` on instead of leaving the registry selective.
#[test]
fn cold_check_overrides_selective_demand_to_full_scope() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    // Cold eval populates eval_state (the snapshot graph the cone rebuilds over).
    engine.eval(&compiled);

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));
    let sb = NodeId::Value(ValueCellId::new(e, "sb"));

    // ALL-VISIBLE baseline: demanding BOTH realizations covers every value cell ⇒
    // the "total" the single-body selective cone must come in UNDER.
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    let total = engine.demand_cone_size();

    // SELECTIVE: only `body_a` visible — `body_b` hidden.
    engine.set_demand_selective([body_a.clone()]);
    assert!(
        engine.demand_cone_size() < total,
        "selective cone ({}) must be < the all-visible total ({total})",
        engine.demand_cone_size()
    );
    assert!(
        !engine.demand_is_demanded(&sb),
        "`sb` (read ONLY by hidden body_b) must be pruned from the selective cone"
    );
    assert!(
        !engine.demand_is_full_scope(),
        "selective demand must leave the cold full-scope override OFF before the cold pass"
    );

    // COLD check() — the deterministic CI / build scope. It must OVERRIDE the
    // selective demand to FULL scope (the signal this RED test pins, GREEN at step-6).
    let check_result = engine.check(&compiled);

    // (1) The cold override is ON: every node is demanded regardless of the cone.
    assert!(
        engine.demand_is_full_scope(),
        "a cold check() must flip the production demand to FULL scope (the cold override)"
    );
    // (2) The previously-hidden body is re-demanded under full scope …
    assert!(
        engine.demand_is_demanded(&body_b),
        "under the cold full-scope override the hidden body_b's realization must be demanded again"
    );
    assert!(
        engine.demand_is_demanded(&sb),
        "under full scope body_b's exclusive cell `sb` must be demanded again"
    );
    // (3) … and the cold path actually EVALUATED the whole graph: body_b's
    // exclusive `sb` is present (fresh) in the check result, not pruned away.
    assert!(
        check_result.values.get(&ValueCellId::new(e, "sb")).is_some(),
        "cold check() must evaluate the whole graph — hidden body_b's `sb` must be present in the result"
    );
}
