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
/// `lower_unit_expr` reads the operator from the CST `op` node, which is kind `*`
/// for both spellings (`grammar.js` aliases the scanner token).  This pins that a
/// paren group on either side of the `·` leaves that node, and so the `Mul`, in
/// place.
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
// recognise the `·` operator and returns `None`; that `None` propagates
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
/// character is satisfied by any diagnostic whose source excerpt merely CONTAINS
/// it, such as `invalid let: let x = 5N·/m`, wherever that diagnostic points.
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
/// `syntax error in structure body` diagnostic starting at the middle dot.
///
/// "Points at" is asserted POSITIONALLY, on the span.  An earlier version tested
/// `message.contains('·')`, which a diagnostic satisfies merely by quoting source
/// that contains the `·` — `5N·/m` reports `invalid let: let x = 5N·/m` — so it
/// said nothing about where the diagnostic points.
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
// Comments are parser `extras`, so one written between a `unit_expr`'s operands
// parses clean (`has_error() == false`) as a SIBLING of the `op` node:
//
//   5N/*c*/*m   →   unit_expr(left, block_comment, op `*`, right)
//
// `lower_unit_expr` reads the operator from the `op` node's kind, and that node
// never contains a comment, so a commented expression must lower exactly like its
// comment-free twin.  Provenance: the comment-strip pass that resolved #6952 first
// fixed the value, by cutting comments out of a source-slice read of the
// operator; reading the CST `op` node replaced that pass.
//
// A `line_comment` was never observed inside a `unit_expr` (`//…` ends the line,
// and `5N//c⏎*m` reparses as a `binary_expression`), so it is not pinned here.

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
        // PARENS × COMMENT: a paren group on either side of a commented
        // operator.  `5(m)/*c*/*(s)` is the #6952 witness.
        ("5(m)/*c*/*(s)", "5(m)*(s)"),
        ("5W/*c*/*(m/K)", "5W*(m/K)"),
        // A comment INSIDE the left paren group: the one shape whose comment
        // attaches below the outer `unit_expr` rather than beside its operator.
        ("5(m/*c*/)*s", "5(m)*s"),
    ] {
        assert_eq!(
            unit_of(commented),
            unit_of(plain),
            "`{commented}` parses with no ERROR node, so it must lower to the \
             same UnitExpr as `{plain}` — a comment is an `extra`, not an operator"
        );
    }
}

/// The task's done-when witnesses, pinned to EXPLICIT trees: a twin equality alone
/// stays green if both twins regress the same way.
#[test]
fn done_when_witnesses_lower_to_their_explicit_trees() {
    assert_eq!(unit_of("5(m)/*c*/*(s)"), mul(bare("m"), bare("s")));
    assert_eq!(unit_of("5(m)/*c*/*(s)"), unit_of("5(m)*(s)"));
    assert_eq!(
        unit_of("5N·m/rad"),
        div(mul(bare("N"), bare("m")), bare("rad"))
    );
    assert_eq!(unit_of("5kg*m"), mul(bare("kg"), bare("m")));
    assert_eq!(unit_of("5m/s"), div(bare("m"), bare("s")));
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
        "structure def S { let q = 5(m)/*c*/*(s) }",
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
    // Accepting a comment BETWEEN operands does not widen what the SCANNER
    // accepts: `·` still requires a unit-start character immediately after it,
    // and a comment is not one.  Measured: `5N·/*c*/m` yields
    // `(ERROR … (UNEXPECTED 183))`.
    assert_loud_middot_parse_error("structure def S { let x = 5N·/*c*/m }");
}

// ── Detached unit suffix (task #7190) ────────────────────────────────────────
//
// PRD docs/prds/unit-expressions.md §3.1 (contiguity): a unit must touch its
// number.  `1000.0 N/m` is therefore a parse ERROR on purpose — the same rule is
// what keeps `5kg * m` a binary expression — so the parser must not accept the
// detached spelling.  It must instead explain the fix: name the binding and the
// attached spelling, with the span on the unit word.

const HINT_MARK: &str = "must follow its number with no whitespace";

/// The remedy suffix appended to a detached-unit diagnostic.
fn detached_unit_hint(unit: &str, binding: &str, number: &str) -> String {
    format!(" — unit `{unit}` in `{binding}` {HINT_MARK}: write `{number}{unit}`")
}

/// The single diagnostic whose message starts with `prefix`.
fn only_error_starting_with(
    errors: &[(String, reify_core::SourceSpan)],
    prefix: &str,
) -> (String, reify_core::SourceSpan) {
    let matching: Vec<_> = errors
        .iter()
        .filter(|(message, _)| message.starts_with(prefix))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one diagnostic starting with `{prefix}`; got {errors:?}"
    );
    matching[0].clone()
}

/// Byte offset of `needle` in `source`, plus `skip` bytes.
fn byte_of(source: &str, needle: &str, skip: usize) -> u32 {
    (source
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not found in `{source}`"))
        + skip) as u32
}

#[test]
fn connect_param_detached_unit_names_the_param_and_the_attached_spelling() {
    let source = "structure Leaf {\n\
                  \x20 port p : LocatedPort { frame = frame3() }\n\
                  }\n\
                  structure Asm {\n\
                  \x20 sub e1 : Leaf\n\
                  \x20 sub e2 : Leaf\n\
                  \x20 connect e1.p -> e2.p : Prismatic { axis = vec3(1.0,0.0,0.0), spring_rate = 1000.0 N/m }\n\
                  }\n";
    let (_, _, errors) = parse_shape_spanned(source);
    let (message, span) = only_error_starting_with(&errors, "invalid connect: ");
    assert_eq!(
        span.start,
        byte_of(source, " N/m", 1),
        "the diagnostic must point at the detached unit word `N`; got {message:?}"
    );
    let hint = detached_unit_hint("N", "spring_rate", "1000.0");
    assert!(
        message.ends_with(&hint),
        "the diagnostic must name the param and the attached spelling;\n \
         expected suffix: {hint:?}\n got: {message:?}"
    );
}

#[test]
fn connect_param_detached_unit_is_hinted_for_any_horizontal_gap() {
    for gap in [" ", "\t", "   "] {
        let source = format!(
            "structure A {{ connect e1.p -> e2.p : Prismatic {{ neutral = 0.0{gap}mm }} }}"
        );
        let (_, _, errors) = parse_shape_spanned(&source);
        let (message, span) = only_error_starting_with(&errors, "invalid connect: ");
        assert_eq!(
            span.start,
            byte_of(&source, "mm", 0),
            "gap {gap:?}: the diagnostic must point at `mm`; got {message:?}"
        );
        let hint = detached_unit_hint("mm", "neutral", "0.0");
        assert!(
            message.ends_with(&hint),
            "gap {gap:?}: expected suffix {hint:?}, got {message:?}"
        );
    }
}

#[test]
fn detached_unit_inside_a_call_argument_names_the_enclosing_param() {
    let source =
        "structure A { connect e1.p -> e2.p : Prismatic { axis = vec3(1.0 mm, 0.0, 0.0) } }";
    let (_, _, errors) = parse_shape_spanned(source);
    let (message, span) = only_error_starting_with(&errors, "invalid connect: ");
    assert_eq!(
        span.start,
        byte_of(source, "mm", 0),
        "the diagnostic must point at `mm`; got {message:?}"
    );
    let hint = detached_unit_hint("mm", "axis", "1.0");
    assert!(
        message.ends_with(&hint),
        "the call's `name` field is not a binding — the hint must name the param \
         `axis` the author edits;\n expected suffix: {hint:?}\n got: {message:?}"
    );
}

#[test]
fn param_default_and_sub_override_carry_the_same_hint() {
    let hint = detached_unit_hint("N", "k", "1000.0");
    for (source, prefix) in [
        (
            "structure S { param k : TranslationalStiffness = 1000.0 N/m }",
            "invalid param: ",
        ),
        (
            "structure Leaf { param k : TranslationalStiffness = 1.0N/m }\n\
             structure S { sub l : Leaf { k = 1000.0 N/m } }",
            "invalid sub: ",
        ),
    ] {
        let (_, _, errors) = parse_shape_spanned(source);
        let (message, span) = only_error_starting_with(&errors, prefix);
        assert_eq!(
            span.start,
            byte_of(source, " N/m", 1),
            "`{source}`: the diagnostic must point at `N`; got {message:?}"
        );
        assert!(
            message.ends_with(&hint),
            "`{source}`: expected suffix {hint:?}, got {message:?}"
        );
    }
}

#[test]
fn fn_body_let_with_detached_unit_carries_the_hint() {
    let source = "fn f(x: Real) -> Real {\n  let y = 5 mm;\n  y\n}\n";
    let (_, _, errors) = parse_shape_spanned(source);
    let expected = format!(
        "syntax error in function definition{}",
        detached_unit_hint("mm", "y", "5")
    );
    let mm_at = byte_of(source, "mm", 0);
    assert!(
        errors
            .iter()
            .any(|(message, span)| *message == expected && span.start == mm_at),
        "expected {expected:?} pointing at `mm`; got {errors:?}"
    );
}

/// A structure-body `let` reaches the generic `syntax error in structure body` arm
/// rather than the `invalid <label>` refusal, so it needs the hint there too.  The
/// hint changes wording only: the arm's per-row dedupe still yields ONE diagnostic.
#[test]
fn structure_body_let_with_detached_unit_carries_the_hint() {
    let source = "structure A {\n  let q = 5 mm\n}\n";
    let (_, _, errors) = parse_shape_spanned(source);
    let expected = format!(
        "syntax error in structure body{}",
        detached_unit_hint("mm", "q", "5")
    );
    let mm_at = byte_of(source, "mm", 0);
    assert!(
        errors
            .iter()
            .any(|(message, span)| *message == expected && span.start == mm_at),
        "expected {expected:?} pointing at `mm`; got {errors:?}"
    );
    assert_eq!(
        errors
            .iter()
            .filter(|(message, _)| message.starts_with("syntax error in structure body"))
            .count(),
        1,
        "the hint must not change how many diagnostics the row yields; got {errors:?}"
    );
}

/// GREEN on arrival: the attached spelling is the accepted one, and this pins it
/// at AST level inside a connect body — both spellings are pinned, the detached
/// one by the diagnostics above.
#[test]
fn attached_unit_in_connect_param_lowers_to_a_quantity() {
    let source = "structure A { connect e1.p -> e2.p : Prismatic { \
                  spring_rate = 1000.0N/m, neutral = 0.0mm } }";
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("unit_expr_lowering_test"),
    );
    assert!(
        module.errors.is_empty(),
        "the attached spelling must parse cleanly; got {:?}",
        module.errors
    );
    let Some(Declaration::Structure(structure)) = module.declarations.first() else {
        panic!("expected a structure, got {:?}", module.declarations);
    };
    let connect = structure
        .members
        .iter()
        .find_map(|member| match member {
            MemberDecl::Connect(c) => Some(c),
            _ => None,
        })
        .expect("the structure must keep its connect member");
    let quantity_of = |name: &str| {
        let (_, expr) = connect
            .params
            .iter()
            .find(|(param, _)| param == name)
            .unwrap_or_else(|| panic!("connect must carry `{name}`; got {:?}", connect.params));
        match &expr.kind {
            ExprKind::QuantityLiteral { value, unit } => (*value, unit.clone()),
            other => panic!("`{name}` must lower to a QuantityLiteral, got {other:?}"),
        }
    };
    assert_eq!(
        quantity_of("spring_rate"),
        (1000.0, div(bare("N"), bare("m")))
    );
    assert_eq!(quantity_of("neutral"), (0.0, bare("mm")));
}

/// GREEN on arrival: the hint fires only for a word directly after a number on
/// the same line.  A newline gap is the absorbed-next-line shape, a preceding
/// unit name means the quantity is already attached, and a non-word fault is
/// not a unit at all — a wrong remedy is worse than none.
#[test]
fn detached_unit_hint_needs_a_number_directly_before_the_word_on_the_same_line() {
    for source in [
        "structure A {\n  param k : Real = 1000.0\n  N\n}",
        "structure A { let x = 5mm mm }",
        "structure A { let a = 1 1 }",
        "structure def S { let x = 5N ·m }",
    ] {
        let (_, _, errors) = parse_shape_spanned(source);
        assert!(
            !errors.is_empty(),
            "`{source}` must still produce a diagnostic"
        );
        assert!(
            errors
                .iter()
                .all(|(message, _)| !message.contains(HINT_MARK)),
            "`{source}` is not a detached unit suffix and must carry no remedy hint; \
             got {errors:?}"
        );
    }
}
