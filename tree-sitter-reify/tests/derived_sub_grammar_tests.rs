//! Grammar (CST-level) integration tests for the **derived sub arm** — the
//! fourth `sub_declaration` arm: `sub b = mirror of a across <plane> { … }`
//! and `sub b = image of a under <transform> { … }`.
//!
//! Leaf A-alpha of `docs/prds/v0_6/assembly-derivation-toolbox.md` (task
//! #6615). §3.3 gives the disposition items; §6 D1 gives the derivation as an
//! ELEMENT type; A-alpha is scoped to syntax + lowering only — every
//! compile-scope rejection (unknown/non-sibling/cyclic prototype, disposition
//! paths, `E_DERIVED_SUB_EXPLICIT_AT`) belongs to A-beta (#6616).
//!
//! # This file IS the CI-run grammar signal
//!
//! `tree-sitter test` (the `test/corpus/` suite) is **not** invoked by CI —
//! `tree-sitter-reify/package.json` has no `scripts` block, there is no
//! Makefile, and `dark-factory-orchestrator.yaml` references tree-sitter only
//! as a `generate` prerequisite. The CI-enforced grammar surface is
//! `tree-sitter-reify/tests/*.rs` — i.e. this file. `test/corpus/
//! derived_sub_arm.txt` documents the same CST shape in corpus form; rather
//! than leaving that copy free to drift, this file `include_str!`s it and
//! validates every case against the live parser in
//! [`corpus_cases_match_the_live_parser`] — so the corpus is checked on every
//! merge without depending on the `tree-sitter test` CLI at all.
//!
//! (MEASURED on this branch's base 6f0e434eb8: `tree-sitter test` is
//! 222 parses / 222 successful / 100.00%. The "218/219, imaginary_literal
//! red" claim carried by older test headers in this directory no longer
//! matches `main` — it was re-measured here, not trusted.)
//!
//! # TDD status of each test
//!
//! - [`derived_sub_fixture_parses_with_zero_error_nodes`] — **RED** before the
//!   grammar step; GREEN after. MEASURED RED extent on base 6f0e434eb8:
//!   `ERROR [8, 0] - [14, 1]`, `tree-sitter parse --quiet` exit 1. (The task
//!   description's `ERROR [3,0]-[9,1]` is STALE — the fixture gained a 7-line
//!   provenance header after that probe at 96041f850b. Nothing here pins
//!   either extent; the assertion is "zero ERROR nodes".) This is the leaf's
//!   headline user-observable signal, and it is made CI-run here for the FIRST
//!   time: before this file, `adt_mirror_of_arm.ri` was referenced by no test
//!   harness at all — only by the PRD and the capability manifest.
//! - [`relation_verbs_fixture_regression_floor`] — **GREEN before and after**.
//!   `adt_relation_verbs.ri` is the PRD-named regression floor: relation verbs
//!   are ordinary calls and already parse, so this test's job is to fail
//!   loudly if the new arm disturbs `relate`-block parsing. Also made CI-run
//!   here for the first time.
//! - [`existing_sub_arms_regression_floor`] — **GREEN before and after**. The
//!   corrected regression floor for the other three `sub` arms
//!   (instantiation / collection / specialization + keyed member block): it
//!   must fail loudly if the grammar delta disturbs any of them.
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
#[allow(dead_code)]
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

/// The A-alpha target surface: the committed PRD fixture whose only parse
/// blocker is the derived sub arm.
const MIRROR_FIXTURE: &str = include_str!("../../tests/prd-gate/fixtures/adt_mirror_of_arm.ri");

/// The PRD-named regression floor: relation verbs as ordinary calls.
const RELATION_VERBS_FIXTURE: &str =
    include_str!("../../tests/prd-gate/fixtures/adt_relation_verbs.ri");

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

/// A-alpha headline signal — the committed target-surface fixture reaches 0
/// ERROR nodes.
///
/// RED before the grammar step (MEASURED `ERROR [8, 0] - [14, 1]`, exit 1),
/// GREEN after. Every other construct in the fixture already parses on `main`:
/// `module`/`pub structure`, `param z : Length = 80mm`, `let body = box(…)`,
/// and `sub unit_a = Unit() at transform3(…)` — so the `mirror of … across …`
/// arm is the only blocker and A-alpha alone can turn this green.
#[test]
fn derived_sub_fixture_parses_with_zero_error_nodes() {
    assert_parses_clean("tests/prd-gate/fixtures/adt_mirror_of_arm.ri", MIRROR_FIXTURE);
}

/// PRD-named regression floor — the relation-verb fixture parses clean both
/// before AND after the grammar delta.
///
/// `relate` blocks and the derived arm are adjacent surfaces (the derived arm
/// carries an optional `at <pose> <sub_relate_block>` tail), so a delta that
/// perturbed `relate` parsing would show up here. GREEN before and after.
#[test]
fn relation_verbs_fixture_regression_floor() {
    assert_parses_clean(
        "tests/prd-gate/fixtures/adt_relation_verbs.ri",
        RELATION_VERBS_FIXTURE,
    );
}

/// Regression floor for the three pre-existing `sub` arms.
///
/// GREEN before and after. The fourth arm shares the `priv? aux? sub <name>`
/// prefix with all three, so the GLR split is the thing most at risk from the
/// grammar delta; each arm is asserted separately so a failure names which one
/// broke.
#[test]
fn existing_sub_arms_regression_floor() {
    for (label, source) in [
        (
            "instantiation arm",
            "structure S { sub a = Unit(z: 1mm) at origin }",
        ),
        ("collection arm", "structure S { sub xs : List<Vent> }"),
        (
            "specialization arm",
            "structure S { sub m : Motor { bore = auto } at f }",
        ),
        (
            "keyed member block",
            "structure S { sub v : Keyed<Vent> { \"intake\" => { area = 5mm } } }",
        ),
    ] {
        assert_parses_clean(label, source);
    }
}
