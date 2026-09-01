//! INV-SF-7 (task #7094): a structure-member-body `let` silently absorbs the
//! following line.
//!
//! `extras: [/\s/, ...]` (tree-sitter-reify/grammar.js:88) makes the grammar
//! wholly newline-insensitive, and member-list bodies are bare `repeat(...)`
//! with no separator token. A member's trailing expression therefore greedily
//! joins whatever begins the next line whenever that line can continue it —
//! a leading binary operator (`- 3mm`) or a `(`-led call/argument list.
//!
//! These tests pin the DIAGNOSTIC, not the parse. The CST reading is
//! deliberately UNCHANGED by #7094 (the grammar is untouched); what changes is
//! that the join is now reported instead of being silently picked. See
//! `crates/reify-syntax/src/member_continuation.rs` for the normative rule.

use reify_core::ModulePath;

/// REPRO 1 from task #7094: leading-operator continuation inside a structure
/// member body.
const REPRO_ONE: &str = "structure S {\n  let d = 5mm\n  - 3mm\n}\n";

/// REPRO 2 from task #7094: `(`-led continuation inside a structure member
/// body.
const REPRO_TWO: &str = "structure S {\n  let x = a.b\n  (c)\n}\n";

/// Does this diagnostic message identify a member-continuation ambiguity?
///
/// Kept as one predicate so every test in this file agrees on what counts,
/// and so a wording change lands in exactly one place.
fn is_member_continuation_error(message: &str) -> bool {
    message.contains("continuation") && message.contains("member")
}

/// Collect the member-continuation diagnostics from a parse, as
/// `(span_start, span_end, message)` triples.
fn member_continuation_errors(source: &str) -> Vec<(u32, u32, String)> {
    let parsed = reify_syntax::parse(source, ModulePath::single("m"));
    parsed
        .errors
        .iter()
        .filter(|e| is_member_continuation_error(&e.message))
        .map(|e| (e.span.start, e.span.end, e.message.clone()))
        .collect()
}

// ── (a) the two repros must be reported ──────────────────────────────────────

#[test]
fn leading_operator_continuation_is_rejected_at_the_operator() {
    let src = REPRO_ONE;
    let parsed = reify_syntax::parse(src, ModulePath::single("m"));
    assert_eq!(
        parsed.errors.len(),
        1,
        "expected exactly one ParseError for the leading-operator join, got {:?}",
        parsed.errors
    );
    let err = &parsed.errors[0];

    // The span must cover ONLY the offending row-leading `-`, not the whole
    // member: a member-wide span would bury the actual boundary.
    let minus = src.find("- 3mm").expect("REPRO_ONE must contain `- 3mm`") as u32;
    assert_eq!(
        (err.span.start, err.span.end),
        (minus, minus + 1),
        "span must cover exactly the `-` token at byte {minus}, got {:?} ({:?})",
        err.span,
        &src[err.span.start as usize..err.span.end as usize]
    );

    assert!(
        is_member_continuation_error(&err.message),
        "message must name the member-continuation ambiguity, got: {:?}",
        err.message
    );
}

#[test]
fn paren_led_continuation_is_rejected_at_the_open_paren() {
    let src = REPRO_TWO;
    let parsed = reify_syntax::parse(src, ModulePath::single("m"));
    assert_eq!(
        parsed.errors.len(),
        1,
        "expected exactly one ParseError for the `(`-led join, got {:?}",
        parsed.errors
    );
    let err = &parsed.errors[0];

    let lparen = src.find("(c)").expect("REPRO_TWO must contain `(c)`") as u32;
    assert_eq!(
        (err.span.start, err.span.end),
        (lparen, lparen + 1),
        "span must cover exactly the `(` token at byte {lparen}, got {:?} ({:?})",
        err.span,
        &src[err.span.start as usize..err.span.end as usize]
    );

    assert!(
        is_member_continuation_error(&err.message),
        "message must name the member-continuation ambiguity, got: {:?}",
        err.message
    );
}

// ── (b) the fix is LOUD, not silent ──────────────────────────────────────────

/// #7094 makes the join VISIBLE; it does not change which reading the grammar
/// picks. Pinning that distinction here means a later grammar-level fix (an
/// indent-sensitive external scanner token, say) cannot change the accepted
/// reading without turning this test red — which is exactly the review it
/// deserves.
#[test]
fn the_joined_reading_is_still_what_the_grammar_produces() {
    let parsed = reify_syntax::parse(REPRO_ONE, ModulePath::single("m"));

    let structure = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) => Some(s),
            _ => None,
        })
        .expect("REPRO_ONE must lower to one structure declaration");

    assert_eq!(
        structure.members.len(),
        1,
        "the grammar still joins both lines into ONE member; got {:?}",
        structure.members
    );

    let let_decl = match &structure.members[0] {
        reify_ast::MemberDecl::Let(l) => l,
        other => panic!("expected the single member to be a Let, got {other:?}"),
    };
    assert_eq!(let_decl.name, "d");

    match &let_decl.value.kind {
        reify_ast::ExprKind::BinOp { op, .. } => {
            assert_eq!(
                op, "-",
                "the joined value is still `5mm - 3mm`; got op {op:?}"
            );
        }
        other => panic!(
            "the joined reading must still be a subtraction BinOp \
             (this fix reports the join, it does not re-parse it); got {other:?}"
        ),
    }
}

// ── (c) shared helper is exercised ───────────────────────────────────────────

#[test]
fn member_continuation_errors_helper_agrees_with_the_repro_assertions() {
    assert_eq!(
        member_continuation_errors(REPRO_ONE).len(),
        1,
        "REPRO_ONE must yield exactly one member-continuation diagnostic"
    );
    assert_eq!(
        member_continuation_errors(REPRO_TWO).len(),
        1,
        "REPRO_TWO must yield exactly one member-continuation diagnostic"
    );
}
