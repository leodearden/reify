//! Acceptance tests for cross-module type-alias propagation through PreludeContext.
//!
//! TDD structure (task 2750):
//!   step-3: headline acceptance tests (pub prelude alias resolves in user module)
//!   step-5: user-alias shadowing tests
//!   step-7: exclusion tests (#no_prelude, non-pub, parametric skip)
//!   step-9: stdlib safety-net

use reify_compiler::{CompiledTypeAlias, compile_with_prelude};
use reify_test_support::CompiledModuleBuilder;
use reify_types::{ContentHash, DimensionVector, ModulePath, Severity, SourceSpan, Type};

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
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
        Type::Scalar { dimension: DimensionVector::LENGTH },
        true,
    );
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("shadow_prelude"))
        .type_alias(prelude_alias)
        .build();

    // User module declares `type Foo = Mass` — must shadow the prelude's Length.
    let source = "type Foo = Mass\nstructure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("shadow_user"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
        Type::Scalar { dimension: DimensionVector::MASS },
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
        Type::Scalar { dimension: DimensionVector::LENGTH },
        true,
    );
    let prelude_m = CompiledModuleBuilder::new(ModulePath::single("visible_prelude"))
        .type_alias(prelude_alias)
        .build();

    // User module does NOT declare type Foo — must pick it up from prelude.
    let source = "structure def S { param p : Foo }";
    let parsed = reify_syntax::parse(source, ModulePath::single("visible_user"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
        Type::Scalar { dimension: DimensionVector::LENGTH },
        "param `p : Foo` must resolve to LENGTH from prelude alias"
    );
}
