//! Parser-level tests for `sub name : StructName { body }` specialization-scope body (task 3569).
//!
//! User-observable signal: `cargo test -p reify-syntax -- sub_decl_specialization_body_parses_from_source`
//! Leaf signal from phase-3-grammar-fiction-triage-log.md §B3.
//!
//! These tests verify that the tree-sitter grammar admits the new specialization-body
//! production and produces a well-formed CST (zero ERROR nodes). Both permitted and
//! forbidden body member kinds must parse — rejection of forbidden kinds is deferred
//! to the downstream validator (task 3571/3573).
//! AST-shape assertions (lowering to `SubDecl.body: Some(...)`) are deferred to
//! sibling task 3571.

use reify_types::ModulePath;

mod common;
use common::{find_cst_node, make_ts_parser};

// ── High-level parse tests (user-observable signal) ─────────────────────────

/// User-signal test: all forms of specialization-body sub declarations parse
/// without errors. Covers permitted-only body, where-guarded param_assignment,
/// where-guard-before-body, and forbidden-but-must-parse bodies.
///
/// `!tree.root_node().has_error()` is the primary signal — it confirms the new
/// grammar production is reachable from `_member` with no CST ERROR nodes.
///
/// `module.errors.is_empty()` is a regression guard for the *surrounding*
/// source (the `structure S { ... }` wrapper): it will fire if an unrelated
/// change breaks the surrounding structure parse, but carries no direct signal
/// about the `specialization_body` construct itself, because lowering for that
/// construct stays `body: None` until sibling task 3571 wires CST→AST.
#[test]
fn sub_decl_specialization_body_parses_from_source() {
    let sources: &[(&str, &str)] = &[
        (
            "permitted-only body (param_assignment + let + constraint + connect)",
            "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm  let m = shaft_diameter * 2  constraint shaft_diameter > 1mm  connect a -> b } }",
        ),
        (
            "where-guarded parameter assignment",
            "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm where high_torque } }",
        ),
        (
            "where-guard on sub before body",
            "structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }",
        ),
        (
            "forbidden body — param member (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { param x : Length } }",
        ),
        (
            "forbidden body — port member (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { port p : MechanicalPort } }",
        ),
        (
            "forbidden body — nested bodyless sub (must parse, rejection deferred to validator)",
            "structure S { sub motor : ElectricMotor { sub child : Foo } }",
        ),
        (
            "bare colon, no body — new grammar branch, no body child expected",
            "structure S { sub a : Foo }",
        ),
        (
            "type_args + guard + body — exercises the full optional chain together",
            "structure S { sub m : Gear<Steel> where n > 0 { ratio = 2 } }",
        ),
    ];

    for (label, source) in sources {
        // Regression guard: asserts the surrounding `structure S { ... }` source
        // still parses cleanly. Does NOT verify specialization_body lowering —
        // lower_sub returns body: None until sibling task 3571 wires CST→AST.
        let module = reify_syntax::parse(source, ModulePath::single("test"));
        assert!(
            module.errors.is_empty(),
            "expected zero parse errors for {label:?} form, got: {:?}",
            module.errors,
        );

        // CST-level check: no ERROR nodes in the parse tree.
        let mut parser = make_ts_parser();
        let tree = parser
            .parse(source.as_bytes(), None)
            .expect("tree-sitter parse failed");
        assert!(
            !tree.root_node().has_error(),
            "expected no CST ERROR nodes for {label:?} form; \
             has_error() returned true for source: {source:?}",
        );
    }
}

// ── CST-shape assertions ─────────────────────────────────────────────────────

/// A `sub motor : ElectricMotor { shaft_diameter = 8mm }` must produce a
/// `sub_declaration` whose `structure_name` field text is `"ElectricMotor"` and
/// which has a `body` child of kind `specialization_body`.
#[test]
fn sub_decl_specialization_body_cst_has_specialization_body_node() {
    let source = "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    let structure_name = sub_decl
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    let structure_name_text = structure_name
        .utf8_text(source.as_bytes())
        .expect("structure_name node must be valid utf8");
    assert_eq!(
        structure_name_text, "ElectricMotor",
        "structure_name field text must be 'ElectricMotor', got: {structure_name_text:?}",
    );

    let body = sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a `body` field for specialization form");
    assert_eq!(
        body.kind(),
        "specialization_body",
        "body child must be of kind 'specialization_body', got: {:?}",
        body.kind(),
    );
}

/// The `specialization_body` of `{ shaft_diameter = 8mm }` must contain a
/// `param_assignment` node whose `name` field text is `"shaft_diameter"`.
#[test]
fn sub_decl_specialization_body_cst_param_assignment_name_field() {
    let source = "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let param_assign = find_cst_node(tree.root_node(), "param_assignment")
        .expect("expected a param_assignment node in the CST");

    let name = param_assign
        .child_by_field_name("name")
        .expect("param_assignment must have a `name` field");
    let name_text = name
        .utf8_text(source.as_bytes())
        .expect("name node must be valid utf8");
    assert_eq!(
        name_text, "shaft_diameter",
        "param_assignment name field text must be 'shaft_diameter', got: {name_text:?}",
    );
}

/// A forbidden body `{ param x : Length }` still yields ZERO CST ERROR nodes —
/// forbidden kinds parse; rejection is the validator's job.
#[test]
fn sub_decl_specialization_body_forbidden_param_body_parses_without_errors() {
    let source = "structure S { sub motor : ElectricMotor { param x : Length } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "a forbidden param member inside a specialization body must NOT produce CST ERROR \
         nodes — rejection is deferred to the validator; has_error() returned true",
    );
}

/// The where-guard-before-body form must expose both a `guard` (where_clause)
/// child and a `body` child on the same `sub_declaration`.
#[test]
fn sub_decl_specialization_body_cst_has_guard_and_body() {
    let source = "structure S { sub left : TreeBracket where depth > 0 { depth = depth - 1 } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    sub_decl
        .child_by_field_name("guard")
        .expect("sub_declaration must have a `guard` (where_clause) field for where-guard-before-body form");

    sub_decl
        .child_by_field_name("body")
        .expect("sub_declaration must have a `body` field for where-guard-before-body form");
}

/// Regression lock: the `where high_torque` clause inside a specialization body
/// must attach to the inner `param_assignment` as its `guard` field, NOT be
/// absorbed by the enclosing `sub_declaration`'s own optional `guard` field.
///
/// The `where_clause` grammar rule is shared across `param_assignment`,
/// `sub_declaration` (specialization form), `param`, `let`, `constraint`,
/// `minimize`, `maximize`, and `port`. The grammar also declares GLR conflicts
/// for `[$.sub_declaration]` and `[$.param_assignment]`. A future grammar
/// reshuffle could silently re-route this attachment with zero CST ERROR nodes,
/// so a pass-only test on `!has_error()` alone would not catch it. This test
/// fully constrains the attachment by asserting both halves:
///
/// * POSITIVE: `param_assignment.guard` is a `where_clause` whose `condition`
///   text is `"high_torque"`.
/// * NEGATIVE: the enclosing `sub_declaration` has NO `guard` field (the `where`
///   is NOT absorbed by the sub).
#[test]
fn sub_decl_specialization_body_cst_where_guard_binds_to_param_assignment_not_sub() {
    let source = "structure S { sub motor : ElectricMotor { shaft_diameter = 8mm where high_torque } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");

    // Defensive pre-check: a future parse regression yields a clear message
    // instead of an opaque Option::expect panic during child navigation.
    assert!(
        !tree.root_node().has_error(),
        "expected no CST ERROR nodes; has_error() returned true for source: {source:?}",
    );

    // POSITIVE half: the where-guard must be bound to param_assignment.
    let pa = find_cst_node(tree.root_node(), "param_assignment")
        .expect("expected a param_assignment node in the CST");

    let guard = pa
        .child_by_field_name("guard")
        .expect("param_assignment must carry the where-guard as its `guard` field");
    assert_eq!(
        guard.kind(),
        "where_clause",
        "param_assignment.guard must be a where_clause node, got: {:?}",
        guard.kind(),
    );

    let cond = guard
        .child_by_field_name("condition")
        .expect("where_clause must have a `condition` field");
    let cond_text = cond
        .utf8_text(source.as_bytes())
        .expect("condition node must be valid utf8");
    assert_eq!(
        cond_text, "high_torque",
        "where_clause condition text must be 'high_torque', got: {cond_text:?}",
    );

    // NEGATIVE half: the enclosing sub_declaration must NOT absorb the inner where-guard.
    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");
    assert!(
        sub_decl.child_by_field_name("guard").is_none(),
        "enclosing sub_declaration must NOT absorb the inner where-guard as its own `guard` field",
    );
}

/// `structure S { sub a : Foo }` (bare colon, no body) must parse without ERROR
/// nodes and produce a `sub_declaration` with a `structure_name` field but NO
/// `body` child — this is the new grammar branch that previously would have been
/// a parse error.
#[test]
fn sub_decl_bare_colon_no_body_cst_shape() {
    let source = "structure S { sub a : Foo }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "bare colon no-body form must parse without CST ERROR nodes",
    );

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in the CST");

    let structure_name = sub_decl
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field");
    let text = structure_name
        .utf8_text(source.as_bytes())
        .expect("utf8");
    assert_eq!(text, "Foo", "structure_name must be 'Foo', got: {text:?}");

    assert!(
        sub_decl.child_by_field_name("body").is_none(),
        "bare colon no-body form must NOT have a `body` field",
    );
}

// ── Negative grammar tests ────────────────────────────────────────────────────

/// A malformed body `structure S { sub s : Foo { = 3mm } }` (assignment missing
/// its name) must cause the parser to emit a CST ERROR node.
#[test]
fn sub_decl_specialization_body_rejects_malformed_assignment() {
    let source = "structure S { sub s : Foo { = 3mm } }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "expected a CST ERROR node when a body assignment is missing its name; \
         has_error() returned false for source: {source:?}",
    );
}

// ── D4 collection / instantiation non-regression guard ───────────────────────

/// Regression: `structure S { sub a = Foo() }` still lowers to a `MemberDecl::Sub`
/// with `is_collection == false`, `body.is_none()`, and `structure_name == "Foo"`.
#[test]
fn sub_decl_instantiation_form_regression() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a = Foo() }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration");
    };
    let MemberDecl::Sub(sub) = s.members.iter().find(|m| matches!(m, MemberDecl::Sub(_))).expect("expected a Sub member") else {
        unreachable!()
    };
    assert!(!sub.is_collection, "instantiation form must have is_collection == false");
    assert!(sub.body.is_none(), "instantiation form must have body == None");
    assert_eq!(sub.structure_name, "Foo", "structure_name must be 'Foo'");
}

/// Regression: `structure S { sub a : List<Foo> }` still lowers with
/// `is_collection == true`, `body.is_none()`, `structure_name == "Foo"`.
#[test]
fn sub_decl_collection_form_regression() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a : List<Foo> }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration");
    };
    let MemberDecl::Sub(sub) = s.members.iter().find(|m| matches!(m, MemberDecl::Sub(_))).expect("expected a Sub member") else {
        unreachable!()
    };
    assert!(sub.is_collection, "collection form must have is_collection == true");
    assert!(sub.body.is_none(), "collection form must have body == None");
    assert_eq!(sub.structure_name, "Foo", "structure_name must be 'Foo'");
}

// ── D4 List/specialization explicit-precedence invariant lock ─────────────────
//
// The collection arm in grammar.js uses a bare `'List'` string token.
// Disambiguation between the collection arm (`sub a : List<Foo>`) and the
// specialization arm (`sub a : Foo<Bar>`) relies on two documented tree-sitter
// lexer rules — NOT on `token(prec(...))` (which was considered and rejected
// because it overrides rule #1 and breaks the longest-match case; see
// grammar.js lines 523–528 and escalation esc-3712-201):
//
//   Rule #1 (longest-match, evaluated FIRST): for `Listicle<Foo>` the
//   `$.identifier` regex matches 8 chars vs `'List'`'s 4 — identifier wins.
//
//   Rule #2 (string-vs-regex tie-break, on equal-length matches): for
//   `List<Foo>` both `'List'` and `$.identifier` match exactly 4 chars, so
//   the anonymous string token wins and the collection arm is taken.
//
// The `List<Foo>` AST baseline (rule #2 positive case) is already pinned by
// `sub_decl_collection_form_regression` above. The three tests below lock the
// complementary invariants independently so failures isolate to a single rule:
//
//   - sub_decl_non_list_specialization_arm: negative control — non-List
//     identifiers must NOT be captured by the collection arm (rule #2 boundary).
//   - sub_decl_listicle_longest_match: longest-match guard — a List-prefixed
//     identifier must win via rule #1 before any tie-break is reached.
//   - sub_decl_cst_shape_for_list_collection: CST-level pin — confirms `List`
//     is consumed as the collection keyword, not as `structure_name`.
//
// RED-BAR PROOF (not committed): an earlier draft verified RED-bar by reordering
// the specialization arm above the collection arm in grammar.js; assertions on
// `List<Foo>` failed, proving this is a real regression lock. Revert was
// confirmed before commit.

/// Non-List specialization: `sub a : Foo<Bar>` must route to the specialization
/// arm (`is_collection==false`) — the bare `'List'` token must NOT capture
/// non-List identifiers (they don't equal-length-match `'List'`).
///
/// Note: this is distinct from `sub_decl_instantiation_form_regression`, which
/// tests `sub a = Foo()` (the `=` / call-syntax instantiation form). This test
/// exercises `sub a : Ident<…>` (the colon / type-args specialization form).
#[test]
fn sub_decl_non_list_specialization_arm() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a : Foo<Bar> }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration for Foo<Bar> source");
    };
    let MemberDecl::Sub(sub) = s
        .members
        .iter()
        .find(|m| matches!(m, MemberDecl::Sub(_)))
        .expect("expected a Sub member")
    else {
        unreachable!()
    };
    assert!(
        !sub.is_collection,
        "INVARIANT BROKEN: `sub a : Foo<Bar>` must parse via the specialization arm \
         (is_collection==false). The bare `'List'` token must NOT capture non-List \
         identifiers — they don't equal-length-match `List`.",
    );
    assert_eq!(
        sub.structure_name, "Foo",
        "specialization form `sub a : Foo<Bar>` structure_name must be 'Foo', got: {:?}",
        sub.structure_name,
    );
}

/// Longest-match guard: `sub a : Listicle<Foo>` (List-prefixed identifier) must
/// route to the specialization arm — rule #1 (longest-match) takes 'Listicle'
/// (8 chars) over 'List' (4 chars) before any tie-break kicks in.
#[test]
fn sub_decl_listicle_longest_match() {
    use reify_syntax::{Declaration, MemberDecl};
    let source = "structure S { sub a : Listicle<Foo> }";
    let module = reify_syntax::parse(source, ModulePath::single("test"));
    let Declaration::Structure(s) = &module.declarations[0] else {
        panic!("expected Structure declaration for Listicle<Foo> source");
    };
    let MemberDecl::Sub(sub) = s
        .members
        .iter()
        .find(|m| matches!(m, MemberDecl::Sub(_)))
        .expect("expected a Sub member")
    else {
        unreachable!()
    };
    assert!(
        !sub.is_collection,
        "LONGEST-MATCH REGRESSION: `sub a : Listicle<Foo>` must parse via the \
         specialization arm (is_collection==false). Rule #1 (longest-match) must \
         take 'Listicle' (8 chars) over 'List' (4 chars) before any tie-break \
         kicks in.",
    );
    assert_eq!(
        sub.structure_name, "Listicle",
        "specialization form `sub a : Listicle<Foo>` structure_name must be 'Listicle', \
         got: {:?}",
        sub.structure_name,
    );
}

/// CST shape: in the `List<Foo>` parse, `sub_declaration`'s `structure_name`
/// field text is "Foo" (not "List") and there is no `body` child.
///
/// This CST-level pin is complementary to the AST-level `sub_decl_collection_form_regression`:
/// it confirms that `List` is consumed as the collection keyword rather than being
/// lexed as the `structure_name` identifier in the parse tree.
#[test]
fn sub_decl_cst_shape_for_list_collection() {
    let source = "structure S { sub a : List<Foo> }";
    let mut parser = make_ts_parser();
    let tree = parser
        .parse(source.as_bytes(), None)
        .expect("tree-sitter parse failed for List<Foo>");
    assert!(
        !tree.root_node().has_error(),
        "CST must have no ERROR nodes for `sub a : List<Foo>`",
    );

    let sub_decl = find_cst_node(tree.root_node(), "sub_declaration")
        .expect("expected a sub_declaration node in CST for List<Foo>");

    let structure_name_node = sub_decl
        .child_by_field_name("structure_name")
        .expect("sub_declaration must have a `structure_name` field for List<Foo>");
    let structure_name_text = structure_name_node
        .utf8_text(source.as_bytes())
        .expect("structure_name node must be valid utf8");
    assert_eq!(
        structure_name_text, "Foo",
        "INVARIANT BROKEN: CST `structure_name` field must be 'Foo' (not 'List') for \
         `sub a : List<Foo>`. 'List' must be consumed as the collection keyword, not \
         the structure name. Got: {structure_name_text:?}",
    );

    assert!(
        sub_decl.child_by_field_name("body").is_none(),
        "CST: `sub a : List<Foo>` (collection form) must NOT have a `body` field",
    );
}
