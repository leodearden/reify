//! Round-trip lowering tests for `UnitExpr` (PRD §8 row β).
//!
//! Pair 1 (steps 1–2): bare `5mm` lowers to `UnitExpr::Unit("mm")`.
//! Pair 2 (steps 3–4): PRD §7 compound fixture trees + signed exponent, plus
//! two negative BinOp fixtures that must NOT collapse into one quantity_literal.

use reify_ast::*;

/// Helper: parse source and return the ExprKind of the first `param` member's default.
fn parse_param_default_kind(source: &str) -> ExprKind {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("unit_expr_lowering_test"),
    );
    let structure = match module.declarations.into_iter().next() {
        Some(Declaration::Structure(s)) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    match structure.members.into_iter().next() {
        Some(MemberDecl::Param(p)) => p
            .default
            .expect("param should have a default")
            .kind,
        Some(other) => panic!("expected Param member, got {:?}", other),
        None => panic!("structure has no members"),
    }
}

// ── Pair 1: bare unit round-trip ─────────────────────────────────────────────

/// PRD §8 row β: bare `5mm` lowers to `QuantityLiteral { value: 5.0, unit: Unit("mm") }`.
///
/// RED before step-2: `UnitExpr` does not exist yet, so this file fails to
/// compile. step-2 defines `UnitExpr` and updates `lower_quantity_literal`,
/// making this test pass.
#[test]
fn bare_unit_5mm_lowers_to_unit_mm() {
    let kind = parse_param_default_kind("structure S { param x : Length = 5mm }");
    match kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert!(
                (value - 5.0).abs() < f64::EPSILON,
                "expected value 5.0, got {}",
                value
            );
            assert_eq!(
                unit,
                UnitExpr::Unit("mm".to_string()),
                "bare 5mm should lower to UnitExpr::Unit(\"mm\")"
            );
        }
        other => panic!("expected QuantityLiteral, got {:?}", other),
    }
}

// ── Pair 2: compound fixture trees (PRD §7) ──────────────────────────────────

// Concise constructors for the expected `UnitExpr` trees.
fn bare(name: &str) -> UnitExpr {
    UnitExpr::Unit(name.to_string())
}
fn pow(base: UnitExpr, exp: i32) -> UnitExpr {
    UnitExpr::Pow(Box::new(base), exp)
}
fn mul(a: UnitExpr, b: UnitExpr) -> UnitExpr {
    UnitExpr::Mul(Box::new(a), Box::new(b))
}
fn div(a: UnitExpr, b: UnitExpr) -> UnitExpr {
    UnitExpr::Div(Box::new(a), Box::new(b))
}

/// Parse `param x : T = <quantity>` and return the lowered `UnitExpr`.
fn unit_of(quantity: &str) -> UnitExpr {
    let source = format!("structure S {{ param x : Length = {quantity} }}");
    match parse_param_default_kind(&source) {
        ExprKind::QuantityLiteral { unit, .. } => unit,
        other => panic!("expected QuantityLiteral for `{quantity}`, got {:?}", other),
    }
}

#[test]
fn density_div_pow() {
    // 7850kg/m^3 → Div(kg, Pow(m, 3))
    assert_eq!(unit_of("7850kg/m^3"), div(bare("kg"), pow(bare("m"), 3)));
}

#[test]
fn acceleration_div_pow() {
    // 9.81m/s^2 → Div(m, Pow(s, 2))
    assert_eq!(unit_of("9.81m/s^2"), div(bare("m"), pow(bare("s"), 2)));
}

#[test]
fn torque_mul() {
    // 5kN*m → Mul(kN, m)
    assert_eq!(unit_of("5kN*m"), mul(bare("kN"), bare("m")));
}

#[test]
fn area_pow() {
    // 25mm^2 → Pow(mm, 2)
    assert_eq!(unit_of("25mm^2"), pow(bare("mm"), 2));
}

#[test]
fn viscosity_left_assoc_div() {
    // 0.001kg/m/s → Div(Div(kg, m), s) (left-associative)
    assert_eq!(
        unit_of("0.001kg/m/s"),
        div(div(bare("kg"), bare("m")), bare("s"))
    );
}

#[test]
fn thermal_conductivity_paren_unwrapped() {
    // 0.5W/(m*K) → Div(W, Mul(m, K)) — paren is transparently unwrapped (no Paren variant)
    assert_eq!(
        unit_of("0.5W/(m*K)"),
        div(bare("W"), mul(bare("m"), bare("K")))
    );
}

#[test]
fn paren_group_raised_to_power() {
    // 5(kg*m/s)^2 → Pow(Div(Mul(kg, m), s), 2)
    assert_eq!(
        unit_of("5(kg*m/s)^2"),
        pow(div(mul(bare("kg"), bare("m")), bare("s")), 2)
    );
}

#[test]
fn signed_negative_exponent() {
    // 1m/s^-2 → Div(m, Pow(s, -2)) — grammar's signed_integer is `-?\d+`
    assert_eq!(unit_of("1m/s^-2"), div(bare("m"), pow(bare("s"), -2)));
}

// ── Pair 2: negative fixtures — must stay BinOp, not collapse ─────────────────

#[test]
fn space_separated_mul_stays_binop() {
    // `5kg * m` (space before `*`) → BinOp(*, QuantityLiteral(5, kg), Ident(m))
    // The external scanner's unit-mul op only fires when immediately adjacent.
    let kind = parse_param_default_kind("structure S { param x : Length = 5kg * m }");
    match kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*");
            match &left.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 5.0).abs() < f64::EPSILON);
                    assert_eq!(unit, &bare("kg"));
                }
                other => panic!("expected QuantityLiteral on left, got {:?}", other),
            }
            assert!(
                matches!(&right.kind, ExprKind::Ident(n) if n == "m"),
                "right should be Ident(\"m\"), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}

#[test]
fn digit_after_slash_stays_binop() {
    // `25USD/1kg` (slash followed by a digit) → BinOp(/, 25USD, 1kg)
    // The external scanner's unit-div op only fires when the next char is a unit-start.
    let kind = parse_param_default_kind("structure S { param x : Length = 25USD/1kg }");
    match kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "/");
            match &left.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 25.0).abs() < f64::EPSILON);
                    assert_eq!(unit, &bare("USD"));
                }
                other => panic!("expected QuantityLiteral on left, got {:?}", other),
            }
            match &right.kind {
                ExprKind::QuantityLiteral { value, unit } => {
                    assert!((value - 1.0).abs() < f64::EPSILON);
                    assert_eq!(unit, &bare("kg"));
                }
                other => panic!("expected QuantityLiteral on right, got {:?}", other),
            }
        }
        other => panic!("expected BinOp, got {:?}", other),
    }
}
