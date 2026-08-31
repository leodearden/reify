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

// ═══════════════════════════════════════════════════════════════════════════════
// `auto(seed)` in positional call-argument position — PRD §5 D6
// ═══════════════════════════════════════════════════════════════════════════════
//
// Added by step-15. RED until step-16 adds `lower_expr`'s `"auto_seed"` arm.
//
// The grammar half landed in step-8: `auto( <expr> )` is admitted in ANY
// positional call-argument position, recording Leo's 2026-07-25 OPTION B
// decision as a PARTIAL reversal of task 3808. Until `lower_expr` learns the
// node kind, it returns `None` and the argument is SILENTLY DROPPED — the
// `point(auto(10mm), 0mm)` call would lower to a one-argument `point(0mm)`,
// changing the user's value with no diagnostic. That is precisely the
// silent-accept Option B forbids, and is what these tests exist to catch.
//
// The convergence contract: positional `auto(<expr>)` lowers to EXACTLY the same
// `ExprKind::Auto` the existing named binding-site form `auto(seed = <expr>)`
// produces. One AST shape, two surfaces. That is what keeps the `Expr` blast
// radius at zero (~1590 `Expr::` sites in crates/*/src) and what makes the
// pre-existing `E_AUTO_NOT_AT_BINDING_SITE` gate fire on the new surface for
// free — Option B's "typed semantic rejection outside sketch scope".

// ── auto_seed helpers ────────────────────────────────────────────────────────

/// Parse `source` and return the value expression of the first structure-level
/// `let`.
fn first_let_value(source: &str) -> Expr {
    structure_members(source)
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) => Some(l.value.clone()),
            _ => None,
        })
        .expect("expected a structure-level let member")
}

/// Parse `source` and return the default expression of the first structure-level
/// `param`.
fn first_param_default(source: &str) -> Expr {
    structure_members(source)
        .iter()
        .find_map(|m| match m {
            MemberDecl::Param(p) => p.default.clone(),
            _ => None,
        })
        .expect("expected a structure-level param with a default")
}

/// The positional arguments of a `FunctionCall` expression.
fn call_args(expr: &Expr) -> &[Expr] {
    match &expr.kind {
        ExprKind::FunctionCall { args, .. } => args,
        other => panic!("expected ExprKind::FunctionCall, got {other:?}"),
    }
}

/// Destructure an `ExprKind::Auto`, panicking with the actual kind otherwise.
fn as_auto(expr: &Expr) -> (bool, Vec<(String, Expr)>) {
    match &expr.kind {
        ExprKind::Auto { free, params } => (*free, params.clone()),
        other => panic!("expected ExprKind::Auto, got {other:?}"),
    }
}

/// Assert an expression is the quantity literal `<value><unit>`.
fn assert_quantity(expr: &Expr, value: f64, unit: &str) {
    match &expr.kind {
        ExprKind::QuantityLiteral { value: v, unit: u } => {
            assert_eq!(*v, value, "quantity magnitude");
            assert_eq!(
                *u,
                UnitExpr::Unit(unit.to_string()),
                "quantity unit"
            );
        }
        other => panic!("expected ExprKind::QuantityLiteral, got {other:?}"),
    }
}

// ── The new surface ──────────────────────────────────────────────────────────

/// `point(auto(10mm), 0mm)` — the positional seed form lowers to
/// `ExprKind::Auto { free: false, params: [("seed", 10mm)] }`.
///
/// The param NAME is the load-bearing part: `"seed"` is the canonical name the
/// existing named form already uses (corpus `auto(seed = 5mm)`), so every
/// downstream consumer of `ExprKind::Auto.params` reads the positional form
/// without knowing it exists.
#[test]
fn positional_auto_seed_lowers_to_expr_kind_auto_with_a_seed_param() {
    let value = first_let_value("structure def T { let b = point(auto(10mm), 0mm) }");
    let args = call_args(&value);
    assert_eq!(args.len(), 2, "the auto arg must survive, not be dropped: {args:?}");

    let (free, params) = as_auto(&args[0]);
    assert!(!free, "the positional seed form is not `free`");
    assert_eq!(params.len(), 1, "exactly one param, got {params:?}");
    assert_eq!(params[0].0, "seed", "the param must be named `seed`");
    assert_quantity(&params[0].1, 10.0, "mm");

    // The second argument is untouched — a lowering that mis-indexed the arg
    // list would still satisfy the assertions above.
    assert_quantity(&args[1], 0.0, "mm");
}

/// The `auto_seed` node's span covers the whole `auto(10mm)` text, not just the
/// seed sub-expression.
#[test]
fn positional_auto_seed_span_covers_the_whole_construct() {
    const SOURCE: &str = "structure def T { let b = point(auto(10mm), 0mm) }";
    let value = first_let_value(SOURCE);
    let auto_arg = &call_args(&value)[0];
    let text = &SOURCE[auto_arg.span.start as usize..auto_arg.span.end as usize];
    assert_eq!(
        text, "auto(10mm)",
        "span must cover the whole construct, got {text:?}"
    );
}

// ── Convergence with the existing binding-site surface ───────────────────────

/// `param p : Frame = auto(seed = 5mm)` still lowers through `_binding_value` to
/// the SAME `ExprKind::Auto` shape.
///
/// This is the convergence assertion: two surfaces, one AST shape. If the new
/// arm had introduced a distinct `ExprKind`, this test would keep passing while
/// every downstream `ExprKind::Auto` consumer silently ignored the new form.
#[test]
fn named_auto_seed_at_a_binding_site_lowers_to_the_same_shape() {
    let default = first_param_default("structure def T { param p : Frame = auto(seed = 5mm) }");
    let (free, params) = as_auto(&default);
    assert!(!free);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].0, "seed");
    assert_quantity(&params[0].1, 5.0, "mm");
}

/// Bare `auto` at a binding site still lowers to `params: vec![]`.
///
/// The negative control for the convergence test above: the new arm must not
/// steal or reshape the pre-existing binding-site forms.
#[test]
fn bare_auto_at_a_binding_site_still_lowers_to_empty_params() {
    let default = first_param_default("structure def T { param p : Frame = auto }");
    let (free, params) = as_auto(&default);
    assert!(!free, "bare `auto` is strict, not free");
    assert!(params.is_empty(), "bare `auto` carries no params, got {params:?}");
}

// ── The PRD gate fixture ─────────────────────────────────────────────────────

/// The committed `auto(seed)` PRD gate fixture lowers with zero diagnostics, and
/// its `auto(10mm)` argument survives into the AST.
///
/// The second headline user-observable signal, at AST level. Asserting the
/// argument SURVIVES (not merely that the file lowers without complaint) is the
/// point: `lower_expr` returning `None` produces a clean lowering of the wrong
/// program.
#[test]
fn sketch_auto_seed_target_fixture_lowers_cleanly() {
    const FIXTURE: &str =
        include_str!("../../../../tests/prd-gate/fixtures/sketch_auto_seed_target.ri");

    let members = structure_members(FIXTURE);
    let sketch = members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sketch(s) => Some(s),
            _ => None,
        })
        .unwrap_or_else(|| panic!("fixture must contain a sketch member: {members:?}"));

    let b = sketch
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "b" => Some(l),
            _ => None,
        })
        .expect("fixture's sketch body declares `let b = point(auto(10mm), 0mm)`");

    let args = call_args(&b.value);
    assert_eq!(
        args.len(),
        2,
        "`point(auto(10mm), 0mm)` must keep both arguments: {args:?}"
    );
    let (free, params) = as_auto(&args[0]);
    assert!(!free);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].0, "seed");
    assert_quantity(&params[0].1, 10.0, "mm");
}
