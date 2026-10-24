//! Every call form written in a span of code text, read as text because
//! fragments and listings have no parser.

/// One call form written in code text: a call site or a `fn` declaration head.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WrittenCall {
    pub(crate) name: String,
    pub(crate) arity: usize,
    /// Whether the form is a `fn` declaration head rather than a call site.
    pub(crate) declaration: bool,
    /// The `name(…)` source as written, a generic list on the name included.
    pub(crate) text: String,
    /// 1-based line the name sits on.
    pub(crate) line: usize,
}

/// Every call form written in `code`, whose first line is `first_line`.
pub(crate) fn written_calls(_code: &str, _first_line: usize) -> Vec<WrittenCall> {
    Vec::new()
}

/// `(name, arity, declaration)` for every call form written in `code`, in
/// document order.
fn forms(code: &str) -> Vec<(String, usize, bool)> {
    written_calls(code, 1)
        .into_iter()
        .map(|call| (call.name, call.arity, call.declaration))
        .collect()
}

fn form(name: &str, arity: usize, declaration: bool) -> (String, usize, bool) {
    (name.to_string(), arity, declaration)
}

#[test]
fn a_call_site_is_read_with_its_argument_count_and_a_nested_call_in_its_own_right() {
    assert_eq!(
        forms("box(1mm, 2mm, 3mm)"),
        vec![form("box", 3, false)],
        "a plain call site counts its depth-0 arguments"
    );
    assert_eq!(
        forms("translate(box(1mm, 1mm, 1mm), 0mm, 0mm, 0mm)"),
        vec![form("translate", 4, false), form("box", 3, false)],
        "the nested box call is one argument of translate AND a call read in its own right"
    );
    assert_eq!(
        forms("mechanism()"),
        vec![form("mechanism", 0, false)],
        "an empty argument list is arity 0"
    );
}

#[test]
fn a_fn_declaration_head_is_read_as_a_declaration_with_its_parameter_count() {
    assert_eq!(
        forms("fn clamp(x : Real, lo : Real, hi : Real) -> Real { ... }"),
        vec![form("clamp", 3, true)],
        "a plain parameter list counts its parameters, and the `{{ ... }}` body elision does not \
         count as an elided argument list"
    );
    assert_eq!(
        forms("fn von_mises(t : Tensor<2, 3, Pressure>) -> Scalar<Pressure> { ... }"),
        vec![form("von_mises", 1, true)],
        "commas inside a generic argument list do not split a parameter"
    );
    assert_eq!(
        forms(
            "fn mover<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) \
             -> G { ... }\n\
             fn mover<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G { ... }"
        ),
        vec![form("mover", 3, true), form("mover", 2, true)],
        "a generic list on the NAME is skipped, not mistaken for the parameter list"
    );
    assert_eq!(
        forms("pub fn extrude_to(profile : Surface, height : Length) -> Solid { ... }"),
        vec![form("extrude_to", 2, true)],
        "a `pub fn` head is a declaration like any other"
    );
    assert_eq!(
        forms("fn apply<F: Fn(A) -> B>(f : F, a : A) -> B { ... }"),
        vec![form("apply", 2, true)],
        "an `->` inside a function-typed bound is an arrow, not the close of the generic list"
    );

    let wrapped = written_calls(
        "// the head wraps\nfn wrapped(\n    a : Length,\n    b : Length,\n) -> Length { ... }",
        1,
    );
    assert_eq!(
        wrapped,
        vec![WrittenCall {
            name: "wrapped".to_string(),
            arity: 2,
            declaration: true,
            text: "wrapped(\n    a : Length,\n    b : Length,\n)".to_string(),
            line: 2,
        }],
        "a declaration WRAPPED across lines is read whole, at the line its name sits on, and a \
         trailing comma adds no parameter"
    );
}

#[test]
fn argument_shapes_contribute_separators_only_at_depth_zero() {
    assert_eq!(
        forms("clamp(x, lo < hi, z)"),
        vec![form("clamp", 3, false)],
        "a `<` NOT directly after an identifier character is an operator, not a generic opener"
    );
    assert_eq!(
        forms("f(g(a > b, c), d)"),
        vec![form("f", 2, false), form("g", 2, false)],
        "a `>` closes only an open `<`, so a comparison cannot unbalance the nesting"
    );
    assert_eq!(
        forms("map_or(o, dflt, |a, b| a + b)"),
        vec![form("map_or", 3, false)],
        "a `|…|` lambda-parameter list contributes no separators"
    );
    assert_eq!(
        forms(r#"f([1, 2], map{"a" => 1, "b" => 2})"#),
        vec![form("f", 2, false)],
        "`[]` and `{{}}` nest, so their commas contribute no separators"
    );
}

#[test]
fn qualified_capitalised_unbalanced_and_elided_forms_are_not_read() {
    for (code, why) in [
        ("obj.method(x)", "a `.`-qualified name is a method, not a bare call"),
        (
            "Defaultable::make_default()",
            "a `::`-qualified name is static dispatch, not a bare call",
        ),
        (
            "pin.(Cylindrical::lateral_area)()",
            "a `::`-qualified name inside a parenthesised callee is not a bare call",
        ),
        (
            "bracket@face(top_surface)",
            "an `@` ad-hoc port selector is not a call",
        ),
        ("Selector(Face)", "a capitalised name is a constructor or type, not a call"),
        ("2(x)", "a digit-led run is a number, not a name"),
        (
            "point3 (x)",
            "a name not DIRECTLY followed by `(` is not a call",
        ),
        ("broken(a, b", "an argument list that never balances has no arity"),
        ("box(...)", "an elided argument list has no knowable arity"),
        ("union_all(a, b, …)", "a U+2026-elided argument list has no knowable arity"),
    ] {
        assert_eq!(forms(code), vec![], "`{code}`: {why}");
    }

    assert_eq!(
        forms("xmin_clearance(s, a, b)"),
        vec![form("xmin_clearance", 3, false)],
        "a name is a MAXIMAL identifier run — `min_clearance` is never read out of \
         `xmin_clearance`"
    );
}

#[test]
fn a_call_reports_the_line_its_name_sits_on_framed_by_first_line() {
    let calls = written_calls("let a = 1mm\nlet b = 2mm\nlet c = sphere(a)\n", 10);
    assert_eq!(
        calls,
        vec![WrittenCall {
            name: "sphere".to_string(),
            arity: 1,
            declaration: false,
            text: "sphere(a)".to_string(),
            line: 12,
        }],
        "a call on the third line of code whose first line is 10 sits on line 12"
    );
}
