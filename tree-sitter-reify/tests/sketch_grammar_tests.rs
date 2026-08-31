//! Grammar (CST-level) integration tests for the member-level **`sketch`
//! block** — `sketch profile { aux let cl = …  let a = point(…)  fix(a) }`.
//!
//! Task α of `docs/prds/v0_6/constrained-2d-sketch.md` (§7 C1 gives the grammar
//! contract; §10 scopes α to syntax + AST + lowering).
//!
//! # This file IS the CI-run grammar signal
//!
//! `tree-sitter test` (the `test/corpus/` suite) is **not** invoked by CI —
//! `tree-sitter-reify/package.json` has no `scripts` block, there is no
//! Makefile, and `scripts/verify.sh` references tree-sitter only through
//! `scripts/tree-sitter-generate.sh`. The CI-enforced grammar surface is
//! `tree-sitter-reify/tests/*.rs` — i.e. this file. (A sibling file's header
//! also claims the corpus suite is not green on `main`; that claim is STALE.
//! Measured on this branch point immediately before the step-2 grammar delta:
//! **243 parses, 243 successful, 0 failed, 100.00%** — the
//! `test/corpus/imaginary_literal.txt` failure it cites is fixed. Do not
//! propagate the stale claim.)
//!
//! `tree-sitter generate --force` on that same branch point emits exactly two
//! `unnecessary conflicts` pairs, both PRE-EXISTING and unrelated:
//! `constraint_instantiation`/`constraint_declaration` and
//! `function_definition`/`function_signature` (plus an ABI-14 notice for the
//! absent `tree-sitter.json`). A THIRD pair appearing after a grammar delta is
//! a signal to fix precedence, not to accept.
//!
//! Compare that warning set AS A SET, never as text: the two pairs are emitted
//! in a DIFFERENT ORDER between runs of the same unchanged grammar (measured
//! across the step-2/step-4 regenerations here). A `diff` against a recorded
//! transcript will therefore show a spurious change; only a new *pair* means
//! anything.
//!
//! # TDD status of each test
//!
//! - [`sketch_block_target_fixture_parses_with_zero_error_nodes`] — **RED**
//!   before step-2; GREEN after. This is the task's headline user-observable
//!   signal, made CI-run. The `sketch profile {` header is the only construct
//!   the grammar cannot admit (the `tree-sitter parse` CLI puts its first
//!   ERROR at extent `[5,4]-[5,20]`); recovery from it then scatters further
//!   ERROR nodes through the block body, so the RED assertion deliberately
//!   counts ERROR/MISSING nodes rather than pinning one extent — an extent
//!   pin would be a pin on error-recovery behaviour, not on the grammar.
//! - [`sketch_block_cst_contract`] — **RED** before step-2 (the `sketch_block`
//!   node kind does not exist yet); GREEN after.
//! - [`sketch_premise_fixtures_regression_floor`] — **GREEN before and after**.
//!   The two committed PREMISE fixtures pin PRD §5 D11/D12's "already parses
//!   today" claims against the step-2 delta.
//! - [`sketch_stays_usable_as_an_identifier`] — **GREEN before and after**, and
//!   the single most important non-regression here: the contextual-keyword
//!   guard. Probe-verified exit 0 on the pre-change grammar.
//! - [`on_clause_slot_is_unclaimed_in_v1`] — **GREEN before and after**. Pins
//!   PRD §5 D2's reserved slot as an ERROR, which is what makes the future
//!   widening non-breaking.
//! - [`sketch_block_negative_controls`] — **GREEN before and after**. Header
//!   shape controls, plus the empty-body positive.
//! - [`sketch_block_admitted_in_guarded_block`] — **GREEN before and after**.
//!   Pins the `commonMembers()` -> `_guard_member` consequence at the GRAMMAR
//!   level; the semantic rejection is step-12's.
//!
//! - [`sketch_auto_seed_target_fixture_parses_with_zero_error_nodes`] —
//!   **RED** before step-8; GREEN after. The second headline signal: after
//!   step-2 this fixture's only remaining blocker is `auto(10mm)` in
//!   positional argument position.
//! - [`auto_seed_cst_contract`] — **RED** before step-8 (no `auto_seed` node
//!   kind); GREEN after.
//! - [`bare_auto_stays_rejected_in_positional_operand_position`],
//!   [`named_argument_auto_forms_unchanged`],
//!   [`auto_seed_is_not_admitted_at_binding_sites_in_v1`] — **GREEN before and
//!   after**. Together they keep the OPTION B reversal of task 3808 NARROW.
//! - [`positional_auto_free_is_a_seed_not_a_modifier`] — **RED** before step-8;
//!   GREEN after. Pins the one MEASURED consequence of grammar-generality that
//!   reads as a surprise: positional `auto(free)` is an `auto_seed`, not the
//!   `auto_keyword` modifier.
//!
//! The INV-SF-7 suite in the middle of this file is separate — see its own
//! banner comment for why it exists and how its readings were arrived at:
//! [`sketch_let_quantity_literal_does_not_absorb_the_next_member`],
//! [`sketch_argument_whitespace_splits_the_quantity_literal`],
//! [`sketch_body_records_the_known_item_boundary_join`],
//! [`sketch_body_item_boundary_matches_a_plain_member_body`],
//! [`sketch_body_keeps_consecutive_relation_members_separate`]. All GREEN after
//! step-2, all measured before being asserted.
//!
//! All inline snippets wrap members in `structure S { … }` so the grammar sees
//! them in a valid declaration context.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Walk a tree and collect all node kinds (depth-first, including anonymous nodes).
fn collect_kinds(node: tree_sitter::Node) -> Vec<String> {
    let mut kinds = vec![node.kind().to_string()];
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            kinds.extend(collect_kinds(cursor.node()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    kinds
}

/// Depth-first search for the first node with the given kind.
fn find_node_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Option<tree_sitter::Node<'a>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(found) = find_node_by_kind(cursor.node(), kind) {
                return Some(found);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

/// The α target surface: the committed PRD fixture whose only parse blocker is
/// the `sketch` block header.
const SKETCH_BLOCK_TARGET: &str =
    include_str!("../../tests/prd-gate/fixtures/sketch_block_target.ri");

/// Canonical sketch source used by the CST-shape test. Deliberately exercises
/// all four body shapes the PRD §7 C1 grammar admits: an `aux let` (D12
/// construction geometry), a plain `let`, and two bare relation expressions.
const SKETCH_SOURCE: &str = "structure S { sketch profile { aux let cl = line(a, b)  let p = point(0mm, 0mm)  fix(p)  horizontal(cl) } }";

/// Assert `source` parses with zero `ERROR`/`MISSING` nodes, naming `label` in
/// the failure message.
fn assert_parses_clean(label: &str, source: &str) {
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    let kinds = collect_kinds(root);
    let bad: Vec<&String> = kinds
        .iter()
        .filter(|k| k.as_str() == "ERROR" || k.as_str() == "MISSING")
        .collect();
    assert!(
        !root.has_error() && bad.is_empty(),
        "{label}: expected 0 ERROR/MISSING nodes, got has_error={} bad={:?}\n\
         root s-expression:\n{}",
        root.has_error(),
        bad,
        root.to_sexp()
    );
}

/// Assert `source` DOES contain an `ERROR` node, naming `label`.
///
/// Duplicated from the sibling grammar-test binaries rather than shared: a Rust
/// integration test cannot import helpers from a sibling test binary, so every
/// such file carries its own copy (house convention — see
/// `relate_at_auto_grammar_tests.rs`).
fn assert_has_error(label: &str, source: &str) {
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        root.has_error(),
        "{label}: expected the parse to ERROR, but it parsed cleanly.\n\
         root s-expression:\n{}",
        root.to_sexp()
    );
}

/// α headline signal — the committed target-surface fixture reaches 0 ERROR
/// nodes. RED before step-2 (the `sketch profile {` header, first ERROR at
/// `[5,4]-[5,20]`), GREEN after.
///
/// Every other construct in the fixture already parses on `main`: `aux let`
/// (pinned by `sketch_aux_let_premise.ri`, PRD §5 D12) and a member reference
/// in `extrude`'s profile slot (pinned by `sketch_member_extrude_premise.ri`,
/// PRD §5 D11) — so the `sketch` block header is the only blocker and α alone
/// can turn this green.
#[test]
fn sketch_block_target_fixture_parses_with_zero_error_nodes() {
    assert_parses_clean(
        "tests/prd-gate/fixtures/sketch_block_target.ri",
        SKETCH_BLOCK_TARGET,
    );
}

/// The `sketch` block attaches as a member alternative named `sketch_block`,
/// carrying a `name` field and a body of REUSED node kinds — `let_declaration`
/// and `relation_member`, not a bespoke `sketch_member` wrapper.
///
/// RED before step-2. The name-field half and the body-shape half are asserted
/// together on purpose: a grammar delta that captured the header but wrapped
/// body items in a new node kind would satisfy either half alone, and would
/// silently fork `lower_relation_members` into a second implementation.
#[test]
fn sketch_block_cst_contract() {
    let mut parser = make_parser();
    let tree = parser.parse(SKETCH_SOURCE, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "sketch block must parse cleanly; got:\n{}",
        root.to_sexp()
    );

    let sketch = find_node_by_kind(root, "sketch_block").expect(
        "expected a `sketch_block` node — the sketch block must be its own \
         member alternative, not an ERROR or a re-used relate_block",
    );

    // ── the header ──
    let name = sketch
        .child_by_field_name("name")
        .expect("expected a `name` field on sketch_block");
    assert_eq!(
        name.kind(),
        "identifier",
        "the sketch name must be an identifier node"
    );
    assert_eq!(
        &SKETCH_SOURCE[name.byte_range()],
        "profile",
        "name text must be exactly the sketch's binding name"
    );

    // ── the body, in source order ──
    //
    // The leading `identifier` IS the `name` field (a field-labelled child is
    // still a named child), so it is pinned here rather than filtered out —
    // that also pins the name's position ahead of the body.
    let mut cursor = sketch.walk();
    let named: Vec<String> = sketch
        .named_children(&mut cursor)
        .map(|c| c.kind().to_string())
        .collect();
    assert_eq!(
        named,
        vec![
            "identifier",
            "let_declaration",
            "let_declaration",
            "relation_member",
            "relation_member",
        ],
        "sketch body must REUSE let_declaration and relation_member in source \
         order (no bespoke sketch_member wrapper); got:\n{}",
        sketch.to_sexp()
    );

    // ── `aux let` rides the existing optional('aux') on let_declaration ──
    //
    // Probed exactly as `ts_parser.rs`'s `has_aux_keyword` does it: an
    // ANONYMOUS child whose text is `aux`. If a future delta introduced a
    // separate aux node kind, lowering would silently stop marking
    // construction geometry, so the probe shape is part of the contract.
    let first_let = sketch
        .named_children(&mut sketch.walk())
        .find(|c| c.kind() == "let_declaration")
        .expect("expected a let_declaration in the sketch body");
    let has_aux = first_let
        .children(&mut first_let.walk())
        .any(|c| !c.is_named() && &SKETCH_SOURCE[c.byte_range()] == "aux");
    assert!(
        has_aux,
        "the first body let must carry an anonymous `aux` keyword child (PRD \
         §5 D12 construction geometry rides let_declaration's existing \
         optional('aux') — no new grammar); got:\n{}",
        first_let.to_sexp()
    );
}

/// Regression floor: the two committed PREMISE fixtures must keep parsing with
/// 0 ERROR nodes after the step-2 grammar delta.
///
/// GREEN before and after. These are the fixtures the PRD leans on when it
/// claims D11 and D12 need no new grammar — `aux let` for construction
/// geometry, and a member reference in `extrude`'s profile slot — so a delta
/// that broke either would invalidate the design, not just a test.
///
/// `include_str!` (rather than a runtime read) also gives compile-time drift
/// detection if a fixture is moved or deleted.
#[test]
fn sketch_premise_fixtures_regression_floor() {
    let baselines: [(&str, &str); 2] = [
        (
            "tests/prd-gate/fixtures/sketch_aux_let_premise.ri",
            include_str!("../../tests/prd-gate/fixtures/sketch_aux_let_premise.ri"),
        ),
        (
            "tests/prd-gate/fixtures/sketch_member_extrude_premise.ri",
            include_str!("../../tests/prd-gate/fixtures/sketch_member_extrude_premise.ri"),
        ),
    ];

    for (name, source) in baselines {
        assert_parses_clean(name, source);
    }
}

/// THE contextual-keyword guard, and the single most important non-regression
/// in this file: `sketch` is a plain string token, so it must keep lexing as an
/// `identifier` at every position that is not a member start.
///
/// GREEN before and after (probe-verified exit 0 on the pre-change grammar).
/// If this ever reds, a member-start state has begun admitting both `'sketch'`
/// and `identifier`. The fix is NOT `token(prec(…))` or an external token — it
/// is to check that `sketch_block` was added to `commonMembers()` only, and
/// never to `_primary_expression`, `trait_member` or `purpose_member`.
///
/// Each case asserts BOTH halves: it parses clean, AND it produced no
/// `sketch_block`. The second half is what catches a silent capture — a state
/// that swallowed `sketch` into a block would still parse clean in some of
/// these shapes.
#[test]
fn sketch_stays_usable_as_an_identifier() {
    let cases: [&str; 5] = [
        "structure S { let sketch = 5 }",
        "structure S { let y = sketch + 1 }",
        "structure S { let z = f(sketch) }",
        "structure S { param sketch : Length = 5mm }",
        "structure S { sub sketch : B }",
    ];

    for source in cases {
        assert_parses_clean(source, source);

        let mut parser = make_parser();
        let tree = parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert!(
            find_node_by_kind(root, "sketch_block").is_none(),
            "`sketch` must still lex as an identifier here — found a \
             sketch_block in `{source}`:\n{}",
            root.to_sexp()
        );
        assert!(
            collect_kinds(root).iter().any(|k| k == "identifier"),
            "`{source}` must contain at least one identifier node \
             (guard against a vacuous pass):\n{}",
            root.to_sexp()
        );
    }
}

/// PRD §5 D2: the datum-plane `on <expr>` clause is NOT in v1, and the slot it
/// will occupy — between `name` and `{` — is unclaimed.
///
/// GREEN before and after. That the slot is a parse ERROR *today* is precisely
/// what makes the future widening non-breaking: no v1 source can already mean
/// something there, so `optional(seq('on', field('plane', $._expression)))`
/// can be added without reinterpreting any committed program.
///
/// EXPECTED TO BE DELETED, not weakened, by the future anchoring PRD. If you
/// are here because you are implementing `on <expr>`, remove this test in the
/// same commit that adds the clause — do not relax it to "parses somehow".
#[test]
fn on_clause_slot_is_unclaimed_in_v1() {
    assert_has_error(
        "the reserved `on <expr>` slot must not parse in v1",
        "structure S { sketch s on plane { fix(a) } }",
    );
}

/// Shape controls for the block header, plus the empty-body positive.
///
/// GREEN before and after. Guards against a future widening that made the name
/// optional (which would collide with `relate`'s nameless form and leave the
/// bound profile member unnameable) or admitted a braceless single-relation
/// body (which would make the member/relation boundary ambiguous).
#[test]
fn sketch_block_negative_controls() {
    // The name is mandatory — a sketch block BINDS a member (PRD §5 D11), so
    // an anonymous one would have no profile to hand to `extrude`.
    assert_has_error(
        "a nameless sketch block must be rejected",
        "structure S { sketch { fix(a) } }",
    );
    // Braces are mandatory — no braceless single-relation shorthand.
    assert_has_error(
        "a braceless sketch body must be rejected",
        "structure S { sketch profile fix(a) }",
    );

    // POSITIVE: the empty body is admitted, matching `relate { }` — `repeat`
    // is zero-or-more, and an empty sketch is a zero-DOF sketch, a semantic
    // matter for γ rather than a parse error.
    assert_parses_clean(
        "an empty sketch block parses",
        "structure S { sketch s { } }",
    );
    let mut parser = make_parser();
    let tree = parser
        .parse("structure S { sketch s { } }", None)
        .expect("parse failed");
    let empty = find_node_by_kind(tree.root_node(), "sketch_block")
        .expect("expected an empty sketch_block node");
    assert_eq!(
        empty.named_child_count(),
        1,
        "an empty sketch_block's only named child must be its `name` \
         identifier; got:\n{}",
        empty.to_sexp()
    );
}

/// A sketch block is admitted inside a `where <cond> { … }` guarded block —
/// the direct consequence of registering it in `commonMembers()`, which feeds
/// `_guard_member` as well as `_member`. Matches `relate_block`.
///
/// GREEN before and after step-2 in the sense that matters: this pins the
/// GRAMMAR-level admission only. Semantically a guarded sketch is not
/// supported at α, and step-12 supplies the loud compiler-level rejection —
/// mirroring exactly how `guards.rs` already rejects `MemberDecl::Relate`
/// inside a guard. Admit-then-reject-loudly is the house pattern: it keeps the
/// diagnostic a typed message instead of a parse error pointing at `sketch`.
#[test]
fn sketch_block_admitted_in_guarded_block() {
    const SOURCE: &str = "structure S { where cond { sketch s { fix(a) } } }";
    assert_parses_clean("sketch inside a guarded block", SOURCE);

    let mut parser = make_parser();
    let tree = parser.parse(SOURCE, None).expect("parse failed");
    let root = tree.root_node();
    let guard = find_node_by_kind(root, "guarded_block")
        .expect("expected a guarded_block node");
    assert!(
        find_node_by_kind(guard, "sketch_block").is_some(),
        "the sketch_block must sit INSIDE the guarded_block, not beside it:\n{}",
        root.to_sexp()
    );
}

// ── INV-SF-7 `parse-is-value-faithful` ───────────────────────────────────────
//
// `docs/legibility/design-invariants.md:195-220`. The sketch body is the FIRST
// construct in the language where a `let_declaration` and a bare expression are
// siblings with no separator token (`relate` bodies hold only bare expressions;
// `_member` holds only keyword-led declarations), so the invariant's checkable
// question — "can any statement/expression boundary in the new grammar absorb a
// following line without a diagnostic?" — has to be answered by measurement,
// not assumed.
//
// It was. Every reading below was probed against the CLI first and the
// assertions written to match; none was predicted then asserted.
//
// The suite asserts NODE KINDS and MEMBER COUNTS, never error spans. That
// looseness is deliberate and has a precedent-rationale at
// `imaginary_literal_grammar_tests.rs:202-226`: pinning exact spans just
// relocates the drift problem into a CI-gated guard that must be re-blessed on
// every unrelated recovery tweak.

/// Count nodes of `kind` in the subtree rooted at `node`.
fn count_kind(node: tree_sitter::Node, kind: &str) -> usize {
    collect_kinds(node).iter().filter(|k| k.as_str() == kind).count()
}

/// Parse `source` and return its single `sketch_block` node's named-child kinds
/// (the leading `identifier` is the `name` field and is dropped, so the result
/// is exactly the BODY member list, in source order).
fn sketch_body_members(source: &str) -> Vec<String> {
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    // The tree must outlive the borrow, so do the whole walk here.
    let sketch = find_node_by_kind(tree.root_node(), "sketch_block")
        .expect("expected a sketch_block node");
    let mut cursor = sketch.walk();
    sketch
        .named_children(&mut cursor)
        .skip(1) // the `name` field identifier
        .map(|c| c.kind().to_string())
        .collect()
}

/// Parse `source` and return the `value:` field s-expression of its FIRST
/// `let_declaration`, with byte extents stripped so two different sources can
/// be compared for structural identity.
fn first_let_value_shape(source: &str) -> String {
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    let decl = find_node_by_kind(tree.root_node(), "let_declaration")
        .expect("expected a let_declaration node");
    decl.child_by_field_name("value")
        .expect("expected a `value` field on let_declaration")
        .to_sexp()
}

/// INV-SF-7, positive half: a `let` whose value ends in a quantity literal must
/// NOT absorb the following bare-expression member.
///
/// GREEN after step-2 (measured). This is the shape INV-SF-7's Evidence section
/// names as the worst silent-failure form — a misparse yielding a well-typed
/// WRONG value — so it gets its own test rather than a row in a table.
#[test]
fn sketch_let_quantity_literal_does_not_absorb_the_next_member() {
    let cases: [(&str, &str); 2] = [
        ("fix(a)", "structure S {\n  sketch s {\n    let d = 5mm\n    fix(a)\n  }\n}\n"),
        (
            "horizontal(ab)",
            "structure S {\n  sketch s {\n    let d = 5mm\n    horizontal(ab)\n  }\n}\n",
        ),
    ];

    for (tail, source) in cases {
        assert_parses_clean(tail, source);
        assert_eq!(
            sketch_body_members(source),
            vec!["let_declaration", "relation_member"],
            "`let d = 5mm` must not absorb the following `{tail}` line"
        );

        // The relation_member's TEXT is asserted, not just its kind: a reading
        // that fused the two lines into one over-long relation_member would
        // still produce the right kind.
        let mut parser = make_parser();
        let tree = parser.parse(source, None).expect("parse failed");
        let rel = find_node_by_kind(tree.root_node(), "relation_member")
            .expect("expected a relation_member");
        assert_eq!(
            &source[rel.byte_range()],
            tail,
            "the relation_member must span exactly `{tail}` and nothing else"
        );
    }
}

/// INV-SF-7, whitespace-splits-the-literal half: `5mm` is a quantity literal,
/// `5 mm` is a parse ERROR — never a quiet reinterpretation.
///
/// GREEN after step-2 (measured). This is the existing law pinned by
/// `test/corpus/unit_expr.txt`; the test's job is to prove the sketch body
/// inherits it rather than opening a second reading inside a sketch argument.
#[test]
fn sketch_argument_whitespace_splits_the_quantity_literal() {
    const TIGHT: &str = "structure S {\n  sketch s {\n    let p = point(5mm, 0mm)\n  }\n}\n";
    const LOOSE: &str = "structure S {\n  sketch s {\n    let p = point(5 mm, 0mm)\n  }\n}\n";

    assert_parses_clean("point(5mm, 0mm)", TIGHT);
    let mut parser = make_parser();
    let tight = parser.parse(TIGHT, None).expect("parse failed");
    assert_eq!(
        count_kind(tight.root_node(), "quantity_literal"),
        2,
        "`point(5mm, 0mm)` must yield two quantity_literals"
    );

    assert_has_error("point(5 mm, 0mm) must not parse", LOOSE);
    let loose = parser.parse(LOOSE, None).expect("parse failed");
    let root = loose.root_node();
    // The resolution must be a diagnostic, not a quiet pick: no quantity_literal
    // may span the whitespace-separated `5 mm`.
    let mut spans_loose = false;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.kind() == "quantity_literal" && LOOSE[n.byte_range()].contains(' ') {
            spans_loose = true;
        }
        let mut c = n.walk();
        stack.extend(n.children(&mut c));
    }
    assert!(
        !spans_loose,
        "`5 mm` must not be quietly re-read as one quantity_literal:\n{}",
        root.to_sexp()
    );
}

/// INV-SF-7, THE KNOWN ITEM-BOUNDARY JOIN — recorded, not silently changed.
///
/// `let x = a.b` followed by a line starting `(` collapses into ONE member: the
/// `(c)` is consumed as the argument list of a `namespaced_call`. This is the
/// reading `test/corpus/namespaced_ref.txt` already commits for `relate { a.b ⏎
/// (x) }`, and the sketch body inherits it verbatim through the shared
/// `relation_member`/`_expression` machinery.
///
/// GREEN after step-2 (measured — the assertion was written to the observed
/// tree, not to a prediction). It is pinned rather than "fixed" because the
/// same absorb is reproducible with NO sketch block anywhere (see
/// [`sketch_body_item_boundary_matches_a_plain_member_body`], which proves that
/// rather than claiming it): it belongs to `let_declaration`'s `value:` being a
/// full `_expression`, which is the INV-SF-7 seam task #5392 owns. Narrowing it
/// here would fork the sketch body away from every other member body in the
/// language — strictly worse than one seam handled in one place.
#[test]
fn sketch_body_records_the_known_item_boundary_join() {
    // THE JOIN: one member, not two.
    const JOINED: &str = "structure S {\n  sketch s {\n    let x = a.b\n    (c)\n  }\n}\n";
    assert_parses_clean("a.b then (c)", JOINED);
    assert_eq!(
        sketch_body_members(JOINED),
        vec!["let_declaration"],
        "`a.b` ⏎ `(c)` is ONE member — the namespaced_ref.txt item-boundary \
         reading, inherited from relate_block"
    );
    let mut parser = make_parser();
    let joined = parser.parse(JOINED, None).expect("parse failed");
    assert_eq!(
        count_kind(joined.root_node(), "namespaced_call"),
        1,
        "the join must surface as a namespaced_call"
    );
    assert_eq!(
        count_kind(joined.root_node(), "relation_member"),
        0,
        "no relation_member survives the join"
    );

    // THE COUNTER-FORM: an identifier-led next line does NOT join.
    const SPLIT: &str = "structure S {\n  sketch s {\n    let x = a.b\n    fix(c)\n  }\n}\n";
    assert_parses_clean("a.b then fix(c)", SPLIT);
    assert_eq!(
        sketch_body_members(SPLIT),
        vec!["let_declaration", "relation_member"],
        "`a.b` ⏎ `fix(c)` stays TWO members — only a `(`-led line joins"
    );
}

/// INV-SF-7, the pre-existence PROOF for the two absorbing readings above.
///
/// Rather than *claiming* in prose that the sketch body merely inherits
/// `let_declaration`'s line-spanning `value:`, this asserts it: the same two
/// source lines are parsed inside a sketch body and inside a plain structure
/// member body, and the resulting `value:` shapes must be IDENTICAL.
///
/// GREEN after step-2 (measured, both readings reproduce with no sketch block
/// anywhere). Two things follow, and both are the point of the test:
///  • α did not invent these absorbs — so narrowing them is out of scope here.
///  • If task #5392 later makes `let_declaration` value-faithful at this seam,
///    this test keeps the sketch body in LOCKSTEP automatically instead of
///    silently leaving it behind on the old reading.
///
/// If this ever reds, the sketch body has diverged from every other member body
/// in the language. That is the bug, whichever side moved.
#[test]
fn sketch_body_item_boundary_matches_a_plain_member_body() {
    let pairs: [(&str, &str, &str); 2] = [
        (
            "`a.b` ⏎ `(c)` — the namespaced_call join",
            "structure S {\n  sketch s {\n    let x = a.b\n    (c)\n  }\n}\n",
            "structure S {\n  let x = a.b\n  (c)\n}\n",
        ),
        (
            "`5mm` ⏎ `- 3mm` — the leading-operator continuation",
            "structure S {\n  sketch s {\n    let d = 5mm\n    - 3mm\n  }\n}\n",
            "structure S {\n  let d = 5mm\n  - 3mm\n}\n",
        ),
    ];

    for (label, in_sketch, in_plain) in pairs {
        assert_parses_clean(label, in_sketch);
        assert_parses_clean(label, in_plain);
        assert_eq!(
            first_let_value_shape(in_sketch),
            first_let_value_shape(in_plain),
            "{label}: the sketch body must read this exactly as a plain member \
             body does — it inherits let_declaration's line-spanning `value:`, \
             it does not add a reading of its own"
        );
    }
}

/// Relate-block parity: two bare relation members on consecutive lines stay two
/// members. GREEN after step-2 (measured).
///
/// The baseline case for the whole INV-SF-7 suite: with no `let` involved, the
/// sketch body must behave exactly like `relate { }`.
#[test]
fn sketch_body_keeps_consecutive_relation_members_separate() {
    const SOURCE: &str =
        "structure S {\n  sketch s {\n    fix(a)\n    horizontal(ab)\n  }\n}\n";
    assert_parses_clean("two consecutive relation members", SOURCE);
    assert_eq!(
        sketch_body_members(SOURCE),
        vec!["relation_member", "relation_member"],
        "consecutive identifier-led relation members must not fuse"
    );
}

// ── `auto(seed)` in positional call-argument position ────────────────────────
//
// PRD §5 D6, implementing Leo's 2026-07-25 OPTION B decision: grammar-general
// `auto(<expr>)` in CALL position. That decision records a PARTIAL, explicit
// reversal of task 3808, which rejected `auto` at operand positions. Only the
// PARENTHESIZED form is reversed: positionally, bare `auto` and the
// named-parameter `auto(name = value)` both stay parse errors, and
// [`bare_auto_stays_rejected_in_positional_operand_position`] is what keeps the
// reversal narrow rather than letting it drift wide later.
//
// `auto(free)` is the one case where "grammar-general" bites: positionally it
// is now a CLEAN parse — an `auto_seed` whose seed happens to be the identifier
// `free` — because `auto_keyword`'s modifier arm is not reachable in argument
// position at all. That is a consequence of the generality, not a special case,
// and [`positional_auto_free_is_a_seed_not_a_modifier`] pins the measured
// reading so it cannot be mistaken for the modifier later.

/// The α target surface for `auto(seed)`: the committed PRD fixture whose only
/// remaining parse blocker after step-2 is `auto(10mm)` in positional argument
/// position.
const SKETCH_AUTO_SEED_TARGET: &str =
    include_str!("../../tests/prd-gate/fixtures/sketch_auto_seed_target.ri");

/// Second α headline signal — RED before step-8 (probe-verified exit 1), GREEN
/// after.
///
/// This fixture's sketch block already parses after step-2; `point(auto(10mm),
/// 0mm)` is the only thing left, so step-8 alone can turn it green.
#[test]
fn sketch_auto_seed_target_fixture_parses_with_zero_error_nodes() {
    assert_parses_clean(
        "tests/prd-gate/fixtures/sketch_auto_seed_target.ri",
        SKETCH_AUTO_SEED_TARGET,
    );
}

/// `auto(<expr>)` surfaces as its own `auto_seed` node with a `seed` field, in
/// ANY call's positional argument list — not just inside a sketch.
///
/// RED before step-8 (the `auto_seed` node kind does not exist).
///
/// The generality is asserted, not assumed: OPTION B says "grammar-general in
/// call position", so the same form is exercised through a plain
/// `function_call` AND a `namespaced_call`, the two `callTail($)` consumers a
/// sketch actually reaches. Without this, generality would be incidental —
/// true only because nothing tested the other consumers.
#[test]
fn auto_seed_cst_contract() {
    const SOURCE: &str = "structure S { let b = point(auto(10mm), 0mm) }";
    assert_parses_clean("point(auto(10mm), 0mm)", SOURCE);

    let mut parser = make_parser();
    let tree = parser.parse(SOURCE, None).expect("parse failed");
    let root = tree.root_node();

    assert_eq!(
        count_kind(root, "auto_seed"),
        1,
        "expected exactly one auto_seed node:\n{}",
        root.to_sexp()
    );
    let auto_seed = find_node_by_kind(root, "auto_seed").expect("expected an auto_seed node");

    let seed = auto_seed
        .child_by_field_name("seed")
        .expect("expected a `seed` field on auto_seed");
    assert_eq!(
        seed.kind(),
        "quantity_literal",
        "the seed must be the parsed expression, not raw text"
    );
    assert_eq!(
        &SOURCE[seed.byte_range()],
        "10mm",
        "seed text must be exactly the seeded value"
    );

    // The argument list keeps its ordinary shape: auto_seed is a sibling
    // ALTERNATIVE to an expression argument, not a wrapper around the list.
    let args = find_node_by_kind(root, "argument_list").expect("expected an argument_list");
    let mut cursor = args.walk();
    let arg_kinds: Vec<String> = args
        .named_children(&mut cursor)
        .map(|c| c.kind().to_string())
        .collect();
    assert_eq!(
        arg_kinds,
        vec!["auto_seed", "quantity_literal"],
        "auto_seed must be the FIRST of two ordinary positional arguments:\n{}",
        args.to_sexp()
    );

    // Generality across callTail($) consumers.
    for (label, source) in [
        ("function_call", "structure S { let b = point(auto(10mm), 0mm) }"),
        ("namespaced_call", "structure S { let b = m.f(auto(1mm)) }"),
    ] {
        assert_parses_clean(label, source);
        let t = parser.parse(source, None).expect("parse failed");
        assert_eq!(
            count_kind(t.root_node(), "auto_seed"),
            1,
            "`auto(<expr>)` must be admitted in {label} position too — OPTION B \
             is grammar-general in CALL position, not sketch-only:\n{}",
            t.root_node().to_sexp()
        );
    }
}

/// The half of task 3808 that is NOT reversed: bare `auto` stays a parse error
/// at every positional operand position.
///
/// GREEN before and after step-8 (probe-verified exit 1 on the first four
/// today). The first four mirror `test/corpus/auto_operand_rejection.txt`
/// case-for-case. Their job is to prove the OPTION B reversal is NARROW — the
/// parenthesized form only — so a later widening cannot quietly restore bare
/// `auto` as an operand and call it precedent.
///
/// The fifth case is the other half of the narrowness contract and has no
/// corpus counterpart: `auto(seed = 5mm)` is a MODIFIER with a named
/// parameter, and positionally it must stay an error rather than degrade into
/// an `auto_seed` that silently drops the parameter NAME. It stays an error
/// for a structural reason, not a precedence one — `seed = 5mm` is not an
/// `_expression`, so it cannot fill `auto_seed`'s `seed` field.
#[test]
fn bare_auto_stays_rejected_in_positional_operand_position() {
    let cases: [(&str, &str); 5] = [
        ("bare auto as a positional call arg", "structure S { let x : Length = clamp(auto) }"),
        ("bare auto as a binary operand", "structure S { let x : Length = auto + 2mm }"),
        ("bare auto as a list element", "structure S { let xs = [auto] }"),
        ("bare auto as a constraint expr", "structure S { param x : Length  constraint auto }"),
        ("named-param auto as a positional arg", "structure S { let b = f(auto(seed = 5mm)) }"),
    ];
    for (label, source) in cases {
        assert_has_error(label, source);
    }
}

/// The one measured consequence of "grammar-general in CALL position" that
/// reads as a surprise, pinned so it cannot be mistaken for the modifier arm.
///
/// RED before step-8 (`f(auto(free))` is a parse ERROR today); GREEN after.
///
/// In ARGUMENT position `auto_keyword` is unreachable, so `auto(free)` is not
/// the free-MODIFIER — it is an ordinary `auto_seed` whose seed happens to be
/// the identifier `free`. Nothing is ambiguous, because the two readings live
/// in disjoint positions: this test asserts the positional reading, and
/// [`named_argument_auto_forms_unchanged`] asserts that the SAME text in named
/// argument position still reaches `auto_keyword` through `_binding_value`.
///
/// Keeping both halves asserted is the point. Either one alone would let a
/// future widening collapse the positions together and give one token sequence
/// two readings — precisely the INV-SF-7 failure the position split avoids.
#[test]
fn positional_auto_free_is_a_seed_not_a_modifier() {
    const SOURCE: &str = "structure S { let b = f(auto(free)) }";
    assert_parses_clean("f(auto(free))", SOURCE);

    let mut parser = make_parser();
    let tree = parser.parse(SOURCE, None).expect("parse failed");
    let root = tree.root_node();

    assert_eq!(
        count_kind(root, "auto_keyword"),
        0,
        "positional `auto(free)` must NOT reach auto_keyword's modifier arm:\n{}",
        root.to_sexp()
    );
    let auto_seed = find_node_by_kind(root, "auto_seed").expect("expected an auto_seed node");
    let seed = auto_seed
        .child_by_field_name("seed")
        .expect("expected a `seed` field on auto_seed");
    assert_eq!(
        seed.kind(),
        "identifier",
        "`free` must be read as an ordinary seed expression here:\n{}",
        root.to_sexp()
    );
    assert_eq!(&SOURCE[seed.byte_range()], "free");
}

/// The existing NAMED-argument and binding-site `auto` forms are untouched: the
/// new node must not steal them.
///
/// GREEN before and after step-8 (probe-verified exit 0 today — named-argument
/// position already reaches `auto_keyword` through `_binding_value`, so task
/// 3808's rejection never covered it).
///
/// The `auto_keyword`-vs-`auto_seed` discrimination is the real content here: a
/// delta that admitted `auto_seed` too widely would silently re-route
/// `auto(seed = 5mm)` — a MODIFIER with a named parameter — into the new
/// single-expression node and lose the parameter name.
#[test]
fn named_argument_auto_forms_unchanged() {
    let mut named_parser = make_parser();
    for (label, source) in [
        ("f(x: auto)", "structure S { let y = f(x: auto) }"),
        ("f(x: auto(free))", "structure S { let y = f(x: auto(free)) }"),
        ("f(x: auto(seed = 5mm))", "structure S { let y = f(x: auto(seed = 5mm)) }"),
    ] {
        assert_parses_clean(label, source);
        // Clean-parse alone would not catch the real hazard: `auto_seed`
        // leaking into named-argument position would ALSO parse clean, while
        // silently changing the node kind (and, for the third case, dropping
        // the parameter name). Assert the kind, not just the absence of errors.
        let tree = named_parser.parse(source, None).expect("parse failed");
        let root = tree.root_node();
        assert_eq!(
            (count_kind(root, "auto_keyword"), count_kind(root, "auto_seed")),
            (1, 0),
            "{label} must still reach auto_keyword via _binding_value, never \
             the new auto_seed node:\n{}",
            root.to_sexp()
        );
    }

    const PARAM: &str = "structure S { param p : Frame = auto(seed = 5mm) }";
    assert_parses_clean("param default auto(seed = 5mm)", PARAM);
    let mut parser = make_parser();
    let tree = parser.parse(PARAM, None).expect("parse failed");
    let root = tree.root_node();
    assert_eq!(
        count_kind(root, "auto_keyword"),
        1,
        "the binding-site modifier must still be an auto_keyword:\n{}",
        root.to_sexp()
    );
    assert_eq!(
        count_kind(root, "auto_param_list"),
        1,
        "`seed = 5mm` must still parse as an auto_param_list, keeping the \
         parameter NAME:\n{}",
        root.to_sexp()
    );
    assert_eq!(
        count_kind(root, "auto_seed"),
        0,
        "auto_seed must NOT steal the binding-site modifier form:\n{}",
        root.to_sexp()
    );
}

/// D6's surface is CALL position only: `auto(<expr>)` is not admitted at a
/// binding site in v1.
///
/// GREEN before and after step-8 (probe-verified exit 1 today).
///
/// Admitting it in `_binding_value` later would be a non-breaking widening, and
/// is DELIBERATELY not taken here: at a binding site `auto(free)` already means
/// the `auto_keyword` free-modifier arm, so a second reading of the same token
/// sequence as `auto_seed` with `seed` = the identifier `free` would be exactly
/// the two-reading ambiguity INV-SF-7 forbids. In CALL position no such
/// collision exists, because `auto_keyword` is not reachable there.
#[test]
fn auto_seed_is_not_admitted_at_binding_sites_in_v1() {
    assert_has_error(
        "let binding site must reject auto(5mm)",
        "structure S { let x : Length = auto(5mm) }",
    );
    assert_has_error(
        "param default must reject auto(5mm)",
        "structure S { param p : Length = auto(5mm) }",
    );
}
