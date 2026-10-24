//! Every call form written in a span of code text, read as text because
//! fragments and listings have no parser.
//!
//! A call form is a lowercase name directly followed by a balanced `(…)` —
//! either a call site or the head of a `fn` declaration — and its arity is the
//! number of depth-0 arguments in that list (see [`count_parameters`]).
//!
//! # Limits
//!
//! - A `<` written WITHOUT spaces as a comparison (`x<lo`) is read as a
//!   generic opener, so the separators after it go uncounted until its bracket
//!   closes. Spaced comparisons (`x < lo`) are read correctly.
//! - Brackets are counted as text: a bracket quoted inside a string literal is
//!   counted like any other.

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

/// Every call form written in `code`, whose first line is `first_line`, in
/// document order.
///
/// Every maximal identifier run is a candidate, wherever it sits — including
/// inside another call's argument list, which is how a nested call is read in
/// its own right. A candidate is read only when it:
///
/// - starts with an ASCII lowercase letter (a capitalised name is a type or
///   constructor, a digit-led run a number);
/// - is not `.`-, `::`- or `@`-qualified (a method, a static dispatch, an
///   ad-hoc port selector);
/// - is followed — after an optional generic list — IMMEDIATELY by `(`;
/// - has an argument list that balances and elides nothing (`...` or `…`): an
///   elided list has no knowable arity, and inventing one would be worse than
///   reading none.
pub(crate) fn written_calls(code: &str, first_line: usize) -> Vec<WrittenCall> {
    let mut calls = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = code[cursor..].find(is_identifier_char) {
        let start = cursor + offset;
        let end = code[start..]
            .find(|c: char| !is_identifier_char(c))
            .map_or(code.len(), |len| start + len);
        cursor = end;
        if let Some(call) = call_form_at(code, start, end) {
            calls.push(WrittenCall {
                line: first_line + code[..start].matches('\n').count(),
                ..call
            });
        }
    }
    calls
}

/// The call form whose name is the identifier run `code[start..end]`, or
/// `None` when that run does not open one (see [`written_calls`]). Its `line`
/// is left for the caller to frame.
fn call_form_at(code: &str, start: usize, end: usize) -> Option<WrittenCall> {
    let name = &code[start..end];
    let before = &code[..start];
    if !name.starts_with(|c: char| c.is_ascii_lowercase())
        || before.ends_with('.')
        || before.ends_with("::")
        || before.ends_with('@')
    {
        return None;
    }
    let after_generics = skip_balanced(&code[end..], '<', '>')?;
    let arguments = balanced_run(after_generics, '(', ')')?;
    if arguments.contains("...") || arguments.contains('…') {
        return None;
    }
    let after_call = skip_balanced(after_generics, '(', ')')?;
    Some(WrittenCall {
        name: name.to_string(),
        arity: count_parameters(arguments),
        declaration: is_declared_by(before),
        text: code[start..code.len() - after_call.len()].to_string(),
        line: 0,
    })
}

/// Whether the text `before` a name ends with the WORD `fn`, making the name a
/// declaration head.
fn is_declared_by(before: &str) -> bool {
    before
        .trim_end()
        .strip_suffix("fn")
        .is_some_and(|rest| !rest.ends_with(is_identifier_char))
}

/// A character of an identifier run: `[A-Za-z0-9_]`.
fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `c`, preceded by `prev`, is the `>` of an `->` arrow rather than a
/// closing angle bracket.
///
/// A generic bound may carry a function type — `F: Fn(A) -> B` — whose arrow
/// would otherwise close the bound early, leaving `B>(…)` where the parameter
/// list is expected, and DROPPING a well-formed declaration that balances
/// perfectly well. The same rule stops a comma nested in `Map<Fn(X) -> Y, Z>`
/// from being counted at depth 0. Stated once here so every bracket walk below
/// reads the arrow identically.
fn is_arrow(prev: char, c: char) -> bool {
    c == '>' && prev == '-'
}

/// `s` with a leading balanced `open`…`close` run removed, or `s` unchanged when
/// it does not start with `open`. `None` if such a run starts but never closes.
pub(crate) fn skip_balanced(s: &str, open: char, close: char) -> Option<&str> {
    if !s.starts_with(open) {
        return Some(s);
    }
    let mut depth = 0usize;
    let mut prev = ' ';
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close && !is_arrow(prev, c) {
            depth -= 1;
            if depth == 0 {
                return Some(&s[i + c.len_utf8()..]);
            }
        }
        prev = c;
    }
    None
}

/// The INTERIOR of the balanced `open`…`close` run that `s` starts with, or
/// `None` when `s` does not start with `open` or the run never closes.
///
/// `s` must start with `open` IMMEDIATELY: a name separated from its `(` by
/// whitespace (`point3 (x)`) does not open a call.
pub(crate) fn balanced_run(s: &str, open: char, close: char) -> Option<&str> {
    if !s.starts_with(open) {
        return None;
    }
    let after = skip_balanced(s, open, close)?;
    let end = s.len() - after.len() - close.len_utf8();
    Some(&s[open.len_utf8()..end])
}

/// The number of DEPTH-0 comma-separated parameters in a parameter- or
/// argument-list interior. A whitespace-only interior is 0, and a trailing
/// comma adds none.
///
/// Depth is a bracket STACK, so a nested list contributes no separators:
///
/// - `(`, `[` and `{` always open; `<` opens only directly after an identifier
///   character (`Tensor<2, 3>`), never as a spaced comparison (`lo < hi`);
/// - `)`, `]` and `}` close through any unmatched `<` to their own opener;
/// - a `>` that is not an arrow (see [`is_arrow`]) closes only a `<` on top, so
///   a comparison (`a > b`) or a `=>` cannot unbalance the nesting;
/// - a `|` outside any bracket opens or closes a lambda-parameter list, whose
///   commas never count (`|a, b| a + b`).
pub(crate) fn count_parameters(inner: &str) -> usize {
    if inner.trim().is_empty() {
        return 0;
    }
    let mut open: Vec<char> = Vec::new();
    let mut in_lambda_parameters = false;
    let mut separators = 0usize;
    let mut last_separator_end = 0usize;
    let mut prev = ' ';
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '[' | '{' => open.push(c),
            '<' if is_identifier_char(prev) => open.push(c),
            ')' | ']' | '}' => close_through_angles(&mut open, c),
            '>' if !is_arrow(prev, c) && open.last() == Some(&'<') => {
                open.pop();
            }
            '|' if open.is_empty() => in_lambda_parameters = !in_lambda_parameters,
            ',' if open.is_empty() && !in_lambda_parameters => {
                separators += 1;
                last_separator_end = i + c.len_utf8();
            }
            _ => {}
        }
        prev = c;
    }
    let trailing_comma = separators > 0 && inner[last_separator_end..].trim().is_empty();
    separators + 1 - usize::from(trailing_comma)
}

/// Pop `open` through any unmatched `<` and then the opener `close` matches,
/// if that is what is on top.
fn close_through_angles(open: &mut Vec<char>, close: char) {
    let opener = match close {
        ')' => '(',
        ']' => '[',
        _ => '{',
    };
    while open.last() == Some(&'<') {
        open.pop();
    }
    if open.last() == Some(&opener) {
        open.pop();
    }
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
        (
            "obj.method(x)",
            "a `.`-qualified name is a method, not a bare call",
        ),
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
        (
            "Selector(Face)",
            "a capitalised name is a constructor or type, not a call",
        ),
        ("2(x)", "a digit-led run is a number, not a name"),
        (
            "point3 (x)",
            "a name not DIRECTLY followed by `(` is not a call",
        ),
        (
            "broken(a, b",
            "an argument list that never balances has no arity",
        ),
        ("box(...)", "an elided argument list has no knowable arity"),
        (
            "union_all(a, b, …)",
            "a U+2026-elided argument list has no knowable arity",
        ),
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
