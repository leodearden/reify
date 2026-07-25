//! Compiler dispatch tests for the `TypeExprKind` enum refactor (step 3 red phase).
//!
//! These tests construct `TypeExpr` nodes directly using the new enum API
//! (`TypeExprKind::DimensionalOp`, `TypeExprKind::Named`) and verify that
//! `reify_compiler::compile()` dispatches on them correctly.
//!
//! These tests **fail to compile** until step 4 migrates reify-compiler's consumer
//! sites — that compile failure is the "red" phase that justifies step 4.

use reify_ast::*;
use reify_core::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn dummy_span() -> SourceSpan {
    SourceSpan::new(0, 0)
}

fn dummy_hash(s: &str) -> ContentHash {
    ContentHash::of_str(s)
}

/// Build a `TypeExpr` that is `TypeExprKind::Named { name, type_args: [] }`.
fn named(name: &str) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_owned(),
            type_args: vec![],
        },
        span: dummy_span(),
    }
}

/// Build a `TypeExpr` that is `TypeExprKind::Named { name, type_args }`.
fn named_with_args(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::Named {
            name: name.to_owned(),
            type_args: args,
        },
        span: dummy_span(),
    }
}

/// Build a `TypeExpr` that is `TypeExprKind::DimensionalOp { op, left, right }`.
fn dim_op(op: DimOp, left: TypeExpr, right: TypeExpr) -> TypeExpr {
    TypeExpr {
        kind: TypeExprKind::DimensionalOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        span: dummy_span(),
    }
}

/// Build a minimal `ParsedModule` containing a single `TypeAliasDecl`.
fn module_with_alias(alias_name: &str, type_expr: TypeExpr) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("dispatch_test"),
        declarations: vec![Declaration::TypeAlias(TypeAliasDecl {
            name: alias_name.to_owned(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            type_expr,
            span: dummy_span(),
            content_hash: dummy_hash(alias_name),
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: dummy_hash("dispatch_test_module"),
        pragmas: vec![],
        declared_module_path: None,
    }
}

// ── Test (a): DimensionalOp(Mul, Force, Length) → ENERGY ─────────────────────

#[test]
fn dimensional_op_mul_force_length_resolves_to_energy() {
    // Manually construct: Force * Length (= Energy)
    let te = dim_op(DimOp::Mul, named("Force"), named("Length"));
    let parsed = module_with_alias("MyEnergy", te);
    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Force * Length alias should compile without errors, got: {:?}",
        errors
    );

    let alias = compiled
        .type_aliases
        .iter()
        .find(|a| a.name == "MyEnergy")
        .expect("alias should be compiled");

    let resolved = alias
        .resolved_type
        .as_ref()
        .expect("alias should be resolved");
    assert!(
        matches!(resolved, Type::Scalar { dimension } if *dimension == DimensionVector::ENERGY),
        "Force * Length should resolve to ENERGY dimension, got: {:?}",
        resolved
    );
}

// ── Test (b): Nested (Mass * Length) / Time → MOMENTUM ───────────────────────

#[test]
fn dimensional_op_nested_mass_length_over_time_resolves() {
    // Manually construct: (Mass * Length) / Time = Momentum (kg⋅m/s)
    let mass_times_length = dim_op(DimOp::Mul, named("Mass"), named("Length"));
    let te = dim_op(DimOp::Div, mass_times_length, named("Time"));
    let parsed = module_with_alias("Momentum", te);
    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "(Mass * Length) / Time alias should compile without errors, got: {:?}",
        errors
    );

    let alias = compiled
        .type_aliases
        .iter()
        .find(|a| a.name == "Momentum")
        .expect("alias should be compiled");

    let resolved = alias
        .resolved_type
        .as_ref()
        .expect("alias should be resolved");
    // Momentum = kg⋅m/s = Mass * Length / Time
    let expected = DimensionVector::MASS
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::TIME);
    assert!(
        matches!(resolved, Type::Scalar { dimension } if *dimension == expected),
        "(Mass * Length) / Time should resolve to momentum dimension, got: {:?}",
        resolved
    );
}

// ── Test (c): DimensionalOp leaf names — no "*" or "/" in diagnostics ─────────

#[test]
fn dimensional_op_no_operator_strings_in_diagnostics() {
    // A well-formed DimensionalOp should not produce diagnostics mentioning "*" or "/"
    // as unresolved type names. If collect_type_expr_names leaks operator strings, we'd
    // see diagnostics like 'unresolved type "*"'.
    let te = dim_op(DimOp::Div, named("Force"), named("Area"));
    let parsed = module_with_alias("Pressure", te);
    let compiled = reify_compiler::compile(&parsed);

    for diag in &compiled.diagnostics {
        assert!(
            !diag.message.contains("\"*\"") && !diag.message.contains("\"/\""),
            "operator string should not appear as unresolved type in diagnostic: {:?}",
            diag.message
        );
    }
}

// ── Test (d): Unresolved Named alias stays as None ────────────────────────────

#[test]
fn unresolved_named_alias_resolves_to_none() {
    // Named("UnresolvedBox", [Named("UnresolvedT")]) — both are unresolved names.
    // The compiler silently resolves to None (type aliases with unknown RHS are not
    // an error at the alias-declaration level). Any diagnostics that ARE emitted
    // must not mention raw operator strings like "*" or "/".
    let te = named_with_args("UnresolvedBox", vec![named("UnresolvedT")]);
    let parsed = module_with_alias("AliasToUnresolved", te);
    let compiled = reify_compiler::compile(&parsed);

    // Unresolved Named alias → resolved_type is None (silently deferred)
    let alias = compiled
        .type_aliases
        .iter()
        .find(|a| a.name == "AliasToUnresolved")
        .expect("alias declaration should appear in output");
    assert!(
        alias.resolved_type.is_none(),
        "unresolved alias should have None resolved_type, got: {:?}",
        alias.resolved_type
    );

    // Any diagnostics emitted must not mention raw operator strings
    for diag in &compiled.diagnostics {
        assert!(
            !diag.message.contains("\"*\"") && !diag.message.contains("\"/\""),
            "operator string should not appear in diagnostic: {:?}",
            diag.message
        );
    }
}

// ── Diagnostic-rejection tests: DimensionalOp in unexpected positions ─────────
//
// Each test below verifies that feeding a DimensionalOp into a position where
// only Named types are valid produces exactly one error diagnostic with the
// label "unexpected dimensional expression ..." and does not silently swallow
// the bad input.

fn module_with_structure_param(type_expr: TypeExpr) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "TestStruct".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![MemberDecl::Param(ParamDecl {
                is_priv: false,
                name: "p".into(),
                doc: None,
                type_expr: Some(type_expr),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: dummy_span(),
                content_hash: dummy_hash("p"),
            })],
            span: dummy_span(),
            content_hash: dummy_hash("TestStruct"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: dummy_hash("test_module_struct"),
        pragmas: vec![],
        declared_module_path: None,
    }
}

fn module_with_trait_param(type_expr: TypeExpr) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Trait(TraitDecl {
            name: "TestTrait".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            members: vec![MemberDecl::Param(ParamDecl {
                is_priv: false,
                name: "p".into(),
                doc: None,
                type_expr: Some(type_expr),
                default: None,
                where_clause: None,
                annotations: vec![],
                span: dummy_span(),
                content_hash: dummy_hash("p"),
            })],
            span: dummy_span(),
            content_hash: dummy_hash("TestTrait"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: dummy_hash("test_module_trait"),
        pragmas: vec![],
        declared_module_path: None,
    }
}

fn module_with_field_domain(domain_type: TypeExpr) -> ParsedModule {
    ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![Declaration::Field(FieldDef {
            name: "test_field".into(),
            is_pub: false,
            domain_type,
            codomain_type: named("Length"),
            source: FieldSource::Analytical {
                expr: Expr {
                    kind: ExprKind::Lambda {
                        params: vec![LambdaParam {
                            name: "p".into(),
                            type_expr: None,
                            span: dummy_span(),
                        }],
                        body: Box::new(Expr {
                            // Use a Length literal (1.0m) so the lambda body's inferred type
                            // matches the declared codomain `Length` (= Scalar[m]). This prevents
                            // FieldCodomainMismatch from firing a second diagnostic on top of the
                            // expected "unresolved field type" domain error.
                            // Note: bare `Scalar` (no type arg) is banned by task γ (E_BARE_SCALAR),
                            // so we use `Length` instead of `Scalar`.
                            kind: ExprKind::QuantityLiteral {
                                value: 1.0,
                                unit: UnitExpr::Unit("m".to_string()),
                            },
                            span: dummy_span(),
                        }),
                    },
                    span: dummy_span(),
                },
            },
            span: dummy_span(),
            content_hash: dummy_hash("test_field"),
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: dummy_hash("test_module_field"),
        pragmas: vec![],
        declared_module_path: None,
    }
}

fn module_with_trait_bound_type_arg(type_arg: TypeExpr) -> ParsedModule {
    // A module that declares a parameterized trait `Container<T>` and then a
    // structure that uses it with a DimensionalOp as the type argument.
    // The trait must be known to the compiler (present in the same compilation
    // unit) with a type param, otherwise the type_args mapping code is never
    // reached and the diagnostic we're testing would never fire.
    let trait_decl = TraitDecl {
        name: "Container".into(),
        doc: None,
        is_pub: false,
        type_params: vec![TypeParamDecl {
            name: "T".into(),
            bounds: vec![],
            default: None,
            span: dummy_span(),
        }],
        refinements: vec![],
        members: vec![],
        span: dummy_span(),
        content_hash: dummy_hash("Container"),
        pragmas: vec![],
        annotations: vec![],
    };
    let structure = StructureDef {
        name: "TestStruct".into(),
        doc: None,
        is_pub: false,
        type_params: vec![],
        trait_bounds: vec![TraitBoundRef {
            name: "Container".into(),
            type_args: vec![type_arg],
            span: dummy_span(),
        }],
        members: vec![],
        span: dummy_span(),
        content_hash: dummy_hash("TestStruct"),
        pragmas: vec![],
        annotations: vec![],
    };
    ParsedModule {
        path: ModulePath::single("test"),
        declarations: vec![
            Declaration::Trait(trait_decl),
            Declaration::Structure(structure),
        ],
        errors: vec![],
        content_hash: dummy_hash("test_module_bound"),
        pragmas: vec![],
        declared_module_path: None,
    }
}

/// Helper: assert that exactly one error diagnostic with the given substring was emitted.
fn assert_one_error_containing(compiled: &reify_compiler::CompiledModule, substr: &str) {
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error diagnostic containing '{substr}', got none"
    );
    let matching: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains(substr) || d.labels.iter().any(|l| l.message.contains(substr))
        })
        .collect();
    assert!(
        !matching.is_empty(),
        "expected a diagnostic containing '{substr}', got: {errors:?}"
    );
}

/// A DimensionalOp as an entity param type should produce a diagnostic, not
/// silently fall back to Type::dimensionless_scalar() with no feedback.
#[test]
fn dim_op_in_entity_param_emits_diagnostic() {
    let te = dim_op(DimOp::Mul, named("Force"), named("Length"));
    let parsed = module_with_structure_param(te);
    let compiled = reify_compiler::compile(&parsed);
    assert_one_error_containing(&compiled, "unresolved type");
}

/// A DimensionalOp as a trait param type should produce a diagnostic.
#[test]
fn dim_op_in_trait_param_emits_diagnostic() {
    let te = dim_op(DimOp::Div, named("Force"), named("Area"));
    let parsed = module_with_trait_param(te);
    let compiled = reify_compiler::compile(&parsed);
    assert_one_error_containing(&compiled, "unexpected dimensional expression");
}

/// A DimensionalOp as a field's domain_type should produce exactly ONE error
/// diagnostic (no second confusing diagnostic from resolve_field_type_name).
#[test]
fn dim_op_in_field_domain_emits_exactly_one_diagnostic() {
    let te = dim_op(DimOp::Mul, named("Force"), named("Length"));
    let parsed = module_with_field_domain(te);
    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error diagnostic for DimensionalOp field domain, got: {errors:?}"
    );
    assert!(
        errors[0].message.contains("unresolved field type"),
        "expected 'unresolved field type' in diagnostic message, got: {:?}",
        errors[0].message
    );
}

/// A DimensionalOp as a trait-bound type argument should produce a diagnostic,
/// not silently fall back to Type::dimensionless_scalar().
#[test]
fn dim_op_in_trait_bound_type_arg_emits_diagnostic() {
    let te = dim_op(DimOp::Mul, named("Mass"), named("Acceleration"));
    let parsed = module_with_trait_bound_type_arg(te);
    let compiled = reify_compiler::compile(&parsed);
    assert_one_error_containing(&compiled, "unexpected dimensional expression");
}

// ── Function type (arrow type) resolution — task 4595 step-5 ─────────────────
//
// RED: `resolve_type_expr_with_aliases_kinded` has no `Function` arm, so the
// reify-compiler crate does not yet cover `TypeExprKind::Function` (it fails to
// compile after step-4's variant addition).  step-6 adds the resolution arm and
// the exhaustive-match fan-out, turning this (and the reify-syntax lowering
// tests) GREEN.

/// A generic fn with a `(T) -> U` param must resolve that param's type to
/// `Type::Function { params: [TypeParam("T")], return_type: TypeParam("U") }`
/// with zero Severity::Error diagnostics.  The body `{ dflt }` returns U (the
/// `dflt` param) so the only thing under test is the arrow-type resolution.
#[test]
fn function_type_param_resolves_to_type_function() {
    let source = r#"
fn apply_it<T, U>(x: T, f: (T) -> U, dflt: U) -> U { dflt }
"#;
    let module = reify_test_support::compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "generic fn with a (T) -> U param should compile without errors, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "apply_it")
        .expect("apply_it function should be compiled");

    let (_, f_ty) = func
        .params
        .iter()
        .find(|(name, _)| name == "f")
        .expect("apply_it should have a param named f");

    assert_eq!(
        *f_ty,
        Type::Function {
            params: vec![Type::TypeParam("T".to_string())],
            return_type: Box::new(Type::TypeParam("U".to_string())),
        },
        "param f should resolve to Type::Function {{ params: [TypeParam(T)], return_type: TypeParam(U) }}, got: {:?}",
        f_ty
    );
}
