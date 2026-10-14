use super::*;
use super::super::walk::entity_members;
use crate::decl_oracle::test_fixtures::{occurrences, span_of};
use reify_core::ModulePath;

// --- κ step-1 (task 4210): single-file structure-name collector ---

#[test]
fn collect_decl_name_spans_decl_plus_same_file_sub_uses() {
    // `SubDecl.structure_name` is a plain `String` field, structurally
    // invisible to the Expr-walking `collect_uses`/`collect_idents_in_expr`.
    // The dedicated structure-name collector must surface the home
    // declaration token (`structure Hole`) PLUS each same-file
    // `sub _ = Hole` construction-site token, ascending by start, each
    // covering exactly `Hole`.
    // NB: the bare `sub x = Hole` form (no parens) parses clean only as the
    // final member of a block; two consecutive bare subs are a syntax error,
    // so multi-sub fixtures use the parenthesized constructor form (which the
    // completeness fixtures above also rely on).
    let source = "\
structure Hole {
    param diameter: Length = 10mm
}
structure Assembly {
    sub a = Hole(diameter: 6mm)
    sub b = Hole(diameter: 8mm)
}";
    let parsed = reify_syntax::parse(source, ModulePath::single("kappa1"));
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse clean: {:?}",
        parsed.errors
    );

    // `Hole` appears exactly 3×: the structure decl + 2 sub construction sites
    // (`param diameter` / `Assembly` contain no `Hole` substring).
    let hole = occurrences(source, "Hole");
    assert_eq!(hole.len(), 3, "1 structure decl + 2 sub construction sites");
    // hole[0]=`structure Hole` decl token, hole[1]=`sub a = Hole`,
    // hole[2]=`sub b = Hole`.

    let spans = collect_decl_name_spans(source, &parsed, "Hole");
    assert_eq!(
        spans,
        vec![
            span_of(hole[0], "Hole"),
            span_of(hole[1], "Hole"),
            span_of(hole[2], "Hole"),
        ],
        "decl token + both sub-use tokens, ascending, each covering exactly `Hole`"
    );
}

/// Every collected span must cover EXACTLY the target name, and the collected
/// set must be every whole-word occurrence of it in the fixture.
///
/// Both halves matter. The set equality is the completeness half: each
/// fixture below is written so that every occurrence of the target is either
/// the declaration token or the one use under test, so a missed use is a
/// missing element. The exact-cover check is the RENAME-SAFETY half:
/// `compute_rename_cross_file` uses the reference set as its exact edit set,
/// so a span one byte too wide — `Box<Hole>` instead of `Hole`, or `pp.Hole`
/// instead of its last segment — is a DESTRUCTIVE edit, not a cosmetic one.
fn assert_collects_every_occurrence(label: &str, source: &str, name: &str) {
    let parsed = reify_syntax::parse(source, ModulePath::single("t"));
    assert!(
        parsed.errors.is_empty(),
        "[{label}] fixture must parse clean, got {:?} for:\n{source}",
        parsed.errors
    );
    let expected: Vec<SourceSpan> = occurrences(source, name)
        .into_iter()
        .map(|at| span_of(at, name))
        .collect();
    assert!(
        expected.len() >= 2,
        "[{label}] fixture must hold a declaration AND at least one use, or it \
         asserts nothing:\n{source}"
    );
    let got = collect_decl_name_spans(source, &parsed, name);
    assert_eq!(
        got, expected,
        "[{label}] every occurrence of {name:?} must be collected, each span \
         covering exactly the name token:\n{source}"
    );
}

/// AXIS 1 — every `TypeExpr` ROOT reachable from a top-level declaration.
///
/// Each row was probe-verified to parse clean, and each names the AST field
/// the use travels through. A root missing from `collect_decl_name_uses`'s
/// fan-out is a silently uncollected use site, which for an admitted kind is
/// a stale rename.
#[test]
fn type_position_use_is_collected_from_every_type_expr_root() {
    const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
    // (label + the AST field under test, source tail, target name)
    let rows: &[(&str, String, &str)] = &[
        ("FnDef.return_type", format!("{DECL}fn mk(x: Length) -> Hole {{ x }}"), "Hole"),
        ("FnParam.type_expr", format!("{DECL}fn id(h: Hole) -> Hole {{ h }}"), "Hole"),
        (
            "FieldDef.domain_type",
            format!("{DECL}field def f : Hole -> Real {{ source = analytical {{ |p| p }} }}"),
            "Hole",
        ),
        ("UnitDecl.dimension_type", format!("{DECL}unit hoop : Hole"), "Hole"),
        ("TypeAliasDecl.type_expr", format!("{DECL}type H = Hole"), "Hole"),
        ("DefaultDecl.type_expr", format!("{DECL}default Hole = 1"), "Hole"),
        (
            "JointDofField.type_expr",
            format!("{DECL}joint ball(c: Point, d: Point) with orientation: Hole = coincident(c, d)"),
            "Hole",
        ),
        ("TypeParamDecl.default", format!("{DECL}type Foo<T = Hole> = T"), "Hole"),
        ("AssociatedTypeDecl.default_type", format!("{DECL}trait T1 {{ type A = Hole }}"), "Hole"),
        ("VariantPayload::Named", format!("{DECL}enum E {{ V {{ h: Hole }} }}"), "Hole"),
        (
            "LambdaParam.type_expr",
            format!(
                "{DECL}field def f : Point3 -> Real {{ source = analytical {{ |p: Hole| 1.0 }} }}"
            ),
            "Hole",
        ),
        // The same root reached the other way — through a MEMBER's own
        // expression rather than a top-level declaration's — since the two
        // are separate legs of the fan-out and either could rot alone.
        (
            "LambdaParam.type_expr (member expression)",
            format!("{DECL}structure A {{\n    let f = |p: Hole| 1.0\n}}"),
            "Hole",
        ),
        // The two non-`TypeExpr` type references. `TraitDecl.refinements` is a
        // `Vec<SpannedIdent>` carrying exact per-name spans; `trait_bounds` is
        // a `Vec<TraitBoundRef>` whose `span` covers the whole bound and must
        // be narrowed.
        (
            "TraitDecl.refinements",
            "trait Physical { param mass : Mass }\ntrait Solid : Physical { param v : Volume }"
                .to_string(),
            "Physical",
        ),
        (
            "StructureDef.trait_bounds",
            "trait Physical { param mass : Mass }\nstructure S : Physical {\n    param mass : Mass = 1kg\n}"
                .to_string(),
            "Physical",
        ),
        (
            "OccurrenceDef.trait_bounds",
            "trait Physical { param mass : Mass }\noccurrence def O : Physical {\n    param mass : Mass = 1kg\n}"
                .to_string(),
            "Physical",
        ),
        // A third non-`TypeExpr` type reference, beyond the two the plan
        // named: a port's type is a TRAIT name, resolved against the trait
        // registry by the compiler (`entity.rs`'s port-type check), so a
        // trait rename that left it behind would break the port.
        (
            "PortDecl.type_name",
            "trait Flange { param d : Length }\nstructure A {\n    port mount : Flange { param d: Length = 5mm }\n}"
                .to_string(),
            "Flange",
        ),
    ];
    for (label, source, name) in rows {
        assert_collects_every_occurrence(label, source, name);
    }
}

/// AXIS 2 — every type-expression FORM, i.e. every shape the recursion and
/// the name-narrowing rule must handle.
///
/// `DimensionalOp` is exercised through a type alias rather than a `param`
/// annotation: `param p : Hole / Time` is a probe-verified SYNTAX ERROR (a
/// dimensional operator is not accepted in param-annotation position), so the
/// alias root is the only place the form is reachable.
///
/// The NAMESPACED row is the subtle one. `ts_parser`'s `namespaced_name_text`
/// dot-JOINS `pp.Hole` into the single `String` "pp.Hole", so a bare
/// declaration name never equals it and the use is silently dropped — while
/// the user sees a perfectly ordinary reference. The emitted span must be the
/// LAST SEGMENT alone, since that is the token a rename may rewrite.
#[test]
fn type_position_use_is_collected_for_every_type_expression_form() {
    const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
    let rows: &[(&str, String, &str)] = &[
        (
            "Named with type_args (Box<Hole> \u{2014} span covers the brackets, must narrow)",
            format!("{DECL}structure A {{\n    param p : Box<Hole>\n}}"),
            "Hole",
        ),
        (
            "DimensionalOp (Hole / Time)",
            format!("{DECL}type Q = Hole / Time"),
            "Hole",
        ),
        (
            "Function ((Hole) -> Real)",
            format!("{DECL}structure A {{\n    param p : (Hole) -> Real\n}}"),
            "Hole",
        ),
        (
            "QualifiedAssoc (Hole::Material)",
            format!("{DECL}structure A {{\n    param p : Hole::Material\n}}"),
            "Hole",
        ),
        (
            "namespaced (pp.Hole \u{2014} dot-joined into one String)",
            format!("{DECL}structure A {{\n    param p : pp.Hole\n}}"),
            "Hole",
        ),
        (
            "SubDecl.type_args (Wrap<Hole>)",
            format!("{DECL}structure A {{\n    sub s = Wrap<Hole>()\n}}"),
            "Hole",
        ),
        (
            "nested guarded scope",
            format!(
                "{DECL}structure A {{\n    param f : Bool = true\n    where f {{\n        param p : Hole\n    }}\n}}"
            ),
            "Hole",
        ),
    ];
    for (label, source, name) in rows {
        assert_collects_every_occurrence(label, source, name);
    }
}

/// AXIS 3 — the two child regions of a PURPOSE that no collector reaches.
///
/// `entity_members` admits Structure|Occurrence|Trait|Purpose, so a purpose's
/// `members` are walked — but a `PurposeDef` has THREE child regions, and the
/// other two are sibling vecs it never sees: `structures: Vec<StructureDef>`
/// (a structure lexically nested in the purpose body, kept out of `members`
/// by task 4639) and `defaults: Vec<DefaultDecl>` (kept out by task 4496,
/// each carrying a real `DefaultDecl.type_expr` type reference).
///
/// Every use below is therefore invisible today: a construction site, a type
/// annotation and an ambient default, none of them exotic. This is the
/// surface workstreams D and C share — a cross-file oracle that resolved a
/// purpose-nested name while these uses stayed uncollected would rename the
/// declaration and leave every one of them stale, which is precisely the
/// hazard the collectors-before-oracle rule exists to prevent.
#[test]
fn use_inside_a_purpose_body_is_collected() {
    const DECL: &str = "structure Hole {\n    param d: Length = 1mm\n}\n";
    let rows: &[(&str, String, &str)] = &[
        (
            "PurposeDef.structures — `sub s = Hole()` construction site",
            format!(
                "{DECL}purpose P() {{\n    structure def Nested {{\n        sub s = Hole()\n    }}\n}}"
            ),
            "Hole",
        ),
        (
            "PurposeDef.structures — `param p : Hole` type annotation",
            format!(
                "{DECL}purpose P() {{\n    structure def Nested {{\n        param p : Hole\n    }}\n}}"
            ),
            "Hole",
        ),
        (
            "PurposeDef.defaults — `default Hole = …`",
            format!("{DECL}purpose P() {{\n    default Hole = 1\n}}"),
            "Hole",
        ),
        // All three at once, in a purpose that also takes a param, so the
        // three regions are pinned as coexisting rather than only one at a
        // time — a fan-out that handled each alone but dropped one when the
        // others were present would still be caught.
        (
            "all three regions together",
            format!(
                "{DECL}purpose P(subject : Structure) {{\n    default Hole = 1\n    structure def Nested {{\n        param p : Hole\n        sub s = Hole()\n    }}\n}}"
            ),
            "Hole",
        ),
    ];
    for (label, source, name) in rows {
        assert_collects_every_occurrence(label, source, name);
    }
}

/// AXIS 4 — a CONSTRAINT-DEF instantiation, the third declaration-name use
/// site whose name is a plain `String` rather than an expression.
///
/// `ConstraintInstDecl { name: String, args, where_clause, span, … }`, and
/// `for_each_member_direct_expr` yields only its `args` and `where_clause`,
/// so the instantiation NAME is structurally invisible to every ident walk —
/// the exact same shape as `SubDecl.structure_name`, which is why this
/// traversal exists at all.
///
/// WHAT THIS ASSERTS AND WHAT IT DELIBERATELY DOES NOT. The subject here is
/// the COLLECTOR, so the use site is asserted exactly: present, and spanning
/// the name token alone rather than the whole member statement — the
/// rename-safety property, since `compute_rename_cross_file` uses the
/// reference set as its exact edit set. The DECLARATION token is the
/// ORACLE's to supply, so the declaration half is asserted as an
/// IMPLICATION — if the oracle offers a home token, the set carries it.
/// Written that way when `decl_name_span_in` still refused `Constraint`, so
/// it was VACUOUSLY true; step-18 admitted the kind, and the implication
/// became a real assertion with no edit — which is the point of phrasing a
/// coupling in the direction that lets the gap be closed.
///
/// Non-vacuity: the fixture is asserted to parse clean AND to really contain
/// a `MemberDecl::ConstraintInst` named `Foo`, so grammar drift fails loudly
/// here instead of turning the whole test into a pass over an empty AST.
#[test]
fn constraint_def_instantiation_is_collected_as_a_use_site() {
    const DECL: &str = "constraint def Foo {\n    param x : Length\n    x > 0mm\n}\n";
    let rows: &[(&str, String)] = &[
        (
            "instantiated in a structure member",
            format!(
                "{DECL}structure A {{\n    param w : Length = 1mm\n    constraint Foo(x: w)\n}}"
            ),
        ),
        // Also through the purpose regions step-14 opened, so the two legs
        // are pinned as composing rather than each working alone.
        //
        // It must be a purpose-NESTED STRUCTURE, not the purpose body
        // itself: measured, `constraint Foo(x: 1mm)` written directly in a
        // purpose body lowers to `MemberDecl::Constraint` wrapping an
        // `ExprKind::FunctionCall`, NOT to a `ConstraintInst` — the
        // instantiation form is a structure-body form. A `FunctionCall`'s
        // callee is a `String` with no span of its own, so that spelling is
        // a separate, pre-existing use-site gap (filed as a follow-up), not
        // this arm's business.
        (
            "instantiated in a purpose-nested structure",
            format!(
                "{DECL}purpose P(subject : Structure) {{\n    structure def Nested {{\n        constraint Foo(x: 1mm)\n    }}\n}}"
            ),
        ),
    ];
    for (label, source) in rows {
        let parsed = reify_syntax::parse(source, ModulePath::single("t"));
        assert!(
            parsed.errors.is_empty(),
            "[{label}] fixture must parse clean, got {:?} for:\n{source}",
            parsed.errors
        );
        let instantiates_foo = |members: &[MemberDecl]| {
            members
                .iter()
                .any(|m| matches!(m, MemberDecl::ConstraintInst(c) if c.name == "Foo"))
        };
        let found_inst = parsed.declarations.iter().any(|decl| {
            entity_members(decl).is_some_and(instantiates_foo)
                || matches!(decl, Declaration::Purpose(p)
                    if p.structures.iter().any(|n| instantiates_foo(&n.members)))
        });
        assert!(
            found_inst,
            "[{label}] fixture must really contain a `constraint Foo(…)` \
             instantiation member, or this test asserts nothing:\n{source}"
        );

        let use_at = *occurrences(source, "Foo")
            .last()
            .expect("fixture mentions Foo");
        let got = collect_decl_name_spans(source, &parsed, "Foo");
        assert!(
            got.contains(&span_of(use_at, "Foo")),
            "[{label}] the instantiation name token at {use_at} must be \
             collected, got {got:?}:\n{source}"
        );
        // Rename safety: the whole member statement is `constraint Foo(x: w)`,
        // so a span wider than the name is a destructive edit set.
        for span in &got {
            assert_eq!(
                &source[span.start as usize..span.end as usize],
                "Foo",
                "[{label}] every collected span must cover exactly the name \
                 token, got {span:?}:\n{source}"
            );
        }
        // The declaration half, as an implication over the oracle.
        if let Some(home) = crate::goto_def::find_declaration_name_span(source, "Foo") {
            assert!(
                got.contains(&home),
                "[{label}] once the oracle offers a home token it must be in \
                 the reference set, got {got:?}:\n{source}"
            );
        }
    }
}
