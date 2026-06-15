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

/// A distinct, fully-valid `Material(...)` constructor, used by the
/// explicit-member-wins test to distinguish the explicitly-provided value from
/// the in-scope ambient steel default by name (`"aluminum"` vs `"steel"`).
const ALUMINUM_CTOR: &str =
    r#"Material(name: "aluminum", density: 2700kg/m^3, youngs_modulus: 69GPa)"#;

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

// ─── (a) file-scope injection fills an omitted Material member ───────────────

/// With a file-level `default Material = Material(...)` in scope, a top-level
/// `Bracket : Physical` that OMITS `material` conforms cleanly: the ambient
/// default is injected as the structure's `material` value cell, and `mass`
/// (the Physical `let mass = volume(geometry) * material.density`) derives
/// because `material` is now present — the user-observable task-B signal.
///
/// Mirrors `bracket_conforms_to_physical_with_geometry_and_material`
/// (structural_physical_tests.rs) at the compile level (no numeric magnitude).
#[test]
fn file_default_injects_material_into_top_level_structure() {
    let source = format!(
        r#"
default Material = {STEEL_CTOR}

structure def Bracket : Physical {{
    param geometry : Solid = box(10mm, 20mm, 30mm)
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
        "Bracket should conform to Physical via the injected ambient material \
         default; got errors: {errors:?}"
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("expected a compiled `Bracket` template");

    for expected_cell in &["material", "mass"] {
        assert!(
            template
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *expected_cell),
            "expected an injected '{expected_cell}' value cell on Bracket; got cells: {:?}",
            template
                .value_cells
                .iter()
                .map(|vc| vc.id.member.as_str())
                .collect::<Vec<_>>()
        );
    }
}

// ─── (b) no default in scope → missing-member error (negative control) ───────

/// The SAME `Bracket : Physical` WITHOUT any ambient `default Material` line
/// errors on the missing required `material` member — confirming injection is
/// what fills it in (a), not some unrelated default.
#[test]
fn bracket_without_default_errors_on_missing_material() {
    let source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::MissingRequiredMember)),
        "Bracket without an ambient material default should error on the \
         missing `material` member; got diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ─── (c) explicit member wins over the ambient default (DD3) ─────────────────

/// A `Bracket : Physical` that provides its OWN `param material : Material =
/// Material(name: "aluminum", ...)` while a file-level steel default is in scope
/// keeps the explicit aluminum value: exactly ONE `material` cell, and it is the
/// explicit value — the ambient default is never injected over an explicit
/// member (DD3, resolution ladder: explicit > trait-default > ambient).
#[test]
fn explicit_material_member_wins_over_ambient_default() {
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
        "explicit material member + in-scope ambient default should compile \
         cleanly; got errors: {errors:?}"
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
        "explicit member wins: expected exactly one `material` cell (ambient not \
         injected over it); got {} cells",
        material_cells.len()
    );

    // The retained cell carries the EXPLICIT aluminum value, not the steel
    // ambient (DD3). The compiled ctor expr embeds its `name:` string literal.
    let debug = format!("{:?}", material_cells[0].default_expr);
    assert!(
        debug.contains("aluminum"),
        "the `material` cell should carry the explicit aluminum value; got: {debug}"
    );
    assert!(
        !debug.contains("steel"),
        "the ambient steel default must be ignored when an explicit member \
         exists; got: {debug}"
    );
}

// ─── (d) multiple Material-typed params are all injected ────────────────────

/// A structure conforming to a trait with TWO required Material-typed params,
/// both OMITTED, gets BOTH filled from a single file-level ambient default —
/// the table is keyed by type name (DD1), so every unfilled
/// `Param(StructureRef("Material"))` requirement is a candidate.
#[test]
fn multiple_material_params_are_all_injected() {
    let source = format!(
        r#"
default Material = {STEEL_CTOR}

trait DualMaterial {{
    param primary : Material
    param secondary : Material
}}

structure def TwoMats : DualMaterial {{
    param geometry : Solid = box(10mm, 20mm, 30mm)
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
        "both Material params should be filled by the single ambient default; \
         got errors: {errors:?}"
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "TwoMats")
        .expect("expected a compiled `TwoMats` template");

    for expected_cell in &["primary", "secondary"] {
        assert!(
            template
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *expected_cell),
            "expected an injected '{expected_cell}' Material cell on TwoMats; got cells: {:?}",
            template
                .value_cells
                .iter()
                .map(|vc| vc.id.member.as_str())
                .collect::<Vec<_>>()
        );
    }
}
