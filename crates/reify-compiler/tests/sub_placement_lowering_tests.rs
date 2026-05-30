//! Sub placement lowering tests (task 3900).
//!
//! Exercises that `at` pose clauses and `aux` modifiers on `sub` declarations
//! are correctly lowered into `SubComponentDecl.pose` /
//! `SubComponentDecl.is_aux` in the compiled IR.
//!
//! All tests use the `parse->compile->inspect` pattern against
//! `reify_test_support::compile_source_with_stdlib` — stdlib builtins
//! (transform3/orient_identity/vec3) must be resolvable by the compiler.

// ── Step 1: SubComponentDecl.pose / is_aux ───────────────────────────────────

/// `aux sub … at <pose>` lowers to `is_aux = true` and `pose = Some(…)`.
#[test]
fn aux_sub_lowers_pose_and_is_aux() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    aux sub jig : Child at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");

    let jig = parent
        .sub_components
        .iter()
        .find(|s| s.name == "jig")
        .expect("sub 'jig' not found in Parent.sub_components");

    assert!(
        jig.pose.is_some(),
        "expected jig.pose to be Some(…) after `at` lowering"
    );
    assert!(jig.is_aux, "expected jig.is_aux = true for `aux sub`");
}

/// A plain `sub` without `aux` or `at` lowers to `is_aux = false`, `pose = None`.
#[test]
fn plain_sub_has_no_pose_not_aux() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    sub plate : Child
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");

    let plate = parent
        .sub_components
        .iter()
        .find(|s| s.name == "plate")
        .expect("sub 'plate' not found in Parent.sub_components");

    assert!(
        plate.pose.is_none(),
        "expected plate.pose = None for plain sub"
    );
    assert!(
        !plate.is_aux,
        "expected plate.is_aux = false for plain sub"
    );
}

// ── Step 3: ValueCellDecl.is_aux and pub⊥aux orthogonality ──────────────────

/// `aux let` lowers to `is_aux = true` on the ValueCellDecl.
#[test]
fn aux_let_sets_is_aux() {
    let source = r#"structure S {
    aux let blank = 5mm
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let s = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    let blank = s
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "blank")
        .expect("let 'blank' not found in S.value_cells");

    assert!(blank.is_aux, "expected blank.is_aux = true for `aux let`");
}

/// `pub` and `aux` are orthogonal axes — all four (visibility × is_aux) combos
/// are independently representable in the IR.
#[test]
fn aux_and_pub_are_independent() {
    let source = r#"structure S {
    pub aux let a = 1mm
    aux let b = 1mm
    pub let c = 1mm
    let d = 1mm
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics: {:?}",
        errors
    );

    let s = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");

    let find_cell = |name: &str| {
        s.value_cells
            .iter()
            .find(|vc| vc.id.member == name)
            .unwrap_or_else(|| panic!("let '{}' not found in S.value_cells", name))
    };

    let a = find_cell("a");
    let b = find_cell("b");
    let c = find_cell("c");
    let d = find_cell("d");

    // pub aux: exported AND auxiliary
    assert_eq!(
        a.visibility,
        reify_compiler::Visibility::Public,
        "a should be Public"
    );
    assert!(a.is_aux, "a should be is_aux=true");

    // aux (no pub): private AND auxiliary
    assert_eq!(
        b.visibility,
        reify_compiler::Visibility::Private,
        "b should be Private"
    );
    assert!(b.is_aux, "b should be is_aux=true");

    // pub (no aux): exported, not auxiliary
    assert_eq!(
        c.visibility,
        reify_compiler::Visibility::Public,
        "c should be Public"
    );
    assert!(!c.is_aux, "c should be is_aux=false");

    // plain let: private, not auxiliary
    assert_eq!(
        d.visibility,
        reify_compiler::Visibility::Private,
        "d should be Private"
    );
    assert!(!d.is_aux, "d should be is_aux=false");
}

// ── Step 5: collection+`at` diagnostic and clean-compile guard ───────────────

/// `at` on a collection sub must produce at least one Error-severity diagnostic
/// with `DiagnosticCode::AtOnCollectionSub`, and the lowered `SubComponentDecl`
/// must have `pose == None` (the invalid pose is discarded, per PRD §10).
///
/// This is a runtime RED in step-5: the compiler does not yet reject this
/// combination, so no diagnostic is produced and the assertion fails.
#[test]
fn at_on_collection_sub_is_rejected() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param n: Int = 3
    sub bolts : List<Child> at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "`at` on a collection sub must produce at least one Error diagnostic; got zero. \
         Diagnostics: {:?}",
        compiled.diagnostics
    );

    // The error must carry the specific AtOnCollectionSub code so test assertions
    // don't pass on a spurious / unrelated error.
    assert!(
        errors
            .iter()
            .any(|d| d.code == Some(reify_core::DiagnosticCode::AtOnCollectionSub)),
        "expected at least one diagnostic with code AtOnCollectionSub; got: {:?}",
        errors
    );

    // The lowered SubComponentDecl must have pose == None: the compiler must
    // discard the invalid pose rather than propagating it into the IR.
    let parent = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");
    let bolts = parent
        .sub_components
        .iter()
        .find(|s| s.name == "bolts")
        .expect("sub 'bolts' not found in Parent.sub_components");
    assert!(
        bolts.pose.is_none(),
        "collection sub's pose must be discarded (None) when `at` is present; got Some(…)"
    );
}

/// A structure using `aux let`, a plain `sub … at <pose>`, and an `aux sub … at <pose>`
/// together must compile with ZERO Error-severity diagnostics — this pins the
/// "diagnostics accept at/aux cleanly" acceptance criterion.
#[test]
fn valid_at_and_aux_compile_clean() {
    let source = r#"structure Child {
    param h: Scalar = 10mm
}
structure Parent {
    param w: Scalar = 80mm
    aux let offset = 30mm
    sub plate : Child at transform3(orient_identity(), vec3(10mm, 0mm, 0mm))
    aux sub jig : Child at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
}"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "valid at/aux usage must produce zero Error diagnostics; got: {:?}",
        errors
    );
}

// ── Match-arm pose lowering (suggestion 3) ───────────────────────────────────

/// `sub … at <pose>` inside a `match`-arm decl group is lowered correctly:
/// the resulting `SubComponentDecl` carries `pose = Some(…)`.
///
/// Uses a hand-constructed `ParsedModule` (the tree-sitter grammar restricts
/// `match_arm_sub_decl` to bare `sub name : Type` with no `at` clause, so
/// source-string compilation cannot exercise this path directly).
#[test]
fn match_arm_sub_pose_is_lowered() {
    use reify_ast::{
        Declaration, EnumDecl, Expr, ExprKind, MatchArmDeclArmDecl, MatchArmDeclGroupDecl,
        MemberDecl, ParamDecl, ParsedModule, StructureDef, SubDecl, TypeExpr, TypeExprKind,
    };
    use reify_core::{ContentHash, ModulePath, SourceSpan};

    fn zero_span() -> SourceSpan {
        SourceSpan::new(0, 0)
    }

    fn ident_expr(name: &str) -> Expr {
        Expr {
            kind: ExprKind::Ident(name.to_string()),
            span: zero_span(),
        }
    }

    fn named_type(name: &str) -> TypeExpr {
        TypeExpr {
            kind: TypeExprKind::Named {
                name: name.to_string(),
                type_args: vec![],
            },
            span: zero_span(),
        }
    }

    fn empty_struct(name: &str) -> Declaration {
        Declaration::Structure(StructureDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![],
            span: zero_span(),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        })
    }

    // Build a SubDecl that carries a pose expression: `sub comp : ChildA at kind`
    // (using the discriminant param `kind` as the pose expression — T4 will
    // type-check it as Transform; T2 just needs to lower it to Some(CompiledExpr)).
    // Both variants must be covered (exhaustiveness check); both arms carry a pose.
    let make_arm_sub = |structure: &str| {
        MemberDecl::Sub(SubDecl {
            name: "comp".to_string(),
            structure_name: structure.to_string(),
            type_args: vec![],
            args: vec![],
            is_collection: false,
            where_clause: None,
            body: None,
            keyed_members: vec![],
            is_aux: false,
            pose_expr: Some(ident_expr("kind")),
            span: zero_span(),
            content_hash: ContentHash(0),
        })
    };

    let match_group = MemberDecl::MatchArmDeclGroup(MatchArmDeclGroupDecl {
        discriminant: ident_expr("kind"),
        arms: vec![
            MatchArmDeclArmDecl {
                patterns: vec!["A".to_string()],
                member: Box::new(make_arm_sub("ChildA")),
                span: zero_span(),
            },
            MatchArmDeclArmDecl {
                patterns: vec!["B".to_string()],
                member: Box::new(make_arm_sub("ChildA")),
                span: zero_span(),
            },
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
    });

    let parent = Declaration::Structure(StructureDef {
        name: "Parent".to_string(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![],
        members: vec![
            MemberDecl::Param(ParamDecl {
                name: "kind".to_string(),
                doc: None,
                type_expr: Some(named_type("ShapeKind")),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: zero_span(),
                content_hash: ContentHash(0),
            }),
            match_group,
        ],
        span: zero_span(),
        content_hash: ContentHash(0),
        pragmas: vec![],
        annotations: vec![],
    });

    let parsed = ParsedModule {
        path: ModulePath::single("test_match_arm_pose"),
        declarations: vec![
            Declaration::Enum(EnumDecl {
                name: "ShapeKind".to_string(),
                doc: None,
                is_pub: false,
                variants: vec!["A".to_string(), "B".to_string()],
                span: zero_span(),
                content_hash: ContentHash(0),
                annotations: vec![],
            }),
            empty_struct("ChildA"),
            parent,
        ],
        errors: vec![],
        content_hash: ContentHash(0),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&parsed);

    let parent_tpl = compiled
        .templates
        .iter()
        .find(|t| t.name == "Parent")
        .expect("Parent template not found");

    let comp = parent_tpl
        .sub_components
        .iter()
        .find(|s| s.name == "comp")
        .expect("sub 'comp' not found in Parent.sub_components after match-arm lowering");

    assert!(
        comp.pose.is_some(),
        "match-arm sub with `at <pose>` must lower to SubComponentDecl.pose = Some(…); got None"
    );
}
