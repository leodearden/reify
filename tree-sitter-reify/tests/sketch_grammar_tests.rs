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
//! `unnecessary conflicts` warnings, both PRE-EXISTING and unrelated:
//! `constraint_instantiation`/`constraint_declaration` and
//! `function_definition`/`function_signature` (plus an ABI-14 notice for the
//! absent `tree-sitter.json`). A THIRD warning appearing after a grammar delta
//! is a signal to fix precedence, not to accept.
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
