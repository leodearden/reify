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
