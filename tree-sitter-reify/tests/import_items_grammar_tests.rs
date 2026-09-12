//! Grammar integration tests for the canonical destructured-import form.
//!
//! Task 5931, step-1 (TDD RED): pins the canonical surface syntax for a
//! destructured import as the DOTTED form `import a.b.{C, D}`.
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
//! Until step-4 (the `grammar.js` edit) lands, these are RED — and RED in two
//! distinct directions, which is the whole point:
//!   * the CANONICAL dotted form parses only via tree-sitter ERROR RECOVERY —
//!     the stray `.` lands in a sibling `(ERROR [0,15]-[0,16])` node inside the
//!     `import_declaration`, leaving `path`/`items` intact.  The AST-level
//!     tests in `crates/reify-syntax/tests/harness_syntax/import_tests.rs`
//!     have therefore been passing on main for ~5 months without the grammar
//!     ever being correct.  Their greenness is NOT evidence of a correct rule.
//!   * the SPACED form `import a.b {C, D}` parses CLEANLY today, i.e. today's
//!     grammar officially admits the wrong spelling and merely tolerates the
//!     right one.
//!
//! Coverage:
//! * **(a)** Canonical `import std.mech.{Bolt, Nut}` — zero ERROR nodes, and the
//!   CST shape survives: `path` = `import_path(std, mech)`, `items` =
//!   `import_items(Bolt, Nut)`.
//! * **(b)** Single item `import std.mech.{Bolt}` — likewise clean.
//! * **(c)** NEGATIVE: spaced `import std.mech {Bolt, Nut}` MUST produce at
//!   least one ERROR node.  This is the assertion that actually pins the
//!   decision rather than merely permitting it.
//! * **(d)** REGRESSION: the other `import_path` consumers keep parsing cleanly.
//!   `import_path` gains a GLR `conflicts` entry in step-4, so these guard
//!   against a split-state regression.
//! * **(e)** DIVERGENCE: interior whitespace before the brace list
//!   (`import a . { B }`) is accepted HERE and rejected by the GUI's Lezer
//!   port — the authoritative half of that port's one deliberate narrowing.
//! * **(f)** LATITUDE: the empty (`import a.{}`) and trailing-comma
//!   (`import a.{Foo,}`) item lists, which §15's EBNF does not describe but
//!   `commaSep` admits. Added in the review round, never RED.
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
/// `Node::has_error()` alone is not sufficient here: the pre-step-4 failure mode
/// is a *nested* `(ERROR)` sibling inside `import_declaration` while `path` and
/// `items` remain well-formed, so the assertion must look at every node.
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

// ── (a) Canonical form: `import std.mech.{Bolt, Nut}` ────────────────────────

/// (a) The canonical dotted destructured import parses with a CLEAN CST.
///
/// RED until step-4: today the stray `.` becomes `(ERROR [0,15]-[0,16])`
/// nested inside the `import_declaration`, alongside intact `path`/`items`
/// fields — which is exactly why the AST-level tests never noticed.
#[test]
fn canonical_dotted_destructured_import_parses_cleanly() {
    let source = "import std.mech.{Bolt, Nut}";
    parse_clean(source);
}

/// (a) The canonical form's CST shape survives the fix unchanged:
/// `import_declaration` keeps a `path` field holding `import_path(std, mech)`
/// and a separate `items` field holding `import_items(Bolt, Nut)`.
///
/// The separate `items` field is load-bearing: `lower_import`
/// (`crates/reify-syntax/src/ts_parser.rs:520-548`) distinguishes
/// `ImportKind::Destructured` from Aliased/Entity/Module by which optional
/// FIELD is present, so step-4 must add only the `'.'` terminal and must NOT
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
///
/// RED until step-4.
#[test]
fn single_item_dotted_destructured_import_parses_cleanly() {
    let source = "import std.mech.{Bolt}";
    let tree = parse_clean(source);
    let root = tree.root_node();
    let kinds = collect_kinds(root);

    let decl = find_node_by_kind(root, "import_declaration")
        .unwrap_or_else(|| panic!("expected an `import_declaration` node; got kinds: {kinds:?}"));
    let items = decl.child_by_field_name("items").unwrap_or_else(|| {
        panic!("`import_declaration` must have an `items` field; kinds: {kinds:?}")
    });
    assert_eq!(
        identifier_children(items, source.as_bytes()),
        vec!["Bolt".to_string()],
        "single-item `items` must hold exactly [Bolt]; kinds: {kinds:?}"
    );
}

// ── (c) NEGATIVE: the spaced form is not Reify ───────────────────────────────

/// (c) The SPACED form `import std.mech {Bolt, Nut}` MUST be a parse error.
///
/// This is the assertion that pins the decision rather than merely permitting
/// the canonical spelling.  RED until step-4 for the opposite reason to (a) and
/// (b): today the spaced form parses CLEANLY, so today's grammar officially
/// admits the form the spec does not describe.
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

/// (d) Every other production that reaches `import_path` still parses with zero
/// ERROR nodes after step-4.
///
/// `import_path` gains a GLR `conflicts` entry in step-4 (mandatory — without it
/// `tree-sitter generate` aborts on "Unresolved conflict for symbol sequence:
/// 'import' identifier • '.'").  A `conflicts` entry changes the generated parse
/// table's state splitting, so these shared-rule consumers must be pinned
/// against a split-state regression.
///
/// These are GREEN before AND after step-4.
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

// ── Review round: the shapes adjacent to the settled one ────────────────
//
// #5931 settled the SEPARATOR. These pin the two neighbouring properties that
// the settling left unasserted, so that neither can flip without a test
// noticing.

/// The identifiers inside the `items` field of the single import in `source`,
/// with the parse asserted clean.
///
/// The enumerated tests above predate this helper and are left as they are —
/// this is an amendment, not a refactor of the file.
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

/// (e) Interior whitespace before the brace list is ACCEPTED here.
///
/// This is the authoritative half of the GUI Lezer port's ONE deliberate
/// narrowing. tree-sitter lexes the `.` and the `{` as two separate anonymous
/// tokens with whitespace between them as an extra, so `import a . { B }` is
/// well-formed. The port cannot follow: it folds both into a single
/// `ImportItemsOpen` token to escape a shift/reduce conflict that
/// lezer-generator, having no `conflicts` escape hatch, cannot otherwise
/// resolve — see the ImportDeclaration comment in gui/src/editor/reify.grammar.
///
/// Pinning BOTH halves is what makes that divergence a decision rather than
/// drift: the rejecting half is asserted by `rejects interior whitespace in the
/// opener` in gui/src/__tests__/reifyGrammarCorpus.test.ts, so whichever side
/// moves, a test fails instead of the two grammars silently parting ways.
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
