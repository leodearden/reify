//! Integration tests for task 4493 (type-hygiene ζ): compile-time per-arg type
//! signatures for builtin families.
//!
//! Uses real `.ri` snippets compiled against the stdlib, inspecting
//! `module.diagnostics` for `ArgTypeMismatch` codes.
//!
//! Cases 1 & 3 are RED before step-6 wires `check_builtin_arg_types` into
//! `expr.rs`; cases 2, 4, 5 are no-error guards that hold both before and after
//! wiring.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ─── helper ────────────────────────────────────────────────────────────────────

/// Wrap `body` in a minimal structure def containing a box geometry, then
/// compile with the full stdlib prelude.  The caller provides the interior
/// let-bindings; `b` (a `box(50mm,30mm,10mm)`) is always in scope.
fn compile_struct_body(body: &str) -> reify_compiler::CompiledModule {
    let source = format!(
        "structure def Test {{\n    let b = box(50mm, 30mm, 10mm)\n{body}\n}}"
    );
    compile_source_with_stdlib(&source)
}

// ── Case 1: SIGNAL — moment_of_inertia with bare Real density ─────────────────

/// A bare-Real `7850.0` passed as the density argument to `moment_of_inertia`
/// is a DEFINITE dimensionless-where-Density-expected mismatch.  Once wired,
/// the compiler must emit exactly 1 `ArgTypeMismatch` Error naming "Density"
/// and "moment_of_inertia".
///
/// RED before step-6 wires the check into expr.rs.
#[test]
fn moment_of_inertia_bare_real_density_gives_arg_type_mismatch() {
    let compiled = compile_struct_body("    let i = moment_of_inertia(b, 7850.0)\n");
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error)
        .collect();
    assert!(
        !arg_type_mismatches.is_empty(),
        "expected at least 1 ArgTypeMismatch error for bare-Real density arg, got no ArgTypeMismatch.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    // The error should name the builtin and expected type.
    let d = &arg_type_mismatches[0];
    assert!(
        d.message.contains("moment_of_inertia"),
        "message should name the builtin 'moment_of_inertia': {}",
        d.message
    );
    assert!(
        d.message.contains("Density"),
        "message should name the expected type 'Density': {}",
        d.message
    );
}

// ── Case 2: BOUNDARY ok — dimensioned density → no ArgTypeMismatch ────────────

/// Passing `7850kg/m^3` (a `Scalar{MASS_DENSITY}` literal) to
/// `moment_of_inertia` is the correct form.  Must compile with NO
/// `ArgTypeMismatch` diagnostic (both before and after wiring).
#[test]
fn moment_of_inertia_dimensioned_density_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let d = 7850kg/m^3\n    let i = moment_of_inertia(b, d)\n",
    );
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect();
    assert!(
        arg_type_mismatches.is_empty(),
        "moment_of_inertia with dimensioned 7850kg/m^3 density must emit no ArgTypeMismatch, \
         got: {:#?}",
        arg_type_mismatches
    );
}

// ── Case 3: SIGNAL — faces_by_normal with LENGTH tol ──────────────────────────

/// `5.0mm` (a length scalar) passed as the `tol` argument to `faces_by_normal`
/// where an ANGLE is expected is a DEFINITE dimension mismatch.  Once wired,
/// the compiler must emit exactly 1 `ArgTypeMismatch` Error naming "Angle".
///
/// RED before step-6 wires the check into expr.rs.
#[test]
fn faces_by_normal_length_tol_gives_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let dir = vec3(0.0, 0.0, 1.0)\n    let sel = faces_by_normal(b, dir, 5.0mm)\n",
    );
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error)
        .collect();
    assert!(
        !arg_type_mismatches.is_empty(),
        "expected at least 1 ArgTypeMismatch error for LENGTH tol where ANGLE expected, \
         got no ArgTypeMismatch.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
    let d = &arg_type_mismatches[0];
    assert!(
        d.message.contains("Angle"),
        "message should name the expected type 'Angle': {}",
        d.message
    );
}

// ── Case 4: BOUNDARY ok — faces_by_normal with ANGLE tol → no ArgTypeMismatch ─

/// `1deg` (an angle scalar) passed as `tol` to `faces_by_normal` is correct.
/// Must compile with NO `ArgTypeMismatch` diagnostic.
#[test]
fn faces_by_normal_angle_tol_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let dir = vec3(0.0, 0.0, 1.0)\n    let sel = faces_by_normal(b, dir, 1deg)\n",
    );
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect();
    assert!(
        arg_type_mismatches.is_empty(),
        "faces_by_normal with 1deg tol must emit no ArgTypeMismatch, got: {:#?}",
        arg_type_mismatches
    );
}

// ── Case 5: STDLIB REGRESSION GUARD — material.density path ──────────────────

/// The stdlib `Rigid` trait (structural_physical.ri) injects
/// `let moment_of_inertia = moment_of_inertia(geometry, body_density)` where
/// `body_density = material.density` which is `Scalar{MASS_DENSITY}`.
///
/// Since that call site is typechecked on EVERY stdlib load once the check is
/// wired, a false-positive on `material.density` would break the entire stdlib.
///
/// This regression guard compiles an explicit snippet matching the same shape
/// and asserts NO `ArgTypeMismatch` is emitted.  Holds both before and after
/// wiring.
#[test]
fn moment_of_inertia_via_material_density_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body(concat!(
        "    param material : Material = Material(\n",
        "        name: \"steel\",\n",
        "        density: 7850kg/m^3,\n",
        "        youngs_modulus: 200GPa\n",
        "    )\n",
        "    let d = material.density\n",
        "    let i = moment_of_inertia(b, d)\n",
    ));
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect();
    assert!(
        arg_type_mismatches.is_empty(),
        "moment_of_inertia(b, material.density) must emit NO ArgTypeMismatch \
         (material.density is Scalar{{MASS_DENSITY}} — exact match). \
         A false-positive here would break the stdlib Rigid trait universally.\n\
         Got: {:#?}",
        arg_type_mismatches
    );
}
