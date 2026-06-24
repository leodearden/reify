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
use reify_test_support::CompiledModuleBuilder;
use reify_core::{ContentHash, DimensionVector, ModulePath, Severity, SourceSpan, Type};
use reify_ir::TypeParam;

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
fn parametric_pub_prelude_alias_skipped_with_no_panic() {
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
