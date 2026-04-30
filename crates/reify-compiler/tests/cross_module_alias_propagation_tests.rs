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
