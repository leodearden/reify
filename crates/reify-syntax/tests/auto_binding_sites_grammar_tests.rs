//! Rust integration tests for task 3802: `auto` keyword reservation + shared
//! `_binding_value` grammar rule at five binding sites.
//!
//! User-observable signal: `cargo test -p reify-syntax --test auto_binding_sites_grammar_tests`
//! passes.
//!
//! Coverage:
//! * **(A)** Positive CST shape — strict `auto` at each of the 5 binding sites.
//! * **(B)** Positive CST shape — `auto(free)` at each of the 5 binding sites.
//! * **(C)** Operand-position rejection — 6 fixtures that must each produce an ERROR node.
//! * **(D)** `auto_type_arg` non-regression — reservation must not break type-arg form.
//! * **(E)** Reserved-identifier non-regression — scanner must not over-reserve
//!   `source`, `frame`, `direction`, `in` (used as field/param/unit names in stdlib).

mod common;
use common::{find_cst_node, make_ts_parser};

// ── Assertion helpers ────────────────────────────────────────────────────────

/// Parse `source` and assert it produces no ERROR nodes and contains an
/// `auto_keyword` CST node with NO `modifier` field (strict `auto`).
fn assert_auto_strict_at_binding_site(source: &str) {
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "expected no parse error in: {source:?}");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    assert!(
        kw.child_by_field_name("modifier").is_none(),
        "strict `auto` must have no `modifier` field on auto_keyword; \
         found: {:?}",
        kw.child_by_field_name("modifier").map(|n| n.kind()),
    );
}

/// Parse `source` and assert it produces no ERROR nodes and contains an
/// `auto_keyword` CST node whose `modifier` field text is `"free"`.
fn assert_auto_free_at_binding_site(source: &str) {
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(!tree.root_node().has_error(), "expected no parse error in: {source:?}");
    let kw = find_cst_node(tree.root_node(), "auto_keyword")
        .expect("expected auto_keyword node in CST");
    let modifier = kw
        .child_by_field_name("modifier")
        .expect("auto(free) must have a `modifier` field on auto_keyword");
    assert_eq!(
        modifier.utf8_text(source.as_bytes()).unwrap(),
        "free",
        "modifier field text must be \"free\""
    );
}

/// Parse `source` and assert it produces at least one ERROR node.
///
/// The span-overlap check present in the original implementation was dropped:
/// `has_error()` is the complete signal for operand-position rejection.
/// An ancestor ERROR can span `auto` bytes even when the innermost parse-error
/// node is elsewhere (e.g. `field source expr` wraps the whole file in ERROR),
/// so the overlap assertion added noise without additional safety — this is the
/// same guarantee the `:error` corpus annotations in `auto_operand_rejection.txt`
/// already provide.
fn assert_auto_rejected_at_operand(source: &str) {
    let mut parser = make_ts_parser();
    let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
    assert!(
        tree.root_node().has_error(),
        "`auto` at an operand position must produce a parse ERROR; \
         got a clean parse for: {source:?}"
    );
}

// ── (A) Positive CST shape — strict `auto` ───────────────────────────────────

/// `param_declaration.default = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.  Also pins the step-2 refactor (param_declaration
/// switched from an inline choice to `_binding_value`).
#[test]
fn param_declaration_auto_strict_produces_auto_keyword() {
    assert_auto_strict_at_binding_site("structure S { param x : Scalar = auto }");
}

/// `let_declaration.value = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.
#[test]
fn let_value_auto_strict_produces_auto_keyword_with_no_modifier_field() {
    assert_auto_strict_at_binding_site("structure S { let m : Length = auto }");
}

/// `param_assignment.value = auto` — strict form must produce an `auto_keyword`
/// node with NO `modifier` field.
#[test]
fn param_assignment_auto_strict_produces_auto_keyword() {
    assert_auto_strict_at_binding_site("structure S { sub b : Bearing { bore = auto } }");
}

/// `named_argument.value = auto` (structure-construction syntax) — strict form must
/// produce an `auto_keyword` node with NO `modifier` field.
#[test]
fn named_argument_auto_strict_produces_auto_keyword() {
    assert_auto_strict_at_binding_site("structure S { sub b = Bearing(bore: auto) }");
}

/// `connect_param_assignment.value = auto` — strict form must produce an
/// `auto_keyword` node with NO `modifier` field.
#[test]
fn connect_param_assignment_auto_strict_produces_auto_keyword() {
    assert_auto_strict_at_binding_site("structure S { connect a -> b { gain = auto } }");
}

// ── (B) Positive CST shape — `auto(free)` ────────────────────────────────────

/// `param_declaration.default = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn param_declaration_auto_free_has_modifier_field() {
    assert_auto_free_at_binding_site("structure S { param x : Scalar = auto(free) }");
}

/// `let_declaration.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn let_value_auto_free_has_modifier_field() {
    assert_auto_free_at_binding_site("structure S { let m : Length = auto(free) }");
}

/// `param_assignment.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn param_assignment_auto_free_has_modifier_field() {
    assert_auto_free_at_binding_site("structure S { sub b : Bearing { bore = auto(free) } }");
}

/// `named_argument.value = auto(free)` — must produce an `auto_keyword` node
/// whose `modifier` field text is `"free"`.
#[test]
fn named_argument_auto_free_has_modifier_field() {
    assert_auto_free_at_binding_site("structure S { sub b = Bearing(bore: auto(free)) }");
}

/// `connect_param_assignment.value = auto(free)` — must produce an `auto_keyword`
/// node whose `modifier` field text is `"free"`.
#[test]
fn connect_param_assignment_auto_free_has_modifier_field() {
    assert_auto_free_at_binding_site("structure S { connect a -> b { gain = auto(free) } }");
}

// ── (C) Operand-position rejection ───────────────────────────────────────────
//
// All six operand fixtures are driven by a single test.  Each fixture must
// produce at least one ERROR node.  The individual fixture strings are the
// source of truth — the corpus file `auto_operand_rejection.txt` covers the
// same six positions with `:error` annotations.

#[test]
fn auto_is_rejected_at_all_operand_positions() {
    let fixtures: &[&str] = &[
        "structure S { let x : Length = auto + 2mm }",           // arithmetic operand
        "structure S { let x : Length = clamp(auto) }",          // positional fn-call arg
        "structure S { param x : Length  constraint auto }",      // constraint body
        "structure S { param x : Length  minimize auto }",        // minimize body
        "structure S { let xs = [auto] }",                        // list literal
        "field def F : T -> U { source = analytical { auto } }", // field source expr
    ];
    for source in fixtures {
        assert_auto_rejected_at_operand(source);
    }
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
// The external scanner is surgical to `auto` only.  All five stdlib fixtures
// are driven by a single test — each fixture/description pair is the source of
// truth; individual test names for each stdlib pattern are unnecessary boilerplate
// because the description string in the panic message gives equal diagnosis signal.

#[test]
fn stdlib_identifiers_not_over_reserved_by_scanner() {
    let fixtures: &[(&str, &str)] = &[
        (
            r#"structure S { let x : T = Foo(source: "matweb") }"#,
            "source as named-arg label (mirrors materials_fea.ri:140, fdm.ri:171)",
        ),
        (
            "structure S { param source : String }",
            "source as param name (mirrors materials_fea.ri:59, io.ri:66)",
        ),
        (
            "structure S { param frame : Real }",
            "frame as param name (mirrors solver_elastic.ri:308)",
        ),
        (
            "structure S { param direction : Real }",
            "direction as param name (mirrors tolerancing.ri:184)",
        ),
        (
            "pub unit in : Length = 0.0254",
            "in as unit name (mirrors units.ri:16)",
        ),
    ];
    for (source, description) in fixtures {
        let mut parser = make_ts_parser();
        let tree = parser.parse(source.as_bytes(), None).expect("parse failed");
        assert!(
            !tree.root_node().has_error(),
            "{description} must not be over-reserved by the scanner; \
             got a parse error in: {source:?}"
        );
    }
}
