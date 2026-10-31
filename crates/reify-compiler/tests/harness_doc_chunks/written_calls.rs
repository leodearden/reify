//! Every call form written in a span of code text, read as text because
//! fragments and listings have no parser.
//!
//! A call form is a lowercase name directly followed by a balanced `(…)` —
//! either a call site or the head of a `fn` declaration — and its arity is the
//! number of depth-0 arguments in that list (see [`count_parameters`]). Text
//! from `//` to the end of a line is a comment and is never read.
//!
//! # Limits
//!
//! - A `<` written WITHOUT spaces as a comparison (`x<lo`) is read as a
//!   generic opener, so the separators after it go uncounted until its bracket
//!   closes. Spaced comparisons (`x < lo`) are read correctly.
//! - String literals are read as text: a bracket quoted inside one is counted
//!   like any other, and a `//` inside one is read as the start of a comment.
//! - A `/* … */` block comment is read as code, so a call written inside one is
//!   read like any other.

use crate::chunk_markdown::{Fence, parse_fences};
use crate::doc_forms::unfenced_code_spans;

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
    /// The kind of chunk text the form is written in.
    pub(crate) source: CallSource,
}

/// The kind of chunk text a reader copies a call form out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CallSource {
    /// An unfenced prose code span.
    ProseSpan,
    /// A ```` ```reify ```` fence: a complete module.
    ReifyFence,
    /// A ```` ```reify-fragment ```` fence: member-level Reify.
    FragmentFence,
    /// A ```` ```reify-schematic ```` signature listing.
    SchematicListing,
}

/// Every fence tag a chunk may carry, each with the [`CallSource`] its body is
/// read as — `None` for a body a reader never copies as accepted Reify. The
/// vocabulary is `fence_gate.rs`'s "Tag vocabulary".
///
/// A fence documenting a REJECTED form (`reify-invalid`, `reify-rejected`,
/// `reify-rejected-at-eval`) shows exactly the call forms the compiler refuses,
/// and a non-reify language (`ebnf`, `text`) writes none.
///
/// CLOSED, unlike `fence_gate`'s open vocabulary: a fence whose tag is not
/// listed, or that carries none, makes its chunk unreadable. A new tag is
/// therefore a red that forces the copyable-or-not decision, never a silent
/// hole in what [`chunk_written_calls`] reads.
const FENCE_TAGS: &[(&str, Option<CallSource>)] = &[
    ("reify", Some(CallSource::ReifyFence)),
    ("reify-fragment", Some(CallSource::FragmentFence)),
    ("reify-schematic", Some(CallSource::SchematicListing)),
    ("reify-invalid", None),
    ("reify-rejected", None),
    ("reify-rejected-at-eval", None),
    ("ebnf", None),
    ("text", None),
];

/// Every call form `markdown` writes as copyable code — in its unfenced prose
/// code spans and in the bodies of the fences [`FENCE_TAGS`] reads — each at
/// the chunk line its name sits on. Prose first, then fences, each in document
/// order.
///
/// A whole fence body is cut at once, never line by line, so a call wrapped
/// across lines is read. Maintainer notes and bare prose are never read. `Err`
/// when the markup cannot be read (an unterminated fence or note) or a fence's
/// tag is not classified, never a short list.
pub(crate) fn chunk_written_calls(markdown: &str) -> Result<Vec<WrittenCall>, String> {
    let mut calls: Vec<WrittenCall> = unfenced_code_spans(markdown)?
        .into_iter()
        .flat_map(|span| written_calls(&span.text, span.line, CallSource::ProseSpan))
        .collect();
    for fence in parse_fences(markdown)? {
        if let Some(source) = fence_source(&fence)? {
            calls.extend(written_calls(&fence.body, fence.open_line + 1, source));
        }
    }
    Ok(calls)
}

/// The [`CallSource`] `fence`'s body is read as, `None` for a body that is
/// never copied, or `Err` when [`FENCE_TAGS`] does not classify its tag.
fn fence_source(fence: &Fence) -> Result<Option<CallSource>, String> {
    let tag = fence.tag.as_deref();
    FENCE_TAGS
        .iter()
        .find(|(listed, _)| tag == Some(*listed))
        .map(|(_, source)| *source)
        .ok_or_else(|| {
            format!(
                "the fence opened at line {} carries {}, which written_calls.rs's FENCE_TAGS \
                 does not classify, so whether a reader copies its body is unknown (tag the \
                 fence from that table, or classify a new tag there as copyable or not)",
                fence.open_line,
                tag.map_or_else(|| "no tag".to_string(), |tag| format!("the tag `{tag}`"))
            )
        })
}

/// Every call form written in `code`, whose first line is `first_line`, in
/// document order, each recorded as written in `source`. Text from `//` to the
/// end of a line is a comment and is never read.
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
pub(crate) fn written_calls(code: &str, first_line: usize, source: CallSource) -> Vec<WrittenCall> {
    let code = without_line_comments(code);
    let mut calls = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = code[cursor..].find(is_identifier_char) {
        let start = cursor + offset;
        let end = code[start..]
            .find(|c: char| !is_identifier_char(c))
            .map_or(code.len(), |len| start + len);
        cursor = end;
        calls.extend(call_form_at(&code, start, end, first_line, source));
    }
    calls
}

/// `code` with the text from each line's first `//` dropped, its line breaks
/// kept so every call keeps its line.
fn without_line_comments(code: &str) -> String {
    code.split('\n')
        .map(|line| line.find("//").map_or(line, |comment| &line[..comment]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The call form whose name is the identifier run `code[start..end]` of code
/// whose first line is `first_line`, written in `source` — or `None` when that
/// run does not open one (see [`written_calls`]).
fn call_form_at(
    code: &str,
    start: usize,
    end: usize,
    first_line: usize,
    source: CallSource,
) -> Option<WrittenCall> {
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
        line: first_line + before.matches('\n').count(),
        source,
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
fn skip_balanced(s: &str, open: char, close: char) -> Option<&str> {
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
fn balanced_run(s: &str, open: char, close: char) -> Option<&str> {
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
fn count_parameters(inner: &str) -> usize {
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
/// document order. The source is only passed through, so any one serves.
fn forms(code: &str) -> Vec<(String, usize, bool)> {
    written_calls(code, 1, CallSource::FragmentFence)
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
        CallSource::SchematicListing,
    );
    assert_eq!(
        wrapped,
        vec![WrittenCall {
            name: "wrapped".to_string(),
            arity: 2,
            declaration: true,
            text: "wrapped(\n    a : Length,\n    b : Length,\n)".to_string(),
            line: 2,
            source: CallSource::SchematicListing,
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
    let calls = written_calls(
        "let a = 1mm\nlet b = 2mm\nlet c = sphere(a)\n",
        10,
        CallSource::FragmentFence,
    );
    assert_eq!(
        calls,
        vec![WrittenCall {
            name: "sphere".to_string(),
            arity: 1,
            declaration: false,
            text: "sphere(a)".to_string(),
            line: 12,
            source: CallSource::FragmentFence,
        }],
        "a call on the third line of code whose first line is 10 sits on line 12, recorded as \
         written in the source it was cut from"
    );
}

#[test]
fn a_line_comment_is_never_read_and_keeps_every_later_line() {
    assert_eq!(
        forms("let s = sphere(r) // was cone(r1, r2, h)"),
        vec![form("sphere", 1, false)],
        "a call written in a `//` comment is not a written call"
    );
    assert_eq!(
        forms("let w = wedge(a, // the width, in mm\n    b)"),
        vec![form("wedge", 2, false)],
        "a comment inside a wrapped argument list contributes no separators"
    );
    assert_eq!(
        written_calls(
            "// was torus(a)\nlet t = torus(a, b)",
            5,
            CallSource::FragmentFence
        )
        .into_iter()
        .map(|call| (call.name, call.arity, call.line))
        .collect::<Vec<_>>(),
        vec![("torus".to_string(), 2, 6)],
        "a dropped comment keeps its line break, so the call below it keeps its line"
    );
}

/// A chunk writing one call in every source kind. Its line layout is part of
/// the fixture: the test below names the chunk line of each copyable call.
const DEMO_CHUNK: &str = r##"# Demo

Call `box(w, d, h)` for a box; sphere(r) is bare prose.
See `rotate(geo, ax,
ay)` when wrapped.

<!-- maintainer: `note_call(a, b, c)` -->

```reify
structure def Demo {
    let s = cylinder(1mm, 2mm) // was comment_call(a)
}
```

```reify-fragment
let a = 1mm
let b = 2mm
let c = torus(a, b)
```

```reify-schematic
cone(r1, r2, h) -> Solid
```

```reify-invalid
let bad = invalid_call(1mm)
```

```reify-rejected
let bad = rejected_call(1mm)
```

```reify-rejected-at-eval
let bad = rejected_at_eval_call(1mm)
```

```ebnf
call = ebnf_call(name) ;
```

```text
text_call(x)
```
"##;

#[test]
fn chunk_written_calls_reads_prose_spans_and_copyable_fences_at_their_chunk_lines() {
    let mut read: Vec<(String, usize, usize, CallSource)> = chunk_written_calls(DEMO_CHUNK)
        .expect("the demo chunk is well-formed")
        .into_iter()
        .map(|call| (call.name, call.arity, call.line, call.source))
        .collect();
    read.sort_by_key(|(_, _, line, _)| *line);

    assert_eq!(
        read,
        vec![
            ("box".to_string(), 3, 3, CallSource::ProseSpan),
            ("rotate".to_string(), 3, 4, CallSource::ProseSpan),
            ("cylinder".to_string(), 2, 11, CallSource::ReifyFence),
            ("torus".to_string(), 2, 18, CallSource::FragmentFence),
            ("cone".to_string(), 3, 22, CallSource::SchematicListing),
        ],
        "read, each recorded with its source kind: a prose code span (box, line 3); a span \
         wrapped across two lines, at the line its name sits on (rotate, line 4); a ```reify \
         fence (cylinder), a ```reify-fragment fence on its THIRD body line (torus, open_line \
         15 + 3) and a ```reify-schematic listing (cone). NOT read: a bare un-backticked call in \
         prose (sphere), a maintainer note (note_call), a `//` comment (comment_call), a fence \
         documenting a REJECTED form (reify-invalid, reify-rejected, reify-rejected-at-eval) and \
         a non-reify language (ebnf, text)"
    );
}

#[test]
fn chunk_written_calls_is_an_err_when_the_chunk_cannot_be_read() {
    let span = "Call `box(w, d, h)` here.\n\n";
    assert_eq!(
        chunk_written_calls(span).map(|calls| calls.len()),
        Ok(1),
        "control: the span alone is read"
    );

    for (defect, why) in [
        (
            "```reify-fragment\nlet c = torus(a, b)\n",
            "an unterminated fence hides the rest of the chunk",
        ),
        (
            "<!-- an unterminated note\n",
            "an unterminated HTML comment hides the rest of the chunk",
        ),
        (
            "```reify-example\nlet c = torus(a, b)\n```\n",
            "a fence tag FENCE_TAGS does not classify leaves unknown whether its body is copied",
        ),
        (
            "```\nlet c = torus(a, b)\n```\n",
            "an untagged fence leaves unknown whether its body is copied",
        ),
    ] {
        assert!(
            chunk_written_calls(&format!("{span}{defect}")).is_err(),
            "{why}, so it is an Err, never a short list of calls"
        );
    }

    let unclassified = chunk_written_calls(&format!(
        "{span}```reify-example\nlet c = torus(a, b)\n```\n"
    ))
    .expect_err("an unclassified tag is an Err");
    for expected in ["line 3", "`reify-example`", "FENCE_TAGS"] {
        assert!(
            unclassified.contains(expected),
            "the error names the fence's line, its tag and the table to classify it in \
             (`{expected}`); got {unclassified}"
        );
    }
}
