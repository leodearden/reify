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
/// Unused until step-3 adds the negative controls; kept here with the other
/// helpers because a Rust integration-test binary cannot share helpers with a
/// sibling, so every such file carries its own copy (house convention — see
/// `relate_at_auto_grammar_tests.rs`).
#[allow(dead_code)]
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
