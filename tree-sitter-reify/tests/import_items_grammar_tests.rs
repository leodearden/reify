//! Grammar integration tests for the canonical destructured-import form, the
//! DOTTED `import a.b.{C, D}` (#5931).
//!
//! Authority: the `import_path` production in `docs/reify-language-spec.md`
//! §15 "Grammar Summary" — the section that gives the "[c]omplete EBNF grammar
//! incorporating all updates from all documents and design review resolutions"
//! — makes the `'.'` an explicit terminal:
//!
//! ```ebnf
//! import_path ::= module_path ('.' '{' IDENT (',' IDENT)* '}')?
//!               | module_path '.' TYPE_IDENT
//! ```
//!
//! Corroborated by that same spec's §7.3 "Import Forms" table (the
//! "Destructured import" row) and by the identical `import_path` production in
//! `docs/initial-design/syntax-design-decisions.md` §11 "Grammar summary".
//!
//! These tests assert on the CST itself, counting every ERROR node, so they pin
//! the separator at the grammar level independently of how the lowering in
//! `crates/reify-syntax` reports a recovered parse: tree-sitter can
//! error-recover a stray `.` into an `(ERROR)` node nested inside
//! `import_declaration` while leaving `path` and `items` intact.
//!
//! Coverage:
//! * **(a)** Canonical `import std.mech.{Bolt, Nut}` — zero ERROR nodes, with
//!   `path` = `import_path(std, mech)` and `items` = `import_items(Bolt, Nut)`.
//! * **(b)** Single item `import std.mech.{Bolt}` — likewise clean.
//! * **(c)** NEGATIVE: spaced `import std.mech {Bolt, Nut}` produces at least
//!   one ERROR node.  This is the assertion that pins the decision rather than
//!   merely permitting the canonical spelling.
//! * **(d)** REGRESSION: the other `import_path` consumers parse cleanly despite
//!   the GLR `conflicts` entry the dotted form requires on `import_path`.
//! * **(e)** DIVERGENCE: interior whitespace before the brace list
//!   (`import a . { B }`) is accepted here and rejected by the GUI's Lezer port.
//! * **(f)** LATITUDE: the empty (`import a.{}`) and trailing-comma
//!   (`import a.{Foo,}`) item lists, which §15's EBNF does not describe but
//!   `commaSep` admits.
//!
//! See also: `tree-sitter-reify/test/corpus/import_items.txt` for the
//! corpus-level CST documentation, runnable via `tree-sitter test`.

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
fn find_node_by_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
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

/// Count nodes of kind `ERROR` anywhere in the tree.
///
/// The failure mode this guards is a *nested* `(ERROR)` sibling inside
/// `import_declaration` while `path` and `items` remain well-formed, so the
/// count looks at every node rather than at the declaration's fields.
fn count_error_nodes(node: tree_sitter::Node) -> usize {
    collect_kinds(node).iter().filter(|k| *k == "ERROR").count()
}

/// Text of every direct `identifier` child of `node`, in source order.
fn identifier_children(node: tree_sitter::Node, source: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "identifier" {
                out.push(
                    child
                        .utf8_text(source)
                        .expect("identifier text is valid utf8")
                        .to_string(),
                );
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    out
}

/// Parse `source`, assert zero ERROR nodes (and no `has_error()`), return the tree.
fn parse_clean(source: &str) -> tree_sitter::Tree {
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    let root = tree.root_node();
    let kinds = collect_kinds(root);
    assert!(
        !root.has_error(),
        "`{source}` must parse with no ERROR nodes; got node kinds: {kinds:?}"
    );
    assert_eq!(
        count_error_nodes(root),
        0,
        "`{source}` must contain zero nested ERROR nodes; got node kinds: {kinds:?}"
    );
    tree
}

/// The identifiers inside the `items` field of the single import in `source`,
/// with the parse asserted clean.
fn item_identifiers(source: &str) -> Vec<String> {
    let tree = parse_clean(source);
    let root = tree.root_node();
    let kinds = collect_kinds(root);

    let decl = find_node_by_kind(root, "import_declaration")
        .unwrap_or_else(|| panic!("expected an `import_declaration` node; got kinds: {kinds:?}"));
    let items = decl
        .child_by_field_name("items")
        .unwrap_or_else(|| panic!("`{source}` must have an `items` field; kinds: {kinds:?}"));
    assert_eq!(
        items.kind(),
        "import_items",
        "the `items` field must be an `import_items` node; kinds: {kinds:?}"
    );
    identifier_children(items, source.as_bytes())
}

// ── (a) Canonical form: `import std.mech.{Bolt, Nut}` ────────────────────────

/// (a) The canonical dotted destructured import parses with a CLEAN CST: the
/// `.` is a terminal of the rule, not a token the parser recovers from.
#[test]
fn canonical_dotted_destructured_import_parses_cleanly() {
    let source = "import std.mech.{Bolt, Nut}";
    parse_clean(source);
}

/// (a) The canonical form's CST keeps two separate fields on
/// `import_declaration`: `path` holding `import_path(std, mech)` and `items`
/// holding `import_items(Bolt, Nut)`.
///
/// The separate `items` field is load-bearing: `lower_import` in
/// `crates/reify-syntax/src/ts_parser.rs` distinguishes
/// `ImportKind::Destructured` from Aliased/Entity/Module by which optional
/// FIELD is present, so the grammar adds only the `'.'` terminal and does NOT
/// fold the braces into `import_path` the way the spec EBNF nests them.
#[test]
fn canonical_dotted_destructured_import_has_path_and_items_fields() {
    let source = "import std.mech.{Bolt, Nut}";
    let tree = parse_clean(source);
    let root = tree.root_node();
    let kinds = collect_kinds(root);

    let decl = find_node_by_kind(root, "import_declaration")
        .unwrap_or_else(|| panic!("expected an `import_declaration` node; got kinds: {kinds:?}"));

    let path = decl.child_by_field_name("path").unwrap_or_else(|| {
        panic!("`import_declaration` must have a `path` field; kinds: {kinds:?}")
    });
    assert_eq!(
        path.kind(),
        "import_path",
        "the `path` field must be an `import_path` node; kinds: {kinds:?}"
    );
    assert_eq!(
        identifier_children(path, source.as_bytes()),
        vec!["std".to_string(), "mech".to_string()],
        "`path` must hold exactly the identifiers std, mech; kinds: {kinds:?}"
    );

    let items = decl.child_by_field_name("items").unwrap_or_else(|| {
        panic!("`import_declaration` must have an `items` field; kinds: {kinds:?}")
    });
    assert_eq!(
        items.kind(),
        "import_items",
        "the `items` field must be an `import_items` node; kinds: {kinds:?}"
    );
    assert_eq!(
        identifier_children(items, source.as_bytes()),
        vec!["Bolt".to_string(), "Nut".to_string()],
        "`items` must hold exactly the identifiers Bolt, Nut; kinds: {kinds:?}"
    );
}

// ── (b) Single item: `import std.mech.{Bolt}` ────────────────────────────────

/// (b) A single-item destructured import parses cleanly and yields `items == [Bolt]`.
#[test]
fn single_item_dotted_destructured_import_parses_cleanly() {
    assert_eq!(
        item_identifiers("import std.mech.{Bolt}"),
        vec!["Bolt".to_string()],
        "single-item `items` must hold exactly [Bolt]"
    );
}

// ── (c) NEGATIVE: the spaced form is not Reify ───────────────────────────────

/// (c) The SPACED form `import std.mech {Bolt, Nut}` MUST be a parse error.
///
/// This is the assertion that pins the decision rather than merely permitting
/// the canonical spelling: without it, a grammar that admitted both spellings
/// would pass every other test here.
#[test]
fn spaced_destructured_import_is_a_parse_error() {
    let source = "import std.mech {Bolt, Nut}";
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    let root = tree.root_node();
    let kinds = collect_kinds(root);
    assert!(
        count_error_nodes(root) > 0,
        "`{source}` (space instead of `.`) must produce at least one ERROR node — \
         the canonical destructured form is `import a.b.{{C, D}}` per \
         docs/reify-language-spec.md §15's `import_path`; got node kinds: {kinds:?}"
    );
}

// ── (d) REGRESSION: the other `import_path` consumers stay clean ─────────────

/// (d) Every other production that reaches `import_path` parses with zero
/// ERROR nodes.
///
/// `import_path` carries a GLR `conflicts` entry (mandatory — without it
/// `tree-sitter generate` aborts on "Unresolved conflict for symbol sequence:
/// 'import' identifier • '.'").  A `conflicts` entry changes the generated parse
/// table's state splitting, so these shared-rule consumers are pinned against a
/// split-state regression.
#[test]
fn regression_other_import_path_forms_parse_cleanly() {
    for source in [
        "module a.b",
        "import std.units",
        "import a.b as c",
        "pub import a.b.C as X",
        "import std.math.Sqrt",
    ] {
        parse_clean(source);
    }
}

/// (d) The regression forms also survive when combined in a single file, which
/// exercises `repeat($._declaration)` across the split states rather than one
/// declaration in isolation.
#[test]
fn regression_combined_import_file_parses_cleanly() {
    let source = "module a.b\n\
                  \n\
                  import std.units\n\
                  import a.b as c\n\
                  pub import a.b.C as X\n\
                  import std.math.Sqrt\n\
                  import std.mech.{Bolt, Nut}\n\
                  import a.{Foo}\n\
                  import parts.{Bolt, Nut}\n";
    parse_clean(source);
}

// ── The shapes adjacent to the separator ────────────────────────────────
//
// These pin the two properties next to the separator — whitespace around it
// and the bounds of the item list — so that neither can flip without a test
// noticing.

/// (e) Interior whitespace before the brace list is ACCEPTED here: tree-sitter
/// lexes the `.` and the `{` as two separate tokens with whitespace between
/// them as an extra.
///
/// The GUI's Lezer port rejects it — its one deliberate divergence, explained
/// at the ImportDeclaration comment in gui/src/editor/reify.grammar — and pins
/// that half in `rejects interior whitespace in the opener` in
/// gui/src/__tests__/reifyGrammarCorpus.test.ts, so whichever side moves, a
/// test fails instead of the two grammars silently parting ways.
#[test]
fn interior_whitespace_before_the_brace_list_is_accepted() {
    assert_eq!(
        item_identifiers("import a . { B }"),
        vec!["B".to_string()],
        "whitespace around the `.` is lexed away here, unlike in the Lezer port"
    );
}

/// (f) The empty and trailing-comma item lists are DELIBERATE LATITUDE.
///
/// §15's EBNF is tighter than this rule: `'{' IDENT (',' IDENT)* '}'` requires
/// at least one IDENT and admits no trailing comma. `import_items` is built on
/// the shared `commaSep` helper, which admits both — as does the Lezer port's
/// `(Identifier ("," Identifier)* ","?)?`.
///
/// That latitude is KEPT, and this test is what makes keeping it a decision.
/// A trailing comma is admitted uniformly by every comma-separated list in both
/// grammars (enum variants, meta entries, match arms, set/map literals), so
/// rejecting it only for imports would be a local inconsistency; and an empty
/// list is a transient state while typing `.{}` before filling it in, which an
/// error-tolerant editing grammar should keep parsing. A diagnostic for a
/// vacuous import belongs to the semantic layer, not to either grammar.
///
/// What the empty list LOWERS to — `ImportKind::Destructured(vec![])`, not
/// `Module` and not an error — is pinned at the AST level by
/// `empty_and_trailing_comma_destructured_imports_lower_as_written` in
/// crates/reify-syntax/tests/harness_syntax/import_tests.rs.
#[test]
fn empty_and_trailing_comma_item_lists_are_deliberate_latitude() {
    assert!(
        item_identifiers("import a.{}").is_empty(),
        "the empty item list is accepted and holds no identifiers"
    );
    assert_eq!(
        item_identifiers("import a.{Foo,}"),
        vec!["Foo".to_string()],
        "a trailing comma is accepted and contributes no phantom identifier"
    );
}
