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
//! The CST-shape group below (`derived_sub_cst_exposes_*`, `derived_body_*`,
//! `keep_disposition_*`, `exclude_disposition_*`, `derived_sub_accepts_at_*`,
//! `derived_sub_modifiers_*`) is **RED before the grammar step** — every one of
//! its field names is introduced by that step — and GREEN after.
//!
//! # Assertion style: fields, not whole-tree S-expressions
//!
//! These tests assert *field presence, node kind and source text*, never a
//! whole-subtree S-expression. That is the lesson recorded in the sibling
//! indexed-sub corpus header: a whole-tree comparison couples every derived-sub
//! test to the internals of unrelated nodes (`quantity_literal`, `unit_expr`,
//! `binary_expression`, …), so an unrelated grammar change fails derived-sub
//! tests for no reason. Whole-root equality is confined to
//! [`corpus_cases_match_the_live_parser`], whose sources are kept minimal
//! precisely so that coupling stays bounded.
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

// ─────────────────────────────────────────────────────────────────────────────
// CST shape: the fields the lowering step (and A-beta) will read
// ─────────────────────────────────────────────────────────────────────────────

/// Parse `source`, assert it is clean, and return the parse tree.
///
/// Returning the tree (not a `Node`) keeps the borrow alive for the caller;
/// each test re-derives `root_node()` from it.
fn parse_clean(label: &str, source: &str) -> tree_sitter::Tree {
    assert_parses_clean(label, source);
    make_parser()
        .parse(source, None)
        .expect("parse failed")
}

/// Fetch a named field, failing with the containing node's S-expression.
fn field<'a>(node: tree_sitter::Node<'a>, name: &str) -> tree_sitter::Node<'a> {
    node.child_by_field_name(name).unwrap_or_else(|| {
        panic!(
            "expected a `{name}` field on `{}`; got:\n{}",
            node.kind(),
            node.to_sexp()
        )
    })
}

/// The `sub_declaration` node of a single-sub source.
fn sole_sub(tree: &tree_sitter::Tree) -> tree_sitter::Node<'_> {
    find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node")
}

/// Kinds of a node's *named* children, in source order.
fn named_child_kinds(node: tree_sitter::Node) -> Vec<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(|c| c.kind().to_string())
        .collect()
}

/// `mirror of <proto> across <plane>` reaches `derivation` as a
/// `sub_derivation` carrying `prototype` and `plane`.
///
/// The `prototype` span is asserted to cover the prototype token ALONE — A-beta
/// (#6616) underlines exactly that span for its unknown/non-sibling-prototype
/// diagnostic, so a derivation-wide span here would silently degrade that
/// message.
#[test]
fn derived_sub_cst_exposes_mirror_derivation_fields() {
    const SRC: &str = "structure S { sub b = mirror of a across plane_yz { z = 55mm } }";
    let tree = parse_clean("mirror derivation", SRC);
    let sub = sole_sub(&tree);

    let derivation = field(sub, "derivation");
    assert_eq!(derivation.kind(), "sub_derivation");

    let prototype = field(derivation, "prototype");
    assert_eq!(prototype.kind(), "identifier");
    assert_eq!(
        &SRC[prototype.byte_range()],
        "a",
        "the prototype span must cover the prototype token alone, so A-beta's \
         unknown-prototype diagnostic underlines exactly it"
    );

    let plane = field(derivation, "plane");
    assert_eq!(&SRC[plane.byte_range()], "plane_yz");

    assert!(
        derivation.child_by_field_name("transform").is_none(),
        "the mirror alternative must not expose a `transform` field"
    );
    assert_eq!(field(sub, "body").kind(), "derived_body");
}

/// `image of <proto> under <transform>` reaches `derivation` as a
/// `sub_derivation` carrying `prototype` and `transform`.
///
/// One rule with two alternatives, not two sibling rules: PRD §6 D1 makes the
/// derivation an ELEMENT type whose Layer-3 group elements become further
/// constructors, so consumers switch on the constructor rather than on which
/// of several node kinds appeared. Both alternatives therefore share the
/// `sub_derivation` kind and differ only in their transform-side field.
#[test]
fn derived_sub_cst_exposes_image_derivation_fields() {
    const SRC: &str =
        "structure S { sub b = image of a under transform_compose(c2_z, t) { } }";
    let tree = parse_clean("image derivation", SRC);
    let derivation = field(sole_sub(&tree), "derivation");

    assert_eq!(derivation.kind(), "sub_derivation");
    assert_eq!(&SRC[field(derivation, "prototype").byte_range()], "a");
    assert_eq!(
        &SRC[field(derivation, "transform").byte_range()],
        "transform_compose(c2_z, t)"
    );
    assert!(
        derivation.child_by_field_name("plane").is_none(),
        "the image alternative must not expose a `plane` field"
    );
}

/// A `derived_param_assignment` exposes `name` and `value`, and `value`
/// discriminates a RESET (`default_reset`) from an ordinary override.
///
/// `default_reset` is a named node rather than a bare anonymous `'default'`
/// token so lowering can tell the two apart without re-reading source text.
#[test]
fn derived_param_assignment_exposes_name_and_value() {
    const SRC: &str = "structure S { sub b = mirror of a across P { span_bu = default
    z = 55mm } }";
    let tree = parse_clean("param overrides", SRC);
    let body = field(sole_sub(&tree), "body");

    let mut cursor = body.walk();
    let assignments: Vec<_> = body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "derived_param_assignment")
        .collect();
    assert_eq!(assignments.len(), 2, "got:\n{}", body.to_sexp());

    assert_eq!(&SRC[field(assignments[0], "name").byte_range()], "span_bu");
    assert_eq!(
        field(assignments[0], "value").kind(),
        "default_reset",
        "`<param> = default` must reach `value` as a `default_reset` node"
    );

    assert_eq!(&SRC[field(assignments[1], "name").byte_range()], "z");
    assert_eq!(field(assignments[1], "value").kind(), "quantity_literal");
}

/// `auto` / `auto(free)` reach `value` through `_binding_value`, exactly as on
/// the specialization arm — so a derived override lowers to `ExprKind::Auto`
/// with no extra lowering path. A `where` guard attaches as `guard`.
#[test]
fn derived_param_assignment_admits_auto_and_where_guard() {
    const SRC: &str = "structure S { sub b = mirror of a across P { z = auto
    w = auto(free)
    q = 55mm where cond } }";
    let tree = parse_clean("auto + guard overrides", SRC);
    let body = field(sole_sub(&tree), "body");

    let mut cursor = body.walk();
    let assignments: Vec<_> = body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "derived_param_assignment")
        .collect();
    assert_eq!(assignments.len(), 3, "got:\n{}", body.to_sexp());

    assert_eq!(field(assignments[0], "value").kind(), "auto_keyword");
    assert_eq!(field(assignments[1], "value").kind(), "auto_keyword");
    assert_eq!(&SRC[field(assignments[1], "value").byte_range()], "auto(free)");

    assert!(
        assignments[0].child_by_field_name("guard").is_none(),
        "an unguarded override must expose no `guard` field"
    );
    assert_eq!(field(assignments[2], "guard").kind(), "where_clause");
}

/// `keep <path>` exposes `path` as a `disposition_path`; the reserved
/// `using <plane>` tail adds a `plane` field (PRD §3.3/§11 — parsed and stored,
/// no v1 meaning).
#[test]
fn keep_disposition_exposes_path_and_reserved_using_plane() {
    const SRC: &str = "structure S { sub b = mirror of a across P { keep capstan
    keep drum using plane_xz } }";
    let tree = parse_clean("keep dispositions", SRC);
    let body = field(sole_sub(&tree), "body");

    let mut cursor = body.walk();
    let keeps: Vec<_> = body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "keep_disposition")
        .collect();
    assert_eq!(keeps.len(), 2, "got:\n{}", body.to_sexp());

    let bare = field(keeps[0], "path");
    assert_eq!(bare.kind(), "disposition_path");
    assert_eq!(&SRC[bare.byte_range()], "capstan");
    assert!(
        keeps[0].child_by_field_name("plane").is_none(),
        "a bare `keep` must expose no `plane` field"
    );

    assert_eq!(&SRC[field(keeps[1], "path").byte_range()], "drum");
    assert_eq!(
        &SRC[field(keeps[1], "plane").byte_range()],
        "plane_xz",
        "the reserved `using <plane>` tail must reach a `plane` field"
    );
}

/// `exclude <a>.<b>` exposes a dotted `disposition_path` whose identifier
/// segments are separately addressable.
///
/// `disposition_path` is the `import_path` dotted-identifier SHAPE, not
/// `$._expression`: an expression-valued path could absorb the next
/// newline-separated body item at its right edge (INV-SF-7), and it would
/// silently accept the `xs[<element>]` addressing that PRD §8 item (iii)
/// reserves but does not define.
#[test]
fn exclude_disposition_exposes_dotted_disposition_path() {
    const SRC: &str = "structure S { sub b = mirror of a across P { exclude web.hub } }";
    let tree = parse_clean("dotted exclude", SRC);
    let body = field(sole_sub(&tree), "body");

    let exclude = find_node_by_kind(body, "exclude_disposition")
        .expect("expected an exclude_disposition node");
    let path = field(exclude, "path");
    assert_eq!(path.kind(), "disposition_path");
    assert_eq!(&SRC[path.byte_range()], "web.hub");
    assert_eq!(
        named_child_kinds(path),
        vec!["identifier", "identifier"],
        "a dotted path must expose one identifier per segment"
    );
}

/// The derived body admits `let` and `constraint` members alongside overrides
/// and dispositions — and each body item is its OWN sibling node.
///
/// The sibling-count assertion is the INV-SF-7 evidence at CST level: four
/// newline-separated items with no separator token must yield four children,
/// proving no item's right edge absorbed the following line.
#[test]
fn derived_body_admits_members_and_keeps_items_separate() {
    const SRC: &str = "structure S { sub b = mirror of a across P { z = 55mm
    keep drum
    exclude web
    w = 2mm
    let g = 3mm
    constraint g > 1mm } }";
    let tree = parse_clean("mixed derived body", SRC);
    let body = field(sole_sub(&tree), "body");

    assert_eq!(
        named_child_kinds(body),
        vec![
            "derived_param_assignment",
            "keep_disposition",
            "exclude_disposition",
            "derived_param_assignment",
            "let_declaration",
            "constraint_declaration",
        ],
        "each newline-separated body item must be its own sibling node \
         (INV-SF-7: nothing absorbs the following line); got:\n{}",
        body.to_sexp()
    );
}

/// An empty `{ }` body is legal — the derivation alone is a complete
/// specification, so `derived_body` uses `repeat`, not `repeat1`.
#[test]
fn derived_body_may_be_empty() {
    let tree = parse_clean("empty body", "structure S { sub b = mirror of a across P { } }");
    let body = field(sole_sub(&tree), "body");
    assert_eq!(body.kind(), "derived_body");
    assert!(named_child_kinds(body).is_empty(), "got:\n{}", body.to_sexp());
}

/// `at <pose>` PARSES CLEAN on the derived arm and reaches the `pose` field.
///
/// This pins the D3-adversary ownership ruling AT THE GRAMMAR LAYER: placement
/// of a derived sub is derived, so an explicit `at` is an error — but it is
/// `E_DERIVED_SUB_EXPLICIT_AT` (T8), A-beta's (#6616) COMPILE-scope diagnostic.
/// A future change that smuggles that rejection into the grammar (making this a
/// parse error) must fail here, because a parse error cannot carry T8's
/// message.
#[test]
fn derived_sub_accepts_at_pose_leaving_t8_to_the_compiler() {
    const SRC: &str = "structure S { sub b = mirror of a across P { } at origin }";
    let tree = parse_clean("derived sub with explicit at", SRC);
    let sub = sole_sub(&tree);
    assert_eq!(&SRC[field(sub, "pose").byte_range()], "origin");
}

/// `priv` and `aux` are both accepted on the derived arm and both survive into
/// the CST, so the arm carries the same modifier surface as the other three.
#[test]
fn derived_sub_modifiers_priv_and_aux_are_preserved() {
    const SRC: &str = "structure S { priv aux sub b = mirror of a across P { } }";
    let tree = parse_clean("priv aux derived sub", SRC);
    let kinds = collect_kinds(sole_sub(&tree));
    for modifier in ["priv", "aux"] {
        assert!(
            kinds.iter().any(|k| k == modifier),
            "the `{modifier}` modifier token must survive into the CST; got {kinds:?}"
        );
    }
}
