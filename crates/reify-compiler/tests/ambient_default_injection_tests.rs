//! Compiler-level tests for ambient-default scope resolution + conformance
//! injection (ambient-default-material task B).
//!
//! All signals are pinned at the COMPILE level via `compile_source_with_stdlib`
//! (diagnostics + injected value cells), mirroring the existing Physical tests.
//! No numeric mass magnitude is asserted — that belongs to task D's eval gate.
//!
//! `Material` (a stdlib `structure def` with required params `name : String`,
//! `density : Density`, `youngs_modulus : Pressure`) and the `Physical` trait
//! (`param material : Material`, REQUIRED) are stdlib substrate seeded by the
//! prelude.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// A fully-valid `Material(...)` constructor expression (all three required
/// params provided), usable as an ambient-default value without itself
/// producing a missing-member error.
const STEEL_CTOR: &str =
    r#"Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)"#;

// ─── (a) same-scope duplicate (file scope) ──────────────────────────────────

/// Two top-level `default Material = ...` declarations in the same (file) scope
/// produce exactly one `DiagnosticCode::DuplicateAmbientDefault` error (DD5).
#[test]
fn duplicate_top_level_default_is_one_dup_error() {
    let source = format!("default Material = {STEEL_CTOR}\ndefault Material = {STEEL_CTOR}");
    let module = compile_source_with_stdlib(&source);

    let dups: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DuplicateAmbientDefault))
        .collect();

    assert_eq!(
        dups.len(),
        1,
        "two same-type top-level defaults should yield exactly one \
         DuplicateAmbientDefault error; got diagnostics: {:?}",
        module.diagnostics
    );
}

// ─── (b) declaration-site type mismatch (file scope) ────────────────────────

/// `default Material = 5mm` declares a Length value against the `Material` type.
/// It must error with `DiagnosticCode::AmbientDefaultTypeMismatch`, and the
/// diagnostic must be anchored at the DECLARATION span (DD4), not at a use site
/// (there is none in this source).
#[test]
fn type_mismatch_default_errors_at_declaration() {
    let source = "default Material = 5mm";
    let module = compile_source_with_stdlib(source);

    let mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AmbientDefaultTypeMismatch))
        .collect();

    assert_eq!(
        mismatches.len(),
        1,
        "a Length default for Material should yield exactly one \
         AmbientDefaultTypeMismatch error; got diagnostics: {:?}",
        module.diagnostics
    );

    // The error is attributed to the declaration, not a use site. The decl span
    // covers the whole `default Material = 5mm` declaration, so the labelled
    // source slice names the declared type.
    let d = mismatches[0];
    assert!(
        !d.labels.is_empty(),
        "type-mismatch error should carry a label at the declaration span"
    );
    let span = d.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("Material"),
        "label should be anchored at the `default Material = ...` declaration, \
         got slice {sliced:?}"
    );
}

// ─── (c) single valid default: no error, no leftover wired warning ──────────

/// A single valid top-level `default Material = Material(...)` with no consumer
/// compiles with NO error AND NO leftover `W_DEFAULT_NOT_WIRED` warning (the
/// task-A placeholder warning is fully replaced by real semantics).
#[test]
fn single_valid_default_has_no_error_and_no_wired_warning() {
    let source = format!("default Material = {STEEL_CTOR}");
    let module = compile_source_with_stdlib(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a single valid ambient default should produce no errors; got: {errors:?}"
    );

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("W_DEFAULT_NOT_WIRED")),
        "the task-A W_DEFAULT_NOT_WIRED warning must be gone for top-level \
         defaults; got diagnostics: {:?}",
        module.diagnostics
    );
}
