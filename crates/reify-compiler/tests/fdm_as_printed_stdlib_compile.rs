//! Tests for `crates/reify-compiler/stdlib/fdm_as_printed.ri` —
//! `std.fdm.as_printed` module: the `AsPrintedOptions` options record and the
//! `@optimized("fdm::as_printed_material_r_fast")` `as_printed_material`
//! function — the R-fast call surface that produces a heterogeneous
//! `Field<Point3<Length>, AnisotropicMaterial>` for an FDM-printed body.
//!
//! PRD reference: docs/prds/v0_5/fdm-as-printed-fea.md task δ.
//!
//! These tests pin the human-facing surface: the function's `@optimized`
//! dispatch target, its resolved return type, and that a user source calling
//! `as_printed_material(...)` + `AsPrintedOptions(...)` compiles cleanly
//! through the production stdlib prelude path.

use reify_compiler::*;
use reify_core::*;
use reify_test_support::compile_source_with_stdlib;

/// Return the `std/fdm/as_printed` CompiledModule from the production stdlib
/// loader (same embedded + sequential-prelude path as production).
fn load_stdlib_module() -> &'static CompiledModule {
    reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fdm/as_printed")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fdm/as_printed module; available paths: {:?}",
                reify_compiler::stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// The std/fdm/as_printed module must load through the production stdlib path
/// with zero error-severity diagnostics.
#[test]
fn std_fdm_as_printed_module_loads_with_no_errors() {
    let module = load_stdlib_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in fdm_as_printed.ri: {:?}",
        errors
    );
}

/// `as_printed_material` must be an `@optimized` ComputeNode whose dispatch
/// target is the R-fast trampoline string, and whose resolved return type is
/// the heterogeneous material field `Field<Point3<Length>, AnisotropicMaterial>`.
#[test]
fn as_printed_material_is_optimized_field_producer() {
    let module = load_stdlib_module();
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "as_printed_material")
        .unwrap_or_else(|| {
            panic!(
                "expected `fn as_printed_material` in std/fdm/as_printed; got functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        func.optimized_target,
        Some("fdm::as_printed_material_r_fast".to_string()),
        "as_printed_material must dispatch to the R-fast trampoline target"
    );

    // Return type: Field<Point3<Length>, AnisotropicMaterial>. Check
    // structurally — the Length alias resolves to its canonical Scalar[m]
    // dimension, so an exact Display match on "Length" is wrong; pin the
    // δ-relevant shape instead (Field, Point3 domain, AnisotropicMaterial
    // codomain).
    match &func.return_type {
        Type::Field { domain, codomain } => {
            assert!(
                matches!(domain.as_ref(), Type::Point { n: 3, .. }),
                "as_printed_material domain should be Point3<...>, got: {}",
                domain
            );
            assert_eq!(
                format!("{}", codomain),
                "AnisotropicMaterial",
                "as_printed_material codomain should be AnisotropicMaterial, got: {}",
                codomain
            );
        }
        other => panic!(
            "as_printed_material return type should be a Field, got: {:?}",
            other
        ),
    }
}

/// `AsPrintedOptions` is the consumer-side options record. It must exist as a
/// structure template in the module.
#[test]
fn as_printed_options_structure_exists() {
    let module = load_stdlib_module();
    let found = module
        .templates
        .iter()
        .any(|t| t.name == "AsPrintedOptions" && t.entity_kind == EntityKind::Structure);
    assert!(
        found,
        "expected `structure def AsPrintedOptions` template in std/fdm/as_printed; got: {:?}",
        module
            .templates
            .iter()
            .map(|t| (&t.name, &t.entity_kind))
            .collect::<Vec<_>>()
    );
}

/// User-observable signal: a source that calls `as_printed_material(...)` with
/// a body, an `FDMProcess`, and an `AsPrintedOptions(...)` (with a subset of
/// overrides) must compile cleanly through the stdlib prelude, and the bound
/// result must resolve to the heterogeneous material field type.
#[test]
fn as_printed_material_call_compiles_cleanly() {
    let source = r#"
fn probe(body: Solid) -> Field<Point3<Length>, AnisotropicMaterial> {
    as_printed_material(body, FDMProcess(), AsPrintedOptions(orthotropic: true))
}
"#;
    let compiled = compile_source_with_stdlib(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "as_printed_material call should compile without errors; got: {:?}",
        errors
    );
}
