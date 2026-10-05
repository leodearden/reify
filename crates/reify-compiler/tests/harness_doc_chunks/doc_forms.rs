//! Documented call FORMS — `(name, arity)` — read from markdown spans and from
//! parsed sources, and paired: a documented form is exercised when a source
//! calls that name at a matching arity.
//!
//! A span is a documented form only when it is SIGNATURE-SHAPED, whole: a
//! lowercase snake_case name, then `(params)` of metavariables, `label: metavar`
//! named arguments, a bare `…` or an `ident…` (U+2026 — the ASCII `...` elides
//! an argument list rather than declaring one variadic), then optionally
//! `-> Type`. Concrete idioms, expressions, declarations, qualified or
//! capitalised names and lambdas are prose, never signatures — see
//! [`doc_form_of_span`]. A ```` ```reify-schematic ```` listing is read span by
//! span, as [`listing_signature_spans`] cuts it, through that same rule.
//!
//! A source's call forms are read off its parsed AST by [`call_forms`], and a
//! chunk's ```` ```reify ```` fences are read the same way, one module per
//! fence, by [`fence_call_forms`].

use reify_ast::{Declaration, Expr, ExprKind, MemberDecl, ParsedModule, StringPart, WhereClause};
use reify_compiler::parse_with_stdlib;
use reify_core::ModulePath;

use crate::chunk_markdown::tagged_fence_bodies;

/// A documented form's declared argument count: either an exact arity, or a
/// variadic form carrying the given MINIMUM arity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Arity {
    Exact(usize),
    AtLeast(usize),
}

/// One documented (name, arity) overload. A single row commonly documents
/// several of these for the same name (e.g. `mirror(geo, plane)` and
/// `mirror(geo, ox, oy, oz, nx, ny, nz)`) — they are deliberately NOT
/// collapsed, which is the whole point of pairing at FORM granularity rather
/// than by name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DocForm {
    pub(crate) name: String,
    pub(crate) arity: Arity,
}

impl DocForm {
    /// Does any `(name, arg count)` call exercise this form? `Exact(n)` needs a
    /// call with exactly `n` arguments, `AtLeast(n)` one with `n` or more.
    pub(crate) fn is_exercised_by(&self, calls: &[(String, usize)]) -> bool {
        calls.iter().any(|(name, count)| {
            *name == self.name
                && match self.arity {
                    Arity::Exact(n) => *count == n,
                    Arity::AtLeast(n) => *count >= n,
                }
        })
    }
}

/// The documented form a code span declares, or `None` when the span is not
/// signature-shaped as a WHOLE (see the module doc).
///
/// A bare `…` or an `ident…` argument contributes nothing to the count and
/// makes the form `AtLeast`; every other argument counts one.
pub(crate) fn doc_form_of_span(span: &str) -> Option<DocForm> {
    let span = span.trim();
    let open = span.find('(')?;
    let close = open + span[open..].find(')')?;
    let name = &span[..open];
    let params = &span[open + 1..close];
    if !is_metavariable(name) || params.contains('(') || !is_return_annotation(&span[close + 1..]) {
        return None;
    }

    if params.trim().is_empty() {
        return Some(DocForm {
            name: name.to_string(),
            arity: Arity::Exact(0),
        });
    }
    let mut count = 0;
    let mut variadic = false;
    for param in params.split(',').map(str::trim) {
        match param.strip_suffix('…') {
            Some(stem) if stem.is_empty() || is_metavariable(stem) => variadic = true,
            Some(_) => return None,
            None if is_argument(param) => count += 1,
            None => return None,
        }
    }
    Some(DocForm {
        name: name.to_string(),
        arity: if variadic {
            Arity::AtLeast(count)
        } else {
            Arity::Exact(count)
        },
    })
}

/// The candidate signature spans on each line of a ```` ```reify-schematic ````
/// `listing`, in order, each for [`doc_form_of_span`] to read.
///
/// Per line, text from the first `//` is a comment and is dropped. Every
/// `ident(` whose identifier is not `.`-qualified yields `ident(…)` through its
/// balancing paren; a trailing `-> Type` carries no arity, so it is left
/// behind. A call nested in another's parentheses is part of that span, never
/// cut on its own. A listing is read line by line, so a `(` that does not
/// balance on its line yields the rest of that line — a span no signature
/// reading accepts, so a wrapped signature is surfaced rather than dropped.
pub(crate) fn listing_signature_spans(listing: &str) -> Vec<String> {
    listing
        .lines()
        .flat_map(|line| line_signature_spans(line.split("//").next().unwrap_or_default()))
        .collect()
}

/// [`listing_signature_spans`] over one comment-free line.
fn line_signature_spans(code: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    while let Some(found) = code[cursor..].find('(') {
        let open = cursor + found;
        let end = closing_paren(code, open).map_or(code.len(), |close| close + 1);
        let start = code[..open]
            .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
            .len();
        if start < open && !code[..start].ends_with('.') {
            spans.push(code[start..end].trim_end().to_string());
        }
        cursor = end;
    }
    spans
}

/// The byte index of the `)` that balances the `(` at `open`, if `code` has one.
fn closing_paren(code: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, c) in code[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// A lowercase snake_case identifier: the only shape a documented name or a
/// metavariable takes.
fn is_metavariable(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A metavariable, or a `label: metavar` named argument.
fn is_argument(param: &str) -> bool {
    match param.split_once(':') {
        Some((label, value)) => is_metavariable(label.trim()) && is_metavariable(value.trim()),
        None => is_metavariable(param),
    }
}

/// Nothing at all, or `-> Type` with a capitalised type (generics, commas and
/// parenthesised arguments allowed).
fn is_return_annotation(rest: &str) -> bool {
    let rest = rest.trim();
    if rest.is_empty() {
        return true;
    }
    let Some(return_type) = rest.strip_prefix("->").map(str::trim) else {
        return false;
    };
    return_type.starts_with(|c: char| c.is_ascii_uppercase())
        && return_type.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '<' | '>' | ',' | ' ' | '(' | ')')
        })
}

/// Every documented form no call exercises, sorted and deduped so a failure
/// names the exact form(s).
pub(crate) fn unmirrored_forms(documented: &[DocForm], calls: &[(String, usize)]) -> Vec<DocForm> {
    let mut unmirrored: Vec<DocForm> = documented
        .iter()
        .filter(|form| !form.is_exercised_by(calls))
        .cloned()
        .collect();
    unmirrored.sort();
    unmirrored.dedup();
    unmirrored
}

/// `source` parsed prelude-aware (the `compile_with_stdlib` companion). A parse
/// error is a bug in the fixture or snippet, not the property under test, so it
/// panics distinctly rather than being skipped.
pub(crate) fn parse_or_panic(source: &str, label: &str) -> ParsedModule {
    let parsed = parse_with_stdlib(source, ModulePath::single("doc_forms_source"));
    assert!(
        parsed.errors.is_empty(),
        "{label} must parse cleanly, got parse errors:\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );
    parsed
}

/// Every `(call name, arg count)` form in `source`, deduped and sorted for
/// deterministic output. A named argument counts one, exactly as a documented
/// form counts it.
///
/// `source` must be `structure def`s whose members are `let`, `param` or
/// `constraint` declarations — the shape of the signature fixtures, the
/// chunks' ```` ```reify ```` fences and the cited examples. A `param`
/// default and every member's `where` guard are walked too. Any other
/// declaration or member kind PANICS rather than being skipped, so growing a
/// source a new kind is a loud "extend the walker", never a silent coverage
/// hole.
pub(crate) fn call_forms(source: &str, label: &str) -> Vec<(String, usize)> {
    let parsed = parse_or_panic(source, label);

    let mut forms = Vec::new();
    for decl in &parsed.declarations {
        let Declaration::Structure(structure) = decl else {
            panic!(
                "{label}: `call_forms` only walks `structure def` declarations, but this source \
                 has another declaration kind — extend `call_forms` rather than leaving those \
                 call sites unchecked"
            );
        };
        for member in &structure.members {
            match member {
                MemberDecl::Let(binding) => {
                    collect_call_forms(&binding.value, &mut forms);
                    collect_guard_call_forms(&binding.where_clause, &mut forms);
                }
                MemberDecl::Param(param) => {
                    if let Some(default) = &param.default {
                        collect_call_forms(default, &mut forms);
                    }
                    collect_guard_call_forms(&param.where_clause, &mut forms);
                }
                MemberDecl::Constraint(constraint) => {
                    collect_call_forms(&constraint.expr, &mut forms);
                    collect_guard_call_forms(&constraint.where_clause, &mut forms);
                }
                _ => panic!(
                    "{label}: `call_forms` only walks `let`, `param` and `constraint` members of \
                     `{}`, but it has another member kind — extend `call_forms` rather than \
                     leaving those call sites unchecked",
                    structure.name
                ),
            }
        }
    }

    forms.sort();
    forms.dedup();
    forms
}

/// Every `(call name, arg count)` form across the bare ```` ```reify ````
/// fences of `markdown`, each fence parsed as its own module through
/// [`call_forms`] — so prose, other-tagged fences, comments and string literals
/// contribute nothing. Deduped and sorted across fences.
///
/// A fence that does not parse, or that [`call_forms`] cannot walk, panics
/// naming `chunk_path` and the fence's 1-based position among the bare
/// ```` ```reify ```` fences.
pub(crate) fn fence_call_forms(markdown: &str, chunk_path: &str) -> Vec<(String, usize)> {
    let mut forms: Vec<(String, usize)> = tagged_fence_bodies(markdown, "reify", chunk_path)
        .iter()
        .enumerate()
        .flat_map(|(index, body)| {
            call_forms(body, &format!("{chunk_path} ```reify fence #{}", index + 1))
        })
        .collect();
    forms.sort();
    forms.dedup();
    forms
}

/// The distinct callee names of `forms`, sorted: every overload of a name
/// collapses to one entry.
pub(crate) fn callee_names(forms: &[(String, usize)]) -> Vec<String> {
    let mut names: Vec<String> = forms.iter().map(|(name, _arity)| name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// [`collect_call_forms`] over a member's `where` guard, when it has one.
fn collect_guard_call_forms(guard: &Option<WhereClause>, out: &mut Vec<(String, usize)>) {
    if let Some(guard) = guard {
        collect_call_forms(&guard.condition, out);
    }
}

/// Push `(callee name, arg count)` for every `FunctionCall` in `expr`'s
/// subtree onto `out`.
///
/// The match is intentionally exhaustive with **no `_` wildcard**, so adding an
/// `ExprKind` variant breaks this file at compile time rather than silently
/// dropping a whole class of call site from the guard (same posture as
/// `find_node` in `tests/harness_langcore/type_error_propagation_tests.rs`).
/// Walking the parsed AST — rather than lexing the source — means no comment or
/// string-literal blind spots and no keyword/heuristic allowlists.
///
/// Non-`FunctionCall` callee names (a trait method, an ad-hoc port selector)
/// are deliberately NOT collected: they are dispatched through a different
/// resolver and are not documented call forms.
fn collect_call_forms(expr: &Expr, out: &mut Vec<(String, usize)>) {
    match &expr.kind {
        // Leaves — no subexpressions, no callee name.
        ExprKind::NumberLiteral { .. }
        | ExprKind::QuantityLiteral { .. }
        | ExprKind::StringLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Ident(_)
        | ExprKind::EnumAccess { .. }
        | ExprKind::Undef => {}

        // The variant under test.
        ExprKind::FunctionCall { name, args, .. } => {
            out.push((name.clone(), args.len()));
            for arg in args {
                collect_call_forms(arg, out);
            }
        }

        // Compound variants — recurse into every child subexpression.
        ExprKind::BinOp { left, right, .. } => {
            collect_call_forms(left, out);
            collect_call_forms(right, out);
        }
        ExprKind::UnOp { operand, .. } => collect_call_forms(operand, out),
        ExprKind::MemberAccess { object, .. } => collect_call_forms(object, out),
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_call_forms(condition, out);
            collect_call_forms(then_branch, out);
            collect_call_forms(else_branch, out);
        }
        ExprKind::ListLiteral(items) | ExprKind::SetLiteral(items) => {
            for item in items {
                collect_call_forms(item, out);
            }
        }
        ExprKind::MapLiteral(entries) => {
            for (key, value) in entries {
                collect_call_forms(key, out);
                collect_call_forms(value, out);
            }
        }
        ExprKind::IndexAccess { object, index } => {
            collect_call_forms(object, out);
            collect_call_forms(index, out);
        }
        ExprKind::Match { discriminant, arms } => {
            collect_call_forms(discriminant, out);
            for arm in arms {
                collect_call_forms(&arm.body, out);
            }
        }
        ExprKind::Auto { params, .. } => {
            for (_, value) in params {
                collect_call_forms(value, out);
            }
        }
        ExprKind::Lambda { body, .. } => collect_call_forms(body, out),
        ExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            collect_call_forms(collection, out);
            collect_call_forms(predicate, out);
        }
        ExprKind::AdHocSelector { base, args, .. } => {
            collect_call_forms(base, out);
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::QualifiedAccess { qualifier, .. } => collect_call_forms(qualifier, out),
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            collect_call_forms(object, out);
            collect_call_forms(qualified, out);
        }
        ExprKind::Range { lower, upper, .. } => {
            if let Some(lower) = lower {
                collect_call_forms(lower, out);
            }
            if let Some(upper) = upper {
                collect_call_forms(upper, out);
            }
        }
        ExprKind::TraitMethodCall { object, args, .. } => {
            collect_call_forms(object, out);
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::TraitStaticCall { args, .. } => {
            for arg in args {
                collect_call_forms(arg, out);
            }
        }
        ExprKind::VariantConstruct { fields, .. } => {
            for (_, value) in fields {
                collect_call_forms(value, out);
            }
        }
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                match part {
                    StringPart::Literal(_) => {}
                    StringPart::Hole(inner) => collect_call_forms(inner, out),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hermetic tests — synthetic spans and sources only.
// ---------------------------------------------------------------------------

fn form(name: &str, arity: Arity) -> DocForm {
    DocForm {
        name: name.to_string(),
        arity,
    }
}

#[test]
fn doc_form_of_span_reads_a_signature_shaped_span() {
    let cases = [
        (
            "rotate(geo, ax, ay, az, angle)",
            form("rotate", Arity::Exact(5)),
            "each metavariable counts one",
        ),
        (
            "mechanism()",
            form("mechanism", Arity::Exact(0)),
            "an empty list is Exact(0)",
        ),
        (
            "union_all(a, b, …)",
            form("union_all", Arity::AtLeast(2)),
            "a bare `…` marks the form variadic and counts nothing",
        ),
        (
            "nurbs(degree, n_points, coords…, weights…)",
            form("nurbs", Arity::AtLeast(2)),
            "an `ident…` marks the form variadic and counts nothing",
        ),
        (
            "loft_guided(profile1, profile2, …, guide)",
            form("loft_guided", Arity::AtLeast(3)),
            "an argument after the `…` still counts",
        ),
        (
            "isosurface(grid, iso: level)",
            form("isosurface", Arity::Exact(2)),
            "a `label: metavar` named argument counts one",
        ),
        (
            "volume(solid) -> Scalar<Volume>",
            form("volume", Arity::Exact(1)),
            "a return type may follow",
        ),
        (
            "curvature(surface, at) -> Matrix<2, 2, Curvature>",
            form("curvature", Arity::Exact(2)),
            "the return type may carry commas and generics",
        ),
        (
            "  mechanism()  ",
            form("mechanism", Arity::Exact(0)),
            "surrounding whitespace is trimmed",
        ),
    ];

    for (span, expected, why) in cases {
        assert_eq!(doc_form_of_span(span), Some(expected), "`{span}`: {why}");
    }
}

#[test]
fn doc_form_of_span_reads_nothing_else_as_a_signature() {
    let cases = [
        (
            "translate(z=-height/2)",
            "a keyword snippet with an expression",
        ),
        (
            "translate(cylinder(r, h), 0mm, 0mm, -h/2)",
            "a concrete idiom: nested call, literals, expression",
        ),
        ("box(20, 20, 10)", "literal arguments"),
        ("scale(g, 2mm)", "a quantity literal"),
        ("2*corner_r < min(width, depth)", "an expression"),
        ("distance(a, b) > tol", "a comparison"),
        ("some(c) => ...", "a match arm"),
        ("alt = some(0.25mm)", "an assignment"),
        ("let all_faces = faces(b)", "a let binding"),
        (
            "fn faces(solid: Solid) -> List<Surface>",
            "a declaration with typed parameters",
        ),
        ("Selector(Face)", "a capitalised type"),
        ("ScalarForce(Real)", "a capitalised variant"),
        ("Engine::new(.., None)", "a qualified call"),
        ("Trait::fn(args)", "a qualified call"),
        ("point3(...)", "an ASCII elision of an argument list"),
        ("map_or(o, dflt, |x: T| ...)", "a lambda parameter"),
        ("broken(a, b", "unbalanced parentheses"),
        ("List<Geometry>", "a type, with no call at all"),
    ];

    for (span, why) in cases {
        assert_eq!(
            doc_form_of_span(span),
            None,
            "`{span}` must not read as a signature: {why}"
        );
    }
}

#[test]
fn is_exercised_by_matches_exact_by_equality_and_at_least_by_minimum() {
    let calls = vec![("rotate".to_string(), 5), ("union_all".to_string(), 3)];

    assert!(form("rotate", Arity::Exact(5)).is_exercised_by(&calls));
    assert!(
        !form("rotate", Arity::Exact(2)).is_exercised_by(&calls),
        "Exact needs a call with exactly that many arguments"
    );
    assert!(
        form("union_all", Arity::AtLeast(2)).is_exercised_by(&calls),
        "AtLeast accepts more arguments"
    );
    assert!(
        form("union_all", Arity::AtLeast(3)).is_exercised_by(&calls),
        "AtLeast accepts exactly the minimum"
    );
    assert!(
        !form("union_all", Arity::AtLeast(4)).is_exercised_by(&calls),
        "AtLeast rejects fewer arguments"
    );
    assert!(
        !form("translate", Arity::Exact(5)).is_exercised_by(&calls),
        "a call to a different name never exercises the form"
    );
}

#[test]
fn call_forms_counts_a_named_argument_as_one() {
    let source = r#"
structure def NamedArgument {
    let s = sphere(5mm)
    let shell = isosurface(s, iso: 3mm)
}
"#;

    assert_eq!(
        call_forms(source, "named-argument snippet"),
        vec![("isosurface".to_string(), 2), ("sphere".to_string(), 1)],
        "a named argument is one argument, in the same currency the documented form is read in"
    );
}

#[test]
fn call_forms_walks_param_defaults_constraints_and_where_guards() {
    let source = r#"
structure def EveryMemberSlot {
    param h : Length = default_height(40mm) where param_enabled(1mm, 2mm)
    let b = box(h, h, h) where let_enabled(h)
    constraint volume(b) > 1mm^3 where constraint_enabled(b, h, h, h)
}
"#;

    let forms: Vec<(String, usize)> = [
        ("box", 3),
        ("constraint_enabled", 4),
        ("default_height", 1),
        ("let_enabled", 1),
        ("param_enabled", 2),
        ("volume", 1),
    ]
    .iter()
    .map(|(name, arity)| (name.to_string(), *arity))
    .collect();
    assert_eq!(
        call_forms(source, "member-slot snippet"),
        forms,
        "a param default, a constraint expression and every member's `where` guard (the `let` \
         guard included) are call sites"
    );
}

#[test]
fn fence_call_forms_reads_only_the_parsed_code_of_bare_reify_fences() {
    let markdown = r#"# Demo chunk

Prose may name `prose_only(a)` in a code span, or prose_only(a) bare.

```reify
structure def Members {
    param h : Length = 5mm
    let s = sphere(h)
    constraint volume(s) > 1mm^3
}
```

```reify-schematic
schematic_only(a, b)
```

```reify
structure def Commented {
    // line_commented(a, b)
    /* block_commented(a) */
    let note = "string_call(a)"
    let g = box(1mm, 2mm, 3mm)
    let ball = sphere(1mm)
}
```
"#;

    let forms: Vec<(String, usize)> = [("box", 3), ("sphere", 1), ("volume", 1)]
        .iter()
        .map(|(name, arity)| (name.to_string(), *arity))
        .collect();
    assert_eq!(
        fence_call_forms(markdown, "demo.md"),
        forms,
        "only the parsed code of bare ```reify fences is read, each fence as its own module: \
         prose, a schematic listing, comments and string literals contribute nothing, and the \
         forms of every fence are sorted and deduped together"
    );
}

#[test]
fn callee_names_collapses_overloads_to_sorted_distinct_names() {
    let forms = vec![
        ("b".to_string(), 1),
        ("a".to_string(), 2),
        ("a".to_string(), 3),
    ];

    assert_eq!(
        callee_names(&forms),
        vec!["a".to_string(), "b".to_string()],
        "two overloads of one name are one callee name, and the names come back sorted"
    );
}

#[test]
fn listing_signature_spans_cuts_every_unqualified_call_on_a_line() {
    let cases: [(&str, &[&str], &str); 9] = [
        (
            "point2(x, y)          point3(x, y, z)",
            &["point2(x, y)", "point3(x, y, z)"],
            "two signatures on one line are both cut",
        ),
        (
            "box(width, depth, height)   -> Solid   // alias of box_centered(w, d, h)",
            &["box(width, depth, height)"],
            "the return type is left behind, and a call in the comment is never read",
        ),
        (
            "Orientation.from_quaternion(w, x, y, z)",
            &[],
            "a `.`-qualified name is not a listed signature",
        ),
        (
            "Point<N: Nat, Q: Dimension>     // Position",
            &[],
            "a type listing has no call",
        ),
        (
            "broken(a, b   ",
            &["broken(a, b"],
            "an unbalanced paren is cut through the end of the line, trailing space trimmed",
        ),
        (
            "Orientation.from_quaternion(w, x,",
            &[],
            "a `.`-qualified name is not a listed signature, balanced or not",
        ),
        (
            "translate(cylinder(r, h), dx)",
            &["translate(cylinder(r, h), dx)"],
            "a nested call is part of the enclosing span",
        ),
        (
            "sphere(radius)\ntorus(major_radius, minor_radius)",
            &["sphere(radius)", "torus(major_radius, minor_radius)"],
            "every line of the listing is read",
        ),
        (
            "wrapped(a,\nb)",
            &["wrapped(a,"],
            "a span never crosses a line: a wrapped signature's first line is its span",
        ),
    ];

    for (listing, expected, why) in cases {
        assert_eq!(
            listing_signature_spans(listing),
            expected.to_vec(),
            "`{listing}`: {why}"
        );
    }
}

#[test]
fn a_listed_span_is_read_by_the_same_rule_as_a_prose_span() {
    let cases: [(&str, &[Option<DocForm>], &str); 3] = [
        (
            "isosurface(grid, iso: level)                         -> Solid",
            &[Some(form("isosurface", Arity::Exact(2)))],
            "a named argument counts one",
        ),
        (
            "polygon(x1, y1, x2, y2, x3, y3, …)   rectangle(width, height)",
            &[
                Some(form("polygon", Arity::AtLeast(6))),
                Some(form("rectangle", Arity::Exact(2))),
            ],
            "a U+2026 tail is variadic over the arguments before it",
        ),
        (
            "polygon(x1, y1, x2, y2, ...)   ellipse(semi_major, semi_minor)",
            &[None, Some(form("ellipse", Arity::Exact(2)))],
            "an ASCII `...` elides rather than declares, so the span is unreadable",
        ),
    ];

    for (listing, expected, why) in cases {
        let read: Vec<Option<DocForm>> = listing_signature_spans(listing)
            .iter()
            .map(|span| doc_form_of_span(span))
            .collect();
        assert_eq!(read, expected.to_vec(), "`{listing}`: {why}");
    }
}
