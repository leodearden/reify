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

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/fields` CompiledModule from the production stdlib loader.
/// Panics if absent — which is the expected failure mode until step-2
/// registers the module and creates fields.ri.
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

// ─── step-1: module loads clean + no circular Field alias ────────────────────

/// The std/fields module must load through the production stdlib path with zero
/// error-severity diagnostics, and the source must NOT contain a `pub type Field`
/// declaration (esc-4025-76: a `type Field<D,C> = Field<D,C>` self-alias is a
/// HARD circular-alias Error that panics stdlib_loader's assert! and breaks
/// the whole build; the builtin Field<D,C> resolves everywhere without import).
#[test]
fn std_fields_loads_clean_and_has_no_field_alias() {
    let module = load_stdlib_module();

    // Zero Error diagnostics.
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

    // Regression guard: no `pub type Field` (circular-alias esc-4025-76).
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/stdlib/fields.ri"
    ))
    .expect("stdlib/fields.ri should exist");
    assert!(
        !src.contains("pub type Field"),
        "fields.ri must NOT declare `pub type Field` — \
         the builtin Field<D,C> resolves without import and a self-alias \
         is a circular-alias Error (esc-4025-76)"
    );
}

// ─── step-3: examples/stdlib/fields.ri compiles clean ────────────────────────

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
