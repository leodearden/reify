//! AST-level (CST→AST lowering) tests for the **indexer clause** on the `sub`
//! instantiation arm — `sub idlers[i in 0..4] = Pulley(…)`.
//!
//! Task α of `docs/prds/v0_6/indexed-sub-instantiation.md`. The CST half of the
//! contract lives in `tree-sitter-reify/tests/indexed_sub_grammar_tests.rs`;
//! this file pins that the new `binder`/`domain` CST fields actually reach
//! `SubDecl.index_binder` / `SubDecl.index_domain`.
//!
//! # Scope: α is syntax only
//!
//! α stores the binder and domain **syntactically**. Binder scoping, typing the
//! domain as `Range<Int>`, and the derived count cell are all β's job — so
//! downstream *semantic* diagnostics on the target-surface fixture are expected
//! and legitimate until β lands. See
//! [`surface_fixture_no_longer_emits_invalid_sub_parse_error`] for why that
//! test asserts the absence of one specific message rather than an empty
//! diagnostic list.
//!
//! # TDD status
//!
//! All four tests are **RED** before step-4 — `SubDecl::index_binder` /
//! `index_domain` do not exist yet, so the RED signal is a compile failure.

use reify_ast::ast::ExprKind;
use reify_ast::decl::{Declaration, MemberDecl, SubDecl};
use reify_core::ModulePath;

/// The α target surface: the committed PRD fixture whose only parse blocker was
/// the indexer clause.
const SURFACE_FIXTURE: &str =
    include_str!("../../../../docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri");

/// Canonical indexed-sub source shared by the lowering assertions.
const INDEXED_SUB_SOURCE: &str = "structure S { sub idlers[i in 0..4] = Pulley(od: 30mm + i * 2mm) at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }";

/// Parse `source` and return the members of its first structure declaration.
fn parse_first_structure_members(source: &str) -> Vec<MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure, got {other:?}"),
    }
}

/// Locate the first `MemberDecl::Sub` in a member slice.
fn first_sub(members: &[MemberDecl]) -> SubDecl {
    members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Sub(s) => Some(s.clone()),
            _ => None,
        })
        .expect("expected at least one MemberDecl::Sub in the parsed structure")
}

/// α headline signal, library-level: the committed target-surface fixture no
/// longer trips the `check_and_lower!` "invalid sub" parse diagnostic.
///
/// The CLI renders this as `Parse error: invalid sub …`
/// (`reify-cli/src/main.rs`), and the message itself is minted by the
/// `check_and_lower!` macro in `ts_parser.rs` whenever the dispatched
/// `sub_declaration` child `is_error()` or `has_error()`. Running
/// `reify_syntax::parse` directly gives the same signal without a binary build.
///
/// Deliberately asserts only the ABSENCE of that one message — **not**
/// `errors.is_empty()`. α does not type the domain as `Range<Int>`, does not
/// scope the binder, and does not create the count cell, so remaining semantic
/// diagnostics are β's to clear. Asserting an empty list would make this a β
/// capability check that α structurally cannot turn green.
#[test]
fn surface_fixture_no_longer_emits_invalid_sub_parse_error() {
    let parsed = reify_syntax::parse(SURFACE_FIXTURE, ModulePath::single("test"));
    let offending: Vec<&str> = parsed
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .filter(|m| m.contains("invalid sub"))
        .collect();
    assert!(
        offending.is_empty(),
        "the indexed sub must no longer produce an `invalid sub` parse error; got: {offending:?}\n\
         (all diagnostics: {:?})",
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// The indexer clause lowers into `index_binder` / `index_domain`.
///
/// The binder span must cover ONLY the `i` character — a whole-declaration span
/// would be useless to the `W_UNUSED`-style unused-binder diagnostic that PRD
/// §9.1 Q1 leans on when it rules out a binder-omission form. `i` is located
/// via `source.find("i in")` rather than hand-computed offsets so the assertion
/// survives whitespace edits to the test source (mirrors
/// `quantifier_tests::parse_quantifier_populates_variable_span`).
#[test]
fn indexed_sub_lowers_binder_and_domain() {
    let members = parse_first_structure_members(INDEXED_SUB_SOURCE);
    let sub = first_sub(&members);

    let binder = sub
        .index_binder
        .as_ref()
        .expect("indexed sub must lower index_binder to Some(..)");
    assert_eq!(binder.name, "i", "binder name must be the index variable");

    let off = INDEXED_SUB_SOURCE
        .find("i in")
        .expect("test source must contain the binder");
    assert_eq!(
        binder.span.start, off as u32,
        "binder span must start at the binder identifier `i`"
    );
    assert_eq!(
        binder.span.end,
        (off + 1) as u32,
        "binder span must end one byte past `i` — it must cover the binder \
         alone, so an unused-binder diagnostic can underline exactly it"
    );
    assert!(
        binder.span.end - binder.span.start < sub.span.end - sub.span.start,
        "binder span must be strictly narrower than the whole sub declaration"
    );

    let domain = sub
        .index_domain
        .as_ref()
        .expect("indexed sub must lower index_domain to Some(..)");
    match &domain.kind {
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(
                lower.is_some() && upper.is_some(),
                "`0..4` is a two-sided range, so both bounds must be present"
            );
            assert!(
                matches!((lower_inclusive, upper_inclusive), (true, true)),
                "`..` (as opposed to `..<`) is inclusive on both ends"
            );
        }
        other => panic!("expected index_domain to lower to ExprKind::Range, got {other:?}"),
    }
}

/// The indexer must not disturb the rest of the instantiation lowering.
///
/// The `is_collection == false` assertion is the explicit hazard guard, not a
/// mere scope marker: `lower_sub`'s collection detection text-scans the DIRECT
/// children of `sub_declaration` for a `List` token/text, and the binder
/// identifier is a NEW direct child introduced by this very change. This pins
/// that α leaves `is_collection` driven by the `List` keyword alone — flipping
/// it here would route an indexed sub into the existing collection-sub compile
/// path with no count cell and no element template, i.e. exactly the silent
/// zero-element elaboration the PRD forbids. Collection semantics are β's.
#[test]
fn indexed_sub_preserves_existing_instantiation_lowering() {
    let members = parse_first_structure_members(INDEXED_SUB_SOURCE);
    let sub = first_sub(&members);

    assert_eq!(sub.name, "idlers", "sub name must be unaffected");
    assert_eq!(
        sub.structure_name, "Pulley",
        "constructed structure name must be unaffected"
    );
    assert!(
        sub.args.iter().any(|(k, _)| k == "od"),
        "named constructor arg `od` must still lower; got keys {:?}",
        sub.args.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
    assert!(
        sub.pose_expr.is_some(),
        "the `at <pose>` clause must still lower to pose_expr"
    );
    assert!(
        !sub.is_collection,
        "α must leave is_collection == false for an indexed sub — collection \
         elaboration is β's, and the binder identifier is a new direct child \
         that must not trip lower_sub's `List` text-scan"
    );
}

/// The three pre-existing `sub` arms lower with both indexer fields `None`.
#[test]
fn non_indexed_subs_lower_to_none_binder_and_domain() {
    let cases: &[(&str, &str)] = &[
        (
            "bare instantiation with pose",
            "structure S { sub a = Foo() at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }",
        ),
        ("collection arm", "structure S { sub xs : List<Foo> }"),
        ("specialization arm", "structure S { sub m : Foo { } }"),
    ];

    for (label, source) in cases {
        let members = parse_first_structure_members(source);
        let sub = first_sub(&members);
        assert!(
            sub.index_binder.is_none(),
            "{label}: non-indexed sub must have index_binder == None, got {:?}",
            sub.index_binder
        );
        assert!(
            sub.index_domain.is_none(),
            "{label}: non-indexed sub must have index_domain == None"
        );
    }
}
