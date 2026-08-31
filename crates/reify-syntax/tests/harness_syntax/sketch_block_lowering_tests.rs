//! CST → AST lowering tests for the member-level `sketch { … }` block
//! (constrained-2d-sketch α, task 5506; PRD `docs/prds/v0_6/constrained-2d-sketch.md`).
//!
//! AST-level companion to `tree-sitter-reify/tests/sketch_grammar_tests.rs`
//! (CST-level). Modelled on the sibling `relate_at_auto_lowering_tests.rs`.
//!
//! ## What this file is for
//!
//! `lower_member`'s fallback is `_ => None` — an unrecognised CST member kind is
//! SILENTLY DROPPED, with no diagnostic. So between the grammar landing
//! (step-2) and the lowering landing (step-14), `sketch profile { … }` parses
//! clean, vanishes from the AST, and compiles to nothing: the user's constraints
//! disappear with no error at all. That is the INV-SF-1 / PRD §5 D14 silent-no-op
//! this file closes.
//!
//! ## Status
//!
//! RED at step-13, GREEN at step-14 (which adds `lower_sketch_block` and its
//! `lower_member` dispatch arm). The `auto_seed` half is added by step-15.
//!
//! ## Body representation
//!
//! A sketch body admits exactly two CST member kinds (grammar.js's `sketch_block`
//! rule): `let_declaration` and `relation_member`. They lower onto EXISTING
//! `MemberDecl` variants — `Let` (whose pre-existing `is_aux` flag delivers PRD
//! §5 D12's construction geometry with no new field) and `Relate` (a
//! single-expression `RelateDecl`, the same bare-expression shape `relate { }`
//! already uses). No `sketch_member` wrapper variant is invented: a second new
//! `MemberDecl` variant would re-open the whole exhaustiveness blast radius for
//! zero gain.

use reify_ast::*;
use reify_core::{ContentHash, ModulePath};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse `source`, assert zero parse errors, and return the first declaration's
/// structure members.
fn structure_members(source: &str) -> Vec<MemberDecl> {
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors: {:?}",
        module.errors
    );
    match &module.declarations[0] {
        Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure, got {other:?}"),
    }
}

/// Parse `source` and return the one and only `SketchDecl` among the first
/// structure's members, asserting there is exactly one.
fn only_sketch(source: &str) -> SketchDecl {
    let members = structure_members(source);
    let sketches: Vec<&SketchDecl> = members
        .iter()
        .filter_map(|m| match m {
            MemberDecl::Sketch(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        sketches.len(),
        1,
        "expected exactly one MemberDecl::Sketch, got {}: {members:?}",
        sketches.len()
    );
    sketches[0].clone()
}

/// The relation expressions carried by a bare-expression sketch body member.
fn relation_exprs(member: &MemberDecl) -> &[Expr] {
    match member {
        MemberDecl::Relate(r) => &r.relations,
        other => panic!("expected a bare relation member (MemberDecl::Relate), got {other:?}"),
    }
}

/// The `let` a sketch body member is, or a panic naming what it actually was.
fn as_let(member: &MemberDecl) -> &LetDecl {
    match member {
        MemberDecl::Let(l) => l,
        other => panic!("expected MemberDecl::Let, got {other:?}"),
    }
}

/// The callee name of a `FunctionCall` expression.
fn call_name(expr: &Expr) -> &str {
    match &expr.kind {
        ExprKind::FunctionCall { name, .. } => name,
        other => panic!("expected ExprKind::FunctionCall, got {other:?}"),
    }
}

/// The canonical multi-member snippet every structural assertion below reads.
const SKETCH_SOURCE: &str = r#"structure def T {
    sketch profile {
        aux let cl = line(a, b)
        let p = point(0mm, 0mm)
        fix(p)
        horizontal(cl)
    }
}"#;

// ── The block itself ─────────────────────────────────────────────────────────

/// The block lowers to exactly one `MemberDecl::Sketch` carrying its name.
#[test]
fn sketch_block_lowers_to_one_named_sketch_member() {
    let sketch = only_sketch(SKETCH_SOURCE);
    assert_eq!(sketch.name, "profile");
}

/// The body's `let`s are NESTED inside the sketch, never leaked to the
/// structure's own member list.
///
/// This is the containment half of PRD §7 C1 ("not visible outside the block in
/// v1"): a lowering that spliced the body's members into the enclosing structure
/// would satisfy every count assertion below while silently making `cl` and `p`
/// entity-level bindings.
#[test]
fn sketch_body_lets_do_not_leak_to_structure_level() {
    let members = structure_members(SKETCH_SOURCE);
    let leaked: Vec<&LetDecl> = members
        .iter()
        .filter_map(|m| match m {
            MemberDecl::Let(l) => Some(l),
            _ => None,
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "sketch-body lets must stay nested, but these reached structure level: {leaked:?}"
    );
    assert_eq!(
        members.len(),
        1,
        "the structure has exactly one member — the sketch: {members:?}"
    );
}

// ── Body contents, in declaration order ──────────────────────────────────────

/// The body carries all four members in DECLARATION ORDER, unclassified.
///
/// Order is the determinism contract (PRD §7 C2): γ's entity/constraint split
/// reads this vector, and a lowering that grouped lets before relations (or the
/// reverse) would silently reorder the user's sketch.
#[test]
fn sketch_body_members_are_carried_in_declaration_order() {
    let sketch = only_sketch(SKETCH_SOURCE);
    assert_eq!(
        sketch.members.len(),
        4,
        "expected 4 body members (2 lets, 2 relations), got {}: {:?}",
        sketch.members.len(),
        sketch.members
    );

    let cl = as_let(&sketch.members[0]);
    assert_eq!(cl.name, "cl");
    let p = as_let(&sketch.members[1]);
    assert_eq!(p.name, "p");

    let fix = relation_exprs(&sketch.members[2]);
    assert_eq!(fix.len(), 1, "one bare expression per relation member");
    assert_eq!(call_name(&fix[0]), "fix");

    let horizontal = relation_exprs(&sketch.members[3]);
    assert_eq!(horizontal.len(), 1);
    assert_eq!(call_name(&horizontal[0]), "horizontal");
}

/// `aux let` sets `is_aux`, and a plain `let` does not.
///
/// PRD §5 D12's construction geometry rides the EXISTING `aux` marking rather
/// than a new field: `lower_let` already calls `has_aux_keyword`, so
/// `lower_sketch_block` must NOT reimplement the detection. Asserting both
/// polarities is what catches a lowering that hard-coded `is_aux: true` (or
/// `false`) for every sketch-body let.
#[test]
fn aux_let_in_a_sketch_body_rides_the_existing_aux_marking() {
    let sketch = only_sketch(SKETCH_SOURCE);
    assert!(
        as_let(&sketch.members[0]).is_aux,
        "`aux let cl = …` must lower with is_aux = true"
    );
    assert!(
        !as_let(&sketch.members[1]).is_aux,
        "plain `let p = …` must lower with is_aux = false"
    );
}

// ── Span and hash — the pair every payload struct carries ────────────────────

/// `span` covers the whole block and `content_hash` is non-default.
///
/// Six exhaustive matches in `ts_parser.rs`'s own test module read this pair off
/// every `MemberDecl`; a `SketchDecl` with a zero hash or a collapsed span would
/// pass those matches while breaking incremental-reparse identity.
#[test]
fn sketch_span_covers_the_block_and_hash_is_non_default() {
    let sketch = only_sketch(SKETCH_SOURCE);
    let text = &SKETCH_SOURCE[sketch.span.start as usize..sketch.span.end as usize];
    assert!(
        text.starts_with("sketch profile"),
        "span must start at the `sketch` keyword, got {text:?}"
    );
    assert!(
        text.ends_with('}'),
        "span must run to the block's closing brace, got {text:?}"
    );
    assert!(
        text.contains("horizontal(cl)"),
        "span must cover the whole body, got {text:?}"
    );
    assert_ne!(
        sketch.content_hash,
        ContentHash(0),
        "content_hash must be populated, not left at the zero default"
    );
}

// ── Empty block — the `relate { }` parity case ───────────────────────────────

/// `sketch s { }` lowers to a sketch with no members — NOT to nothing.
///
/// The grammar admits the empty block (`repeat` is zero-or-more, matching
/// `relate { }`), so lowering must produce the member rather than dropping it:
/// `Some(SketchDecl { members: vec![] })`, not `None`.
#[test]
fn empty_sketch_block_lowers_to_an_empty_member_list() {
    let sketch = only_sketch("structure def T { sketch s { } }");
    assert_eq!(sketch.name, "s");
    assert!(
        sketch.members.is_empty(),
        "expected no body members, got {:?}",
        sketch.members
    );
}

// ── The PRD gate fixture, end to end ─────────────────────────────────────────

/// The committed PRD gate fixture lowers with zero diagnostics and a fully
/// populated sketch body.
///
/// This is the headline user-observable signal at AST level, the companion to
/// the CST-level `sketch_block_target_fixture_parses_with_zero_error_nodes`.
/// `include_str!` also gives compile-time drift detection if the fixture moves
/// or is deleted; the basename is registered in `_RUST_COUPLED_RI_FIXTURES`
/// (scripts/verify.sh) because of exactly this reference.
#[test]
fn sketch_block_target_fixture_lowers_cleanly() {
    const FIXTURE: &str =
        include_str!("../../../../tests/prd-gate/fixtures/sketch_block_target.ri");

    let members = structure_members(FIXTURE);
    let sketch = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sketch(s) => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture must contain a sketch member: {members:?}"));

    assert_eq!(sketch.name, "profile");

    // Counted from the committed fixture, not guessed: 5 `let`s (the first
    // `aux`) then 5 bare relation members.
    let lets: Vec<&MemberDecl> = sketch
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Let(_)))
        .collect();
    let relations: Vec<&MemberDecl> = sketch
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Relate(_)))
        .collect();
    assert_eq!(lets.len(), 5, "fixture has 5 let members: {:?}", sketch.members);
    assert_eq!(
        relations.len(),
        5,
        "fixture has 5 bare relation members: {:?}",
        sketch.members
    );
    assert_eq!(
        sketch.members.len(),
        10,
        "and nothing else — every body child must lower to exactly one member"
    );

    assert_eq!(
        sketch.members.iter().filter(|m| matches!(m, MemberDecl::Let(l) if l.is_aux)).count(),
        1,
        "exactly one `aux let` (the `cl` centreline)"
    );

    // The relation callees, in source order — the fixture's whole point.
    let callees: Vec<&str> = relations
        .iter()
        .map(|m| call_name(&relation_exprs(m)[0]))
        .collect();
    assert_eq!(
        callees,
        vec!["fix", "horizontal", "tangent", "distance", "symmetric"],
        "relation members must survive lowering in source order"
    );
}
