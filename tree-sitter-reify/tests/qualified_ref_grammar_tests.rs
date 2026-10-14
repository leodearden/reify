//! Grammar integration tests for qualified references through an import
//! binding — `pp.Pulley` in TYPE position and as a `sub` structure_name
//! (task 5495 μ, step-1 RED / step-2 GREEN), plus the EXPRESSION-position call
//! form `pp.Pulley()` (step-3 RED / step-4 GREEN).
//!
//! Step-1 RED: grammar.js's `type_expr` rule was
//! `choice(function_type, parameterized_type, qualified_type, identifier)` — it
//! had NO dotted arm, so the `.` in `pp.Pulley` matched no arm and produced an
//! ERROR subtree.  Its `sub_declaration` rule bound
//! `field('structure_name', $.identifier)` in all three arms, so a dotted
//! structure name likewise ERRORs.
//!
//! Step-2 GREEN: one shared rule
//!   `namespaced_name: $ => seq(field('binding', $.identifier), '.',
//!                              field('name', $.identifier))`
//! is added as an arm of `type_expr` (covering all `$.type_expr` use sites plus
//! `type_arg_list` at once) and as an alternative for `structure_name` in all
//! three `sub_declaration` arms.
//!
//! This file is the CI-visible pin for PRD `docs/prds/v0_6/stdlib-namespace.md`
//! §7 boundary #15: `tree-sitter parse --quiet` and `tree-sitter test` are never
//! invoked by any gate or hook, so the two prd-gate fixtures would otherwise be
//! silently unverified.  Each fixture is supplied by `include_str!` at its FULL
//! repo-relative leaf path — see `assert_prd_gate_fixture_parses_clean` for why
//! naming the fixtures DIRECTORY is not an option here.
//!
//! The harness (make_parser / count_errors / collect_kinds / find_node_by_kind /
//! find_all_nodes_by_kind) mirrors
//! tree-sitter-reify/tests/function_type_grammar_tests.rs.

use tree_sitter_reify::language;

fn make_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language().into())
        .expect("Error loading Reify grammar");
    parser
}

/// Depth-first count of ERROR and MISSING nodes.
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = 0;
    if node.is_error() || node.is_missing() {
        count += 1;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            count += count_errors(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    count
}

/// Collect all node kinds depth-first (for error diagnostics).
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

/// Collect all nodes with the given kind (depth-first).
fn find_all_nodes_by_kind<'a>(
    node: tree_sitter::Node<'a>,
    kind: &str,
) -> Vec<tree_sitter::Node<'a>> {
    let mut results = Vec::new();
    if node.kind() == kind {
        results.push(node);
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            results.extend(find_all_nodes_by_kind(cursor.node(), kind));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    results
}

/// Parse `source`, assert 0 ERROR/MISSING nodes, and return the tree.
fn parse_clean(source: &str) -> tree_sitter::Tree {
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "`{source}` must parse with 0 ERROR/MISSING nodes; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    tree
}

/// Kinds of every node whose span is EXACTLY the first occurrence of `text` in
/// `source`, outermost first — the Rust twin of `nodeNamesSpanning` in
/// gui/src/__tests__/reifyGrammarQualifiedRef.test.ts.
///
/// `find_node_by_kind(root, "member_access").is_some()` is a WEAKER control than
/// it reads: on `obj.width` it would still pass if the grammar had wrapped that
/// `member_access` in something else, or produced one over a different span.
/// Anchoring the span is what makes a control assert that the shape did not move
/// (task 5495 μ, amendment). Panics on an absent needle so a typo'd one fails
/// loudly rather than vacuously.
fn kinds_spanning(node: tree_sitter::Node, source: &str, text: &str) -> Vec<String> {
    let from = source
        .find(text)
        .unwrap_or_else(|| panic!("no {text:?} in: {source:?}"));
    let to = from + text.len();
    let mut out = Vec::new();
    let mut cursor = node.walk();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.start_byte() == from && n.end_byte() == to {
            out.push(n.kind().to_string());
        }
        // Push children reversed so the walk stays outermost-first, depth-first.
        let children: Vec<_> = n.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
    out
}

/// Text of a node's named field child, for field-level assertions.
fn field_text<'a>(node: tree_sitter::Node<'a>, field: &str, source: &'a str) -> String {
    node.child_by_field_name(field)
        .unwrap_or_else(|| panic!("node `{}` has no `{field}` field", node.kind()))
        .utf8_text(source.as_bytes())
        .expect("utf8")
        .to_string()
}

// ── (a) The two prd-gate fixtures — boundary #15's CI-visible pin ────────────

/// Assert a prd-gate fixture parses with 0 ERROR/MISSING nodes, naming it by
/// its full repo-relative path in the failure message.
///
/// The content arrives via `include_str!` at each call site rather than being
/// read at runtime from a `CARGO_MANIFEST_DIR`-joined DIRECTORY. That is not a
/// style preference: `scripts/verify.sh`'s prd-gate carve-out rests on the
/// premise that nothing globs `tests/prd-gate/fixtures/`, so ADDING a fixture
/// provably cannot change any Rust target's inputs — and a bare reference to the
/// directory voids it. `tests/infra/test_verify_scope.sh`'s PG-DRIFT-DIR
/// scenario enforces exactly that. Citing each leaf in full is also what
/// `tests/prd-gate/README.md` requires of a coupled fixture, and it buys
/// compile-time drift detection if a fixture is moved or deleted. The sibling
/// `tree-sitter-reify/tests/indexed_sub_grammar_tests.rs` uses the same shape.
///
/// NOTE the differing prefix: `include_str!` resolves relative to THIS SOURCE
/// FILE (`tree-sitter-reify/tests/`, hence `../../`), whereas the
/// `CARGO_MANIFEST_DIR` it replaces was the crate root (`../`).
fn assert_prd_gate_fixture_parses_clean(path: &str, source: &str) {
    let mut parser = make_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse returned None");
    assert_eq!(
        count_errors(tree.root_node()),
        0,
        "prd-gate fixture {path} must parse with 0 ERROR/MISSING nodes \
         (PRD §7 boundary #15); got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// `tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri` (`param p : pp.Pulley`)
/// parses with 0 ERROR/MISSING nodes.
///
/// RED: `type_expr` has no dotted arm — the `.` produces an ERROR subtree.
/// GREEN (step-2): the `namespaced_name` arm accepts it.
#[test]
fn prd_gate_qualified_type_fixture_parses_with_zero_errors() {
    assert_prd_gate_fixture_parses_clean(
        "tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri",
        include_str!("../../tests/prd-gate/fixtures/stdlib_ns_qualified_type.ri"),
    );
}

/// `tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri` (`sub p = pp.Pulley()`)
/// parses with 0 ERROR/MISSING nodes.
///
/// RED: `sub_declaration`'s `structure_name` is a bare `$.identifier`.
/// GREEN (step-2): `choice($.identifier, $.namespaced_name)` accepts it.
#[test]
fn prd_gate_qualified_expr_fixture_parses_with_zero_errors() {
    assert_prd_gate_fixture_parses_clean(
        "tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri",
        include_str!("../../tests/prd-gate/fixtures/stdlib_ns_qualified_expr.ri"),
    );
}

// ── (b) Type position ───────────────────────────────────────────────────────

/// `param p : pp.Pulley` puts a `namespaced_name` under `param_declaration`'s
/// `type` field, with `binding` and `name` child fields.
#[test]
fn qualified_type_annotation_yields_namespaced_name_with_fields() {
    let source = "structure def S {\n    param p : pp.Pulley\n}\n";
    let tree = parse_clean(source);

    let param = find_node_by_kind(tree.root_node(), "param_declaration")
        .expect("expected a param_declaration node");
    let type_node = param
        .child_by_field_name("type")
        .expect("param_declaration must have a `type` field");

    let ns = find_node_by_kind(type_node, "namespaced_name").unwrap_or_else(|| {
        panic!(
            "expected a namespaced_name node under the param's `type` field for \
             `pp.Pulley`; got kinds: {:?}",
            collect_kinds(type_node)
        )
    });

    assert_eq!(field_text(ns, "binding", source), "pp");
    assert_eq!(field_text(ns, "name", source), "Pulley");
}

/// `param q : List<pp.Pulley>` nests the `namespaced_name` inside a
/// `type_arg_list` — the single `type_expr` arm covers type-argument position
/// too, with no separate rule.
#[test]
fn qualified_type_inside_type_arg_list() {
    let source = "structure def S {\n    param q : List<pp.Pulley>\n}\n";
    let tree = parse_clean(source);

    let type_args = find_node_by_kind(tree.root_node(), "type_arg_list")
        .expect("expected a type_arg_list node for `List<pp.Pulley>`");
    let ns = find_node_by_kind(type_args, "namespaced_name").unwrap_or_else(|| {
        panic!(
            "expected a namespaced_name nested inside type_arg_list; got kinds: {:?}",
            collect_kinds(type_args)
        )
    });

    assert_eq!(field_text(ns, "binding", source), "pp");
    assert_eq!(field_text(ns, "name", source), "Pulley");
}

// ── (c) All three `sub_declaration` arms ────────────────────────────────────

/// Instantiation arm: `sub p = pp.Pulley()` binds `structure_name` to a
/// `namespaced_name`.
#[test]
fn sub_instantiation_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub p = pp.Pulley()\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "instantiation-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

/// Specialization arm: `sub h : pp.Pulley` binds `structure_name` to a
/// `namespaced_name`.
#[test]
fn sub_specialization_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub h : pp.Pulley\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "specialization-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

/// Specialization arm WITH a type-argument tail: `sub h : pp.Pulley<T>`.
///
/// THREE-SURFACE PARITY PIN. The specialization arm's `optional(field(
/// 'type_args', …))` slot sits AFTER `structure_name` in that arm, so
/// widening `structure_name` to `namespaced_name` made this form parse —
/// `structure_name` and `type_args` come out as SIBLING fields, not as a
/// `parameterized_type`. `lower_sub` accordingly builds
/// `SubDecl { structure_name: "pp.Pulley", type_args: [T] }` with no
/// diagnostic (pinned by
/// `sub_specialization_with_type_args_lowers_dot_joined_structure_name` in
/// crates/reify-syntax/tests/harness_syntax/namespaced_ref_lowering_tests.rs).
///
/// This is pinned on BOTH grammar surfaces because the GUI lezer grammar spells
/// the `:` arm differently (its type-arg tail rides on `ParameterizedType`
/// rather than on its own slot), so the two can diverge here without any test
/// noticing — and an editor stricter than the compiler is the silent
/// degradation `reifyGrammarQualifiedRef.test.ts` exists to prevent. Note this
/// is NOT the excluded qualified-generic form: `param p : pp.Box<T>` in TYPE
/// position still ERRORs, because `namespaced_name` itself carries no
/// `optional(type_args)` tail (see the `namespaced_name` rule).
#[test]
fn sub_specialization_arm_accepts_namespaced_name_with_type_args() {
    let source = "structure def S {\n    sub h : pp.Pulley<T>\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "structure_name must stay a namespaced_name beside the type_args slot; \
         got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");

    // `field('type_args', seq('<', $.type_arg_list, '>'))` tags every child of
    // the seq, so `child_by_field_name` returns the `<` token; the list itself
    // is found by kind, the same way `list_vs_listicle_lexer_discipline_unchanged`
    // reads it.
    let type_args = find_node_by_kind(sub, "type_arg_list")
        .expect("the specialization arm's own `type_args` slot must still bind");
    assert_eq!(&source[type_args.start_byte()..type_args.end_byte()], "T");
}

/// The excluded companion: a qualified generic in TYPE position stays an error.
///
/// `namespaced_name` deliberately carries NO `optional(type_args)` tail — the
/// `namespaced_name` rule itself records that formulation as the one producing
/// an unresolved LR conflict, and qualified generics are out of scope for μ.
/// Pinned so the `sub h : pp.Pulley<T>` acceptance above cannot be mistaken for
/// a general widening, and so a later deliberate one is a visible change here.
#[test]
fn qualified_generic_in_type_position_still_errors() {
    let source = "structure def S {\n    param p : pp.Box<T>\n}\n";
    let mut parser = make_parser();
    let tree = parser.parse(source, None).expect("parse failed");
    assert!(
        count_errors(tree.root_node()) > 0,
        "`param p : pp.Box<T>` must still ERROR — qualified generics are out of \
         scope for μ; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
}

/// Collection arm: `sub i : List<pp.Pulley>` binds `structure_name` to a
/// `namespaced_name` (the `List` keyword still wins the lexer rule-#2 tie-break).
#[test]
fn sub_collection_arm_accepts_namespaced_structure_name() {
    let source = "structure def S {\n    sub i : List<pp.Pulley>\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "namespaced_name",
        "collection-arm structure_name must be a namespaced_name; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert_eq!(field_text(name, "binding", source), "pp");
    assert_eq!(field_text(name, "name", source), "Pulley");
}

// ── (d) Negative controls — nothing that parses today may change ────────────

/// `sub j = Plain()` keeps a bare `identifier` structure_name.
#[test]
fn bare_structure_name_stays_identifier() {
    let source = "structure def S {\n    sub j = Plain()\n}\n";
    let tree = parse_clean(source);

    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    let name = sub
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    assert_eq!(
        name.kind(),
        "identifier",
        "an unqualified structure_name must stay a bare identifier; got kinds: {:?}",
        collect_kinds(sub)
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_name").is_none(),
        "`Plain()` must not produce a namespaced_name node"
    );
}

/// `param n : Beam::Material` stays a `qualified_type` (the `::` form is
/// untouched by the new dotted arm).
#[test]
fn double_colon_type_stays_qualified_type() {
    let source = "structure def S {\n    param n : Beam::Material\n}\n";
    let tree = parse_clean(source);

    assert!(
        find_node_by_kind(tree.root_node(), "qualified_type").is_some(),
        "`Beam::Material` must stay a qualified_type; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_name").is_none(),
        "`Beam::Material` must not produce a namespaced_name node"
    );
}

/// The `List<Foo>` vs `Listicle<Foo>` lexer rule-#1 / rule-#2 discipline
/// (documented on `sub_declaration`'s specialization arm in
/// tree-sitter-reify/grammar.js) is unchanged by widening `structure_name`.
///
/// `List<Foo>` → collection arm (rule #2: the `'List'` string token beats the
/// equal-length identifier regex), so `structure_name` is `Foo`.
/// `Listicle<Foo>` → specialization arm (rule #1: longest match), so
/// `structure_name` is `Listicle`.
#[test]
fn list_vs_listicle_lexer_discipline_unchanged() {
    let list_src = "structure def S {\n    sub a : List<Foo>\n}\n";
    let tree = parse_clean(list_src);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    assert_eq!(
        field_text(sub, "structure_name", list_src),
        "Foo",
        "`List<Foo>` must take the collection arm (lexer rule #2); got kinds: {:?}",
        collect_kinds(sub)
    );

    let listicle_src = "structure def S {\n    sub b : Listicle<Foo>\n}\n";
    let tree = parse_clean(listicle_src);
    let sub = find_node_by_kind(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node");
    assert_eq!(
        field_text(sub, "structure_name", listicle_src),
        "Listicle",
        "`Listicle<Foo>` must take the specialization arm (lexer rule #1, \
         longest match); got kinds: {:?}",
        collect_kinds(sub)
    );
}

// ── (e) Expression position — the call form `pp.Pulley()` ───────────────────
//
// Step-3 RED: the grammar has NO call-on-dotted-path form at all
// (`function_call`'s callee is a bare `$.identifier`; the only call-after-
// something forms are `ad_hoc_selector` and `trait_method_call`), so
// `pp.Pulley()` produces an ERROR subtree in expression position.
//
// Step-4 GREEN: `namespaced_call: $ => prec(12, seq(field('callee',
// $.member_access), callTail($)))`, added to the `$._expression` choice.

/// `let f = pp.Pulley()` produces a `namespaced_call` whose `callee` field is
/// a `member_access`.
#[test]
fn nullary_qualified_call_yields_namespaced_call() {
    let source = "structure def S {\n    let f = pp.Pulley()\n}\n";
    let tree = parse_clean(source);

    let call = find_node_by_kind(tree.root_node(), "namespaced_call").unwrap_or_else(|| {
        panic!(
            "expected a namespaced_call node for `pp.Pulley()`; got kinds: {:?}",
            collect_kinds(tree.root_node())
        )
    });
    let callee = call
        .child_by_field_name("callee")
        .expect("namespaced_call must have a `callee` field");
    assert_eq!(
        callee.kind(),
        "member_access",
        "the callee must be the SAME node kind as the no-call form, so ν's \
         resolution fixup sees one uniform base; got kinds: {:?}",
        collect_kinds(call)
    );
    assert_eq!(callee.utf8_text(source.as_bytes()).expect("utf8"), "pp.Pulley");
    assert!(
        find_node_by_kind(call, "argument_list").is_none(),
        "a nullary call has no argument_list"
    );
}

/// `let e = pp.compute(1)` — positional argument lands in an `argument_list`.
#[test]
fn positional_qualified_call_has_argument_list() {
    let source = "structure def S {\n    let e = pp.compute(1)\n}\n";
    let tree = parse_clean(source);

    let call = find_node_by_kind(tree.root_node(), "namespaced_call")
        .expect("expected a namespaced_call node for `pp.compute(1)`");
    let args = find_node_by_kind(call, "argument_list").unwrap_or_else(|| {
        panic!(
            "expected an argument_list under namespaced_call; got kinds: {:?}",
            collect_kinds(call)
        )
    });
    assert!(
        find_node_by_kind(args, "number_literal").is_some(),
        "the positional `1` must be an argument_list child; got kinds: {:?}",
        collect_kinds(args)
    );
}

/// `let g = pp.make(a: 1, 2)` — named and positional arguments are both
/// accepted, at parity with the bare `function_call` path (both reuse the
/// shared `callTail($)` helper, so the argument syntax cannot drift).
#[test]
fn mixed_named_and_positional_qualified_call() {
    let source = "structure def S {\n    let g = pp.make(a: 1, 2)\n}\n";
    let tree = parse_clean(source);

    let call = find_node_by_kind(tree.root_node(), "namespaced_call")
        .expect("expected a namespaced_call node for `pp.make(a: 1, 2)`");
    let args = find_node_by_kind(call, "argument_list")
        .expect("expected an argument_list under namespaced_call");
    assert_eq!(
        find_all_nodes_by_kind(args, "named_argument").len(),
        1,
        "expected exactly one named_argument (`a: 1`); got kinds: {:?}",
        collect_kinds(args)
    );
}

// ── (f) Expression-position negative controls ───────────────────────────────

/// `let r = pp.FitClass.Clearance` stays NESTED `member_access` — the
/// enum-access half of NS-Q2 already parsed before μ and deliberately stays on
/// ν's D-9 resolution-fixup path rather than getting its own CST node.
#[test]
fn chained_dotted_access_stays_nested_member_access() {
    let source = "structure def S {\n    let r = pp.FitClass.Clearance\n}\n";
    let tree = parse_clean(source);

    let accesses = find_all_nodes_by_kind(tree.root_node(), "member_access");
    assert_eq!(
        accesses.len(),
        2,
        "`pp.FitClass.Clearance` must stay two nested member_access nodes; \
         got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    // Span-anchored, so "two member_access nodes" cannot be satisfied by two
    // nodes over the WRONG extents: the outer one covers the whole chain and the
    // inner one covers only the first two segments (left-associative nesting).
    assert_eq!(
        kinds_spanning(tree.root_node(), source, "pp.FitClass.Clearance"),
        vec!["member_access".to_string()],
        "the outer member_access must span the whole chain; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert_eq!(
        kinds_spanning(tree.root_node(), source, "pp.FitClass"),
        vec!["member_access".to_string()],
        "the inner member_access must span only `pp.FitClass`; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_call").is_none(),
        "a call-less dotted chain must not produce a namespaced_call node"
    );
}

/// `let s = obj.width` stays a plain `member_access`.
#[test]
fn plain_member_access_unchanged() {
    let source = "structure def S {\n    let s = obj.width\n}\n";
    let tree = parse_clean(source);

    assert_eq!(
        kinds_spanning(tree.root_node(), source, "obj.width"),
        vec!["member_access".to_string()],
        "`obj.width` must be spanned by exactly one member_access and nothing \
         else; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_call").is_none(),
        "`obj.width` must not produce a namespaced_call node"
    );
}

/// `let t = plain(1)` stays a `function_call` — the unqualified call path is
/// untouched.
#[test]
fn unqualified_call_stays_function_call() {
    let source = "structure def S {\n    let t = plain(1)\n}\n";
    let tree = parse_clean(source);

    assert_eq!(
        kinds_spanning(tree.root_node(), source, "plain(1)"),
        vec!["function_call".to_string()],
        "`plain(1)` must be spanned by exactly one function_call and nothing \
         else; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_call").is_none(),
        "`plain(1)` must not produce a namespaced_call node"
    );
}

/// `let u = Foo::bar` stays a `qualified_access` (the `::` value form).
#[test]
fn double_colon_access_stays_qualified_access() {
    let source = "structure def S {\n    let u = Foo::bar\n}\n";
    let tree = parse_clean(source);

    assert_eq!(
        kinds_spanning(tree.root_node(), source, "Foo::bar"),
        vec!["qualified_access".to_string()],
        "`Foo::bar` must be spanned by exactly one qualified_access and nothing \
         else; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_call").is_none(),
        "`Foo::bar` must not produce a namespaced_call node"
    );
}

/// `obj.(Trait::m)(x)` stays a `trait_method_call`.
///
/// A plain SHAPE control, deliberately NOT a precedence one. It used to claim
/// it pinned that "`namespaced_call`'s `prec(12)` must not steal the
/// instance-qualified call form", which it cannot: `obj.(` can never reduce to
/// a `member_access` (that rule requires `'.' identifier`), so this source is
/// shape-disjoint from the precedence question and holds for ANY value of that
/// prec. `prec(12)` is exercised for real by
/// `prec12_shifts_the_paren_but_not_the_at_selector` below.
#[test]
fn instance_qualified_call_stays_trait_method_call() {
    let source = "structure def S {\n    let v = obj.(Trait::m)(x)\n}\n";
    let tree = parse_clean(source);

    assert_eq!(
        kinds_spanning(tree.root_node(), source, "obj.(Trait::m)(x)"),
        vec!["trait_method_call".to_string()],
        "`obj.(Trait::m)(x)` must be spanned by exactly one trait_method_call \
         and nothing else; got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert!(
        find_node_by_kind(tree.root_node(), "namespaced_call").is_none(),
        "`obj.(Trait::m)(x)` must not produce a namespaced_call node"
    );
}

/// The case that GENUINELY exercises `namespaced_call`'s `prec(12)`: the same
/// `a.b` prefix, with the two postfix tails that sit on either side of it.
///
/// `prec(12)` is one level above `member_access`'s `prec.left(11)`, so on
/// `a.b` followed by `(` the parser SHIFTS the `(` into `namespaced_call`
/// instead of reducing to a bare `member_access`. The `@` selector is
/// `prec.left(10)` — BELOW `member_access` — so the same `a.b` prefix followed
/// by `@ sel(1)` must reduce to `member_access` first and leave the selector
/// outside. Lower the `namespaced_call` prec to 11 or below and the first half
/// of this test flips; raise `ad_hoc_selector`'s above 11 and the second half
/// does. Neither is reachable through `obj.(Trait::m)(x)`.
#[test]
fn prec12_shifts_the_paren_but_not_the_at_selector() {
    let call_src = "structure def S {\n    let v = a.b(1)\n}\n";
    let call_tree = parse_clean(call_src);
    assert_eq!(
        kinds_spanning(call_tree.root_node(), call_src, "a.b(1)"),
        vec!["namespaced_call".to_string()],
        "`(` must SHIFT into namespaced_call rather than reduce `a.b` to a bare \
         member_access; got kinds: {:?}",
        collect_kinds(call_tree.root_node())
    );
    assert_eq!(
        kinds_spanning(call_tree.root_node(), call_src, "a.b"),
        vec!["member_access".to_string()],
        "the callee stays a member_access node in its own right; got kinds: {:?}",
        collect_kinds(call_tree.root_node())
    );

    let sel_src = "structure def S {\n    let v = a.b @ sel(1)\n}\n";
    let sel_tree = parse_clean(sel_src);
    assert_eq!(
        kinds_spanning(sel_tree.root_node(), sel_src, "a.b @ sel(1)"),
        vec!["ad_hoc_selector".to_string()],
        "`@` is prec 10, BELOW member_access — `a.b` must reduce first and the \
         selector stay outside; got kinds: {:?}",
        collect_kinds(sel_tree.root_node())
    );
    assert!(
        find_node_by_kind(sel_tree.root_node(), "namespaced_call").is_none(),
        "`a.b @ sel(1)` must not produce a namespaced_call node; got kinds: {:?}",
        collect_kinds(sel_tree.root_node())
    );
}

/// A postfix chain whose head is not a bare identifier still reaches
/// `namespaced_call` — `xs[0].f(1)` — because the rule's callee is a full
/// `member_access` whose `object` is a full `_expression`. Grammar-level
/// ACCEPTANCE only: lowering rejects it (the callee is not a simple
/// `binding.Name`), pinned in
/// crates/reify-syntax/tests/harness_syntax/namespaced_ref_lowering_tests.rs.
#[test]
fn indexed_postfix_chain_still_reaches_namespaced_call() {
    let source = "structure def S {\n    let v = xs[0].f(1)\n}\n";
    let tree = parse_clean(source);

    assert_eq!(
        kinds_spanning(tree.root_node(), source, "xs[0].f(1)"),
        vec!["namespaced_call".to_string()],
        "`xs[0].f(1)` must reduce to a namespaced_call at the grammar level; \
         got kinds: {:?}",
        collect_kinds(tree.root_node())
    );
    assert_eq!(
        field_text(
            find_node_by_kind(tree.root_node(), "namespaced_call")
                .expect("expected a namespaced_call node"),
            "callee",
            source,
        ),
        "xs[0].f",
        "the callee is the whole member_access, index_access object included"
    );
}

/// A plain (unqualified) type annotation is unaffected: `param s : Steel`
/// still has a bare `identifier` under its `type` field.
#[test]
fn bare_type_annotation_unchanged() {
    let source = "structure def S {\n    param s : Steel\n}\n";
    let tree = parse_clean(source);

    let param = find_node_by_kind(tree.root_node(), "param_declaration")
        .expect("expected a param_declaration node");
    let type_node = param
        .child_by_field_name("type")
        .expect("param_declaration must have a `type` field");
    assert!(
        find_node_by_kind(type_node, "identifier").is_some(),
        "`Steel` must stay a bare identifier; got kinds: {:?}",
        collect_kinds(type_node)
    );
    assert!(
        find_all_nodes_by_kind(type_node, "namespaced_name").is_empty(),
        "`Steel` must not produce a namespaced_name node"
    );
}
