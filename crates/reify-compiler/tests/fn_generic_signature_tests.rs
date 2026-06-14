//! Generic-function signature tests — task 4230 (generic-user-fns α).
//!
//! Verifies that:
//!   (step-1/step-2) `CompiledFunction.type_params` is correctly lowered from
//!     `fn_def.type_params` via `convert_type_params`, and is empty for non-generic fns.
//!   (step-3/step-4) Bare type-param names (`T`) resolve to `Type::TypeParam("T")` in
//!     fn parameter and return-type positions.
//!   (step-5/step-6) Type-param names resolve inside parameterised builtin types
//!     (`Field<D, C>`, `List<T>`).
//!   (step-7/step-8) An undeclared name in a generic-fn signature emits
//!     `DiagnosticCode::FnUnknownTypeParam`; a non-generic fn's unknown type still emits
//!     `DiagnosticCode::UnresolvedType` (INV-6 regression pin).
//!
//! All tests use `compile_source` (no stdlib) and minimal bodies (`{ value }`, `{ x }`)
//! that reference a param. `compile_function` does NOT type-check the body against the
//! declared return type, so trivial bodies produce no diagnostics and need no stdlib symbol.

use reify_test_support::compile_source;
use reify_core::{DiagnosticCode, DimensionVector, Severity, Type};

// ────────────────────────────────────────────────────────────────────────────
// Step-1 / Step-2: CompiledFunction.type_params lowering
// ────────────────────────────────────────────────────────────────────────────

/// Generic fn's `type_params` is lowered from the declared `<D, C>` type-param list;
/// non-generic fn's `type_params` is empty (INV-6).
///
/// RED until step-2: `compile_function` stubs `type_params: Vec::new()`, so
/// `constant_field.type_params` is empty instead of ["D", "C"].
#[test]
fn generic_fn_lowers_type_params_and_nongeneric_is_empty() {
    let source = r#"
        fn constant_field<D, C>(value: C) -> Field<D, C> { value }
        fn plain(x: Real) -> Real { x }
    "#;
    let module = compile_source(source);

    // constant_field must be present (compile_function returns Some even with
    // unresolved signature types, falling back to Type::dimensionless_scalar()).
    let cf = module
        .functions
        .iter()
        .find(|f| f.name == "constant_field")
        .expect("function 'constant_field' should be compiled");

    // type_params should be lowered from <D, C>.
    assert_eq!(
        cf.type_params.len(),
        2,
        "constant_field should have 2 type params, got {:?}",
        cf.type_params.iter().map(|tp| &tp.name).collect::<Vec<_>>()
    );
    assert_eq!(cf.type_params[0].name, "D");
    assert_eq!(cf.type_params[1].name, "C");
    // No bounds, no default for simple type params.
    assert!(
        cf.type_params[0].bounds.is_empty(),
        "D should have no bounds"
    );
    assert!(
        cf.type_params[1].bounds.is_empty(),
        "C should have no bounds"
    );
    assert!(
        cf.type_params[0].default.is_none(),
        "D should have no default"
    );
    assert!(
        cf.type_params[1].default.is_none(),
        "C should have no default"
    );

    // Non-generic fn must have empty type_params (INV-6).
    let plain = module
        .functions
        .iter()
        .find(|f| f.name == "plain")
        .expect("function 'plain' should be compiled");
    assert!(
        plain.type_params.is_empty(),
        "non-generic fn 'plain' should have empty type_params, got {:?}",
        plain.type_params
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-3 / Step-4: bare type-param name resolves in param and return position
// ────────────────────────────────────────────────────────────────────────────

/// Bare type-param name `T` resolves to `Type::TypeParam("T")` in both the
/// parameter and the return-type positions of a generic fn, with zero Error
/// diagnostics.
///
/// RED until step-4: `compile_function` passes `empty_params` to
/// `resolve_type_expr_with_aliases`, so `T` is unknown → "unresolved type" Error +
/// `Type::dimensionless_scalar()` fallback.
#[test]
fn bare_type_param_resolves_in_param_and_return() {
    let source = r#"fn id<T>(x: T) -> T { x }"#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for fn id<T>, got: {:?}",
        errors
    );

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "id")
        .expect("function 'id' should be compiled");

    assert_eq!(
        func.params[0].1,
        Type::TypeParam("T".to_string()),
        "param x should resolve to Type::TypeParam(\"T\")"
    );
    assert_eq!(
        func.return_type,
        Type::TypeParam("T".to_string()),
        "return type should resolve to Type::TypeParam(\"T\")"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5 / Step-6: type-param names resolve inside parameterised builtin types
// ────────────────────────────────────────────────────────────────────────────

/// Type-param names resolve correctly when used as type arguments to parameterised
/// builtins (`Field<D, C>`, `List<T>`).
///
/// RED until step-6: `resolve_parameterized_builtin_type` uses its own internal
/// `empty_type_params` for all inner `resolve_type_expr_with_aliases` calls, so D/C/T
/// remain unresolved → the outer Field/List returns None → "unresolved type" Error +
/// `Type::dimensionless_scalar()` fallback.
#[test]
fn type_param_resolves_inside_parameterized_builtin() {
    let source = r#"
        fn constant_field<D, C>(value: C) -> Field<D, C> { value }
        fn single<T>(x: T) -> List<T> { x }
    "#;
    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    let cf = module
        .functions
        .iter()
        .find(|f| f.name == "constant_field")
        .expect("function 'constant_field' should be compiled");

    // param: C
    assert_eq!(
        cf.params[0].1,
        Type::TypeParam("C".to_string()),
        "constant_field param 'value' should resolve to Type::TypeParam(\"C\")"
    );

    // return type: Field<D, C>
    assert_eq!(
        cf.return_type,
        Type::Field {
            domain: Box::new(Type::TypeParam("D".to_string())),
            codomain: Box::new(Type::TypeParam("C".to_string())),
        },
        "constant_field return type should be Field<TypeParam(D), TypeParam(C)>"
    );

    let single = module
        .functions
        .iter()
        .find(|f| f.name == "single")
        .expect("function 'single' should be compiled");

    // return type: List<T>
    assert_eq!(
        single.return_type,
        Type::List(Box::new(Type::TypeParam("T".to_string()))),
        "single return type should be List<TypeParam(T)>"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-7 / Step-8: FnUnknownTypeParam diagnostic (INV-6 regression pin)
// ────────────────────────────────────────────────────────────────────────────

/// A generic fn with an undeclared name in its signature (`U` not in `<T>`)
/// must emit a diagnostic with `code == Some(DiagnosticCode::FnUnknownTypeParam)`,
/// and the message must mention the undeclared name.
///
/// RED until step-8: `DiagnosticCode::FnUnknownTypeParam` doesn't exist yet;
/// the generic case currently emits `DiagnosticCode::UnresolvedType`.
#[test]
fn generic_fn_undeclared_signature_param_emits_fn_unknown_type_param() {
    // `U` is not declared in `<T>` — an undeclared type-param name.
    let source = r#"fn f<T>(x: U) -> T { x }"#;
    let module = compile_source(source);

    let fn_unknown_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnUnknownTypeParam));
    assert!(
        fn_unknown_diag.is_some(),
        "expected a diagnostic with code FnUnknownTypeParam for undeclared type param 'U'; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let diag = fn_unknown_diag.unwrap();
    // Assert on the quoted form so the check is pinned to the intended diagnostic
    // shape ("type 'U' in the signature of generic function 'f' …") rather than
    // matching any coincidental uppercase U in the message.
    assert!(
        diag.message.contains("'U'"),
        "FnUnknownTypeParam diagnostic message should mention \"'U'\" (quoted), got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("'f'"),
        "FnUnknownTypeParam diagnostic message should name the generic function 'f', got: {:?}",
        diag.message
    );
}

/// A non-generic fn with an unknown type keeps `DiagnosticCode::UnresolvedType`
/// and its message unchanged (INV-6 regression pin).
///
/// RED until step-8: only meaningful once FnUnknownTypeParam exists; ensures
/// we didn't accidentally change non-generic fn behavior.
#[test]
fn nongeneric_unknown_type_keeps_unresolved_type() {
    let source = r#"fn g(x: NoSuchType) -> Real { x }"#;
    let module = compile_source(source);

    let unresolved_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::UnresolvedType) && d.severity == Severity::Error);
    assert!(
        unresolved_diag.is_some(),
        "expected a diagnostic with code UnresolvedType for non-generic fn with unknown type; \
         got diagnostics: {:?}",
        module.diagnostics
    );

    let diag = unresolved_diag.unwrap();
    assert!(
        diag.message.contains("unresolved type"),
        "UnresolvedType message should contain 'unresolved type', got: {:?}",
        diag.message
    );
    assert!(
        diag.message.contains("NoSuchType"),
        "UnresolvedType message should mention 'NoSuchType', got: {:?}",
        diag.message
    );

    // Must NOT emit FnUnknownTypeParam for a non-generic fn.
    let fn_unknown_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FnUnknownTypeParam));
    assert!(
        fn_unknown_diag.is_none(),
        "non-generic fn must not emit FnUnknownTypeParam, got: {:?}",
        fn_unknown_diag
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-3 / Step-4 (task 4234 ε): dimension-kinded params — B10 happy path
// ────────────────────────────────────────────────────────────────────────────

/// `fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>` — B10 happy path.
///
/// RED until step-4: today `Scalar<Q>` routes to `resolve_type_alias_expr_to_dimension`,
/// which can't resolve `Q` and emits an Error diagnostic.
///
/// Pinned back-compat: `fn area(w: Scalar<Length>) -> Scalar<Length>` must still
/// resolve to the concrete `Type::Scalar{dimension: DimensionVector::LENGTH}` (INV-10).
#[test]
fn dim_kinded_param_scalar_q_resolves_to_scalar_param() {
    // B10 happy path — Q: Dimension bound
    let source = r#"
        fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q> { x }
        fn area(w: Scalar<Length>) -> Scalar<Length> { w }
    "#;
    let module = compile_source(source);

    // (i) Zero Error-severity diagnostics
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for dim-kinded fn g<Q: Dimension>, got: {:?}",
        errors
    );

    let gf = module
        .functions
        .iter()
        .find(|f| f.name == "g")
        .expect("function 'g' should be compiled");

    // (ii) param resolves to ScalarParam("Q")
    assert_eq!(
        gf.params[0].1,
        Type::ScalarParam("Q".to_string()),
        "param x should resolve to Type::ScalarParam(\"Q\")"
    );

    // (iii) return type resolves to ScalarParam("Q")
    assert_eq!(
        gf.return_type,
        Type::ScalarParam("Q".to_string()),
        "return type should resolve to Type::ScalarParam(\"Q\")"
    );

    // (iv) type_params lowers the bound — Q with bound Dimension
    assert_eq!(gf.type_params.len(), 1, "g should have 1 type param");
    assert_eq!(gf.type_params[0].name, "Q");
    assert_eq!(
        gf.type_params[0].bounds.len(),
        1,
        "Q should have 1 bound (Dimension)"
    );
    assert_eq!(
        gf.type_params[0].bounds[0].trait_ref.name,
        "Dimension",
        "Q's bound should be 'Dimension'"
    );

    // Back-compat (INV-10): concrete Scalar<Length> is unaffected
    let area = module
        .functions
        .iter()
        .find(|f| f.name == "area")
        .expect("function 'area' should be compiled");
    assert_eq!(
        area.params[0].1,
        Type::Scalar { dimension: DimensionVector::LENGTH },
        "area param 'w' should still resolve to concrete Scalar[LENGTH]"
    );
    assert_eq!(
        area.return_type,
        Type::Scalar { dimension: DimensionVector::LENGTH },
        "area return type should still resolve to concrete Scalar[LENGTH]"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-7 / Step-8 (ε): kind-misuse case #1 — non-dim-kinded param in dim slot
// ────────────────────────────────────────────────────────────────────────────

/// A non-dimension-kinded type parameter used in a dimension slot (`Scalar<T>`)
/// emits exactly one `DimParamKind` Error, with no competing
/// `FnUnknownTypeParam`/`UnresolvedType` Error (single root-cause diagnostic).
///
/// RED until step-8: `DiagnosticCode::DimParamKind` does not exist yet (compile-
/// error RED); today `Scalar<T>` also emits a generic dimension-resolve Error
/// instead of a single DimParamKind.
#[test]
fn non_dim_kinded_param_in_scalar_slot_emits_dim_param_kind() {
    // T has no `: Dimension` bound — it is a plain type param.
    // `-> Real` still resolves to dimensionless_scalar() post-4373.
    let source = r#"
        fn h<T>(x: Scalar<T>) -> Real { 1.0 }
    "#;
    let module = compile_source(source);

    let dim_kind_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::DimParamKind))
        .collect();
    assert_eq!(
        dim_kind_errors.len(),
        1,
        "expected exactly one DimParamKind Error, got: {:?}",
        module.diagnostics
    );

    // No competing FnUnknownTypeParam or UnresolvedType errors — single root-cause.
    let competing: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && (d.code == Some(DiagnosticCode::FnUnknownTypeParam)
                    || d.code == Some(DiagnosticCode::UnresolvedType))
        })
        .collect();
    assert!(
        competing.is_empty(),
        "expected no FnUnknownTypeParam/UnresolvedType errors alongside DimParamKind, got: {:?}",
        competing
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-9 / Step-10 (ε): kind-misuse case #2 — dim-kinded param as ordinary type
// ────────────────────────────────────────────────────────────────────────────

/// A dimension-kinded type parameter used as an ordinary type (bare `Q` in a
/// non-dimension position) emits exactly one `DimParamKind` Error, with no
/// competing `FnUnknownTypeParam`/`UnresolvedType` Error.
///
/// RED until step-10: today `Q ∈ type_param_names` so bare `Q` resolves to
/// `Type::TypeParam("Q")` via `resolve_type_with_aliases` with no diagnostic.
#[test]
fn dim_kinded_param_used_as_ordinary_type_emits_dim_param_kind() {
    // Q: Dimension — but x: Q is a bare usage in an ordinary type position.
    // `-> Real` surface syntax still resolves to dimensionless_scalar() post-4373.
    let source = r#"
        fn k<Q: Dimension>(x: Q) -> Real { 1.0 }
    "#;
    let module = compile_source(source);

    let dim_kind_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::DimParamKind))
        .collect();
    assert_eq!(
        dim_kind_errors.len(),
        1,
        "expected exactly one DimParamKind Error for bare Q in ordinary type position, got: {:?}",
        module.diagnostics
    );

    // No competing FnUnknownTypeParam or UnresolvedType errors — single root-cause.
    let competing: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && (d.code == Some(DiagnosticCode::FnUnknownTypeParam)
                    || d.code == Some(DiagnosticCode::UnresolvedType))
        })
        .collect();
    assert!(
        competing.is_empty(),
        "expected no FnUnknownTypeParam/UnresolvedType errors alongside DimParamKind, got: {:?}",
        competing
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5 / Step-6 (ε): Vector3<Q> and Point3<Q> with a dimension-kinded param
// ────────────────────────────────────────────────────────────────────────────

/// Vector3<Q> and Point3<Q> with Q: Dimension resolve their quantity slot to
/// the dim-param representation (B10 extension).
///
/// RED until step-6: only the `Scalar` arm is wired in step-4; `Vector3` and
/// `Point3` arms still route to `resolve_type_alias_expr_to_dimension`, which
/// fails on `Q` and emits an Error diagnostic.
#[test]
fn dim_kinded_vector3_and_point3_resolve_to_scalar_param_slot() {
    let source = r#"
        fn gv<Q: Dimension>(v: Vector3<Q>) -> Vector3<Q> { v }
        fn gp<Q: Dimension>(p: Point3<Q>) -> Point3<Q> { p }
    "#;
    let module = compile_source(source);

    // ── gv: Vector3<Q> ───────────────────────────────────────────────────────

    let errors_gv: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors_gv.is_empty(),
        "expected no Error diagnostics for gv<Q: Dimension>(v: Vector3<Q>) -> Vector3<Q>, got: {:?}",
        errors_gv
    );

    let gv = module
        .functions
        .iter()
        .find(|f| f.name == "gv")
        .expect("function 'gv' should be compiled");

    assert_eq!(
        gv.params[0].1,
        Type::vec3(Type::ScalarParam("Q".to_string())),
        "gv param v should resolve to Type::vec3(ScalarParam(\"Q\"))"
    );
    assert_eq!(
        gv.return_type,
        Type::vec3(Type::ScalarParam("Q".to_string())),
        "gv return type should resolve to Type::vec3(ScalarParam(\"Q\"))"
    );

    // ── gp: Point3<Q> ────────────────────────────────────────────────────────

    let gp = module
        .functions
        .iter()
        .find(|f| f.name == "gp")
        .expect("function 'gp' should be compiled");

    assert_eq!(
        gp.params[0].1,
        Type::point3(Type::ScalarParam("Q".to_string())),
        "gp param p should resolve to Type::point3(ScalarParam(\"Q\"))"
    );
    assert_eq!(
        gp.return_type,
        Type::point3(Type::ScalarParam("Q".to_string())),
        "gp return type should resolve to Type::point3(ScalarParam(\"Q\"))"
    );
}
