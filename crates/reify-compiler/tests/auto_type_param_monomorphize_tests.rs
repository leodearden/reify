//! Tests for monomorphization of resolved generic sub-components in the
//! `phase_auto_type_param_resolution` pass (task 4431, M-013 α).
//!
//! For each resolved `auto:` use-site, the compiler synthesizes a per-(generic,
//! resolved-type-args) MONOMORPH `TopologyTemplate`, substitutes
//! `Type::TypeParam(T)→Type::StructureRef(c)` into the clone's cells and body
//! expressions, strips its `type_params`, and rewrites the originating
//! `SubComponentDecl.structure_name` to the monomorph name.

use reify_core::{Severity, Type};
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

/// Keystone test: a single `auto:` use-site produces a monomorph template.
///
/// Invariant 1 (partial coverage — top-level value_cells only until step-8):
///   No value cell reachable from the resolved sub-component carries `Type::TypeParam`.
///
/// RED until step-2 (no monomorph template is created before the implementation).
#[test]
fn single_auto_use_site_produces_monomorph() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing<T: Seal> { param seal : T }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = compile_source_with_stdlib(source);

    // Zero error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "expected no error diagnostics, got: {:?}",
        errors
    );

    // The monomorph template must exist.
    let monomorph = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bearing$GasketSeal")
        .expect("expected monomorph template 'Bearing$GasketSeal' in compiled.templates");

    // It must have no type parameters (it is a concrete instance).
    assert!(
        monomorph.type_params.is_empty(),
        "monomorph 'Bearing$GasketSeal' must have no type_params, got: {:?}",
        monomorph.type_params
    );

    // The 'seal' value cell must have cell_type == StructureRef("GasketSeal").
    let seal_cell = monomorph
        .value_cells
        .iter()
        .find(|c| c.id.member == "seal")
        .expect("expected 'seal' value cell in 'Bearing$GasketSeal'");
    assert_eq!(
        seal_cell.cell_type,
        Type::StructureRef("GasketSeal".to_string()),
        "'seal' cell_type must be StructureRef(\"GasketSeal\"), got: {:?}",
        seal_cell.cell_type
    );

    // Assembly's sub 'b' must reference the monomorph.
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("expected 'Assembly' template");
    let sub_b = assembly
        .sub_components
        .iter()
        .find(|s| s.name == "b")
        .expect("expected sub 'b' in 'Assembly'");
    assert_eq!(
        sub_b.structure_name,
        "Bearing$GasketSeal",
        "sub 'b' must reference the monomorph 'Bearing$GasketSeal', got: {:?}",
        sub_b.structure_name
    );
    assert_eq!(
        sub_b.type_args.first(),
        Some(&Type::StructureRef("GasketSeal".to_string())),
        "sub 'b' type_args[0] must be StructureRef(\"GasketSeal\"), got: {:?}",
        sub_b.type_args
    );
}

/// Regression lock (invariant 2): a module with no `auto:` use-sites produces
/// zero monomorph templates and leaves `ctx.templates` unchanged.
///
/// The empty-queue early-return at `auto_type_param_phase.rs:83` guarantees this.
/// This test pins that invariant so a future refactor cannot accidentally
/// introduce monomorphs for non-`auto:` modules.
#[test]
fn no_auto_module_produces_zero_monomorphs() {
    let source = r#"
        trait Seal {}
        structure def GasketSeal : Seal { param d : Real = 2.0 }
        structure def Bearing { param d : Real = 10.0 }
        structure def Assembly { sub b = Bearing() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    // No template name should contain '$' (the monomorph name separator).
    let monomorphs: Vec<&str> = compiled
        .templates
        .iter()
        .filter(|t| t.name.contains('$'))
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        monomorphs.is_empty(),
        "no-auto: module must produce zero monomorph templates (none with '$' in name), got: {:?}",
        monomorphs
    );
}
