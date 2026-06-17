//! Acceptance tests for cross-module type-alias propagation through PreludeContext.
//!
//! TDD structure (task 2750):
//!   step-3: headline acceptance tests (pub prelude alias resolves in user module)
//!   step-5: user-alias shadowing tests
//!   step-7: exclusion tests (#no_prelude, non-pub, parametric skip)
//!   step-9: stdlib safety-net

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

/// A parametric pub prelude alias (type_params non-empty) must be silently
/// skipped — the compile must NOT panic — and the user-module reference to
/// the alias must produce an unresolved-type Error (pinning the documented
/// limitation that parametric prelude aliases are not propagated cross-module).
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
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str("Vec_T"),
    };
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_prelude"))
        .type_alias(parametric_alias)
        .build();

    // Use a bare `Vec` reference (no type arguments) so the parser succeeds
    // cleanly and the error is unambiguously from the alias skip, not from the
    // parser's handling of `<`.  The prelude alias has type_params=[T], so it
    // is skipped by phase_aliases, leaving `Vec` unresolved at the use site.
    let source = "structure def S { param p : Vec }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse must succeed for bare Vec reference: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Must not panic (smoke), and the unresolved-type error must name 'Vec'.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "parametric prelude alias 'Vec' must be skipped; expected ≥1 Error diagnostic naming 'Vec', got: {:?}",
        compiled.diagnostics
    );
    assert!(
        errors.iter().any(|d| d.message.contains("Vec")),
        "at least one Error diagnostic must mention 'Vec' (the skipped parametric alias); \
         got errors: {:?}",
        errors
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

// ─── task 2777: parametric prelude alias Info diagnostics ─────────────────────

/// Build a parametric `pub type <name><param_name>` prelude alias.
fn make_parametric_pub_alias(name: &str, param_name: &str) -> CompiledTypeAlias {
    CompiledTypeAlias {
        name: name.to_string(),
        resolved_type: None,
        type_params: vec![TypeParam {
            name: param_name.to_string(),
            bounds: vec![],
            default: None,
        }],
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: ContentHash::of_str(&format!("{}_{}", name, param_name)),
    }
}

/// A user module that references `Vec<Float>` (parameterized form) against a
/// parametric prelude alias `pub type Vec<T>` must receive:
/// - Exactly one `Severity::Info` diagnostic mentioning both `Vec` and `parametric`
/// - At least one `Severity::Error` mentioning `Vec` (existing regression guard)
///
/// This is the positive test for the use-site Info emission added in task 2777.
#[test]
fn parametric_form_use_emits_info_diagnostic() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_info_prelude"))
        .type_alias(vec_alias)
        .build();

    // Vec<Float>: Vec is not a recognized builtin or alias — falls through to
    // the skipped-parametric-prelude Info emission path.
    let source = "structure def S { param p : Vec<Float> }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_info_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Regression guard: Error mentioning Vec must still be present.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("Vec"))
        .collect();
    assert!(
        !errors.is_empty(),
        "expected ≥1 Error mentioning 'Vec'; got diagnostics: {:?}",
        compiled.diagnostics
    );

    // New behavior: exactly one Info diagnostic mentioning 'Vec' and 'parametric'.
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 Info diagnostic; got: {:?}",
        info_diags
    );
    assert!(
        info_diags[0].message.contains("Vec"),
        "Info diagnostic must mention 'Vec'; got: {}",
        info_diags[0].message
    );
    assert!(
        info_diags[0].message.contains("parametric"),
        "Info diagnostic must mention 'parametric'; got: {}",
        info_diags[0].message
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

/// A user module that declares `let x : Vec<Real> = none` (a
/// `fixup_option_none_for_let`-triggering form) against a parametric prelude
/// `pub type Vec<T>` must receive exactly one `Severity::Info` diagnostic on the
/// `Vec<Real>` annotation span.
///
/// This complements `parametric_form_use_emits_info_diagnostic` (which exercises
/// the `param p : Vec<Float>` path with a single binding-site resolution).  The
/// `let ... = none` form routes through `fixup_option_none_for_let`
/// (entity.rs:2715), an additional resolution path that the existing test
/// cannot cover.  The "exactly one Info" assertion guards against any future
/// double-resolve regression — e.g. if a binding-site pre-pass for lets is
/// added to mirror the existing param pre-pass at entity.rs:574, span-level
/// dedup in `TypeAliasRegistry::should_emit_skipped_parametric_prelude_info`
/// (task 2782) keeps the user-visible diagnostic count at one per use site.
///
/// Note: unlike the `param p : Vec<Float>` path, the `let ... = none` form does
/// NOT produce an Error-level diagnostic — `none` is valid as an untyped sentinel
/// and the annotation is advisory in this context.  Only the Info hint fires.
///
/// **Forward-looking guard**: currently only one resolution path runs for this form
/// (via `fixup_option_none_for_let`; `compile_expr` intercepts `none` before
/// consulting the annotation, and the entity-let pre-pass registers a placeholder
/// `Type::dimensionless_scalar()` without resolving), so the `info_diags.len() == 1` assertion holds
/// whether or not span-level dedup is active.  The dedup mechanism itself is
/// exercised by the unit test
/// `should_emit_skipped_parametric_prelude_info_dedups_per_span` in
/// `type_resolution.rs`.  This integration test anchors the "exactly one Info per
/// use site" contract so that any future binding-site pre-pass for lets (mirroring
/// the existing param pre-pass at entity.rs:574) is caught immediately as a
/// regression rather than silently producing duplicate diagnostics.
#[test]
fn parametric_prelude_let_none_emits_single_info_diagnostic() {
    let vec_alias = make_parametric_pub_alias("Vec", "T");
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("param_let_info_prelude"))
        .type_alias(vec_alias)
        .build();

    // `let x : Vec<Real> = none` — `none` produces OptionNone(Option<Real>) at
    // compile_expr time; fixup_option_none_for_let then resolves the annotation
    // `Vec<Real>` against the alias registry, hitting the
    // skipped-parametric-prelude Info-emit branch in resolve_type_expr_with_aliases.
    let source = "structure def S { let x : Vec<Real> = none }";
    let parsed = reify_syntax::parse(source, ModulePath::single("param_let_info_user"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = compile_with_prelude(&parsed, &[prelude_m]);

    // Headline assertion: exactly ONE Info diagnostic on this single use site.
    // (The `let ... = none` form does not produce an Error, unlike the param path.)
    let info_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Info)
        .collect();
    assert_eq!(
        info_diags.len(),
        1,
        "expected exactly 1 Info diagnostic for `let x : Vec<Real> = none`; got: {:?}",
        info_diags
    );
    assert!(
        info_diags[0].message.contains("Vec"),
        "Info diagnostic must mention 'Vec'; got: {}",
        info_diags[0].message
    );
    assert!(
        info_diags[0].message.contains("parametric"),
        "Info diagnostic must mention 'parametric'; got: {}",
        info_diags[0].message
    );
}
