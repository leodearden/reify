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
use reify_test_support::compile_source_with_stdlib;

// ─── helper ────────────────────────────────────────────────────────────────────

/// Wrap `body` in a minimal structure def containing a box geometry, then
/// compile with the full stdlib prelude.  The caller provides the interior
/// let-bindings; `b` (a `box(50mm,30mm,10mm)`) is always in scope.
fn compile_struct_body(body: &str) -> reify_compiler::CompiledModule {
    let source = format!("structure def Test {{\n    let b = box(50mm, 30mm, 10mm)\n{body}\n}}");
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
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
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
    let compiled =
        compile_struct_body("    let d = 7850kg/m^3\n    let i = moment_of_inertia(b, d)\n");
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
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
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

// ── Case 6a: SIGNAL — edges_at_height with bare-Real h (LENGTH expected) ─────

/// `5.0` (a dimensionless Real) passed as the `h` argument to `edges_at_height`
/// where a LENGTH is expected is a DEFINITE dimension mismatch.  Once wired,
/// the compiler must emit an `ArgTypeMismatch` Error naming "Length".
#[test]
fn edges_at_height_bare_real_h_gives_arg_type_mismatch() {
    let compiled = compile_struct_body("    let sel = edges_at_height(b, 5.0, 0.01mm)\n");
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect();
    assert!(
        !arg_type_mismatches.is_empty(),
        "expected at least 1 ArgTypeMismatch error for bare-Real h arg to edges_at_height, \
         got no ArgTypeMismatch.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
    let d = &arg_type_mismatches[0];
    assert!(
        d.message.contains("Length"),
        "message should name the expected type 'Length': {}",
        d.message
    );
}

// ── Case 6b: BOUNDARY ok — edges_at_height with Length args → no ArgTypeMismatch

/// `5mm` and `0.01mm` (Length scalars) passed to `edges_at_height` are the
/// correct forms.  Must compile with NO `ArgTypeMismatch` diagnostic.
#[test]
fn edges_at_height_length_args_give_no_arg_type_mismatch() {
    let compiled = compile_struct_body("    let sel = edges_at_height(b, 5mm, 0.01mm)\n");
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect();
    assert!(
        arg_type_mismatches.is_empty(),
        "edges_at_height with dimensioned Length args must emit no ArgTypeMismatch, \
         got: {:#?}",
        arg_type_mismatches
    );
}

// ── Case 6c: BOUNDARY ok — edges_parallel_to with Angle tol → no ArgTypeMismatch

/// `1deg` (an Angle scalar) passed as the `tol` argument to `edges_parallel_to`
/// is the correct form.  Must compile with NO `ArgTypeMismatch` diagnostic.
///
/// Guards against an arg-position or coercion regression specific to the
/// `edges_parallel_to` call shape (arg2 ANGLE check at index 2).
#[test]
fn edges_parallel_to_angle_tol_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let dir = vec3(0.0, 0.0, 1.0)\n    let sel = edges_parallel_to(b, dir, 1deg)\n",
    );
    let arg_type_mismatches: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect();
    assert!(
        arg_type_mismatches.is_empty(),
        "edges_parallel_to with 1deg tol must emit no ArgTypeMismatch, got: {:#?}",
        arg_type_mismatches
    );
}

// ── Task 5652: pattern spacing must be a Length at compile time ───────────────

/// Every `ArgTypeMismatch` Error in `compiled`.
fn arg_type_mismatch_errors(
    compiled: &reify_compiler::CompiledModule,
) -> Vec<&reify_core::Diagnostic> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect()
}

/// SIGNAL — a bare `10` passed as `linear_pattern`'s `spacing` is a DEFINITE
/// Int-where-Length-expected mismatch.
///
/// Task 5214 already rejects this at EVAL (a `Warning`-then-drop via
/// `required_length_value`). This pins the compile-layer upgrade: an `Error`
/// with a span label, emitted before the design is ever built.
///
/// Exactly 1, not "at least 1": a duplicate would mean the diagnostic is being
/// emitted from two wiring sites.
#[test]
fn linear_pattern_bare_spacing_gives_one_arg_type_mismatch() {
    let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare `10` spacing.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    for needle in ["linear_pattern", "spacing", "Length"] {
        assert!(
            errors[0].message.contains(needle),
            "message must name {needle:?}: {}",
            errors[0].message
        );
    }
}

/// BOUNDARY ok — a dimensioned `10mm` spacing is the correct form and must emit
/// NO `ArgTypeMismatch`. Without this the signal case above could pass for the
/// wrong reason (e.g. a slot that fires on every `linear_pattern` call).
#[test]
fn linear_pattern_dimensioned_spacing_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "linear_pattern with a dimensioned 10mm spacing must emit no \
         ArgTypeMismatch, got: {:#?}",
        errors
    );
}

/// SIGNAL — a DIMENSIONED but WRONG-dimension `10deg` spacing is also rejected,
/// naming the expected `Length` and the offending unit.
///
/// Not a duplicate of the bare-`10` case: that one is a kind mismatch (`Int`
/// where a dimensioned scalar is required), this one a dimension mismatch
/// between two dimensioned scalars, and the two travel different arms of
/// `check_builtin_arg_types`. It is also the likelier slip in practice — the
/// user who has learned that spacing needs a unit can still reach for the wrong
/// one, and gets a differently-worded message for it.
///
/// Pins the observed message end-to-end, including that the ACTUAL side renders
/// as `Scalar[rad]` (`Type::Display`'s SI base-unit form) rather than the
/// friendly `"Angle"` used on the expected side of other slots' messages.
#[test]
fn linear_pattern_wrong_dimension_spacing_gives_one_arg_type_mismatch() {
    let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10deg)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a `10deg` spacing.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message, "linear_pattern: spacing argument expects Length, got Scalar[rad]",
        "a wrong-unit spacing must name the builtin, the arg, the expected type \
         and the offending unit"
    );
}

/// SINGLE-EMISSION lock — a pattern call NESTED inside a CSG combinator still
/// yields exactly 1 diagnostic.
///
/// A geometry-`let` routes through `entity.rs -> compile_geometry_call`, but its
/// value expression is ALSO compiled as a value cell via
/// `compile_expr -> resolve_function_overload`, which is where
/// `check_builtin_arg_types` is wired (expr.rs). Adding a second call site in
/// `compile_geometry_call_inner` would therefore DOUBLE-emit here. This test is
/// what makes that regression loud.
#[test]
fn nested_linear_pattern_bare_spacing_emits_exactly_one_diagnostic() {
    let compiled = compile_struct_body(
        "    let c = box(1mm, 1mm, 1mm)\n\
         \x20   let u = union(linear_pattern(b, 1, 0, 0, 5, 10), c)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "a nested linear_pattern must emit exactly 1 ArgTypeMismatch (not 2 — \
         that would mean a duplicate wiring site).\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// SIGNAL — a bare `10` as `linear_pattern_2d`'s `spacing1` (with a correct
/// `20mm` `spacing2`) yields exactly 1 `ArgTypeMismatch` naming `spacing1`.
///
/// This is the shape from task 5214's litter-tray bug, where a bare-spacing
/// grid scattered cutting tools hundreds of metres from the plate.
#[test]
fn linear_pattern_2d_bare_spacing1_gives_one_arg_type_mismatch() {
    let compiled =
        compile_struct_body("    let g = linear_pattern_2d(b, 1, 0, 0, 3, 10, 0, 1, 0, 4, 20mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare `10` spacing1.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert!(
        errors[0].message.contains("spacing1"),
        "message must name the offending arg `spacing1`, not the other axis: {}",
        errors[0].message
    );
    assert!(
        errors[0].message.contains("Length"),
        "message must name the expected type `Length`: {}",
        errors[0].message
    );
}

/// BOUNDARY ok — both spacings dimensioned → NO `ArgTypeMismatch`. Keeps the
/// signal case above from passing for the wrong reason.
#[test]
fn linear_pattern_2d_dimensioned_spacings_give_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let g = linear_pattern_2d(b, 1, 0, 0, 3, 10mm, 0, 1, 0, 4, 20mm)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "linear_pattern_2d with dimensioned 10mm/20mm spacings must emit no \
         ArgTypeMismatch, got: {:#?}",
        errors
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
