//! Rust integration tests for task 3802: `auto` keyword reservation + shared
//! `_binding_value` grammar rule at five binding sites.
//!
//! User-observable signal: `cargo test -p reify-syntax --test auto_binding_sites_grammar_tests`
//! passes.
//!
//! Coverage:
//! * **(A)** Positive CST shape — strict `auto` at each of the 5 binding sites.
//! * **(B)** Positive CST shape — `auto(free)` at each of the 5 binding sites.
//! * **(C)** Operand-position rejection — 6 positions that must produce ERROR nodes.
//! * **(D)** `auto_type_arg` non-regression — reservation must not break type-arg form.
//! * **(E)** Reserved-identifier non-regression — scanner must not over-reserve
//!   `source`, `frame`, `direction`, `in` (used as field/param/unit names in stdlib).

mod common;
use common::{find_cst_node, make_ts_parser};

// ── Helper ───────────────────────────────────────────────────────────────────

/// Find the first ERROR node and return (start_byte, end_byte).
/// Returns None if no ERROR node exists.
fn first_error_span(root: tree_sitter::Node<'_>) -> Option<(u32, u32)> {
    find_cst_node(root, "ERROR").map(|n| (n.start_byte() as u32, n.end_byte() as u32))
}

// ── (A) Positive CST shape — strict `auto` ───────────────────────────────────

/// `param_declaration.default = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.  Also pins the step-2 refactor (param_declaration
/// switched from an inline choice to `_binding_value`).
#[test]
fn param_declaration_auto_strict_produces_auto_keyword() {
    let source = "structure S { param x : Scalar = auto }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "structure S {{ param x : Scalar = auto }} must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword; \
         found: {:?}",
        kw.child_by_field_name("modifier").map(|n| n.kind()),
    );
}

/// `let_declaration.value = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.
#[test]
fn let_value_auto_strict_produces_auto_keyword_with_no_modifier_field() {
    let source = "structure S { let m : Length = auto }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "structure S {{ let m : Length = auto }} must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword"
    );
}

/// `param_assignment.value = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.
#[test]
fn param_assignment_auto_strict_produces_auto_keyword() {
    let source = "structure S { sub b : Bearing { bore = auto } }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "param_assignment with auto must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword"
    );
}

/// `named_argument.value = auto` (structure-construction syntax) — strict form must
/// produce an `auto_keyword` node with NO `modifier` field.
#[test]
fn named_argument_auto_strict_produces_auto_keyword() {
    let source = "structure S { sub b = Bearing(bore: auto) }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "named_argument with auto must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword"
    );
}

/// `connect_param_assignment.value = auto` — strict form must produce an
/// `auto_keyword` node with NO `modifier` field.
#[test]
fn connect_param_assignment_auto_strict_produces_auto_keyword() {
    let source = "structure S { connect a -> b { gain = auto } }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "connect_param_assignment with auto must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword"
    );
}

// ── (B) Positive CST shape — `auto(free)` ────────────────────────────────────

/// `param_declaration.default = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn param_declaration_auto_free_has_modifier_field() {
    let source = "structure S { param x : Scalar = auto(free) }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "auto(free) in param default must parse without errors");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(
        modifier.utf8_text(source.as_bytes()).unwrap(),
        "free",
        "modifier field text must be 'free'"
    );
}

/// `let_declaration.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn let_value_auto_free_has_modifier_field() {
    let source = "structure S { let m : Length = auto(free) }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "auto(free) in let value must parse without errors");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(modifier.utf8_text(source.as_bytes()).unwrap(), "free");
}

/// `param_assignment.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn param_assignment_auto_free_has_modifier_field() {
    let source = "structure S { sub b : Bearing { bore = auto(free) } }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "auto(free) in param_assignment must parse without errors");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(modifier.utf8_text(source.as_bytes()).unwrap(), "free");
}

/// `named_argument.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn named_argument_auto_free_has_modifier_field() {
    let source = "structure S { sub b = Bearing(bore: auto(free)) }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "auto(free) in named_argument must parse without errors");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(modifier.utf8_text(source.as_bytes()).unwrap(), "free");
}

/// `connect_param_assignment.value = auto(free)` — must produce an `auto_keyword`
/// node whose `modifier` field text is `"free"`.
#[test]
fn connect_param_assignment_auto_free_has_modifier_field() {
    let source = "structure S { connect a -> b { gain = auto(free) } }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "auto(free) in connect_param_assignment must parse without errors"
    );
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(modifier.utf8_text(source.as_bytes()).unwrap(), "free");
}

// ── (C) Operand-position rejection ───────────────────────────────────────────
//
// The scanner emits AUTO_TOKEN unconditionally.  At operand positions where
// `auto_keyword` is not valid, the parser produces an ERROR node.  Each test below:
//   1. Asserts has_error() == true.
//   2. Pins that at least one ERROR node's span overlaps the `auto` token's bytes.
//
// Span-overlap check avoids hard-coded byte offsets by using str::find("auto");
// this is robust to whitespace changes in the fixture.

/// `auto` in an arithmetic operand position must produce a CST ERROR node.
#[test]
fn arithmetic_operand_auto_is_error() {
    let source = "structure S { let x : Length = auto + 2mm }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in arithmetic operand position must produce a parse error"
    );
    // Pin that an ERROR node overlaps the `auto` token.
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

/// `auto` in a positional function-call argument position must produce a CST ERROR node.
#[test]
fn positional_fn_call_arg_auto_is_error() {
    let source = "structure S { let x : Length = clamp(auto) }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in positional fn-call arg position must produce a parse error"
    );
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

/// `auto` in a constraint body position must produce a CST ERROR node.
#[test]
fn constraint_body_auto_is_error() {
    let source = "structure S { param x : Length  constraint auto }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in constraint body position must produce a parse error"
    );
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

/// `auto` in a minimize body position must produce a CST ERROR node.
#[test]
fn minimize_body_auto_is_error() {
    let source = "structure S { param x : Length  minimize auto }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in minimize body position must produce a parse error"
    );
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

/// `auto` in a list literal position must produce a CST ERROR node.
#[test]
fn list_literal_auto_is_error() {
    let source = "structure S { let xs = [auto] }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in list literal position must produce a parse error"
    );
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

/// `auto` in a field source expression position must produce a CST ERROR node.
#[test]
fn field_source_expr_auto_is_error() {
    let source = "field def F : T -> U { source = analytical { auto } }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "auto in field source expr position must produce a parse error"
    );
    let auto_start = source.find("auto").expect("fixture must contain 'auto'") as u32;
    let auto_end = auto_start + 4;
    let (err_start, err_end) = first_error_span(tree.root_node())
        .expect("has_error() is true so at least one ERROR node must exist");
    assert!(
        err_start < auto_end && err_end > auto_start,
        "expected an ERROR node overlapping `auto` (bytes {auto_start}..{auto_end}), \
         got ERROR at {err_start}..{err_end}"
    );
}

// ── (D) auto_type_arg non-regression ─────────────────────────────────────────

/// The keyword reservation must NOT break `auto_type_arg` parsing.
/// `fn f() -> Bearing<auto: Seal> { 0 }` must produce an `auto_type_arg` node
/// with no errors.  Pins PRD §8.1 row 5.
#[test]
fn auto_type_arg_still_parses_after_keyword_reservation() {
    let source = "fn f() -> Bearing<auto: Seal> { 0 }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "auto_type_arg must still parse cleanly after keyword reservation; got errors"
    );
    assert!(
        find_cst_node(tree.root_node(), "auto_type_arg").is_some(),
        "expected an auto_type_arg node in the CST for `Bearing<auto: Seal>`"
    );
}

// ── (E) Reserved-identifier non-regression ───────────────────────────────────
//
// The external scanner is surgical to `auto` only.  These tests pin that the
// scanner does NOT over-reserve `source`, `frame`, `direction`, `in` which are
// used as param names, named-arg labels, and unit names in the stdlib.

/// `source: "matweb"` as a named argument label must NOT produce a parse error.
/// Mirrors materials_fea.ri:140 and fdm.ri:171 patterns.
#[test]
fn stdlib_source_named_arg_label_still_parses() {
    let source = r#"structure S { let x : T = Foo(source: "matweb") }"#;
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "source as named-arg label must not be reserved by the scanner; \
         got a parse error in: {source:?}"
    );
}

/// `param source : String` as a parameter name must NOT produce a parse error.
/// Mirrors materials_fea.ri:59 and io.ri:66 patterns.
#[test]
fn stdlib_source_as_param_name_still_parses() {
    let source = "structure S { param source : String }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "source as param name must not be reserved by the scanner; \
         got a parse error in: {source:?}"
    );
}

/// `param frame : Real` as a parameter name must NOT produce a parse error.
/// Mirrors solver_elastic.ri:308 pattern.
#[test]
fn stdlib_frame_as_param_name_still_parses() {
    let source = "structure S { param frame : Real }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "frame as param name must not be reserved by the scanner; \
         got a parse error in: {source:?}"
    );
}

/// `param direction : Real` as a parameter name must NOT produce a parse error.
/// Mirrors tolerancing.ri:184 pattern.
#[test]
fn stdlib_direction_as_param_name_still_parses() {
    let source = "structure S { param direction : Real }";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "direction as param name must not be reserved by the scanner; \
         got a parse error in: {source:?}"
    );
}

/// `pub unit in : Length = 0.0254` as a unit declaration must NOT produce a parse error.
/// Mirrors units.ri:16 pattern.
#[test]
fn stdlib_in_as_unit_name_still_parses() {
    let source = "pub unit in : Length = 0.0254";
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        !tree.root_node().has_error(),
        "`pub unit in : Length = 0.0254` must not produce a parse error; \
         `in` must not be reserved by the scanner"
    );
}
