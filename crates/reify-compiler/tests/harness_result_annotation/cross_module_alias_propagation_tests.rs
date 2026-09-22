//! Acceptance tests for cross-module type-alias propagation through PreludeContext.
//!
//! TDD structure (task 2750):
//!   step-3: headline acceptance tests (pub prelude alias resolves in user module)
//!   step-5: user-alias shadowing tests
//!   step-7: exclusion tests (#no_prelude, non-pub, parametric skip)
//!   step-9: stdlib safety-net
//!
//! task 4792 amendments:
//!   parametric_prelude_dimensional_alias_resolves_cross_module — headline cross-module test
//!   (flipped 2777/skip tests now assert resolution success instead of Info/skip)

use reify_compiler::{
    CompiledTypeAlias, compile_with_prelude, compile_with_stdlib, parse_with_stdlib,
};
use reify_core::{ContentHash, DimensionVector, ModulePath, Severity, SourceSpan, Type};
use reify_ir::TypeParam;
use reify_test_support::CompiledModuleBuilder;

fn make_pub_alias(name: &str, resolved_type: Type) -> CompiledTypeAlias {
    CompiledTypeAlias {
        name: name.to_string(),
        resolved_type: Some(resolved_type),
        type_params: vec![],
        type_expr: None,
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(name),
    }
}

fn error_count(module: &reify_compiler::CompiledModule) -> usize {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

fn make_alias_with_pub(name: &str, resolved_type: Type, is_pub: bool) -> CompiledTypeAlias {
    CompiledTypeAlias {
        name: name.to_string(),
        resolved_type: Some(resolved_type),
        type_params: vec![],
        type_expr: None,
        is_pub,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(name),
    }
}

// ─── step-3: headline acceptance tests ────────────────────────────────────

/// A `pub type Stress = Pressure` alias in a prelude module must be visible
/// in a user module's param type annotation without any in-module alias decl.
///
/// This is the first of the two "dropped subtests" from the task 2696 plan
/// that 2750 re-enables — now backed by the actual prelude-alias-seeding
/// infrastructure.
#[test]
fn pub_prelude_alias_resolves_in_user_module() {
    let stress = make_pub_alias(
        "Stress",
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
    );
    let prelude_a = CompiledModuleBuilder::new(ModulePath::single("synth_analysis"))
        .type_alias(stress)
        .build();

    let source = "structure def Beam { param yield : Stress }";
    let parsed = reify_syntax::parse(source, ModulePath::single("user_beam"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_a]);

    assert_eq!(
        error_count(&compiled),
        0,
        "compile must produce zero Error diagnostics; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("template `Beam` not found");

    let yield_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "yield")
        .expect("value cell `yield` not found on `Beam`");

    assert_eq!(
        yield_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
        "param `yield : Stress` must resolve to Type::Scalar(PRESSURE)"
    );
}

/// A `pub type Strain = Dimensionless` alias in a prelude module must resolve
/// to `Type::Scalar(DIMENSIONLESS)` in a user module.
///
/// This is the second of the two "dropped subtests" re-enabled by task 2750.
#[test]
fn pub_prelude_alias_strain_resolves_to_dimensionless() {
    let strain = make_pub_alias(
        "Strain",
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        },
    );
    let prelude_b = CompiledModuleBuilder::new(ModulePath::single("synth_analysis2"))
        .type_alias(strain)
        .build();

    let source = "structure def Bar { param elongation : Strain }";
    let parsed = reify_syntax::parse(source, ModulePath::single("user_bar"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_b]);

    assert_eq!(
        error_count(&compiled),
        0,
        "compile must produce zero Error diagnostics; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bar")
        .expect("template `Bar` not found");

    let elong_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "elongation")
        .expect("value cell `elongation` not found on `Bar`");

    assert_eq!(
        elong_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        },
        "param `elongation : Strain` must resolve to Type::Scalar(DIMENSIONLESS)"
    );
}

// ─── step-5: user-alias shadowing tests ───────────────────────────────────

/// A user-module alias with the same name as a prelude alias must shadow the
/// prelude alias — the user's type wins, and NO "duplicate type alias" Error
/// diagnostic must be produced for the collision.
#[test]
fn user_alias_shadows_prelude_without_diagnostic() {
    // Prelude declares pub type Foo = Length
    let prelude_alias = make_alias_with_pub(
        "Foo",
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        true,
    );
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("shadow_prelude"))
        .type_alias(prelude_alias)
        .build();

    // User module declares `type Foo = Mass` — must shadow the prelude's Length.
    let source = "type Foo = Mass\nstructure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("shadow_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // (a) No Error diagnostics — no duplicate-alias error.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "shadowing must not produce Error diagnostics; got: {:?}",
        errors
    );

    // (b) param p resolves to MASS (user's alias), not LENGTH (prelude's).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::MASS
        },
        "param `p : Foo` must resolve to MASS (user alias wins over prelude's LENGTH)"
    );
}

/// When the user module does NOT declare its own alias for a name that appears
/// in the prelude, the prelude alias must be visible.
#[test]
fn prelude_alias_visible_when_user_does_not_shadow() {
    // Prelude declares pub type Foo = Length
    let prelude_alias = make_alias_with_pub(
        "Foo",
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        true,
    );
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("visible_prelude"))
        .type_alias(prelude_alias)
        .build();

    // User module does NOT declare type Foo — must pick it up from prelude.
    let source = "structure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("visible_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    assert_eq!(
        error_count(&compiled),
        0,
        "prelude alias must be visible; no Error diagnostics expected, got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "param `p : Foo` must resolve to LENGTH from prelude alias"
    );
}

// ─── step-7: exclusion tests ───────────────────────────────────────────────

/// A non-pub (`is_pub: false`) prelude alias must NOT be visible in user modules.
/// The user-module param annotation referencing it must produce an unresolved-type Error.
#[test]
fn non_pub_prelude_alias_invisible_in_user_module() {
    // Prelude has a non-pub alias: type Bar = Length (is_pub: false)
    let non_pub_alias = CompiledTypeAlias {
        name: "Bar".to_string(),
        resolved_type: Some(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        type_params: vec![],
        type_expr: None,
        is_pub: false, // NOT exported
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Bar"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("nonpub_prelude"))
        .type_alias(non_pub_alias)
        .build();

    let source = "structure def S { param p : Bar }";
    let parsed = reify_syntax::parse(source, ModulePath::single("nonpub_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-pub prelude alias 'Bar' must NOT be visible; expected ≥1 Error diagnostic"
    );
}

/// The `#no_prelude` pragma must suppress prelude-alias seeding, just as it
/// suppresses units, enums, traits, and functions.
#[test]
fn no_prelude_pragma_suppresses_alias_seeding() {
    let pub_alias = make_pub_alias(
        "Foo",
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("nop_prelude"))
        .type_alias(pub_alias)
        .build();

    // #no_prelude + reference to prelude alias → must be unresolved
    let source = "#no_prelude\nstructure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("nop_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "#no_prelude must suppress alias seeding; expected ≥1 Error diagnostic for 'Foo'"
    );
}

/// After parametric prelude aliases are un-skipped (task 4792), a user module
/// that references `Vec<Real>` against a seeded `pub type Vec<T>` (body: T)
/// resolves to the body type (Real = dimensionless scalar) with zero Error
/// diagnostics.
///
/// Flipped from the task-2750 "parametric skipped with no panic" test: the
/// alias is no longer skipped; `Vec<Real>` resolves to the body type.
///
/// RED on base: Vec is still skipped → unresolved-type Error.
#[test]
fn parametric_pub_prelude_alias_resolves_via_body_no_panic() {
    let parametric_alias = CompiledTypeAlias {
        name: "Vec".to_string(),
        resolved_type: None,
        type_params: vec![TypeParam {
            name: "T".to_string(),
            bounds: vec![],
            default: None,
        }],
        type_expr: Some(reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "T".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::new(0, 0),
        }),
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Vec_T"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_prelude"))
        .type_alias(parametric_alias)
        .build();

    // Vec<Real>: seeded parametric alias Vec<T>=T, instantiated with T=Real.
    // After un-skip, resolves to Real = dimensionless scalar with 0 Error.
    let source = "structure def S { param p : Vec<Real> }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse must succeed for Vec<Real> reference: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Zero Error diagnostics — Vec<Real> resolves to Real (dimensionless scalar).
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Vec<Real> must resolve without Error after parametric alias un-skip; got: {:?}",
        errors
    );

    // p resolves to Real = dimensionless scalar.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::dimensionless_scalar(),
        "param `p : Vec<Real>` must resolve to Type::dimensionless_scalar() via Vec<T>=T, T=Real"
    );
}

// ─── task 4792 step-5: real stdlib prelude resolution ─────────────────────────

/// A user module that references `Rate<Length>` (not declared in user source)
/// must compile cleanly against the real stdlib prelude — `Rate` lives in
/// units.ri, seeded into the user module's alias registry by `compile_with_stdlib`.
///
/// `Rate<Length>` → body `Q / Time` with Q=Length → LENGTH / TIME = VELOCITY.
///
/// This is the precise, harness-free counterpart to the committed .ri signal
/// (S7/S8); it also guards that adding `Rate` to units.ri keeps the stdlib
/// build clean (the `signal_2_real_stdlib_compiles_clean_and_order_is_stable`
/// test in stdlib_topo.rs auto-covers that).
///
/// RED on base: `Rate` is not yet declared in units.ri → unresolved-type Error.
#[test]
fn rate_alias_resolves_via_real_stdlib_prelude() {
    let source = "structure def S { param v : Rate<Length> }";
    let parsed = parse_with_stdlib(source, ModulePath::single("rate_stdlib_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    // Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Rate<Length> via real stdlib prelude must compile without Error; got: {:?}",
        errors
    );

    // `v` resolves to Type::Scalar { dimension: VELOCITY } (Length / Time).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let v_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "v")
        .expect("value cell `v` not found on `S`");
    assert_eq!(
        v_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::VELOCITY,
        },
        "param `v : Rate<Length>` must resolve to Type::Scalar(VELOCITY) via stdlib prelude"
    );
}

// ─── step-9: stdlib safety-net ───────────────────────────────────────────────

/// Stdlib safety-net: the new prelude-alias seeding pass (step-4) must not
/// regress stdlib compilation for modules that don't use any type aliases.
///
/// Verifies that a basic `Length`-typed param still compiles cleanly with
/// `compile_with_stdlib` after the alias-registry seeding changes, and that the
/// resolved cell type is unaffected — i.e. the new pass does not interfere with
/// the existing type-resolution pipeline for non-alias param types.
#[test]
fn compile_with_stdlib_unaffected_for_module_without_alias_use() {
    let source = "structure def S { param x : Length = 1m }";
    let parsed = parse_with_stdlib(source, ModulePath::single("safety_net_module"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    assert_eq!(
        error_count(&compiled),
        0,
        "stdlib compilation of a simple Length param must produce zero Error diagnostics; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");

    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("value cell `x` not found on `S`");

    assert_eq!(
        x_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "param `x : Length` must resolve to Type::Scalar(LENGTH)"
    );
}

// ─── amendment: contract regression guards ────────────────────────────────────

/// A user module that references a prelude alias must NOT re-export that alias
/// through its own `type_aliases` field.  Only aliases declared in the user
/// module's own source (via `type Foo = Bar`) should appear in the output
/// `CompiledModule.type_aliases`.
///
/// Guards against the contract regression identified in task 2750 review:
/// before the fix, `alias_registry.into_compiled()` returned all entries
/// including prelude-seeded ones, so `module.type_aliases` contained the
/// prelude aliases the user had referenced.
#[test]
fn prelude_alias_not_re_exported_in_user_module_type_aliases() {
    let stress = make_pub_alias(
        "Stress",
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
    );
    let prelude_a = CompiledModuleBuilder::new(ModulePath::single("re_export_prelude"))
        .type_alias(stress)
        .build();

    // User module references the prelude alias but does NOT declare it.
    let source = "structure def Beam { param yield : Stress }";
    let parsed = reify_syntax::parse(source, ModulePath::single("re_export_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_a]);

    assert_eq!(
        error_count(&compiled),
        0,
        "must compile without errors; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    // The user module has no type alias declarations of its own — the prelude
    // alias must NOT appear in the output type_aliases.
    assert!(
        compiled.type_aliases.is_empty(),
        "user module must not re-export prelude aliases through type_aliases; \
         expected empty, got: {:?}",
        compiled
            .type_aliases
            .iter()
            .map(|a| &a.name)
            .collect::<Vec<_>>()
    );
}

// ─── task 4794 step-1: Vec3<Pressure> via real stdlib prelude ─────────────────

/// A user module that references `Vec3<Pressure>` (not declared in user source)
/// must compile cleanly against the real stdlib prelude — `Vec3` lives in
/// trajectory.ri, seeded into the user module's alias registry by `compile_with_stdlib`.
///
/// `Vec3<Pressure>` → body `Vector3<Q>` with Q=Pressure → Type::vec3(Type::Scalar{PRESSURE}).
///
/// RED on base: trajectory.ri:102 declares `pub type Vec3 = Vector3<Length>` (0-param),
/// so `Vec3<Pressure>` is an arity error (1 arg vs 0 params).
#[test]
fn vec3_alias_resolves_via_real_stdlib_prelude() {
    let source = "structure def S { param p : Vec3<Pressure> }";
    let parsed = parse_with_stdlib(source, ModulePath::single("vec3_stdlib_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    // Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Vec3<Pressure> via real stdlib prelude must compile without Error; got: {:?}",
        errors
    );

    // Zero Info diagnostics.
    let infos: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert!(
        infos.is_empty(),
        "Vec3<Pressure> via real stdlib prelude must compile without Info; got: {:?}",
        infos
    );

    // `p` resolves to Type::vec3(Type::Scalar { dimension: PRESSURE }).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
        "param `p : Vec3<Pressure>` must resolve to Type::vec3(PRESSURE) via stdlib prelude"
    );
}

// ─── amendment: cross-prelude collision warning ─────────────────────────────

/// Two prelude modules declaring the same pub alias name must produce a
/// `Severity::Warning` diagnostic naming both modules.  First-wins takes effect:
/// the first prelude module's definition is used for resolution.
#[test]
fn cross_prelude_alias_collision_emits_warning() {
    let foo_from_a = make_pub_alias(
        "Foo",
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let foo_from_b = make_pub_alias(
        "Foo",
        Type::Scalar {
            dimension: DimensionVector::MASS,
        },
    );
    let prelude_a = CompiledModuleBuilder::new(ModulePath::single("collision_prelude_a"))
        .type_alias(foo_from_a)
        .build();
    let prelude_b = CompiledModuleBuilder::new(ModulePath::single("collision_prelude_b"))
        .type_alias(foo_from_b)
        .build();

    let source = "structure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("collision_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_a, prelude_b]);

    // Must have a Warning diagnostic mentioning the alias name and both modules.
    let warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message.contains("Foo")
                && d.message.contains("collision_prelude_a")
                && d.message.contains("collision_prelude_b")
        })
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a Warning naming both prelude modules for the Foo collision; \
         got diagnostics: {:?}",
        compiled.diagnostics
    );

    // First-wins: p must resolve to Length (from collision_prelude_a), not Mass.
    assert_eq!(
        error_count(&compiled),
        0,
        "must compile without errors (first-wins resolution); got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );
    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = s_template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "first-wins: p must resolve to LENGTH (from collision_prelude_a)"
    );
}

// ─── task 4792: cross-module parametric alias resolution ──────────────────────

/// A prelude `pub type Rate<Q: Dimension> = Q / Time` alias (parametric,
/// dimensional-op body) must be resolved cross-module when instantiated with a
/// concrete type argument.
///
/// Specifically: `Rate<Length>` → `Scalar { dimension: VELOCITY }` because
/// LENGTH / TIME = VELOCITY by integer-exponent arithmetic.
///
/// RED on base: the skip machinery prevents Rate from being seeded into the user
/// module's alias registry, so `Rate<Length>` is unresolved → Error.
#[test]
fn parametric_prelude_dimensional_alias_resolves_cross_module() {
    let span = SourceSpan::new(0, 0);

    // Build CompiledTypeAlias for `Rate<Q: Dimension> = Q / Time`.
    let rate_alias = CompiledTypeAlias {
        name: "Rate".to_string(),
        resolved_type: None,
        type_params: vec![TypeParam {
            name: "Q".to_string(),
            bounds: vec![],
            default: None,
        }],
        type_expr: Some(reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::DimensionalOp {
                op: reify_ast::DimOp::Div,
                left: Box::new(reify_ast::TypeExpr {
                    kind: reify_ast::TypeExprKind::Named {
                        name: "Q".to_string(),
                        type_args: vec![],
                    },
                    span,
                }),
                right: Box::new(reify_ast::TypeExpr {
                    kind: reify_ast::TypeExprKind::Named {
                        name: "Time".to_string(),
                        type_args: vec![],
                    },
                    span,
                }),
            },
            span,
        }),
        is_pub: true,
        span,
        content_hash: ContentHash::of_str("Rate"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("rate_prelude"))
        .type_alias(rate_alias)
        .build();

    let source = "structure def S { param v : Rate<Length> }";
    let parsed = reify_syntax::parse(source, ModulePath::single("rate_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Zero Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Rate<Length> cross-module must resolve without Error; got: {:?}",
        errors
    );

    // Zero Info diagnostics (no skip hint).
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert!(
        info_diags.is_empty(),
        "Rate<Length> cross-module must produce zero Info diagnostics; got: {:?}",
        info_diags
    );

    // `v` resolves to Type::Scalar { dimension: VELOCITY } (Length / Time).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let v_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "v")
        .expect("value cell `v` not found on `S`");
    assert_eq!(
        v_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::VELOCITY,
        },
        "param `v : Rate<Length>` must resolve to Type::Scalar(VELOCITY)"
    );
}

// ─── task 2777: parametric prelude alias Info diagnostics ─────────────────────

/// Build a parametric `pub type <name><param_name>` prelude alias.
///
/// The alias body is the passthrough `param_name` (e.g. `Vec<T> = T`), stored
/// as `type_expr: Some(TypeExpr{Named{param_name, []}})`.  After task-4792
/// un-skips parametric prelude aliases, this body is used for use-site
/// instantiation: `Vec<Real>` → body `T` with T=Real → dimensionless_scalar.
fn make_parametric_pub_alias(name: &str, param_name: &str) -> CompiledTypeAlias {
    CompiledTypeAlias {
        name: name.to_string(),
        resolved_type: None,
        type_params: vec![TypeParam {
            name: param_name.to_string(),
            bounds: vec![],
            default: None,
        }],
        type_expr: Some(reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: param_name.to_string(),
                type_args: vec![],
            },
            span: SourceSpan::new(0, 0),
        }),
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(&format!("{}_{}", name, param_name)),
    }
}

/// After parametric prelude aliases are un-skipped (task 4792), a user module
/// that references `Vec<Real>` against a seeded `pub type Vec<T>` (body: T)
/// resolves cleanly — zero Info (no skip hint) and zero Error.
///
/// Flipped from the task-2777 "emits Info" test: the skip/Info machinery is
/// retired; `Vec<Real>` now resolves to the body type (Real = dimensionless scalar).
///
/// RED on base: Vec is still skipped → unresolved-type Error for Vec<Real>.
#[test]
fn parametric_form_use_emits_info_diagnostic() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_info_prelude"))
        .type_alias(vec_alias)
        .build();

    // Vec<Real>: seeded parametric alias Vec<T>=T, instantiated with T=Real.
    // After un-skip, resolves to Real = dimensionless_scalar with 0 Info.
    let source = "structure def S { param p : Vec<Real> }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_info_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Zero Error diagnostics — Vec<Real> resolves to Real (dimensionless scalar).
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Vec<Real> must resolve without Error after parametric alias un-skip; got: {:?}",
        errors
    );

    // Zero Info diagnostics — skip hint is retired.
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        0,
        "expected 0 Info diagnostics after parametric alias un-skip; got: {:?}",
        info_diags
    );

    // p resolves to Real = dimensionless scalar.
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::dimensionless_scalar(),
        "param `p : Vec<Real>` must resolve to Type::dimensionless_scalar() via Vec<T>=T, T=Real"
    );
}

/// A user module that declares its own `type Vec = Real` (shadowing the prelude's
/// parametric `pub type Vec<T>`) and references `param p : Vec` must:
/// (1) compile successfully — user's alias wins, p resolves to Real
/// (2) produce zero `Severity::Info` diagnostics — the prelude's parametric Vec
///     is functionally invisible, so Info about cross-module propagation would be
///     misleading
///
/// This is the shadow-guard regression test added in task 2777 step-3.  It
/// verifies the `!user_alias_names.contains(pa.name.as_str())` guard in
/// `phase_aliases` prevents the Info from firing when the user has redeclared
/// the name locally.
#[test]
fn user_shadowed_parametric_prelude_alias_emits_no_info_diagnostic() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("shadow_param_prelude"))
        .type_alias(vec_alias)
        .build();

    // User module shadows the parametric prelude Vec with a non-parametric alias.
    let source = "type Vec = Real\nstructure def S { param p : Vec }";
    let parsed = reify_syntax::parse(source, ModulePath::single("shadow_param_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // (1) No Error diagnostics — user's alias resolved correctly.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "user shadow must produce no Error diagnostics; got: {:?}",
        errors
    );

    // (1b) The p cell type is Real (user's alias wins over prelude's parametric Vec).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template `S` not found");
    let p_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("value cell `p` not found on `S`");
    assert_eq!(
        p_cell.cell_type,
        Type::dimensionless_scalar(),
        "param `p : Vec` must resolve to Type::dimensionless_scalar() via user's shadow alias"
    );

    // (2) Zero Info diagnostics — no misleading Info about cross-module propagation.
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        0,
        "shadowed parametric prelude alias must emit zero Info diagnostics; got: {:?}",
        info_diags
    );
}

/// A user module that references `NotADeclaredType` (a name NOT in the skipped
/// parametric prelude set) must produce zero `Severity::Info` diagnostics.
///
/// This is the negative test: no false-positive Info for unrelated unresolved names.
#[test]
fn unrelated_unresolved_no_info_emitted() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("noinfo_prelude"))
        .type_alias(vec_alias)
        .build();

    let source = "structure def S { param p : NotADeclaredType }";
    let parsed = reify_syntax::parse(source, ModulePath::single("noinfo_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Must have at least one Error for the unresolved name.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected ≥1 Error for unresolved 'NotADeclaredType'; got: {:?}",
        compiled.diagnostics
    );

    // Must have zero Info diagnostics — no false-positive for unrelated names.
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        0,
        "expected 0 Info diagnostics for unrelated unresolved name; got: {:?}",
        info_diags
    );
}

// ─── task 2782: span-level dedup for parametric-prelude Info diagnostic ────────

/// After parametric prelude aliases are un-skipped (task 4792), a user module
/// that declares `let x : Vec<Real> = none` against a seeded `pub type Vec<T>`
/// alias emits ZERO Info diagnostics — the skip hint is no longer fired.
///
/// Flipped from the task-2782 "emits single Info" test: the span-dedup machinery
/// is retired along with the skip logic; `Vec<Real>` now resolves via the
/// standard parametric-alias instantiation path (Vec<T>=T, T=Real = dimensionless).
///
/// RED on base: Vec is still skipped → fixup_option_none_for_let hits the
/// skipped-parametric-prelude path and emits exactly 1 Info.
#[test]
fn parametric_prelude_let_none_emits_single_info_diagnostic() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_let_info_prelude"))
        .type_alias(vec_alias)
        .build();

    // `let x : Vec<Real> = none` — after un-skip, Vec<Real> resolves via the
    // seeded parametric alias body (Vec<T>=T, T=Real = dimensionless).
    // fixup_option_none_for_let no longer hits the skip-Info path.
    let source = "structure def S { let x : Vec<Real> = none }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_let_info_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Headline assertion: ZERO Info diagnostics — skip hint is retired.
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        0,
        "expected 0 Info diagnostics after parametric alias un-skip; got: {:?}",
        info_diags
    );
}

// ─── task 6259: prelude-seeded alias whose body names an entity type ────────
//
// Task #6259 makes a non-parametric `type AL = <Body>` whose body names an
// enum / structure def / occurrence def / trait resolve at its USE SITE (the
// alias DFS in `phase_aliases` runs before those name sets exist, so the entry
// legitimately leaves that phase with `resolved_type: None` and the deferred
// arm in `resolve_type_expr_with_aliases_kinded` reads the retained
// `type_expr` body instead).
//
// That deferral has a second, independent gap on the PRELUDE path:
// `TypeAliasEntry::from_compiled_for_prelude` drops `type_expr` for every
// non-parametric alias, on the premise that "non-parametric ones resolve via
// `resolved_type` and never read `type_expr`" — exactly the premise the
// deferral invalidates. A seeded entry then has BOTH `resolved_type: None` AND
// `type_expr: None`, the deferred arm cannot fire, and `unresolved type: Fq`
// persists across the module boundary.
//
// Fixture names are collision-free with stdlib (`Fit`/`FitCategory` are taken
// by stdlib/tolerancing.ri and would silently resolve against the stdlib
// entity instead of the fixture).

/// Return the declared `Type` of `entity`'s `member` param.
///
/// Params live in `TopologyTemplate.value_cells` — there is no `.params` field.
fn entity_param_type(module: &reify_compiler::CompiledModule, entity: &str, member: &str) -> Type {
    let errors: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect();
    let template = module
        .templates
        .iter()
        .find(|t| t.name == entity)
        .unwrap_or_else(|| panic!("template `{entity}` not found; errors: {errors:?}"));
    template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("value cell `{entity}.{member}` not found; errors: {errors:?}"))
        .cell_type
        .clone()
}

fn error_messages(module: &reify_compiler::CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

/// End-to-end: a prelude module declaring BOTH an enum and a
/// `pub type Fq = <that enum>` must make `param p : Fq` resolve in a
/// downstream user module, identically to spelling the enum directly.
#[test]
fn pub_prelude_alias_to_prelude_enum_resolves_in_user_module() {
    let prelude_src = "enum Zq { Close, Medium }\npub type Fq = Zq\n";
    let prelude_parsed = reify_syntax::parse(
        prelude_src,
        ModulePath::single("entity_alias_prelude"),
    );
    assert!(
        prelude_parsed.errors.is_empty(),
        "prelude parse errors: {:?}",
        prelude_parsed.errors
    );
    let prelude_m = reify_compiler::compile(&prelude_parsed);
    assert_eq!(
        error_count(&prelude_m),
        0,
        "the prelude module itself must compile cleanly; got: {:?}",
        error_messages(&prelude_m)
    );

    // Oracle: the same enum spelled DIRECTLY in the user module, with the same
    // prelude in scope. This baseline also pins that prelude enum names
    // propagate at all — without it the parity assertion below could pass
    // vacuously (both sides Error).
    let direct_parsed = reify_syntax::parse(
        "structure def D { param p : Zq }",
        ModulePath::single("entity_alias_user_direct"),
    );
    assert!(
        direct_parsed.errors.is_empty(),
        "parse errors: {:?}",
        direct_parsed.errors
    );
    let direct = compile_with_prelude(&direct_parsed, std::slice::from_ref(&prelude_m));
    assert_eq!(
        error_count(&direct),
        0,
        "DIRECT baseline must compile cleanly for the parity oracle to mean \
         anything; got: {:?}",
        error_messages(&direct)
    );
    let direct_ty = entity_param_type(&direct, "D", "p");

    let alias_parsed = reify_syntax::parse(
        "structure def D { param p : Fq }",
        ModulePath::single("entity_alias_user_alias"),
    );
    assert!(
        alias_parsed.errors.is_empty(),
        "parse errors: {:?}",
        alias_parsed.errors
    );
    let compiled = compile_with_prelude(&alias_parsed, std::slice::from_ref(&prelude_m));
    assert_eq!(
        error_count(&compiled),
        0,
        "a prelude `pub type Fq = Zq` must resolve in the user module; got: {:?}",
        error_messages(&compiled)
    );
    assert_eq!(
        entity_param_type(&compiled, "D", "p"),
        direct_ty,
        "`param p : Fq` must lower exactly as the direct `param p : Zq` does"
    );
}

/// Unit-level pin at the SOURCE of the prelude-seeding change: seeding a
/// `CompiledTypeAlias` that is non-parametric AND still unresolved must
/// preserve its `type_expr` body in the seeded registry entry, otherwise the
/// use-site deferral has nothing to read.
///
/// The enum lives in the USER module here, so this observes ONLY the body
/// carry-over — prelude enum-name propagation is deliberately not involved
/// (the end-to-end test above covers that half).
#[test]
fn seeded_unresolved_non_parametric_alias_retains_its_body() {
    let alias = CompiledTypeAlias {
        name: "Gq".to_string(),
        // Entity-bodied alias: the DFS in `phase_aliases` could not resolve it
        // (structures/traits/enums are not compiled yet at that point).
        resolved_type: None,
        type_params: vec![],
        type_expr: Some(reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: "Zq".to_string(),
                type_args: vec![],
            },
            span: SourceSpan::new(0, 0),
        }),
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Gq"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("body_carry_prelude"))
        .type_alias(alias)
        .build();

    let direct_parsed = reify_syntax::parse(
        "enum Zq { Close, Medium }\nstructure def D { param p : Zq }",
        ModulePath::single("body_carry_user_direct"),
    );
    assert!(
        direct_parsed.errors.is_empty(),
        "parse errors: {:?}",
        direct_parsed.errors
    );
    let direct = compile_with_prelude(&direct_parsed, std::slice::from_ref(&prelude_m));
    assert_eq!(
        error_count(&direct),
        0,
        "DIRECT baseline must compile cleanly; got: {:?}",
        error_messages(&direct)
    );
    let direct_ty = entity_param_type(&direct, "D", "p");

    let alias_parsed = reify_syntax::parse(
        "enum Zq { Close, Medium }\nstructure def D { param p : Gq }",
        ModulePath::single("body_carry_user_alias"),
    );
    assert!(
        alias_parsed.errors.is_empty(),
        "parse errors: {:?}",
        alias_parsed.errors
    );
    let compiled = compile_with_prelude(&alias_parsed, std::slice::from_ref(&prelude_m));
    assert_eq!(
        error_count(&compiled),
        0,
        "seeding a non-parametric alias with `resolved_type: None` and \
         `type_expr: Some(..)` must keep the body so the use-site deferral can \
         read it; got: {:?}",
        error_messages(&compiled)
    );
    assert_eq!(
        entity_param_type(&compiled, "D", "p"),
        direct_ty,
        "the seeded `Gq` alias must lower exactly as the direct `Zq` does"
    );
}

// ── task 6477: PARAMETRIC entity-bodied aliases across the prelude boundary ──
//
// The parametric register of `pub_prelude_alias_to_prelude_enum_resolves_in_
// user_module` above. #6259 fixed the NON-parametric spelling; the parametric
// one kept reporting `unresolved type: Gq<Real>` because
// `resolve_type_alias_expr_with_subst`'s terminal `Named` arm resolved the body
// against hard-coded EMPTY entity namespaces.
//
// This is the literal reproducer from task 6477's description, and it needs
// BOTH halves of the fix to pass: the alias must be `pub` to cross the module
// boundary at all, which means the def-site guard (enum-blind before #6477)
// rejected it in the PRELUDE module before any consumer ran.

/// Compile `src` as a standalone prelude module, asserting it is clean.
fn compile_prelude(src: &str, name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(name));
    assert!(
        parsed.errors.is_empty(),
        "[{name}] prelude parse errors: {:?}",
        parsed.errors
    );
    let module = reify_compiler::compile(&parsed);
    assert_eq!(
        error_count(&module),
        0,
        "[{name}] the prelude module itself must compile cleanly — a `pub`
         parametric alias is validated at its OWN definition site, so this is
         where an enum-blind def-site guard fires first; got: {:?}",
        error_messages(&module)
    );
    module
}

/// Compile `src` against `prelude`, asserting it is clean, and return the
/// declared type of `D.p`.
fn param_type_against_prelude(
    src: &str,
    name: &str,
    prelude: &reify_compiler::CompiledModule,
    context: &str,
) -> Type {
    let parsed = reify_syntax::parse(src, ModulePath::single(name));
    assert!(parsed.errors.is_empty(), "[{name}] parse errors: {:?}", parsed.errors);
    let compiled = compile_with_prelude(&parsed, std::slice::from_ref(prelude));
    assert_eq!(
        error_count(&compiled),
        0,
        "{context}; got: {:?}",
        error_messages(&compiled)
    );
    entity_param_type(&compiled, "D", "p")
}

/// End-to-end headline case: a prelude declaring `enum Zq` plus
/// `pub type Gq<T> = Option<Zq>` must make `param p : Gq<Real>` resolve in a
/// downstream user module, identically to spelling `Option<Zq>` directly.
///
/// MEASURED on `main` (823502024d): `["unresolved type: Gq<Real>"]`.
#[test]
fn pub_prelude_parametric_alias_to_prelude_enum_resolves_in_user_module() {
    let prelude_m = compile_prelude(
        "enum Zq { Close, Medium }\npub type Gq<T> = Option<Zq>\n",
        "parametric_entity_alias_prelude",
    );

    // Oracle: the same body spelled DIRECTLY, against the same prelude. This
    // also pins that prelude ENUM names propagate at all — without it the
    // parity assertion could pass vacuously with both sides `Type::Error`.
    let direct_ty = param_type_against_prelude(
        "structure def D { param p : Option<Zq> }",
        "parametric_entity_alias_user_direct",
        &prelude_m,
        "DIRECT baseline must compile cleanly for the parity oracle to mean anything",
    );
    assert!(
        !direct_ty.is_error(),
        "DIRECT baseline must lower to a real type, not the `Type::Error` poison; \
         got: {direct_ty:?}"
    );

    let alias_ty = param_type_against_prelude(
        "structure def D { param p : Gq<Real> }",
        "parametric_entity_alias_user_alias",
        &prelude_m,
        "a prelude `pub type Gq<T> = Option<Zq>` must resolve at a cross-module use site",
    );
    assert_eq!(
        alias_ty, direct_ty,
        "`param p : Gq<Real>` must lower exactly as the direct `param p : Option<Zq>` does"
    );
}

/// The param-USING cross-module variant: `pub type Iq<T> = Map<T, Zq>` used as
/// `Iq<Real>`. Proves the substitution and the prelude entity lookup compose
/// across the module boundary, not just one or the other.
///
/// MEASURED on `main`: `["unresolved type: Iq<Real>"]`.
#[test]
fn pub_prelude_parametric_alias_using_its_param_and_a_prelude_enum_resolves() {
    let prelude_m = compile_prelude(
        "enum Zq { Close, Medium }\npub type Iq<T> = Map<T, Zq>\n",
        "param_using_entity_alias_prelude",
    );

    let direct_ty = param_type_against_prelude(
        "structure def D { param p : Map<Real, Zq> }",
        "param_using_entity_alias_user_direct",
        &prelude_m,
        "DIRECT baseline must compile cleanly for the parity oracle to mean anything",
    );
    assert!(
        !direct_ty.is_error(),
        "DIRECT baseline must lower to a real type; got: {direct_ty:?}"
    );

    let alias_ty = param_type_against_prelude(
        "structure def D { param p : Iq<Real> }",
        "param_using_entity_alias_user_alias",
        &prelude_m,
        "a prelude `pub type Iq<T> = Map<T, Zq>` must resolve at a cross-module use site",
    );
    assert_eq!(
        alias_ty, direct_ty,
        "`param p : Iq<Real>` must lower exactly as the direct `param p : Map<Real, Zq>` does"
    );
}

/// Unit-level pin on the seeding precondition the two tests above depend on —
/// the parametric analogue of `seeded_unresolved_non_parametric_alias_retains_
/// its_body`.
///
/// `from_compiled_for_prelude` drops an alias body only when
/// `type_params.is_empty() && resolved_type.is_some()`. A parametric
/// entity-bodied alias fails BOTH conjuncts, so its `type_expr` crosses the
/// module boundary and `resolve_parameterized_alias` has something to
/// substitute into. If the body were dropped, that function reports
/// `internal error: parametric alias 'Gq' has no body` — so this test asserts
/// the absence of that message specifically, not just overall cleanliness.
///
/// The enum lives in the USER module here, so this observes ONLY the body
/// carry-over; prelude enum-name propagation is the end-to-end tests' job.
#[test]
fn seeded_parametric_entity_bodied_alias_retains_its_body() {
    let option_of_zq = reify_ast::TypeExpr {
        kind: reify_ast::TypeExprKind::Named {
            name: "Option".to_string(),
            type_args: vec![reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: "Zq".to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::new(0, 0),
            }],
        },
        span: SourceSpan::new(0, 0),
    };
    let alias = CompiledTypeAlias {
        name: "Gq".to_string(),
        // The realistic shape: the alias DFS cannot resolve a parametric
        // entity-bodied alias, so it leaves `resolved_type: None`.
        resolved_type: None,
        type_params: vec![TypeParam {
            name: "T".to_string(),
            bounds: vec![],
            default: None,
        }],
        type_expr: Some(option_of_zq),
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Gq"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("parametric_body_carry_prelude"))
        .type_alias(alias)
        .build();

    let direct_ty = param_type_against_prelude(
        "enum Zq { Close, Medium }\nstructure def D { param p : Option<Zq> }",
        "parametric_body_carry_user_direct",
        &prelude_m,
        "DIRECT baseline must compile cleanly",
    );

    let alias_parsed = reify_syntax::parse(
        "enum Zq { Close, Medium }\nstructure def D { param p : Gq<Real> }",
        ModulePath::single("parametric_body_carry_user_alias"),
    );
    assert!(
        alias_parsed.errors.is_empty(),
        "parse errors: {:?}",
        alias_parsed.errors
    );
    let compiled = compile_with_prelude(&alias_parsed, std::slice::from_ref(&prelude_m));
    let errs = error_messages(&compiled);
    assert!(
        !errs.iter().any(|m| m.contains("has no body")),
        "the seeded PARAMETRIC alias must carry its `type_expr` across the module \
         boundary — an `internal error: … has no body` here means \
         `from_compiled_for_prelude` dropped it; got: {errs:?}"
    );
    assert_eq!(
        error_count(&compiled),
        0,
        "seeding a parametric alias with `resolved_type: None` and \
         `type_expr: Some(..)` must keep the body so `resolve_parameterized_alias` \
         can substitute into it; got: {errs:?}"
    );
    assert_eq!(
        entity_param_type(&compiled, "D", "p"),
        direct_ty,
        "the seeded `Gq<Real>` alias must lower exactly as the direct `Option<Zq>` does"
    );
}

/// The late-binding case task 6477's description worries about, spelled
/// literally: a PRELUDE `pub type Gq<T> = Option<Zq>` whose body names the
/// prelude's own `Zq`, consumed by a module that declares its OWN `Zq`.
///
/// The task proposes preventing this by snapshotting the body's resolution in
/// the defining module. #6259 commit a98a356da9 recorded the opposite decision
/// — alias bodies get NO separate name-resolution rule; the body resolves
/// through the identical path the direct spelling takes, at the same use site,
/// including under shadowing — so binding to the consumer's declaration is the
/// INTENDED semantics here, and a snapshot is exactly what would break it.
///
/// Asserts PARITY rather than a literal `Type` variant, for the reason #6259
/// gave: `enum-shadow-coherence` leaf α is chartered to revisit the precedence,
/// and a frozen variant would hand α a test to fight. The module-local
/// companion is `type_alias_compile_tests::parametric_alias_body_shadow_parity`,
/// which carries the dated measurement of which binding wins today.
#[test]
fn shadowed_prelude_parametric_alias_body_binds_as_the_direct_spelling_does() {
    let prelude_m = compile_prelude(
        "enum Zq { Close, Medium }\npub type Gq<T> = Option<Zq>\n",
        "shadowed_parametric_alias_prelude",
    );

    // The consumer declares its OWN `Zq`, shadowing the prelude's.
    let shadowing_decl = "structure def Zq { param w : Length = 1.0mm }";

    let direct_ty = param_type_against_prelude(
        &format!("{shadowing_decl}\nstructure def D {{ param p : Option<Zq> }}"),
        "shadowed_parametric_alias_user_direct",
        &prelude_m,
        "DIRECT baseline must compile cleanly for the parity oracle to mean anything",
    );
    assert!(
        !direct_ty.is_error(),
        "DIRECT baseline must lower to a real type, not the `Type::Error` poison; \
         got: {direct_ty:?}"
    );

    let alias_ty = param_type_against_prelude(
        &format!("{shadowing_decl}\nstructure def D {{ param p : Gq<Real> }}"),
        "shadowed_parametric_alias_user_alias",
        &prelude_m,
        "a prelude parametric alias whose body names a SHADOWED entity must still \
         resolve at the consumer's use site",
    );
    assert_eq!(
        alias_ty, direct_ty,
        "the prelude alias body `Option<Zq>` must bind `Zq` exactly as the consumer's \
         own direct `Option<Zq>` spelling does — that is the recorded decision, not a \
         late-binding bug to be fixed with a defining-module snapshot"
    );
}

/// A diagnostic raised while instantiating a PRELUDE parametric alias must be
/// anchored in the CONSUMER's own source (task #6477, amendment).
///
/// `Diagnostic`/`DiagnosticLabel` carry no module identity, so a label built
/// from a prelude alias BODY's span is a raw byte offset into a file the
/// consumer's reader never opened — and for a short consumer it lands past the
/// end of the source it will be rendered against. `reify-cli`'s `mcp_context`
/// feeds the first label's span straight to `byte_offset_to_line_col`, whose
/// `debug_assert!(offset <= source.len())` then trips in debug builds and
/// reports a silently wrong line/col in release.
///
/// MEASURED on this tip with the span threading reverted: this consumer's
/// single Error carried `labels=[SourceSpan { start: 50, end: 55 }]` against a
/// 37-byte source — the offset of `Hq<U>` inside the PRELUDE, 13 bytes past the
/// end of the file it would be rendered against.
///
/// The lock is the containment invariant, not a specific offset: every label
/// on every diagnostic the consumer receives must index the consumer's source.
/// `W<U> = Hq<U>` is the shape that produces one, because the shared name
/// resolver's trait-with-args arm (#5049 α) is the only arm in the alias-body
/// path that emits a label of its own.
#[test]
fn prelude_alias_body_diagnostic_is_anchored_in_the_consumer_source() {
    let prelude_m = compile_prelude(
        "trait Hq {\n    param w : Length\n}\npub type W<U> = Hq<U>\n",
        "parametric_trait_arg_prelude",
    );

    let consumer_src = "structure def D { param p : W<Real> }";
    let parsed = reify_syntax::parse(consumer_src, ModulePath::single("trait_arg_user"));
    assert!(
        parsed.errors.is_empty(),
        "consumer parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_prelude(&parsed, std::slice::from_ref(&prelude_m));

    // The gap is LOUD by design — the point of the lock is where it points,
    // not whether it fires, so a run that reported nothing would silently
    // vacuously pass the containment check below.
    assert!(
        error_messages(&compiled)
            .iter()
            .any(|m| m.contains("E_TYPE_ARG_ON_TRAIT")),
        "the consumer must still be told that `W<Real>`'s body applies type \
         arguments to trait `Hq`; got: {:?}",
        error_messages(&compiled)
    );

    let len = consumer_src.len();
    let escaped: Vec<String> = compiled
        .diagnostics
        .iter()
        .flat_map(|d| {
            d.labels
                .iter()
                .filter(|l| l.span.end as usize > len || l.span.start as usize > len)
                .map(move |l| format!("{:?} @ {:?}", d.message, l.span))
        })
        .collect();
    assert!(
        escaped.is_empty(),
        "every label on a consumer diagnostic must index the consumer's own \
         {len}-byte source; these carry offsets from another module: {escaped:?}"
    );
}
