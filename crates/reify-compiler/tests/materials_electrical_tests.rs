//! Tests for stdlib/materials_electrical.ri — §6.4 electrical material traits.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `ElectricallyCharacterized`, `Conductive`, and `Insulating` are
//! correctly represented in the compiled module, and that trait conformance
//! and constraint injection work as expected.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read).

mod common;

use common::assert_trait_constraint_binop;
use reify_ast::{ExprKind, UnitExpr};
use reify_ir::*;
use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/electrical` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — the expected failure mode until step-4
/// lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/electrical")
        .expect("stdlib should contain std/materials/electrical module")
}

// ─── (a) module loads with three trait defs and zero errors ──────────────────

/// The std/materials/electrical module must load with zero error-severity
/// diagnostics and contain exactly three trait definitions:
/// ElectricallyCharacterized, Conductive, Insulating.
#[test]
fn electrical_module_loads_with_no_errors_and_three_traits() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_electrical.ri: {:?}",
        errors
    );

    assert_eq!(
        module.trait_defs.len(),
        3,
        "expected exactly 3 trait defs in std/materials/electrical, got: {:?}",
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── (b) ElectricallyCharacterized: one required + three optional params ─────

/// ElectricallyCharacterized must refine MaterialSpec and declare exactly one
/// required member (`resistivity`) plus three optional params in `defaults`
/// (task δ — `= undef` added to dielectric_constant, dielectric_strength, and
/// magnetic_permeability by task #3918-enabled optionality).
///
/// Contract after task δ:
///   required_members (len 1):
///     resistivity → RequirementKind::Param(Type::Scalar { ELECTRIC_RESISTIVITY })
///   defaults (at least the three optional params):
///     dielectric_constant   → DefaultKind::Param { cell_type: Type::Real }
///     dielectric_strength   → DefaultKind::Param { cell_type: Type::Scalar { DIELECTRIC_STRENGTH } }
///     magnetic_permeability → DefaultKind::Param { cell_type: Type::Real }
#[test]
fn electrically_characterized_one_required_three_optional_params() {
    let module = load_stdlib_module();

    let ec = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElectricallyCharacterized")
        .expect("expected 'ElectricallyCharacterized' trait in std/materials/electrical");

    assert!(
        ec.refinements.contains(&"MaterialSpec".to_string()),
        "ElectricallyCharacterized must refine MaterialSpec, got refinements: {:?}",
        ec.refinements
    );

    // ── required_members: exactly resistivity (len 1 after task δ) ───────────
    assert_eq!(
        ec.required_members.len(),
        1,
        "ElectricallyCharacterized should have exactly 1 required member after \
         task δ (dielectric_constant/strength/permeability are now optional), \
         got: {:?}",
        ec.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let resistivity_req = ec
        .required_members
        .iter()
        .find(|r| r.name == "resistivity")
        .expect("ElectricallyCharacterized must have 'resistivity' as required member");
    match &resistivity_req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::ELECTRIC_RESISTIVITY,
            },
            "resistivity required member must have type Scalar{{ELECTRIC_RESISTIVITY}}, \
             got {:?}",
            ty
        ),
        other => panic!(
            "resistivity required member must be RequirementKind::Param, got {:?}",
            other
        ),
    }

    // ── defaults: three optional params (DefaultKind::Param) ─────────────────
    let expected_defaults: [(&str, Type); 3] = [
        ("dielectric_constant", Type::Real),
        (
            "dielectric_strength",
            Type::Scalar {
                dimension: DimensionVector::DIELECTRIC_STRENGTH,
            },
        ),
        ("magnetic_permeability", Type::Real),
    ];

    for (name, expected_cell_type) in &expected_defaults {
        let default = ec
            .defaults
            .iter()
            .find(|d| {
                matches!(&d.kind, DefaultKind::Param { .. })
                    && d.name.as_deref() == Some(*name)
            })
            .unwrap_or_else(|| {
                panic!(
                    "ElectricallyCharacterized must have optional param '{}' in defaults, \
                     got defaults: {:?}",
                    name,
                    ec.defaults
                        .iter()
                        .map(|d| &d.name)
                        .collect::<Vec<_>>()
                )
            });
        match &default.kind {
            DefaultKind::Param { cell_type, .. } => assert_eq!(
                cell_type, expected_cell_type,
                "ElectricallyCharacterized optional param '{}' expected cell_type {:?}, \
                 got {:?}",
                name, expected_cell_type, cell_type
            ),
            other => panic!(
                "ElectricallyCharacterized optional param '{}' must be DefaultKind::Param, \
                 got {:?}",
                name, other
            ),
        }
    }
}

// ─── (c) Conductive refines ElectricallyCharacterized with resistivity < 1e-4 ─

/// Conductive must refine ElectricallyCharacterized and carry a constraint
/// `resistivity < 1.0e-4` — verified at the BinOp expression level so that
/// a regression flipping the op or changing the bound is caught here, not just
/// at the eval-level satisfaction check.
#[test]
fn conductive_refines_electrically_characterized_with_constraint() {
    let module = load_stdlib_module();

    let conductive = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Conductive")
        .expect("expected 'Conductive' trait in std/materials/electrical");

    assert!(
        conductive
            .refinements
            .contains(&"ElectricallyCharacterized".to_string()),
        "Conductive must refine ElectricallyCharacterized, got refinements: {:?}",
        conductive.refinements
    );

    // BinOp-level check: op="<", LHS=resistivity, RHS≈1.0e-4
    assert_trait_constraint_binop(
        conductive,
        "Conductive",
        "resistivity",
        "<",
        1.0e-4,
        1.0e-16,
    );
}

// ─── (d) Insulating refines ElectricallyCharacterized with resistivity > 1e6 ─

/// Insulating must refine ElectricallyCharacterized and carry a constraint
/// `resistivity > 1.0e6` — verified at the BinOp expression level.
/// Note: the spec's `determined(dielectric_strength)` predicate is dropped —
/// see header comment in materials_electrical.ri for rationale.
#[test]
fn insulating_refines_electrically_characterized_with_constraint() {
    let module = load_stdlib_module();

    let insulating = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Insulating")
        .expect("expected 'Insulating' trait in std/materials/electrical");

    assert!(
        insulating
            .refinements
            .contains(&"ElectricallyCharacterized".to_string()),
        "Insulating must refine ElectricallyCharacterized, got refinements: {:?}",
        insulating.refinements
    );

    // BinOp-level check: op=">", LHS=resistivity, RHS≈1.0e6
    assert_trait_constraint_binop(insulating, "Insulating", "resistivity", ">", 1.0e6, 1.0);
}

// ─── (d2) Insulating has dielectric_strength > 0 constraint ─────────────────

/// Insulating must carry a `dielectric_strength > 0.0` physical-validity
/// bound: zero breakdown field is degenerate for an insulator. This is the
/// most direct bound expressible in the current grammar (no `determined()`
/// form available); see Decision #3 in materials_electrical.ri.
#[test]
fn insulating_has_dielectric_strength_positive_constraint() {
    let module = load_stdlib_module();

    let insulating = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Insulating")
        .expect("expected 'Insulating' trait in std/materials/electrical");

    // BinOp-level check: op=">", LHS=dielectric_strength, RHS=0.0 (exact)
    assert_trait_constraint_binop(
        insulating,
        "Insulating",
        "dielectric_strength",
        ">",
        0.0,
        0.0,
    );
}

// ─── (e) Copper : Conductive conformance test with inherited constraint ────────

/// A structure conforming to Conductive must compile cleanly via the full
/// stdlib pipeline, carry Conductive as a trait bound, and have the inherited
/// resistivity constraint injected into template.constraints.
#[test]
fn copper_conforms_to_conductive_with_constraint_injection() {
    // resistivity = 0.000000017 (1.7e-8 Ω·m) — clears the < 1e-4 Conductive constraint.
    // Avoids scientific notation with negative exponent which the parser mishandles.
    let source = r#"
structure def Copper : Conductive {
    param density : Real = 8960.0
    param name : String = "copper"
    param resistivity : ElectricResistivity = 0.000000017 * 1ohm * 1m
    param dielectric_constant : Real = 1.0
    param dielectric_strength : DielectricStrength = 0.0 * 1V / 1m
    param magnetic_permeability : Real = 1.0
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Copper : Conductive should compile cleanly, got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Copper")
        .expect("expected 'Copper' template in compiled module");

    assert!(
        template.trait_bounds.contains(&"Conductive".to_string()),
        "Copper must have 'Conductive' trait bound, got: {:?}",
        template.trait_bounds
    );

    // The inherited resistivity < 1e-4 constraint must be injected.
    assert!(
        !template.constraints.is_empty(),
        "Copper template must have injected constraints from Conductive trait"
    );

    // Verify the injected constraint is `resistivity < 0.0001 * 1ohm * 1m`:
    // the `Lt` operator, LHS `ValueRef(resistivity)`, AND a dimensioned RHS
    // (Mul/Div unit chain typed `Scalar { dimension: ELECTRIC_RESISTIVITY }`).
    // The dimensioned-RHS pin is the contract esc-3115-112 enforces — a bare
    // numeric RHS would make runtime `eval_cmp` dim-equality Indeterminate
    // against the dimensioned LHS member. (The earlier check only asserted
    // *some* BinOp referenced resistivity, leaving op and RHS-dimension
    // unverified — the same blind spot closed in the Glass test below.)
    let resistivity_constraint = template.constraints.iter().find(|cc| {
        if let CompiledExprKind::BinOp { op: BinOp::Lt, left, right } = &cc.expr.kind {
            let left_match = matches!(
                &left.kind,
                CompiledExprKind::ValueRef(id) if id.member == "resistivity"
            );
            // shape_match is defense-in-depth: it is logically subsumed by
            // dim_match (a bare Real RHS can never carry an ELECTRIC_RESISTIVITY
            // result_type), but kept deliberately as an explicit shape check so
            // the expected RHS structure is self-documenting at the assertion
            // site. dim_match is the load-bearing contract. Post-task-ζ the
            // migrated Conductive bound `0.0001ohm*m` folds to a single
            // Literal(Scalar); the legacy `0.0001 * 1ohm * 1m` form was a Mul/Div
            // chain — accept either shape.
            let shape_match = matches!(
                &right.kind,
                CompiledExprKind::BinOp { op: BinOp::Mul | BinOp::Div, .. }
                    | CompiledExprKind::Literal(Value::Scalar { .. })
            );
            let dim_match = right.result_type
                == Type::Scalar {
                    dimension: DimensionVector::ELECTRIC_RESISTIVITY,
                };
            left_match && shape_match && dim_match
        } else {
            false
        }
    });
    assert!(
        resistivity_constraint.is_some(),
        "expected dimensioned constraint `resistivity < 0.0001ohm*m` in Copper \
         template — Lt with RHS typed Scalar{{ dimension: ELECTRIC_RESISTIVITY }}, not \
         bare Real; got constraints: {:?}",
        template.constraints
    );
}

// ─── (f) Glass : Insulating conformance test with inherited constraints ────────

/// A structure conforming to Insulating must compile cleanly via the full
/// stdlib pipeline, carry Insulating as a trait bound, and have the inherited
/// `dielectric_strength > 0.0` constraint injected into template.constraints.
///
/// # Deferred: negative eval-level test
///
/// A test asserting that `Glass { dielectric_strength = 0.0 }` produces a
/// constraint-violation diagnostic cannot be written at this layer. The
/// compiler injects constraints structurally (see entity.rs,
/// `MemberDecl::ConstraintInst` handler) but does not evaluate them against
/// literal values at compile time. Constraint satisfaction is enforced by
/// the runtime evaluator/solver. A runtime-level negative test should be
/// added once that evaluation layer exists.
#[test]
fn glass_conforms_to_insulating_with_constraint_injection() {
    // resistivity = 1_000_000_000.0 (1e9 Ω·m, typical glass) — clears the > 1e6
    // Insulating constraint.  dielectric_strength = 10_000_000.0 (1e7 V/m,
    // typical soda-lime glass) — clears the > 0.0 bound.
    // Decimal form avoids the parser's scientific-notation edge cases.
    let source = r#"
structure def Glass : Insulating {
    param density : Real = 2500.0
    param name : String = "glass"
    param resistivity : ElectricResistivity = 1000000000.0 * 1ohm * 1m
    param dielectric_constant : Real = 7.0
    param dielectric_strength : DielectricStrength = 10000000.0 * 1V / 1m
    param magnetic_permeability : Real = 1.0
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Glass : Insulating should compile cleanly, got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Glass")
        .expect("expected 'Glass' template in compiled module");

    assert!(
        template.trait_bounds.contains(&"Insulating".to_string()),
        "Glass must have 'Insulating' trait bound, got: {:?}",
        template.trait_bounds
    );

    // Both inherited constraints must be injected (resistivity > 1e6 and
    // dielectric_strength > 0.0).
    assert!(
        !template.constraints.is_empty(),
        "Glass template must have injected constraints from Insulating trait"
    );

    // Helper: assert a `member > rhs_real * <unit-chain>` BinOp constraint is
    // injected, pinning THREE independent properties so the test fails if any
    // regresses:
    //
    //  1. operator + LHS — the injected constraint is `Gt` with LHS
    //     `ValueRef(member)`;
    //  2. numeric magnitude — the leading coefficient on the RHS spine matches
    //     `rhs_real` (guards the bound value);
    //  3. dimensioned RHS — the RHS is a `Mul`/`Div` unit chain whose inferred
    //     `result_type` is `Scalar { dimension: expected_dim }`, NOT bare
    //     `Real`.
    //
    // Property (3) is the contract esc-3115-112 exists to enforce: a
    // bare-numeric trait-constraint RHS would make runtime `eval_cmp`
    // dim-equality return Indeterminate against the now-dimensioned LHS member.
    // The earlier version only walked the left spine for a numeric literal, so
    // it passed whether or not the stdlib RHS literal was dimensioned — that
    // blind spot is closed by the `result_type` / Mul-Div-root assertions.
    //
    // Decimal-form source tokens (e.g. `1000000.0`, `0.0`) compile to
    // Value::Real after task 3184 added the int-vs-real syntactic distinction;
    // they sit at the head of the RHS spine, so walk the left spine to find
    // the leading coefficient.
    fn rhs_coefficient(expr: &reify_ir::expr::CompiledExpr) -> Option<f64> {
        let mut cursor = expr;
        loop {
            match &cursor.kind {
                CompiledExprKind::Literal(Value::Real(v)) => return Some(*v),
                // Migrated compound literal (`1000000ohm*m`, task ζ) folds to a
                // single Scalar whose si_value is the coefficient.
                CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => {
                    return Some(*si_value)
                }
                CompiledExprKind::BinOp { op: BinOp::Mul | BinOp::Div, left, .. } => {
                    cursor = left;
                }
                _ => return None,
            }
        }
    }
    let assert_gt_constraint =
        |member: &str, rhs_real: f64, epsilon: f64, expected_dim: DimensionVector| {
            let found = template.constraints.iter().find(|cc| {
                if let CompiledExprKind::BinOp { op: BinOp::Gt, left, right } = &cc.expr.kind {
                    let left_match = matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == member);
                    let coeff_match = rhs_coefficient(right)
                        .map(|v| (v - rhs_real).abs() <= epsilon)
                        .unwrap_or(false);
                    // shape_match is defense-in-depth: logically subsumed by
                    // dim_match below (a bare Real RHS can never carry the
                    // expected_dim result_type), but kept as an explicit shape
                    // check so the expected RHS structure is self-documenting
                    // here. dim_match is the load-bearing pin. Post-task-ζ the
                    // migrated resistivity bound `1000000ohm*m` folds to a single
                    // Literal(Scalar); the non-migrated dielectric_strength bound
                    // `0.0 * 1V / 1m` is still a Mul/Div chain — accept either.
                    let shape_match = matches!(
                        &right.kind,
                        CompiledExprKind::BinOp { op: BinOp::Mul | BinOp::Div, .. }
                            | CompiledExprKind::Literal(Value::Scalar { .. })
                    );
                    let dim_match =
                        right.result_type == Type::Scalar { dimension: expected_dim };
                    left_match && coeff_match && shape_match && dim_match
                } else {
                    false
                }
            });
            assert!(
                found.is_some(),
                "expected dimensioned constraint `{member} > {rhs_real}<units>` injected \
                 into Glass template — RHS must be a folded Scalar literal or a Mul/Div \
                 unit chain typed Scalar{{ dimension: {expected_dim:?} }}, not bare Real; \
                 got: {:?}",
                template.constraints
            );
        };

    // Both inherited Insulating constraints must have correct operator, literal,
    // AND dimensioned RHS injected (esc-3115-112).
    assert_gt_constraint(
        "dielectric_strength",
        0.0,
        0.0,
        DimensionVector::DIELECTRIC_STRENGTH,
    ); // dielectric_strength > 0.0 * 1V / 1m
    assert_gt_constraint(
        "resistivity",
        1_000_000.0,
        0.0,
        DimensionVector::ELECTRIC_RESISTIVITY,
    ); // resistivity > 1000000ohm*m
}

// ─── (g) assert_trait_constraint_binop accepts a compound-literal RHS ────────

/// Pins the `assert_trait_constraint_binop` helper contract for a constraint
/// whose RHS is a COMPOUND quantity literal (`0.0001ohm*m`) — the exact form
/// the stdlib electrical migration (step-5) produces. A compound literal folds
/// to a single `ExprKind::QuantityLiteral` node, whereas the legacy
/// `0.0001 * 1ohm * 1m` is a `BinOp(*)` spine.
///
/// Genuinely RED before step-4: the helper's RHS spine-walk matches only
/// `NumberLiteral`/`BinOp(*|/)` and panics on `QuantityLiteral`. Step-4 adds
/// the `QuantityLiteral { value, .. } => break *value` arm, turning this GREEN.
/// The extracted coefficient (0.0001) equals the SI value because `ohm` and `m`
/// have SI factor 1 — consistent with the existing `NumberLiteral` path.
#[test]
fn helper_accepts_compound_quantity_literal_constraint_rhs() {
    let source = r#"
trait CompoundRhsProbe {
    param r : ElectricResistivity
    constraint r < 0.0001ohm*m
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "CompoundRhsProbe trait should compile cleanly, got errors: {:?}",
        errors
    );

    let probe = module
        .trait_defs
        .iter()
        .find(|t| t.name == "CompoundRhsProbe")
        .expect("expected 'CompoundRhsProbe' trait def in compiled module");

    assert_trait_constraint_binop(probe, "CompoundRhsProbe", "r", "<", 1.0e-4, 1.0e-16);
}

// ─── (h) Insulating.dielectric_strength constraint RHS is compound QuantityLiteral ─

/// After step-2 migrates `0.0 * 1V / 1m` → `0.0V/m`, the Insulating
/// trait's `dielectric_strength > 0.0V/m` constraint must have its comparison
/// RHS as a single `ExprKind::QuantityLiteral { value: 0.0,
/// unit: UnitExpr::Div(Unit("V"), Unit("m")) }`.
///
/// RED before step-2: the RHS is the `0.0 * 1V / 1m` BinOp(Mul/Div) spine, so
/// the QuantityLiteral match fails.
/// GREEN after step-2: compound literal `0.0V/m` folds to a single QuantityLiteral.
#[test]
fn insulating_dielectric_strength_constraint_rhs_is_compound_literal() {
    let module = load_stdlib_module();

    let insulating = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Insulating")
        .expect("expected 'Insulating' trait in std/materials/electrical");

    // Find the constraint default whose expression is `dielectric_strength > ...`
    let constraint_default = insulating
        .defaults
        .iter()
        .find(|d| {
            if let DefaultKind::Constraint(decl) = &d.kind {
                matches!(&decl.expr.kind, ExprKind::BinOp { left, .. }
                    if matches!(&left.kind, ExprKind::Ident(n) if n == "dielectric_strength"))
            } else {
                false
            }
        })
        .expect("Insulating must have a dielectric_strength constraint default");

    let DefaultKind::Constraint(decl) = &constraint_default.kind else {
        unreachable!(
            "dielectric_strength constraint default must be DefaultKind::Constraint"
        )
    };
    let ExprKind::BinOp { right, .. } = &decl.expr.kind else {
        unreachable!(
            "dielectric_strength constraint expr must be ExprKind::BinOp, got {:?}",
            decl.expr.kind
        )
    };

    // After migration to `0.0V/m`, the RHS must be a single compound QuantityLiteral.
    // RED today: the RHS is BinOp(Div, BinOp(Mul, 0.0, QuantityLiteral(1.0,V)), QuantityLiteral(1.0,m)).
    let expected_unit = UnitExpr::Div(
        Box::new(UnitExpr::Unit("V".to_string())),
        Box::new(UnitExpr::Unit("m".to_string())),
    );
    match &right.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert_eq!(
                *value, 0.0,
                "Insulating dielectric_strength constraint RHS value should be 0.0, got {}",
                value
            );
            assert_eq!(
                unit, &expected_unit,
                "Insulating dielectric_strength constraint RHS unit should be Div(V,m), got {:?}",
                unit
            );
        }
        other => panic!(
            "Insulating dielectric_strength constraint RHS should be compound \
             QuantityLiteral `0.0V/m` after migration, got: {:?}",
            other
        ),
    }
}
