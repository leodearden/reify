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
