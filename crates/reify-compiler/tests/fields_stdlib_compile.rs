//! Tests for `crates/reify-compiler/stdlib/fields.ri` —
//! `std.fields` module: documentation-only packaging of the existing
//! built-in field differential operators (gradient, divergence, curl,
//! laplacian, sample). No pub fn or pub type is declared — the module
//! is a packaging surface only per PRD decision 5.
//!
//! Reconstructs the lost std.fields stdlib module per PRD
//! docs/prds/v0_6/stdlib-reconstruction.md §Slice C.
//!
//! Tests use the production-path `load_stdlib()` helper, modeled on
//! `process_stdlib_compile.rs` and `constants_example_tests.rs`.

use reify_core::{ModulePath, Severity};
use reify_test_support::errors_only;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/fields` CompiledModule from the production stdlib loader.
/// Panics with a diagnostic listing available modules if the path is absent.
fn load_stdlib_module() -> &'static reify_compiler::CompiledModule {
    reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fields")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fields module; available paths: {:?}",
                reify_compiler::stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

// ─── invariant: std/fields module loads clean ────────────────────────────────

/// The std/fields module must load through the production stdlib path with zero
/// error-severity diagnostics. Any circular `pub type Field<D,C> = Field<D,C>`
/// self-alias (esc-4025-76) would produce an Error diagnostic and fail here,
/// so the diagnostics check is the authoritative guard — no source-text grep needed.
#[test]
fn std_fields_loads_clean() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in fields.ri: {:?}",
        errors
    );
}

// ─── invariant: examples/stdlib/fields.ri compiles clean ────────────────────

/// `examples/stdlib/fields.ri` must parse without errors, compile under stdlib
/// with zero Error diagnostics, contain `import std.fields`, and declare a
/// `temp` field whose domain_type starts with "Point3" and codomain_type
/// contains "Scalar".
#[test]
fn example_fields_ri_compiles_clean_with_imported_module() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/stdlib/fields.ri"
    );

    let src = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "failed to read examples/stdlib/fields.ri — \
         check CARGO_MANIFEST_DIR resolution and that the file exists",
    );

    // Import guard.
    assert!(
        src.contains("import std.fields"),
        "examples/stdlib/fields.ri must contain `import std.fields`"
    );

    // Parse.
    let parsed = reify_syntax::parse(&src, ModulePath::single("fields"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in examples/stdlib/fields.ri: {:?}",
        parsed.errors
    );

    // Compile.
    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling examples/stdlib/fields.ri under stdlib, \
         got:\n{:#?}",
        errors
    );

    // Structural assertion: `temp` field must be present with correct types.
    let temp_field = module
        .fields
        .iter()
        .find(|f| f.name == "temp")
        .unwrap_or_else(|| {
            panic!(
                "examples/stdlib/fields.ri should declare a field named `temp`; \
                 found fields: {:?}",
                module.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    let domain_display = format!("{}", temp_field.domain_type);
    assert!(
        domain_display.starts_with("Point3"),
        "temp field domain_type should start with 'Point3', got '{}'",
        domain_display
    );

    let codomain_display = format!("{}", temp_field.codomain_type);
    assert!(
        codomain_display.contains("Scalar"),
        "temp field codomain_type should contain 'Scalar', got '{}'",
        codomain_display
    );
}

// ─── InterpolationMethod enum resolves in user source (task 4221 γ) ─────────

/// Behavioral (B-signal): `InterpolationMethod.Linear` must resolve as an
/// enum-access expression in user source compiled with the stdlib.
///
/// A structure that uses `InterpolationMethod.Linear` as a `let` binding
/// default must compile with zero `Severity::Error` diagnostics once
/// `enum InterpolationMethod` is declared in `fields.ri` and flattened
/// into the prelude `enum_defs`.
///
/// **RED before step-2**: `InterpolationMethod` is undeclared → `EnumAccess`
/// poisons with "unknown enum type 'InterpolationMethod'" → Error diagnostics.
///
/// **GREEN after step-2**: the enum is declared in fields.ri, flattened into
/// the prelude by `enums_phase::flatten_prelude_enum_defs`, and resolves
/// from any user file without an explicit import — same as `InfillPattern`
/// (fdm.ri) and `SignalKind` (ports).
///
/// Also de-risks the `#no_prelude` prelude-export premise: fields.ri's
/// `#no_prelude` only suppresses what fields.ri itself sees during its OWN
/// compilation; it does NOT remove fields.ri's own enums from the exported
/// prelude.
#[test]
fn interpolation_method_linear_resolves_in_user_source() {
    let source = r#"
structure def InterpolationDemo {
    let method = InterpolationMethod.Linear
}
"#;
    let compiled = reify_test_support::compile_source_with_stdlib(source);

    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "structure using InterpolationMethod.Linear should compile without errors; got: {:?}",
        errors
    );
}
