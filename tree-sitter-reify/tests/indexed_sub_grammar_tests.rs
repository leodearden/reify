//! Grammar (CST-level) integration tests for the **indexer clause** on the
//! `sub` instantiation arm — `sub idlers[i in 0..4] = Pulley(…)`.
//!
//! Task α of `docs/prds/v0_6/indexed-sub-instantiation.md` (§3.1 gives the
//! grammar production verbatim; §7 scopes α to syntax only).
//!
//! # This file IS the CI-run grammar signal
//!
//! `tree-sitter test` (the `test/corpus/` suite) is **not** invoked by CI —
//! `tree-sitter-reify/package.json` has no `scripts` block, there is no
//! Makefile, and `dark-factory-orchestrator.yaml` references tree-sitter only
//! as a `generate` prerequisite. It is also not green on `main` (218/219, due
//! to the pre-existing and unrelated `test/corpus/imaginary_literal.txt`
//! failure, task #5492), so a blanket corpus gate would false-fail. The
//! CI-enforced grammar surface is `tree-sitter-reify/tests/*.rs` — i.e. this
//! file. `test/corpus/indexed_sub_instantiation.txt` documents the same CST
//! shape in corpus form; rather than leaving that copy free to drift, this file
//! `include_str!`s it and validates every case against the live parser in
//! [`corpus_cases_match_the_live_parser`] — so the corpus is checked on every
//! merge without depending on the `tree-sitter test` CLI at all.
//!
//! # TDD status of each test
//!
//! - [`indexed_sub_surface_fixture_parses_with_zero_error_nodes`] — **RED**
//!   before step-2 (ERROR node at the `[i in 0..4]` indexer, extent
//!   `[17,14]-[17,25]`); GREEN after. This is the task's headline
//!   user-observable signal, made CI-run.
//! - [`indexed_sub_cst_exposes_binder_and_domain_fields`] — **RED** before
//!   step-2 (the `binder`/`domain` fields do not exist yet); GREEN after.
//! - [`existing_sub_arms_regression_floor`] — **GREEN before and after**. This
//!   is the capability manifest's corrected regression floor: its job is to
//!   fail loudly if the step-2 grammar delta disturbs an existing `sub` arm.
//! - [`indexer_clause_is_rejected_outside_the_instantiation_arm`] — **GREEN
//!   before and after** (negative controls). Guards against over-widening the
//!   grammar, and pins PRD §9.1 Open Q1 as decided at α: exactly one indexer
//!   form, no binder omission.
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
/// the indexer clause.
const SURFACE_FIXTURE: &str =
    include_str!("../../docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri");

/// Canonical indexed-sub source used by the CST-shape and corpus tests: indexer
/// clause + named constructor args + an `at` pose.
const INDEXED_SUB_SOURCE: &str = "structure S { sub idlers[i in 0..4] = Pulley(od: 30mm + i * 2mm) at transform3(orient_identity(), vec3(0mm, 0mm, 0mm)) }";

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
/// nodes. RED before step-2 (single ERROR at `[17,14]-[17,25]`, the
/// `[i in 0..4]` indexer), GREEN after.
///
/// Every other construct in the fixture already parses on `main`: `structure
/// def X` (`grammar.js` `optional('def')`), `forall i in 0..3 : constraint
/// idlers[i].od < …` (pinned by `indexed_sub_forall_range_baseline.ri`), and
/// `= Ctor(named: args) at transform3(…)` (pinned by
/// `indexed_sub_inst_arm_baseline.ri`) — so the indexer clause is the only
/// blocker and α alone can turn this green.
#[test]
fn indexed_sub_surface_fixture_parses_with_zero_error_nodes() {
    assert_parses_clean(
        "docs/prds/v0_6/fixtures/indexed_sub_instantiation_surface.ri",
        SURFACE_FIXTURE,
    );
}

/// The indexer clause attaches to `sub_declaration` as the named fields
/// `binder` and `domain` (PRD §3.1 names them verbatim; α's capability-manifest
/// `delivered_check` greps for `field('binder'`), and the post-`=` shape of the
/// instantiation arm is left completely unchanged.
///
/// RED before step-2. The positive half (binder/domain present) and the
/// unchanged-shape half (structure_name/args/pose still where they were) are
/// asserted together on purpose: a grammar delta that captured the indexer but
/// re-routed the constructor tail would satisfy either half alone.
#[test]
fn indexed_sub_cst_exposes_binder_and_domain_fields() {
    let mut parser = make_parser();
    let tree = parser.parse(INDEXED_SUB_SOURCE, None).expect("parse failed");
    let root = tree.root_node();
    assert!(
        !root.has_error(),
        "indexed sub must parse cleanly; got:\n{}",
        root.to_sexp()
    );

    let sub = find_node_by_kind(root, "sub_declaration")
        .expect("expected a sub_declaration node");

    // ── the new indexer fields ──
    let binder = sub
        .child_by_field_name("binder")
        .expect("expected a `binder` field on sub_declaration");
    assert_eq!(
        binder.kind(),
        "identifier",
        "binder must be an identifier node"
    );
    assert_eq!(
        &INDEXED_SUB_SOURCE[binder.byte_range()],
        "i",
        "binder text must be exactly the index variable"
    );

    let domain = sub
        .child_by_field_name("domain")
        .expect("expected a `domain` field on sub_declaration");
    assert_eq!(
        domain.kind(),
        "range_expression",
        "`0..4` must reach the domain field as a range_expression \
         (range_expression is already a member of $._expression, so the \
         domain needs no new grammar)"
    );

    // ── the pre-existing instantiation shape, unchanged ──
    let structure_name = sub
        .child_by_field_name("structure_name")
        .expect("structure_name field must survive the indexer delta");
    assert_eq!(
        &INDEXED_SUB_SOURCE[structure_name.byte_range()],
        "Pulley",
        "structure_name must still be the constructed structure"
    );
    assert!(
        find_node_by_kind(sub, "named_argument_list").is_some(),
        "the named constructor argument list must still be reachable under \
         the indexed sub_declaration"
    );
    assert!(
        sub.child_by_field_name("pose").is_some(),
        "the `at <pose>` clause must still attach as the `pose` field"
    );
}

/// Regression floor (capability-manifest D3-corrected form): the three
/// pre-existing `sub` arms plus `forall`-over-range must keep parsing with 0
/// ERROR nodes after the step-2 grammar delta.
///
/// GREEN before and after. Implemented as `include_str!` assertions over the
/// four committed baseline fixtures rather than as a `tree-sitter test` corpus
/// run — see this module's header for why the corpus suite cannot serve as a
/// gate. `include_str!` also gives compile-time drift detection if a fixture is
/// moved or deleted.
#[test]
fn existing_sub_arms_regression_floor() {
    let baselines: [(&str, &str); 4] = [
        (
            "tests/prd-gate/fixtures/indexed_sub_inst_arm_baseline.ri",
            include_str!("../../tests/prd-gate/fixtures/indexed_sub_inst_arm_baseline.ri"),
        ),
        (
            "tests/prd-gate/fixtures/indexed_sub_coll_arm_baseline.ri",
            include_str!("../../tests/prd-gate/fixtures/indexed_sub_coll_arm_baseline.ri"),
        ),
        (
            "tests/prd-gate/fixtures/indexed_sub_spec_arm_baseline.ri",
            include_str!("../../tests/prd-gate/fixtures/indexed_sub_spec_arm_baseline.ri"),
        ),
        (
            "tests/prd-gate/fixtures/indexed_sub_forall_range_baseline.ri",
            include_str!("../../tests/prd-gate/fixtures/indexed_sub_forall_range_baseline.ri"),
        ),
    ];

    for (name, source) in baselines {
        assert_parses_clean(name, source);
    }
}

/// Negative controls — the indexer clause is accepted on the **instantiation
/// arm only**, and in exactly **one** form.
///
/// GREEN before and after step-2: before, because `[` is not in
/// `sub_declaration` at all; after, because step-2 adds the clause to the
/// instantiation arm alone. Its job is to fail loudly if a future widening
/// leaks the indexer onto the collection/specialization arms or admits a
/// second surface form.
///
/// The last two cases pin **PRD §9.1 Open Q1 as decided here**: there is no
/// binder-omission form. `sub legs[in 0..4] = …` and `sub legs[4] = …` are both
/// parse errors; an unused binder is a `W_UNUSED`-conventions matter, not a
/// reason for a second syntax.
#[test]
fn indexer_clause_is_rejected_outside_the_instantiation_arm() {
    // Wrong arm: the indexer must not leak onto the collection arm.
    assert_has_error(
        "collection arm must reject an indexer",
        "structure S { sub xs[i in 0..4] : List<Foo> }",
    );
    // Wrong arm: nor onto the specialization arm.
    assert_has_error(
        "specialization arm must reject an indexer",
        "structure S { sub m[i in 0..4] : Foo { } }",
    );
    // PRD §9.1 Q1: no binder-omission form. (`in` does lex as an identifier —
    // grammar.js declares no `word:` rule — so this fails at the *following*
    // `in` keyword, not at the binder slot. Either way: ERROR.)
    assert_has_error(
        "omitted binder must be rejected",
        "structure S { sub legs[in 0..4] = Foo() }",
    );
    // PRD §9.1 Q1: no bare-count form either — the binder slot is
    // `$.identifier`, and `4` is not an identifier.
    assert_has_error(
        "bare count must be rejected",
        "structure S { sub legs[4] = Foo() }",
    );
}

// ── Corpus conformance: `test/corpus/indexed_sub_instantiation.txt` ──────────
//
// The corpus file states the same CST contract as this file, in
// `tree-sitter test` S-expression form. `tree-sitter test` is not CI-invoked
// (see the module header), so on its own that copy has nothing keeping it
// honest: the next grammar tweak would be forced to update the Rust assertions
// and would leave the corpus quietly wrong — worse than absent, because a
// reader treats it as the CST reference. So instead of deleting or trimming it,
// the test below reads the corpus file directly and validates every case
// against the live parser. The corpus is therefore load-bearing without
// depending on the `tree-sitter test` CLI being green.

/// One `tree-sitter test` corpus case: name, source, expected S-expression.
struct CorpusCase {
    name: String,
    source: String,
    expected_sexp: String,
}

/// True when `line` is a corpus rule line — three or more repetitions of `c`
/// and nothing else. `=` rules delimit a case header, `-` rules separate a
/// case's source from its expected S-expression.
fn is_rule_line(line: &str, c: char) -> bool {
    let t = line.trim_end();
    t.len() >= 3 && t.chars().all(|ch| ch == c)
}

/// True when a `=` header block starts at `i` (`===` / name / `===`).
fn is_header_start(lines: &[&str], i: usize) -> bool {
    is_rule_line(lines[i], '=') && i + 2 < lines.len() && is_rule_line(lines[i + 2], '=')
}

/// Parse the `tree-sitter test` corpus format into cases.
///
/// Deliberately minimal — it handles exactly the subset this corpus file uses
/// (`===` header, source, `---` divider, expected sexp) and asserts loudly on
/// anything malformed rather than skipping it, so a broken corpus file cannot
/// silently reduce this test to a no-op. Leading `;` comment lines before the
/// first header are ignored, as `tree-sitter test` itself ignores them.
fn parse_corpus(text: &str) -> Vec<CorpusCase> {
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !is_header_start(&lines, i) {
            i += 1;
            continue;
        }
        let name = lines[i + 1].trim().to_string();
        let mut j = i + 3;
        let mut source = String::new();
        while j < lines.len() && !is_rule_line(lines[j], '-') {
            source.push_str(lines[j]);
            source.push('\n');
            j += 1;
        }
        assert!(
            j < lines.len(),
            "corpus case `{name}` has no `---` divider between source and \
             expected S-expression"
        );
        j += 1; // consume the `---` divider
        let mut expected_sexp = String::new();
        while j < lines.len() && !is_header_start(&lines, j) {
            expected_sexp.push_str(lines[j]);
            expected_sexp.push('\n');
            j += 1;
        }
        assert!(
            !source.trim().is_empty(),
            "corpus case `{name}` has an empty source block"
        );
        assert!(
            !expected_sexp.trim().is_empty(),
            "corpus case `{name}` has an empty expected S-expression block"
        );
        cases.push(CorpusCase {
            name,
            source,
            expected_sexp,
        });
        i = j;
    }
    cases
}

/// Collapse all whitespace runs to single spaces so a multi-line corpus
/// S-expression compares equal to `Node::to_sexp()`'s single-line form.
fn normalize_sexp(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every case in `test/corpus/indexed_sub_instantiation.txt` parses cleanly AND
/// matches the expected S-expression the corpus file records.
///
/// This is what keeps the corpus from drifting: it is `include_str!`'d, so a
/// grammar change that alters the indexed-sub CST fails HERE, in the CI-run
/// surface, until the corpus expectations are updated too. `tree-sitter test`
/// remains uninvoked by CI (and 218/219 on `main`, task #5492) — this test
/// deliberately reimplements the corpus reader rather than depending on that CLI.
///
/// # Whole-root equality, deliberately narrow sources
///
/// The comparison is whole-root because that is the only form `tree-sitter
/// test` itself accepts, so the corpus stays runnable under the CLI. The
/// brittleness that would otherwise imply is bounded at the SOURCE end instead:
/// every case is kept minimal — no constructor arguments, no compound pose — so
/// the only shapes pinned here are `structure_definition`, `sub_declaration`,
/// and the indexer's own `binder`/`domain`. An unrelated grammar change to
/// expression or argument nodes therefore cannot fail an indexed-sub test. That
/// the indexer coexists with named args and a `transform3(…)` pose is pinned by
/// [`indexed_sub_cst_exposes_binder_and_domain_fields`], whose hand-written
/// field assertions are indifferent to those nodes' internal shape.
///
/// The case count is asserted so silently dropping a case is also a failure.
#[test]
fn corpus_cases_match_the_live_parser() {
    const CORPUS: &str = include_str!("../test/corpus/indexed_sub_instantiation.txt");
    let cases = parse_corpus(CORPUS);
    assert_eq!(
        cases.len(),
        3,
        "expected 3 corpus cases in test/corpus/indexed_sub_instantiation.txt \
         (indexer+pose, indexer+bare ctor, bare instantiation unchanged), \
         got {}: {:?}",
        cases.len(),
        cases.iter().map(|c| &c.name).collect::<Vec<_>>()
    );

    let mut parser = make_parser();
    for case in &cases {
        let label = format!("corpus case `{}`", case.name);
        assert_parses_clean(&label, &case.source);
        let tree = parser
            .parse(case.source.as_bytes(), None)
            .expect("tree-sitter parse failed for corpus case");
        let actual = normalize_sexp(&tree.root_node().to_sexp());
        let expected = normalize_sexp(&case.expected_sexp);
        assert_eq!(
            actual, expected,
            "{label}: the corpus S-expression has drifted from the live parser.\n\
             expected (from the corpus file):\n{expected}\n\
             actual (from the grammar):\n{actual}\n\
             Fix the corpus file — it is documentation OF the grammar, not a \
             second source of truth."
        );
    }
}
