// Integration tests for F-inherit γ (#4824): §10.5 objective inheritance
// end-to-end + provenance.
//
// RED/GREEN cycles:
//   Cycle 1 (step-1/step-2): back-compat + field introduction — BT1/BT2/BT7.
//     References `prov.inherited_from` (does not exist yet) → compile-error RED.
//     After step-2 adds the field (None everywhere), BT1/BT2/BT7 pass.
//   Cycle 2 (step-3/step-4): inheritance signal — BT5/BT6.
//     BT5 fails until step-4 wires ContainmentIndex into the solve loop.
//
// All tests assert GOVERNANCE/PROVENANCE only — never a child-cell numeric
// optimum (PRD §3.2 honesty boundary / esc-3436-210 trap).

use reify_constraints::DimensionalSolver;
use reify_core::{ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ObjectiveSense, ObjectiveSet};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, ge, gt, le, literal, mm,
    value_ref,
};

fn solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver))
}

// ---------------------------------------------------------------------------
// BT1 — explicit own objective: inherited_from=None, synthetic_centrality=false
// ---------------------------------------------------------------------------

/// (BT1) A single scope with its own `minimize` objective: provenance must show
/// `objective.is_some()`, `synthetic_centrality=false`, `inherited_from=None`.
///
/// RED until step-1 (field `inherited_from` does not exist).
/// GREEN after step-2 (field added with default None; no wiring needed for BT1).
#[test]
fn bt1_own_objective_inherited_from_is_none() {
    let a_v = ValueCellId::new("A", "v");

    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "v", Type::length())
        .constraint("A", 0, None, gt(value_ref("A", "v"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("A", "v"),
        ))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .build();

    let mut engine = solver_engine();
    let result = engine.eval(&module);

    let prov = result
        .objective_provenance
        .get(&a_v)
        .expect("no ObjectiveProvenance for A.v");

    assert!(
        prov.objective.is_some(),
        "BT1: explicit objective must be Some; got None"
    );
    assert!(
        !prov.synthetic_centrality,
        "BT1: synthetic_centrality must be false for explicit objective"
    );
    assert!(
        prov.inherited_from.is_none(),
        "BT1: own-objective scope must have inherited_from=None; got {:?}",
        prov.inherited_from
    );
}

// ---------------------------------------------------------------------------
// BT7 — synthetic-centrality: synthetic_centrality=true, inherited_from=None
// ---------------------------------------------------------------------------

/// (BT7) An objective-less scope with a two-sided auto param: the solver
/// synthesises the Chebyshev-centre objective. Provenance must show
/// `synthetic_centrality=true` and `inherited_from=None`.
///
/// RED until step-1. GREEN after step-2.
#[test]
fn bt7_centrality_inherited_from_is_none() {
    let k_id = ValueCellId::new("Bar", "k");

    // Two-sided inequality: 2mm <= k <= 8mm, no objective → centrality.
    let bar = TopologyTemplateBuilder::new("Bar")
        .auto_param("Bar", "k", Type::length())
        .constraint("Bar", 0, None, ge(value_ref("Bar", "k"), literal(mm(2.0))))
        .constraint("Bar", 1, None, le(value_ref("Bar", "k"), literal(mm(8.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(bar)
        .build();

    let mut engine = solver_engine();
    let result = engine.eval(&module);

    let prov = result
        .objective_provenance
        .get(&k_id)
        .expect("no ObjectiveProvenance for Bar.k");

    assert!(
        prov.synthetic_centrality,
        "BT7: synthetic_centrality must be true for centrality scope"
    );
    assert!(
        prov.inherited_from.is_none(),
        "BT7: standalone centrality scope must have inherited_from=None; got {:?}",
        prov.inherited_from
    );
}

// ---------------------------------------------------------------------------
// BT2 — cross-declaration-order value equality (INV-2 identity)
// ---------------------------------------------------------------------------

/// (BT2) Two UNCOUPLED scopes (no cross-scope reads, no containment) built in
/// two declaration orders [A, B] and [B, A]. Each scope's resolved auto-cell
/// value must be EQUAL across the two orderings (INV-2: inheritance is a no-op
/// for container-less scopes). Also asserts inherited_from=None for both.
///
/// RED until step-1 (references inherited_from). GREEN after step-2.
#[test]
fn bt2_uncoupled_scopes_cross_order_value_equality() {
    let a_val = ValueCellId::new("A", "val");
    let b_val = ValueCellId::new("B", "val");

    let a = || {
        TopologyTemplateBuilder::new("A")
            .auto_param("A", "val", Type::length())
            .constraint("A", 0, None, gt(value_ref("A", "val"), literal(mm(1.0))))
            .build()
    };
    let b = || {
        TopologyTemplateBuilder::new("B")
            .auto_param("B", "val", Type::length())
            .constraint("B", 0, None, gt(value_ref("B", "val"), literal(mm(2.0))))
            .build()
    };

    // Order 1: [A, B]
    let module_ab = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a())
        .template(b())
        .build();

    // Order 2: [B, A]
    let module_ba = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(b())
        .template(a())
        .build();

    let result_ab = solver_engine().eval(&module_ab);
    let result_ba = solver_engine().eval(&module_ba);

    // Value equality across orderings (INV-2).
    let a_val_ab = result_ab
        .values
        .get(&a_val)
        .unwrap_or_else(|| panic!("A.val not resolved in [A,B] ordering"));
    let a_val_ba = result_ba
        .values
        .get(&a_val)
        .unwrap_or_else(|| panic!("A.val not resolved in [B,A] ordering"));
    assert_eq!(
        a_val_ab, a_val_ba,
        "BT2 INV-2: A.val must be equal regardless of declaration order"
    );

    let b_val_ab = result_ab
        .values
        .get(&b_val)
        .unwrap_or_else(|| panic!("B.val not resolved in [A,B] ordering"));
    let b_val_ba = result_ba
        .values
        .get(&b_val)
        .unwrap_or_else(|| panic!("B.val not resolved in [B,A] ordering"));
    assert_eq!(
        b_val_ab, b_val_ba,
        "BT2 INV-2: B.val must be equal regardless of declaration order"
    );

    // inherited_from=None for uncoupled scopes in both orderings.
    for (label, result) in [("[A,B]", &result_ab), ("[B,A]", &result_ba)] {
        let prov_a = result
            .objective_provenance
            .get(&a_val)
            .unwrap_or_else(|| panic!("no prov for A.val in {label}"));
        assert!(
            prov_a.inherited_from.is_none(),
            "BT2: A.val inherited_from must be None in {label}; got {:?}",
            prov_a.inherited_from
        );

        let prov_b = result
            .objective_provenance
            .get(&b_val)
            .unwrap_or_else(|| panic!("no prov for B.val in {label}"));
        assert!(
            prov_b.inherited_from.is_none(),
            "BT2: B.val inherited_from must be None in {label}; got {:?}",
            prov_b.inherited_from
        );
    }
}

// ---------------------------------------------------------------------------
// BT5 — child inherits parent objective (INV-3/INV-4)
// ---------------------------------------------------------------------------

/// (BT5) An objective-less child template C (auto param `k`, two-sided
/// inequality) contained by exactly one parent P (auto param `w`, one-sided
/// constraint, minimize objective). C would otherwise qualify for centrality.
///
/// After step-4:
///   prov[C.k].inherited_from == Some("P")  (inheritance wired)
///   prov[C.k].synthetic_centrality == false  (centrality suppressed, INV-3)
///
/// RED until step-4: currently C falls through to centrality
/// (synthetic_centrality=true, inherited_from=None).
///
/// Degenerate inherited objective per §3.2 honesty boundary: P's minimize term
/// uses `literal(mm(1.0))` — a constant expression, zero gradient w.r.t. C.k.
/// The test asserts provenance, NEVER a numeric optimum.
#[test]
fn bt5_child_inherits_parent_objective() {
    let c_k = ValueCellId::new("C", "k");
    let p_w = ValueCellId::new("P", "w");

    // P: source order index 0 (solved first), own minimize, sub c:C
    // Degenerate objective: constant literal so the inherited term is zero-gradient
    // w.r.t. C.k (§3.2 — never assert a child optimum).
    let p = TopologyTemplateBuilder::new("P")
        .auto_param("P", "w", Type::length())
        .constraint("P", 0, None, gt(value_ref("P", "w"), literal(mm(1.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            literal(mm(1.0)), // degenerate: constant, zero-gradient w.r.t. C.k
        ))
        .sub_component("c", "C", vec![])
        .build();

    // C: source order index 1, two-sided inequality, NO objective → would be centrality
    let c = TopologyTemplateBuilder::new("C")
        .auto_param("C", "k", Type::length())
        .constraint("C", 0, None, ge(value_ref("C", "k"), literal(mm(2.0))))
        .constraint("C", 1, None, le(value_ref("C", "k"), literal(mm(8.0))))
        .build();

    // P at index 0, C at index 1 → solve order [P, C] (no cross-reads)
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(p)
        .template(c)
        .build();

    let mut engine = solver_engine();
    let result = engine.eval(&module);

    // P.w must still carry its own explicit objective (unaffected by γ)
    let prov_p = result
        .objective_provenance
        .get(&p_w)
        .expect("no ObjectiveProvenance for P.w");
    assert!(
        prov_p.inherited_from.is_none(),
        "BT5: P.w (own objective) must have inherited_from=None; got {:?}",
        prov_p.inherited_from
    );
    assert!(
        !prov_p.synthetic_centrality,
        "BT5: P.w synthetic_centrality must be false"
    );

    // C.k must inherit P's objective — centrality suppressed
    let prov_c = result
        .objective_provenance
        .get(&c_k)
        .expect("no ObjectiveProvenance for C.k — solver may not have resolved C.k");

    assert_eq!(
        prov_c.inherited_from.as_deref(),
        Some("P"),
        "BT5: C.k must inherit from P (INV-4); got {:?}",
        prov_c.inherited_from
    );
    assert!(
        !prov_c.synthetic_centrality,
        "BT5: C.k synthetic_centrality must be false (centrality suppressed, INV-3)"
    );
}

// ---------------------------------------------------------------------------
// BT6 — child with own objective wins over parent (INV-3 narrowest-scope-wins)
// ---------------------------------------------------------------------------

/// (BT6) Child C has its OWN objective under objective-bearing parent P.
/// C's own objective takes precedence (narrowest-scope-wins, §6.1 INV-3):
///   prov[C.k].objective.is_some()
///   prov[C.k].inherited_from == None  (own objective, not inherited)
///
/// GREEN after step-2 even before step-4 — template.objective governs when
/// present, so no inheritance lookup occurs.
#[test]
fn bt6_own_objective_wins_over_parent() {
    let c_k = ValueCellId::new("C", "k");

    let p = TopologyTemplateBuilder::new("P")
        .auto_param("P", "w", Type::length())
        .constraint("P", 0, None, gt(value_ref("P", "w"), literal(mm(1.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            literal(mm(1.0)),
        ))
        .sub_component("c", "C", vec![])
        .build();

    // C has its OWN minimize objective — must win over parent P's
    let c = TopologyTemplateBuilder::new("C")
        .auto_param("C", "k", Type::length())
        .constraint("C", 0, None, gt(value_ref("C", "k"), literal(mm(0.5))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("C", "k"),
        ))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(p)
        .template(c)
        .build();

    let mut engine = solver_engine();
    let result = engine.eval(&module);

    let prov_c = result
        .objective_provenance
        .get(&c_k)
        .expect("no ObjectiveProvenance for C.k");

    assert!(
        prov_c.objective.is_some(),
        "BT6: C must govern with its own objective; got None"
    );
    assert!(
        prov_c.inherited_from.is_none(),
        "BT6: own-objective child must have inherited_from=None; got {:?}",
        prov_c.inherited_from
    );
    assert!(
        !prov_c.synthetic_centrality,
        "BT6: C.k synthetic_centrality must be false when C has own objective"
    );
}
