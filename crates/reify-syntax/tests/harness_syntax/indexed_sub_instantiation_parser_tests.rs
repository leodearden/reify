//! AST-level (CST→AST lowering) tests for the **indexer clause** on the `sub`
//! instantiation arm — `sub idlers[i in 0..4] = Pulley(…)`.
//!
//! Task α of `docs/prds/v0_6/indexed-sub-instantiation.md`. The CST half of the
//! contract lives in `tree-sitter-reify/tests/indexed_sub_grammar_tests.rs`;
//! this file pins that the new `binder`/`domain` CST fields actually reach
//! `SubDecl.index_binder` / `SubDecl.index_domain`.
//!
//! Interim α→β rejection: rationale lives at the `TODO(#5482)` site in
//! `lower_sub` (`crates/reify-syntax/src/ts_parser.rs`) — the single home for
//! that narrative. [`indexed_sub_emits_interim_unelaborated_diagnostic`] pins
//! the contract; [`non_indexed_subs_emit_no_interim_diagnostic`] is the
//! over-firing guard.

use reify_ast::ast::ExprKind;
use reify_ast::decl::{Declaration, MemberDecl, SubDecl};
use reify_core::ModulePath;

/// The α target surface: the committed PRD fixture whose only parse blocker was
/// the indexer clause.
const SURFACE_FIXTURE: &str =
    include_str!("../../../../docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri");

/// Canonical indexed-sub source shared by the lowering assertions.
const INDEXED_SUB_SOURCE: &str = "structure S { sub idlers[i in 0..4] = Pulley(od: 30mm + i * 2mm) at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }";

/// An indexer clause whose domain parses but does NOT lower.
///
/// `a.(b)` is an `instance_qualified_access`, whose grammar accepts ANY
/// `$._expression` inside the parentheses; `lower_instance_qualified_access`
/// then rejects a non-`qualified_access` inner node and returns `None`
/// (`ts_parser.rs`, "instance qualified access requires a qualified_access").
/// So this is a well-formed CST — no ERROR node, no `invalid sub` — whose
/// `lower_expr(domain)` genuinely returns `None`, which is the only way to
/// reach `lower_sub`'s drop-both arm. `a.(B::c)` would lower fine; the missing
/// `::` is the whole point.
const MALFORMED_DOMAIN_SOURCE: &str = "structure S { sub xs[i in a.(b)] = Foo(a: 1) }";

/// The three pre-existing `sub` arms — the α regression floor.
///
/// Hoisted to module scope because three separate tests assert over exactly
/// this set (no interim diagnostic, both indexer fields `None`, and the pairing
/// invariant); a fourth arm added to `sub_declaration` must be remembered once,
/// here, not in three places.
const NON_INDEXED_ARMS: &[(&str, &str)] = &[
    (
        "bare instantiation with pose",
        "structure S { sub a = Foo() at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }",
    ),
    ("collection arm", "structure S { sub xs : List<Foo> }"),
    ("specialization arm", "structure S { sub m : Foo { } }"),
];

/// Parse `source` and return the members of its first structure declaration.
fn parse_first_structure_members(source: &str) -> Vec<MemberDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    match &parsed.declarations[0] {
        Declaration::Structure(s) => s.members.clone(),
        other => panic!("expected Structure, got {other:?}"),
    }
}

/// Collect every `SubDecl` in `source`, across all structure declarations.
///
/// Unlike [`parse_first_structure_members`] this does not assume
/// `declarations[0]` is a structure, so it also works on the multi-declaration
/// surface fixture.
fn all_subs(source: &str) -> Vec<SubDecl> {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    parsed
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Structure(s) => Some(s.members.clone()),
            _ => None,
        })
        .flatten()
        .filter_map(|m| match m {
            MemberDecl::Sub(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// The diagnostics whose message carries the interim `#5482` rejection cite.
///
/// Filtering on the task cite rather than on the full message keeps the
/// assertions robust to wording edits while staying exact about *which*
/// diagnostic is meant: `#5482` is the canonical, greppable marker β deletes.
fn interim_diagnostics(source: &str) -> Vec<reify_ast::decl::ParseError> {
    reify_syntax::parse(source, ModulePath::single("test"))
        .errors
        .iter()
        .filter(|e| e.message.contains("#5482"))
        .cloned()
        .collect()
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
///
/// # The fixture is still rejected — by a *named* diagnostic now
///
/// Clearing `invalid sub` does not make this source compile, and it must not.
/// On the PRD target surface the ctor arg is `Pulley(od: 30mm + i * 2mm)`: the
/// binder `i` is **unscoped** at α, so without a rejection that `i` would
/// resolve against nothing while the declaration still elaborated to one
/// instance. So α trades a *generic* parse error for a *specific, actionable*
/// one — the interim `#5482` rejection asserted below, which β deletes when it
/// scopes the binder and derives the count cell.
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

    // …but it IS still rejected: the fixture declares exactly one indexed sub,
    // so exactly one interim rejection must fire on it.
    let interim = interim_diagnostics(SURFACE_FIXTURE);
    assert_eq!(
        interim.len(),
        1,
        "the committed PRD surface fixture declares exactly one indexed sub, so \
         exactly one interim `#5482` rejection must fire on it; got {:?}\n\
         (all diagnostics: {:?})",
        interim.iter().map(|e| &e.message).collect::<Vec<_>>(),
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// α closes the silent-miscompile window: an indexed sub is REJECTED, loudly.
///
/// The clause parses and lowers (that is α's contract, and β builds on the
/// populated `SubDecl`), but **no compiler pass reads it yet** — so left
/// unguarded the source would elaborate to exactly ONE instance instead of one
/// per index, silently. The rejection is emitted over the parse-error channel,
/// which every consumer already hard-aborts on (`reify-cli/src/main.rs`,
/// `mcp_context.rs`, and the `parse_or_panic` family in reify-test-support).
///
/// The message must carry the `#5482` cite so β's deletion of the line is
/// greppable and the cite is canonical per CLAUDE.md's TODO-citation
/// convention; the span must underline the indexer clause interior
/// `i in 0..4` rather than the whole `sub`, so the diagnostic points at the
/// construct the user must remove. Offsets are located via `find` rather than
/// hand-computed constants, mirroring `indexed_sub_lowers_binder_and_domain`.
#[test]
fn indexed_sub_emits_interim_unelaborated_diagnostic() {
    let interim = interim_diagnostics(INDEXED_SUB_SOURCE);
    assert_eq!(
        interim.len(),
        1,
        "an indexed sub must emit exactly one interim `#5482` rejection; got {:?}",
        interim.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let diag = &interim[0];

    assert!(
        diag.message.contains("idlers"),
        "the rejection must name the offending sub so it is locatable in a \
         multi-sub structure; got {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("idlers[i in"),
        "the rejection must echo the binder as written, so the user can see \
         which clause to remove; got {:?}",
        diag.message
    );
    assert!(
        !diag.message.contains("invalid sub"),
        "the rejection must NOT reuse the `check_and_lower!` `invalid sub` \
         wording — that string is the α headline signal asserted absent by \
         `surface_fixture_no_longer_emits_invalid_sub_parse_error`; got {:?}",
        diag.message
    );

    let binder_off = INDEXED_SUB_SOURCE
        .find("i in")
        .expect("test source must contain the binder");
    let domain_end = INDEXED_SUB_SOURCE
        .find("0..4")
        .expect("test source must contain the domain")
        + "0..4".len();
    assert_eq!(
        diag.span.start, binder_off as u32,
        "the rejection must start at the binder, not at the `sub` keyword"
    );
    assert_eq!(
        diag.span.end, domain_end as u32,
        "the rejection must end at the domain, so it underlines exactly the \
         indexer clause interior `i in 0..4`"
    );
}

/// The over-firing guard: the rejection fires ONLY when a clause is present.
///
/// The three pre-existing arms are the α regression floor — a diagnostic that
/// leaked onto them would reject every `sub` in the corpus. This asserts the
/// stronger `errors.is_empty()` (not merely "no `#5482`"), because these three
/// arms genuinely parse clean today and must continue to.
#[test]
fn non_indexed_subs_emit_no_interim_diagnostic() {
    for (label, source) in NON_INDEXED_ARMS {
        let parsed = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "{label}: a sub with no indexer clause must parse with no \
             diagnostics at all — the interim `#5482` rejection must not \
             over-fire onto the arms the α regression floor protects; got {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
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
/// mere scope marker: `lower_sub`'s collection detection scans the DIRECT
/// children of `sub_declaration` for the `List` keyword token, and the binder
/// identifier and domain expression are NEW direct children introduced by this
/// very change. This pins that α leaves `is_collection` driven by the anonymous
/// `'List'` keyword token alone — flipping it here would route an indexed sub
/// into the existing collection-sub compile path with no count cell and no
/// element template, i.e. exactly the silent zero-element elaboration the PRD
/// forbids. Collection semantics are β's.
///
/// The adversarial `List`-named binder/domain cases below are the teeth of that
/// guard: before the α amendment, `lower_sub` also matched any direct child
/// whose *source text* was `List`, so `sub xs[List in 0..4] = Foo(a: 1)` and
/// `sub xs[i in List] = Foo(a: 1)` both flipped `is_collection` to `true` —
/// which discards `type_args` and skips the `named_argument_list` loop entirely,
/// silently producing a collection-form `SubDecl` for source the user wrote as
/// an instantiation. `List` is not a reserved word (grammar.js declares no
/// `word:` rule), so both spellings are legal input, not pathological.
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
         that must not trip lower_sub's `List` keyword scan"
    );

    // Adversarial cases: a binder or a domain whose source text is exactly
    // `List`. Both must stay on the instantiation path.
    let list_named_cases: &[(&str, &str)] = &[
        (
            "binder named `List`",
            "structure S { sub xs[List in 0..4] = Foo(a: 1) }",
        ),
        (
            "domain is the bare identifier `List`",
            "structure S { sub xs[i in List] = Foo(a: 1) }",
        ),
    ];
    for (label, source) in list_named_cases {
        let members = parse_first_structure_members(source);
        let sub = first_sub(&members);
        assert!(
            !sub.is_collection,
            "{label}: `List` as binder/domain text must NOT flip is_collection — \
             only the anonymous `'List'` keyword token of the collection arm may"
        );
        assert_eq!(
            sub.structure_name, "Foo",
            "{label}: structure_name must still lower to `Foo`"
        );
        assert!(
            sub.args.iter().any(|(k, _)| k == "a"),
            "{label}: named constructor arg `a` must still lower (the collection \
             branch skips the named_argument_list loop entirely); got keys {:?}",
            sub.args.iter().map(|(k, _)| k).collect::<Vec<_>>()
        );
        assert!(
            sub.index_binder.is_some() && sub.index_domain.is_some(),
            "{label}: the indexer clause must still lower as a pair, got \
             binder={:?} domain_is_some={}",
            sub.index_binder,
            sub.index_domain.is_some()
        );
    }
}

/// The `index_binder` / `index_domain` pairing invariant holds for every arm.
///
/// `SubDecl`'s two flat `Option`s cannot encode "both or neither" in the type,
/// so the invariant is enforced at the single producer (`lower_sub` lowers the
/// halves jointly and drops both if the domain fails to lower). This test is the
/// executable half of that contract, and it covers BOTH sides of it: the
/// populate-both path (the indexed form, the `List`-named adversarial
/// spellings, the surface fixture), the never-populate path (the three
/// pre-existing arms), and — via [`MALFORMED_DOMAIN_SOURCE`] — the drop-both
/// path, the only branch where a naive independent `.and_then(…)` would
/// actually produce a half-populated pair. β types the domain and may rely on
/// the pair.
#[test]
fn index_binder_and_domain_are_always_both_some_or_both_none() {
    let cases: Vec<&str> = [
        INDEXED_SUB_SOURCE,
        "structure S { sub legs[k in 0..3] = Leg() }",
        "structure S { sub xs[List in 0..4] = Foo(a: 1) }",
        "structure S { sub xs[i in List] = Foo(a: 1) }",
        MALFORMED_DOMAIN_SOURCE,
        SURFACE_FIXTURE,
    ]
    .into_iter()
    .chain(NON_INDEXED_ARMS.iter().map(|(_, source)| *source))
    .collect();
    for source in &cases {
        for sub in all_subs(source) {
            assert_eq!(
                sub.index_binder.is_some(),
                sub.index_domain.is_some(),
                "PAIRING INVARIANT BROKEN for sub `{}`: index_binder.is_some() == {} \
                 but index_domain.is_some() == {}. lower_sub must lower the indexer \
                 halves jointly and drop both on a domain-lowering failure.",
                sub.name,
                sub.index_binder.is_some(),
                sub.index_domain.is_some(),
            );
        }
    }
}

/// A domain that parses but does not lower is REPORTED, and drops both halves.
///
/// This is the drop-both arm of `lower_sub`'s joint lowering — the one branch
/// that keeps `SubDecl`'s two flat `Option`s from ever going half-populated, and
/// the reason the pairing invariant is enforced in code rather than merely
/// asserted. It is reachable on well-formed input, not only on ERROR recovery:
/// see [`MALFORMED_DOMAIN_SOURCE`] for why `a.(b)` is the reproducer.
///
/// The absence of `invalid sub` is asserted deliberately — that message is
/// minted by `check_and_lower!` whenever the `sub_declaration` CST node
/// `is_error()`/`has_error()`, so its absence is the in-crate proof that this
/// case is a genuine lowering failure over a clean CST rather than a
/// parse-error artifact.
#[test]
fn malformed_indexer_domain_is_reported_and_drops_both_halves() {
    let parsed = reify_syntax::parse(MALFORMED_DOMAIN_SOURCE, ModulePath::single("test"));
    let messages: Vec<&str> = parsed.errors.iter().map(|e| e.message.as_str()).collect();

    assert!(
        !messages.iter().any(|m| m.contains("invalid sub")),
        "the reproducer must be a clean CST — a lowering failure, not tree-sitter \
         error recovery; got {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("invalid indexer domain") && m.contains("xs[i in")),
        "a domain that fails to lower must be reported, naming the offending sub \
         and its binder; got {messages:?}"
    );

    let sub = first_sub(&parse_first_structure_members(MALFORMED_DOMAIN_SOURCE));
    assert!(
        sub.index_binder.is_none() && sub.index_domain.is_none(),
        "a failed domain lowering must drop BOTH halves — a `Some(binder)` with a \
         `None` domain is the half-populated pair β would panic on; got \
         binder={:?} domain_is_some={}",
        sub.index_binder,
        sub.index_domain.is_some()
    );
    assert!(
        interim_diagnostics(MALFORMED_DOMAIN_SOURCE).is_empty(),
        "the interim `#5482` rejection guards a POPULATED pair; with both halves \
         dropped there is nothing unelaborated left to reject, and firing both \
         diagnostics would double-report one clause; got {:?}",
        interim_diagnostics(MALFORMED_DOMAIN_SOURCE)
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );
}

/// The three pre-existing `sub` arms lower with both indexer fields `None`.
#[test]
fn non_indexed_subs_lower_to_none_binder_and_domain() {
    for (label, source) in NON_INDEXED_ARMS {
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
