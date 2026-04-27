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

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

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

/// Assert that `trait_def` has a `DefaultKind::Constraint` whose expression is
/// `BinOp { op: expected_op, left: Ident(expected_member), right: NumberLiteral(rhs) }`
/// where `|rhs - expected_rhs| <= rhs_epsilon`.
///
/// This tightens the constraint-present check: a regression that flips the
/// operator (`<` → `>`) or changes the bound (e.g. `0.0` instead of `1.0e-4`)
/// will now fail here, not just at the eval-level check test.
#[track_caller]
fn assert_trait_constraint_binop(
    trait_def: &CompiledTrait,
    trait_name: &str,
    expected_member: &str,
    expected_op: &str,
    expected_rhs: f64,
    rhs_epsilon: f64,
) {
    use reify_syntax::ExprKind;

    let constraint_default = trait_def
        .defaults
        .iter()
        .find(|d| {
            if let DefaultKind::Constraint(decl) = &d.kind {
                matches!(&decl.expr.kind, ExprKind::BinOp { left, .. }
                    if matches!(&left.kind, ExprKind::Ident(n) if n == expected_member))
            } else {
                false
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "{} must have a constraint default on '{}', got defaults: {:?}",
                trait_name,
                expected_member,
                trait_def
                    .defaults
                    .iter()
                    .map(|d| format!("{:?}", d.kind))
                    .collect::<Vec<_>>()
            )
        });

    if let DefaultKind::Constraint(decl) = &constraint_default.kind {
        if let ExprKind::BinOp { op, left: _, right } = &decl.expr.kind {
            assert_eq!(
                op.as_str(),
                expected_op,
                "{} constraint op for '{}' should be '{}', got '{}'",
                trait_name,
                expected_member,
                expected_op,
                op
            );
            match &right.kind {
                ExprKind::NumberLiteral(v) => assert!(
                    (*v - expected_rhs).abs() <= rhs_epsilon,
                    "{} constraint RHS for '{}' should be {} (±{}), got {}",
                    trait_name,
                    expected_member,
                    expected_rhs,
                    rhs_epsilon,
                    v
                ),
                other => panic!(
                    "{} constraint RHS for '{}' should be NumberLiteral, got {:?}",
                    trait_name, expected_member, other
                ),
            }
        }
    }
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

// ─── (b) ElectricallyCharacterized refines MaterialSpec with 4 Real members ──

/// ElectricallyCharacterized must refine MaterialSpec and declare four required
/// members, all typed as Real: resistivity, dielectric_constant,
/// dielectric_strength, magnetic_permeability.
#[test]
fn electrically_characterized_refines_material_spec_with_four_real_members() {
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

    assert_eq!(
        ec.required_members.len(),
        4,
        "ElectricallyCharacterized should have exactly 4 required members, got: {:?}",
        ec.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let expected_members = [
        "resistivity",
        "dielectric_constant",
        "dielectric_strength",
        "magnetic_permeability",
    ];

    for expected in &expected_members {
        let req = ec
            .required_members
            .iter()
            .find(|r| r.name == *expected)
            .unwrap_or_else(|| {
                panic!(
                    "ElectricallyCharacterized missing required member '{}', got: {:?}",
                    expected,
                    ec.required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                *ty,
                Type::Real,
                "ElectricallyCharacterized member '{}' should be Real, got {:?}",
                expected,
                ty
            ),
            other => panic!(
                "ElectricallyCharacterized member '{}' should be Param, got {:?}",
                expected, other
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
    assert_trait_constraint_binop(conductive, "Conductive", "resistivity", "<", 1.0e-4, 1.0e-16);
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
    assert_trait_constraint_binop(insulating, "Insulating", "dielectric_strength", ">", 0.0, 0.0);
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
    param resistivity : Real = 0.000000017
    param dielectric_constant : Real = 1.0
    param dielectric_strength : Real = 0.0
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

    // Verify the constraint references 'resistivity' via a BinOp.
    let resistivity_constraint = template.constraints.iter().find(|cc| {
        matches!(&cc.expr.kind, CompiledExprKind::BinOp { left, .. }
            if matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "resistivity"))
    });
    assert!(
        resistivity_constraint.is_some(),
        "expected a constraint referencing 'resistivity' in Copper template, got constraints: {:?}",
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
    param resistivity : Real = 1000000000.0
    param dielectric_constant : Real = 7.0
    param dielectric_strength : Real = 10000000.0
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

    // Helper: assert a `member > rhs_int` BinOp constraint is injected.
    // Verifies the operator is Gt and the right-hand-side is the expected integer literal.
    // The compiler coerces whole-number float literals (e.g. `1000000.0`) to Int, so
    // `rhs_int` is i64. A stale constraint like `resistivity == 0` or a wrong
    // threshold would now fail this check.
    let assert_gt_constraint = |member: &str, rhs_int: i64| {
        let found = template.constraints.iter().find(|cc| {
            matches!(
                &cc.expr.kind,
                CompiledExprKind::BinOp { op: BinOp::Gt, left, right }
                if matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == member)
                    && matches!(&right.kind, CompiledExprKind::Literal(Value::Int(n)) if *n == rhs_int)
            )
        });
        assert!(
            found.is_some(),
            "expected constraint `{member} > {rhs_int}` injected into Glass template, got: {:?}",
            template.constraints
        );
    };

    // Both inherited Insulating constraints must have correct operator and literal injected.
    assert_gt_constraint("dielectric_strength", 0);         // dielectric_strength > 0.0
    assert_gt_constraint("resistivity", 1_000_000);         // resistivity > 1e6
}
