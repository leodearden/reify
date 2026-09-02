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

// ── Pair 3: U+00B7 MIDDLE DOT as unit-multiply (task #5784, angle-units leaf κ) ──
//
// `Display for DimensionVector` joins base-unit parts with `·`, so `reify eval`
// emits `7850 kg·m^-3` and `5 m^2·kg·s^-2·rad^-1`.  Leaf κ makes those strings
// readable back in: the external scanner emits UNIT_MUL_OP for `·` as well as `*`,
// and `lower_unit_expr` maps both spellings to the SAME `UnitExpr::Mul`.
//
// The twin assertions (`unit_of("…·…") == unit_of("…*…")`) are the literal wording
// of this task's user-observable signal, and are what leaf μ's round-trip property
// test will build on.

#[test]
fn middot_mul_lowers_like_star_mul() {
    // 5N·m → Mul(N, m)
    assert_eq!(unit_of("5N·m"), mul(bare("N"), bare("m")));
    assert_eq!(unit_of("5N·m"), unit_of("5N*m"));
}

#[test]
fn middot_mixed_with_div_is_left_associative() {
    // 5N·m/rad → Div(Mul(N, m), rad) — `/` is the outermost operator.
    assert_eq!(
        unit_of("5N·m/rad"),
        div(mul(bare("N"), bare("m")), bare("rad"))
    );
    assert_eq!(unit_of("5N·m/rad"), unit_of("5N*m/rad"));
}

#[test]
fn middot_chain_with_exponents_lowers_like_star_chain() {
    // 5m^2·kg·s^-2 → Mul(Mul(Pow(m,2), kg), Pow(s,-2))
    assert_eq!(
        unit_of("5m^2·kg·s^-2"),
        mul(mul(pow(bare("m"), 2), bare("kg")), pow(bare("s"), -2))
    );
    assert_eq!(unit_of("5m^2·kg·s^-2"), unit_of("5m^2*kg*s^-2"));
}

#[test]
fn middot_density_and_acceleration_lower_like_star_twins() {
    // The two shapes `Display for DimensionVector` actually emits.
    assert_eq!(unit_of("7850kg·m^-3"), mul(bare("kg"), pow(bare("m"), -3)));
    assert_eq!(unit_of("7850kg·m^-3"), unit_of("7850kg*m^-3"));
    assert_eq!(unit_of("9.81m·s^-2"), mul(bare("m"), pow(bare("s"), -2)));
    assert_eq!(unit_of("9.81m·s^-2"), unit_of("9.81m*s^-2"));
}

/// `·` against a PARENTHESISED operand, both sides of the paren.
///
/// `is_unit_start` accepts `(`, so `·` immediately before a paren group is a live
/// accepted path, and it is the only shape that drives `lower_unit_expr`'s
/// paren-unwrap arm (step 3) BENEATH a `·`-spelled Mul.  The `*` spelling has this
/// coverage via `thermal_conductivity_paren_unwrapped`; before this test the `·`
/// spelling had none at any layer.
///
/// It matters more than the `*` twin because the operator is not a field on the
/// node — `lower_unit_expr` reads it as the SOURCE SLICE between the two operands
/// (`source[left.end_byte()..right.start_byte()]`) and, since #5784, matches that
/// slice EXACTLY rather than by `contains('*')`.  A paren moves the operand
/// boundaries, so this is precisely where an exact-match read could go wrong.
#[test]
fn middot_with_parenthesised_operand_lowers_like_star_twin() {
    // `·` to the LEFT of the group: 5W·(m/K) → Mul(W, Div(m, K))
    assert_eq!(
        unit_of("5W·(m/K)"),
        mul(bare("W"), div(bare("m"), bare("K")))
    );
    assert_eq!(unit_of("5W·(m/K)"), unit_of("5W*(m/K)"));
    // `·` INSIDE the group, under an outer `/`: 5W/(m·K) → Div(W, Mul(m, K))
    assert_eq!(
        unit_of("5W/(m·K)"),
        div(bare("W"), mul(bare("m"), bare("K")))
    );
    assert_eq!(unit_of("5W/(m·K)"), unit_of("5W/(m*K)"));
}

// ── The anti-silent-drop lock ────────────────────────────────────────────────
//
// This is the assertion that distinguishes "correct" from "silently dropped", and
// it is the reason leaf κ is not just a one-token scanner edit.  Widening the
// scanner alone makes the CST CLEAN while `lower_unit_expr` still fails to
// recognise the `·` operator slice and returns `None`; that `None` propagates
// through `lower_quantity_literal` and `lower_let` as a DROPPED member, and
// `check_and_lower!` never fires because it keys off `is_error()`/`has_error()` on
// a CST that no longer has an error.  The result is a structure with ZERO members
// and ZERO diagnostics — the exact INV-SF-7 `parse-is-value-faithful`
// ("well-typed WRONG value") shape.  A `unit_of`-style assertion alone cannot
// catch it: it panics with "structure has no members", which reads like a parse
// failure rather than a faithfulness violation.

/// Parse `source` and return (declaration count, member count of the first
/// structure, parse-error messages).
fn parse_shape(source: &str) -> (usize, usize, Vec<String>) {
    let (decls, members, errors) = parse_shape_spanned(source);
    (decls, members, errors.into_iter().map(|(m, _)| m).collect())
}

/// Like [`parse_shape`], but keeps each diagnostic's byte span next to its
/// message.
///
/// The negative locks below need the span: a message-only check for the offending
/// character is satisfied by SOURCE ECHO.  Measured on this branch, the targeted
/// diagnostics are `syntax error: · m` spanning `[28..32]` — starting exactly at
/// the `·` — while nearby malformed inputs (`5N·/m`) produce the whole-line form
/// `invalid let: let x = 5N·/m` spanning `[18..32]`, i.e. starting at `let`.  Both
/// strings contain `·`; only the first POINTS at it.
fn parse_shape_spanned(source: &str) -> (usize, usize, Vec<(String, reify_core::SourceSpan)>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("unit_expr_lowering_test"),
    );
    let errors: Vec<(String, reify_core::SourceSpan)> = module
        .errors
        .iter()
        .map(|e| (e.message.clone(), e.span))
        .collect();
    let decl_count = module.declarations.len();
    let member_count = match module.declarations.first() {
        Some(Declaration::Structure(s)) => s.members.len(),
        _ => 0,
    };
    (decl_count, member_count, errors)
}

#[test]
fn middot_let_member_is_present_and_diagnostic_free() {
    let (decls, members, errors) = parse_shape("structure def S { let x = 5N·m }");
    assert!(
        errors.is_empty(),
        "`5N·m` must lower without any parse diagnostic; got {errors:?}"
    );
    assert_eq!(decls, 1, "expected exactly one declaration");
    assert_eq!(
        members, 1,
        "`let x = 5N·m` must survive lowering as a member — zero members with zero \
         diagnostics is the silent-drop failure this lock exists to catch"
    );
    // The `*` twin is the reference: the two spellings must be indistinguishable.
    assert_eq!(
        parse_shape("structure def S { let x = 5N*m }"),
        (decls, members, errors),
        "`5N·m` and `5N*m` must produce identical module shapes"
    );
}

/// Several `·`-bearing bindings in ONE structure all survive lowering.
///
/// Unique content over `middot_let_member_is_present_and_diagnostic_free`: that
/// test pins ONE member surviving, this one pins that the count SCALES.  A drop
/// that only bites the second or third binding — state carried across successive
/// `lower_let` calls, say — leaves the single-member test green.
///
/// DELIBERATELY NOT COUPLED to `tests/prd-gate/fixtures/unit_middot_mul.ri`, which
/// these three bindings happen to resemble.  This is a self-contained
/// multi-binding case and carries NO obligation to stay in sync with that file;
/// adding or removing a binding here is free.  The fixture-coupled assertion lives
/// one layer up in `crates/reify-compiler/tests/harness_units/`
/// `unit_middot_mul_tests.rs`, which READS the fixture from disk (and pins its `·`
/// count and its cell count) instead of transcribing it — a hand-transcribed copy
/// here would go stale silently while staying green, which is the exact drift the
/// disk-reading test was built to avoid.
#[test]
fn middot_multiple_bindings_in_one_structure_all_survive_lowering() {
    let (decls, members, errors) = parse_shape(
        "structure def S {\n\
         \x20   let torque_like = 5N·m\n\
         \x20   let with_div    = 5N·m/rad\n\
         \x20   let composed    = 5m^2·kg·s^-2\n\
         }",
    );
    assert!(errors.is_empty(), "expected no parse diagnostics, got {errors:?}");
    assert_eq!(decls, 1);
    assert_eq!(
        members, 3,
        "all THREE `·`-bearing lets must survive lowering; a lower count means \
         members were dropped silently"
    );
}

// ── Negative locks: adjacency ────────────────────────────────────────────────
//
// These do NOT mirror the outcome of the `*` negatives above.  `5kg * m` really
// does stay a BinOp because `*` is a general binary operator; there is no general
// `·` operator, so each source below is a PARSE ERROR.  The names say so.
//
// What each must produce is a LOUD failure: at least one parse diagnostic, plus
// the binding RECOVERED as a member carrying the error.  What none may produce is
// a diagnostic-free structure with a missing member.

/// Shared assertion for the adjacency negatives.  Parses `source` ONCE and pins
/// the full recovered shape.
///
/// The `members == 1` half is the part `!errors.is_empty()` does not imply, and
/// is why this helper exists at all: tree-sitter's recovery keeps the `let`
/// binding as a member carrying the ERROR node, so the observable outcome is a
/// loud member — not a vanished one, and not a dropped declaration.  Measured on
/// this branch: all three sources give `(decls, members) == (1, 1)` with one
/// `syntax error: …` diagnostic starting at the middle dot.
///
/// "Points at" is asserted POSITIONALLY, on the span.  An earlier version tested
/// `message.contains('·')`, which any diagnostic that echoes the source line
/// satisfies: `5N·/m` produces `invalid let: let x = 5N·/m` spanning the whole
/// binding from `let`, and would have passed a message-only check while pointing
/// nowhere near the operator.  Measured spans for the three sources below:
/// `5N· m` → `[28..32]`, `5N ·m` → `[29..32]`, `5N·3` → `[28..31]`, each starting
/// exactly at the `·` byte.
fn assert_loud_middot_parse_error(source: &str) {
    let (decls, members, errors) = parse_shape_spanned(source);
    assert!(
        !errors.is_empty(),
        "`{source}` must produce a parse diagnostic — `·` is not a general \
         operator, and a diagnostic-free parse here is the INV-SF-7 silent-drop \
         shape this block exists to forbid"
    );
    let middot_at = source
        .find('·')
        .expect("this helper is only for `·`-bearing sources") as u32;
    assert!(
        errors.iter().any(|(_, span)| span.start == middot_at),
        "`{source}`: a diagnostic must POINT AT the middle dot (start byte \
         {middot_at}), not merely mention it in an echoed source line; got \
         {errors:?}"
    );
    assert_eq!(decls, 1, "`{source}`: expected exactly one declaration");
    assert_eq!(
        members, 1,
        "`{source}`: the binding must be RECOVERED as a member carrying the \
         error, not dropped — got {members} members with {errors:?}"
    );
}

#[test]
fn space_after_middot_is_a_parse_error() {
    // `5N· m` — whitespace after `·` breaks unit contiguity, so `·` is not a
    // unit-multiply here and there is no general `·` operator to fall back to.
    assert_loud_middot_parse_error("structure def S { let x = 5N· m }");
}

#[test]
fn space_before_middot_is_a_parse_error() {
    // `5N ·m` — same, on the other side of the operator.
    assert_loud_middot_parse_error("structure def S { let x = 5N ·m }");
}

#[test]
fn digit_after_middot_is_a_parse_error() {
    // `5N·3` — a digit is not a unit-start character, so the scanner rolls back
    // (the adjacency condition `digit_after_slash_stays_binop` tests for `/`).
    // Unlike `/`, there is no general `·` binary operator to recover into, so
    // this is an error rather than a BinOp.
    assert_loud_middot_parse_error("structure def S { let x = 5N·3 }");
}

// ── Comments inside a unit expression ────────────────────────────────────────
//
// #5784 amendment pass.  Comments are parser `extras`, so one written between a
// `unit_expr`'s operands lands INSIDE the source slice `lower_unit_expr` cuts out
// to recover the operator (the `op` field aliases hidden external tokens, so the
// slice is the only way to read it).  Measured — every source below parses with
// `has_error() == false` and yields `unit_expr(left, block_comment…, right)`:
//
//   5N/*c*/*m        slice `/*c*/*`         5N/*c*/·m   slice `/*c*/·`
//   5N/*c*//m        slice `/*c*//`         5N/*a*//*b*/*m   slice `/*a*//*b*/*`
//
// The contract has moved twice.  Before κ, `op_text.contains('/')` lowered
// `/*c*/*` to `Div` — a well-typed WRONG value from a clean parse, the INV-SF-7
// `parse-is-value-faithful` shape exactly.  κ's exact match made it
// `Unrecognized`, which was loud but rejected source the GRAMMAR ACCEPTED.  This
// pass excises the comment spans first, so the residue classifies as the operator
// the CST plainly shows and the lowered tree agrees with the parse.
//
// A `line_comment` was never observed inside a `unit_expr` (`//…` ends the line,
// and `5N//c⏎*m` reparses as a `binary_expression`), so it is not pinned here —
// only `strip_unit_op_comments`' filter accepts the kind, defensively.

#[test]
fn comment_between_unit_operands_lowers_like_its_comment_free_twin() {
    for (commented, plain) in [
        ("5N/*c*/*m", "5N*m"),
        ("5N/*c*/·m", "5N·m"),
        ("5N/*c*//m", "5N/m"),
        ("5N/*a*//*b*/*m", "5N*m"),
        // Left-associative chain with a comment at each operator.
        ("5N/*c*/*m/*d*/*s", "5N*m*s"),
        // The right operand may still be a `Pow`.
        ("5N/*c*/*m^2", "5N*m^2"),
        // PARENS × COMMENT — the intersection of the two risks this file pins
        // separately.  `middot_with_parenthesised_operand_lowers_like_star_twin`
        // above explains that a paren moves the operand BOUNDARIES, which is
        // where an exact-match read of the slice could go wrong; the excision
        // path is what recovers a slice that is not just the operator.  Only
        // together do they exercise both at once — and `5(m)/*c*/*(s)` is the
        // MEASURED case `lower_unit_expr`'s own comment cites as motivating.
        ("5(m)/*c*/*(s)", "5(m)*(s)"),
        ("5W/*c*/*(m/K)", "5W*(m/K)"),
    ] {
        assert_eq!(
            unit_of(commented),
            unit_of(plain),
            "`{commented}` parses with no ERROR node, so it must lower to the \
             same UnitExpr as `{plain}` — a comment is an `extra`, not an operator"
        );
    }
}

#[test]
fn comment_between_unit_operands_keeps_the_member_and_stays_diagnostic_free() {
    // The `unit_of` equality above reads the lowered tree through a `param`
    // default; this pins the other half — that the binding SURVIVES and that
    // nothing is reported.  A spurious "unrecognized unit operator" here is the
    // regression this test exists to catch.
    for source in [
        "structure def S { let x = 5N/*c*/*m }",
        "structure def S { let x = 5N/*c*/·m }",
        "structure def S { let x = 5N/*a*//*b*/*m }",
    ] {
        let (decls, members, errors) = parse_shape(source);
        assert!(
            errors.is_empty(),
            "`{source}` parses with zero ERROR nodes, so lowering must not \
             invent a diagnostic; got {errors:?}"
        );
        assert_eq!(decls, 1, "`{source}`: expected exactly one declaration");
        assert_eq!(
            members, 1,
            "`{source}`: the binding must survive lowering — zero members with \
             zero diagnostics is the silent-drop shape"
        );
    }
}

#[test]
fn comment_after_a_middot_is_still_a_parse_error() {
    // The comment excision must not widen what the SCANNER accepts: `·` still
    // requires a unit-start character immediately after it, and a comment is not
    // one.  Measured: `5N·/*c*/m` yields `(ERROR … (UNEXPECTED 183))`, so this
    // never reaches `classify_unit_op` at all.
    assert_loud_middot_parse_error("structure def S { let x = 5N·/*c*/m }");
}
