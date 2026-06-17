//! Compiler-layer integration gate — §9 boundary-test table (task #4499/D).
//!
//! # Purpose
//!
//! This file is the COMPILER-layer integration gate for the ambient-default-material
//! PRD (`docs/prds/v0_6/ambient-default-material.md`). It pins the §9 boundary rows
//! that are observable at the compile layer (diagnostics + injected value cells),
//! forming a collected regression surface for the grammar→compiler pipeline.
//!
//! No numeric mass magnitude is asserted here — that belongs to the eval gate
//! (`crates/reify-eval/tests/ambient_default_material_integration_gate.rs`).
//!
//! # §9 boundary-test table — compiler-layer rows + owner cross-reference
//!
//! | Row | Description                                                      | Owner (this file fn)                                               |
//! |-----|------------------------------------------------------------------|--------------------------------------------------------------------|
//! | 1   | parse forms: top-level + purpose-nested both accepted            | `row_1_parse_forms_top_level_and_purpose_nested`                  |
//! | 2   | injection fills required param + mass evaluates (e2e positive)   | `crates/reify-eval/tests/ambient_default_material_integration_gate.rs` |
//! | 3   | explicit member wins over ambient default (DD3)                  | `row_3_explicit_member_wins`                                      |
//! | 4   | file-level + purpose-nested coexist, no cross-scope duplicate    | `row_4_purpose_file_coexistence_no_cross_scope_dup`               |
//! | 5   | duplicate same-scope → exactly one DuplicateAmbientDefault error | `row_5_duplicate_same_scope_errors`                               |
//! | 6   | wrong value type → AmbientDefaultTypeMismatch at decl span       | `row_6_wrong_value_type_errors_at_decl`                           |
//! | 7   | no ambient + no material → E_DynamicsNoDensity (hard error)      | `crates/reify-eval/tests/ambient_default_material_integration_gate.rs` |
//! | 8   | water-default symbol absent from production source               | `crates/reify-eval/tests/ambient_default_material_integration_gate.rs` |

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// A fully-valid `Material(...)` constructor expression (all three required
/// params provided), usable as an ambient-default value without itself
/// producing a missing-member error.
///
/// `steel` is NOT a stdlib top-level binding (only a private Rust helper in
/// flexures). The full ctor form is required for the compiler to accept it.
const STEEL_CTOR: &str =
    r#"Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)"#;

/// A distinct, fully-valid `Material(...)` constructor, used to distinguish the
/// explicitly-provided value from the in-scope ambient steel default.
const ALUMINUM_CTOR: &str =
    r#"Material(name: "aluminum", density: 2700kg/m^3, youngs_modulus: 69GPa)"#;

// ─── §9 row 1: parse forms — top-level + purpose-nested both accepted ─────────

/// §9 row 1: a source containing a top-level `default Material = <ctor>` AND
/// a purpose-nested `default Material = <ctor>` compiles with ZERO Error-severity
/// diagnostics and no parse errors.
///
/// This gates the grammar→compiler pipeline for both declaration forms.
/// The tree-sitter fixtures for these forms are in A's
/// `tree-sitter-reify/test/fixtures/ambient-default-{1,2}.ri`.
#[test]
fn row_1_parse_forms_top_level_and_purpose_nested() {
    let source = format!(
        r#"
default Material = {STEEL_CTOR}

purpose Exploration() {{
    default Material = {STEEL_CTOR}
}}
"#
    );
    let module = compile_source_with_stdlib(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "§9 row 1: top-level + purpose-nested `default Material = ...` forms must compile with \
         zero Error-severity diagnostics (grammar→compiler pipeline accepts both forms); \
         got errors: {errors:#?}"
    );
}

// ─── §9 row 3: explicit member wins over ambient default (DD3) ────────────────

/// §9 row 3: a `Bracket : Physical` declaring `param material : Material =
/// aluminum` while a file-level steel default is in scope keeps exactly ONE
/// `material` value cell carrying the **explicit** aluminum value, not the
/// ambient steel default.
///
/// This mirrors B's `explicit_material_member_wins_over_ambient_default`
/// (ambient_default_injection_tests.rs); here it is re-pinned as the §9 row-3
/// canonical test in the integration gate.
#[test]
fn row_3_explicit_member_wins() {
    let source = format!(
        r#"
default Material = {STEEL_CTOR}

structure def Bracket : Physical {{
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = {ALUMINUM_CTOR}
}}
"#
    );
    let compiled = compile_source_with_stdlib(&source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§9 row 3: explicit material member + in-scope ambient default must compile cleanly; \
         got errors: {errors:#?}"
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("expected a compiled `Bracket` template");

    let material_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "material")
        .collect();

    assert_eq!(
        material_cells.len(),
        1,
        "§9 row 3: explicit member wins — expected exactly one `material` cell \
         (ambient not injected over it); got {} cells",
        material_cells.len()
    );

    // The retained cell carries the EXPLICIT aluminum value, not the steel ambient (DD3).
    let debug = format!("{:?}", material_cells[0].default_expr);
    assert!(
        debug.contains("aluminum"),
        "§9 row 3: the `material` cell must carry the explicit aluminum value; got: {debug}"
    );
    assert!(
        !debug.contains("steel"),
        "§9 row 3: the ambient steel default must be ignored when an explicit member exists; \
         got: {debug}"
    );
}

// ─── §9 row 4: file-level + purpose-nested coexist (no cross-scope dup) ──────

/// §9 row 4 (COEXISTENCE leg): a file-level `default Material = steel` and a
/// purpose-nested `default Material = aluminum` in ONE source produce ZERO
/// `DiagnosticCode::DuplicateAmbientDefault` errors and ZERO Error-severity
/// diagnostics — because the duplicate check is per-scope (DD5), not cross-scope.
///
/// **Resolver-unit innermost-wins leg** is B's existing
/// `ambient_defaults.rs::purpose_entry_wins_over_file_entry_innermost`
/// (referenced here, NOT duplicated).
///
/// **Positive-eval direction** — "a structure governed by the purpose evaluates
/// to aluminum" — is DEFERRED to #4639 and is NOT asserted here per the task
/// DISPOSITION: unsatisfiable on v1 (grammar forbids structures in purpose
/// bodies; B/DD6 declines purpose-scoped injection into file-level structures).
#[test]
fn row_4_purpose_file_coexistence_no_cross_scope_dup() {
    let source = format!(
        r#"
default Material = {STEEL_CTOR}

purpose Exploration() {{
    default Material = {ALUMINUM_CTOR}
}}
"#
    );
    let module = compile_source_with_stdlib(&source);

    // Zero DuplicateAmbientDefault: file-scope steel and purpose-scope aluminum
    // are in different scopes, so DD5's per-scope check never fires.
    let dups: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DuplicateAmbientDefault))
        .collect();
    assert!(
        dups.is_empty(),
        "§9 row 4: file-level steel default + purpose-nested aluminum default must produce \
         ZERO DuplicateAmbientDefault errors (per-scope check, not cross-scope); \
         got dups: {dups:#?}\n(all diagnostics: {:#?})",
        module.diagnostics
    );

    // Zero Error-severity diagnostics overall.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "§9 row 4: file-level + purpose-nested `default Material` coexistence must \
         produce zero Error-severity diagnostics; got: {errors:#?}"
    );
}

// ─── §9 row 5: duplicate same-scope → exactly one DuplicateAmbientDefault ────

/// §9 row 5: two top-level `default Material = ...` declarations in the same
/// (file) scope produce exactly one `DiagnosticCode::DuplicateAmbientDefault`
/// error (DD5 per-scope rule).
#[test]
fn row_5_duplicate_same_scope_errors() {
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
        "§9 row 5: two top-level `default Material` declarations must yield exactly one \
         DuplicateAmbientDefault error; got {} (all diagnostics: {:#?})",
        dups.len(),
        module.diagnostics
    );
}

// ─── §9 row 6: wrong value type → AmbientDefaultTypeMismatch at decl ─────────

/// §9 row 6: `default Material = 5mm` declares a Length value against the
/// `Material` type. It must error with `DiagnosticCode::AmbientDefaultTypeMismatch`,
/// and the diagnostic must be anchored at the DECLARATION span (DD4), not at a
/// use site (there is none in this source).
#[test]
fn row_6_wrong_value_type_errors_at_decl() {
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
        "§9 row 6: `default Material = 5mm` (Length vs Material) must yield exactly one \
         AmbientDefaultTypeMismatch error; got {} (all diagnostics: {:#?})",
        mismatches.len(),
        module.diagnostics
    );

    // The error must be anchored at the declaration span (DD4), not a use site.
    // The labelled source slice must contain "Material" (the declared type name).
    let d = mismatches[0];
    assert!(
        !d.labels.is_empty(),
        "§9 row 6: type-mismatch error must carry a label at the declaration span"
    );
    let span = d.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("Material"),
        "§9 row 6: label must be anchored at the `default Material = ...` declaration \
         (decl-site, DD4); got slice {sliced:?}"
    );
}
