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
        errors[0].message,
        "linear_pattern: spacing argument expects Length, got Scalar[rad]; \
         pass a dimensioned length such as `5mm`",
        "a wrong-unit spacing must name the builtin, the arg, the expected type \
         and the offending unit, and carry the C1 migration hint"
    );
}

/// SINGLE-EMISSION lock — a pattern call NESTED inside a CSG combinator still
/// yields exactly 1 diagnostic.
///
/// A geometry-`let` routes through `entity.rs -> compile_geometry_call`, but its
/// value expression is ALSO compiled as a value cell via
/// `compile_expr -> resolve_function_overload`, which is where
/// `check_builtin_arg_types` is wired (expr.rs).
///
/// SCOPE OF THIS PIN, post-dedup: it no longer proves single-WIRING. Since this
/// task's `emit_mismatch` drops any `ArgTypeMismatch` whose (code, span,
/// message) triple is already in the sink, a second call site in
/// `compile_geometry_call_inner` emitting at the same call span would now be
/// SWALLOWED and this test would stay green. What it still pins is the
/// user-visible contract — exactly one diagnostic reaches the author — which is
/// the property that matters at the CLI. The underlying walk is still doubled;
/// de-duplicating it belongs in `expr.rs` and is tracked as task #6627. Do not
/// cite this test as evidence that the walk is single-wired.
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

// ── Task 5750 (units-length η): the C1 migration hint on compile-layer slots ──
//
// PRD `docs/prds/v0_6/units-length-gate-completion.md`, decision D9. The eval
// layer's `ArgRejection::message` already appends a migration hint to a LENGTH
// rejection; η makes the compile layer reproduce it VERBATIM so the two layers
// read identically for the same authoring mistake.
//
// The tests below pin the hint on an EXISTING LENGTH slot (`linear_pattern`
// spacing), deliberately BEFORE any new slot is added, so the primitive /
// modify / sweep slots that follow are written once against the final
// `ExpectedArg` shape rather than churned by a later field addition.

/// The exact C1 hint clause the eval layer appends to a LENGTH rejection.
///
/// Hard-coded here on purpose: this test file is the DRIFT PIN. Deriving it
/// from the same const the implementation reads would make the assertion a
/// tautology — it would pass for whatever the implementation happened to say.
const LENGTH_HINT: &str = "pass a dimensioned length such as `5mm`";

/// (b) SIGNAL — the BARE-INT arm carries the hint too.
///
/// Not a duplicate of `linear_pattern_wrong_dimension_spacing_gives_one_arg_type_mismatch`:
/// a bare `10` is a KIND mismatch (`Type::Int` where a dimensioned scalar is
/// required) and a `10deg` is a DIMENSION mismatch between two dimensioned
/// scalars. They travel different arms of `check_builtin_arg_types`
/// (`Type::Scalar { .. }` vs the catch-all `other =>`), so pinning the hint on
/// one says nothing about the other.
#[test]
fn linear_pattern_bare_int_spacing_message_carries_the_migration_hint() {
    let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare `10` spacing.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "linear_pattern: spacing argument expects Length, got Int; \
         pass a dimensioned length such as `5mm`",
        "the bare-Int arm must render the full C1 template, hint included"
    );
}

/// (c) LAYER ATTRIBUTION (PRD decision D2) — a compile-layer LENGTH rejection
/// carries `ArgTypeMismatch`, and explicitly NOT `DimensionedArgRejected`.
///
/// This is a REGRESSION PIN, not a RED test: it passes on the pre-η table and
/// must keep passing. It exists because the task text for this leaf contains
/// the ambiguous phrase "give it β's DiagnosticCode", which a future leaf could
/// read literally. `DimensionedArgRejected`'s own minting rationale in
/// `crates/reify-core/src/diagnostics.rs` forecloses that reading — it records
/// that `ArgTypeMismatch` "was the closer candidate and was considered
/// seriously", and was kept SEPARATE because "PRD leaf eta will emit
/// `ArgTypeMismatch` at the compile layer for these very same argument
/// positions, so sharing one code would make 'which layer rejected this?'
/// unanswerable from the code alone".
///
/// Both PRD 3 (ANGLE) and task 5662 land on this same table, so the pin is what
/// keeps the two layers independently observable as they do.
#[test]
fn length_slot_rejection_uses_the_compile_layer_code_not_the_eval_layer_one() {
    let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(errors.len(), 1, "diagnostics: {:#?}", compiled.diagnostics);
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "the compile layer must keep its own code"
    );

    let eval_layer_coded: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DimensionedArgRejected))
        .collect();
    assert!(
        eval_layer_coded.is_empty(),
        "DimensionedArgRejected is the EVAL layer's code (task 5743). The compile \
         layer must not borrow it, or 'which layer rejected this?' stops being \
         answerable from the code alone (PRD decision D2). Got: {eval_layer_coded:#?}"
    );
}

/// (d) NEGATIVE CONTROL — an ANGLE slot's message carries NO hint.
///
/// The compile layer MIRRORS the eval layer exactly: `angle_spec` has no
/// migration hint either, so neither does this. Pinning it stops a future
/// reader mistaking the ANGLE gap for an oversight in this task — PRD 3 owns
/// closing both halves together, by binding seam decree.
#[test]
fn angle_slot_rejection_carries_no_migration_hint() {
    let compiled = compile_struct_body(
        "    let dir = vec3(0.0, 0.0, 1.0)\n    let sel = faces_by_normal(b, dir, 5)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare `5` tol.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message, "faces_by_normal: tol argument expects Angle, got Int",
        "an ANGLE slot must render the un-hinted template — eval's angle path has \
         no hint either, and PRD 3 owns closing both halves together"
    );
    assert!(
        !errors[0].message.contains("pass a dimensioned"),
        "no migration hint may leak onto an ANGLE slot: {}",
        errors[0].message
    );
}

/// (e) ANTI-UNIFICATION GUARD — the builtin-slot LENGTH hint and the
/// struct-ctor/fn-param LENGTH hint are DELIBERATELY not byte-identical.
///
/// `reify-compiler` already hosts a SECOND, unrelated migration-hint generator:
/// `conformance::dimensioned_scalar_migration_hint` (task 5627, decisions
/// D4-6), used for DIMENSIONED ctor / param slots. It is COMPUTED from the
/// dimension via `canonical_name()` + `example_unit_literal()`, so for LENGTH it
/// renders "pass a dimensioned **Length literal** such as `1m`" — capital L, the
/// word "literal", and `1m` rather than `5mm`.
///
/// The two must NOT be unified, in either direction:
/// * this builtin path must reproduce the EVAL-layer C1 text verbatim (D9), so
///   the compile and eval diagnostics for one authoring mistake read the same;
/// * rewording the ctor path to match would silently change already-shipped
///   diagnostics that `struct_ctor_field_conformance_tests.rs` guards via
///   `HINT_CLAUSE_PREFIX` / `HINT_EXAMPLE_INTRO`, and whose derived-from-the-
///   registry shape is what makes that family drift-proof.
///
/// Without this pin a future reader "helpfully" collapsing the two would break
/// one contract or the other, and no existing test would say so.
#[test]
fn builtin_slot_and_ctor_conformance_length_hints_are_deliberately_different() {
    let builtin = {
        let compiled = compile_struct_body("    let p = linear_pattern(b, 1, 0, 0, 5, 10)\n");
        let errors = arg_type_mismatch_errors(&compiled);
        assert_eq!(errors.len(), 1, "diagnostics: {:#?}", compiled.diagnostics);
        errors[0].message.clone()
    };

    let ctor = {
        let compiled = compile_source_with_stdlib(
            "module test.eta_hint_divergence\n\
             structure def W { param p : Scalar<Length> }\n\
             structure def Root { let a = W(p: 5) }\n",
        );
        // No severity filter here, unlike the builtin half: the ctor-conformance
        // walker emits this family at `Severity::Warning` (task 5465's value-floor
        // gradualism), whereas `check_builtin_arg_types` emits `Severity::Error`.
        // That difference is orthogonal to the hint WORDING this test is about, so
        // filtering on Error would make the test fail for an unrelated reason.
        let rejections: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
            .collect();
        assert_eq!(
            rejections.len(),
            1,
            "expected exactly 1 ctor-conformance ArgTypeMismatch.\n\
             All diagnostics: {:#?}",
            compiled.diagnostics
        );
        rejections[0].message.clone()
    };

    assert!(
        builtin.contains(LENGTH_HINT),
        "the builtin slot must carry the EVAL-layer C1 hint verbatim ({LENGTH_HINT:?}); \
         got: {builtin:?}"
    );
    assert!(
        ctor.contains("pass a dimensioned Length literal such as `1m`"),
        "the ctor-conformance path must keep its own COMPUTED hint shape \
         (`dimensioned_scalar_migration_hint`); got: {ctor:?}"
    );
    assert!(
        !ctor.contains(LENGTH_HINT),
        "the ctor-conformance hint must NOT have been rewritten to the builtin \
         wording — that would silently reword already-shipped diagnostics that \
         struct_ctor_field_conformance_tests.rs guards. builtin: {builtin:?}; \
         ctor: {ctor:?}"
    );
}

// ── Task 5750 (units-length η): PRIMITIVE + PROFILE LENGTH slots, end to end ──
//
// PRD `docs/prds/v0_6/units-length-gate-completion.md` boundary row 9. The unit
// tests in `builtin_signatures.rs::tests` pin the TABLE; these pin what an
// author actually sees — that the slots are reached through the real compile
// pipeline and render the eval layer's C1 template verbatim.

/// SIGNAL — a bare `box(20, 20, 10)` is rejected once PER AXIS.
///
/// All three, not just the first: the eval layer reads a multi-slot builtin's
/// whole set in ONE `required_length_values` call precisely so an author fixes
/// `width`, `height` and `depth` in a single edit rather than one per rebuild
/// (`crates/reify-eval/src/arg_acceptance.rs`'s "all-at-once discipline"). The
/// compile layer must not degrade that to a one-at-a-time drip, so the count is
/// asserted, not just non-emptiness.
///
/// The full message is asserted for every axis — the C1 template INCLUDING the
/// D9 migration hint — because that byte-identity with the eval layer is the
/// whole point of hoisting the hint into `reify-core`.
#[test]
fn box_bare_dimensions_are_rejected_once_per_axis() {
    let compiled = compile_struct_body("    let bad = box(20, 20, 10)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "box: width argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
            "box: height argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
            "box: depth argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
        ],
        "a bare box(20, 20, 10) must be diagnosed at width, height AND depth, \
         each rendering the full C1 template.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// BOUNDARY ok — the dimensioned control emits nothing.
///
/// The migration this leaf performs is only safe if `box(20mm, 20mm, 10mm)` —
/// the form every `examples/**/*.ri` file already uses — stays clean. Without
/// this, `box_bare_dimensions_are_rejected_once_per_axis` could be satisfied by
/// a slot that fires unconditionally.
#[test]
fn box_dimensioned_gives_no_arg_type_mismatch() {
    let compiled = compile_struct_body("    let good = box(20mm, 20mm, 10mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "a fully dimensioned box must emit no ArgTypeMismatch, got: {:#?}",
        errors
    );
}

/// SIGNAL — the PROFILE family is gated too, and names its own argument.
///
/// `circle`'s sole argument is `radius`, so the message must say `radius` and
/// not borrow a neighbouring family's name.
#[test]
fn circle_bare_radius_is_rejected_naming_radius() {
    let compiled = compile_struct_body("    let c = circle(4)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare circle radius.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "circle: radius argument expects Length, got Int; \
         pass a dimensioned length such as `5mm`"
    );
}

/// SIGNAL — a dimensionless REAL renders `got Real`, not `got Scalar[dimensionless]`.
///
/// A bare `4.0` types as `Type::Scalar { DIMENSIONLESS }`, whose `Display`
/// special-cases the dimensionless case to `Real`
/// (`crates/reify-core/src/ty.rs`). Worth pinning separately from the bare-Int
/// case: the two travel DIFFERENT arms of `check_builtin_arg_types` (the
/// `Type::Scalar { .. }` dimension comparison vs the catch-all kind mismatch),
/// and an author who wrote `4.0` must not be told about a type name the
/// language does not surface.
#[test]
fn sphere_dimensionless_real_radius_renders_got_real() {
    let compiled = compile_struct_body("    let s = sphere(4.0)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a dimensionless-Real sphere radius.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "sphere: radius argument expects Length, got Real; \
         pass a dimensioned length such as `5mm`"
    );
}

/// GRADUALISM CONTROL (contract C3) — a statically-invisible operand stays
/// SILENT at compile time, so the eval gate is never redundant.
///
/// `missing_thing` is unresolved, so its `CompiledExpr` carries `Type::Error`
/// and `check_builtin_arg_types` skips the slot by design. This is what makes
/// the compile slot a COMPLEMENT to `required_length_values` rather than a
/// replacement: removing the eval gate on the strength of this leaf would leave
/// exactly this shape ungated.
///
/// The unresolved-name error itself is asserted present, so the test cannot
/// pass because compilation quietly did nothing.
#[test]
fn statically_invisible_primitive_operand_stays_silent_at_compile_time() {
    let compiled = compile_struct_body("    let bad = box(missing_thing, 20mm, 10mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "a Type::Error operand must be skipped by the LENGTH slot (PRD decision-6 \
         gradualism), leaving the eval-layer gate to catch it; got: {:#?}",
        errors
    );
    assert!(
        compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("missing_thing")),
        "the fixture must actually reach the slot with an unresolved operand — \
         expected an unresolved-name Error naming `missing_thing`.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// MEASURED DIVERGENCE — an ALIAS slot names the SURFACE builtin the author
/// typed, while the eval layer names the LOWERED KIND.
///
/// `box_centered` lowers to `PrimitiveKind::Box`, and the eval-layer gate
/// renders its `{builtin}` prefix from that kind's `Display`
/// (`crates/reify-eval/src/geometry_ops.rs`'s `prim_box` passes `kind` as
/// `kind_label`; `crates/reify-compiler/src/types.rs:1394` writes `"box"`), so
/// eval says `box:` for a bare `box_centered(20, 20, 10)`. The compile layer is
/// keyed on the CALL, so it says `box_centered:`.
///
/// This is the ONE place decision D9's "byte-identical" wording does NOT hold,
/// and it holds this way DELIBERATELY: the compile layer knows the name that
/// actually appears in the author's source, and reporting it is strictly more
/// useful than reporting a lowering detail they never wrote. D9's substance —
/// the C1 template and the shared migration hint — is unaffected, and both are
/// asserted here alongside the prefix.
///
/// Pinned because it is exactly the kind of difference a later reader would
/// "fix" in the wrong direction, by teaching the compile layer to report the
/// lowered kind.
#[test]
fn centered_alias_slots_name_the_surface_builtin_not_the_lowered_kind() {
    let compiled = compile_struct_body("    let bc = box_centered(20, 20, 10)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "box_centered: width argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
            "box_centered: height argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
            "box_centered: depth argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
        ],
        "the compile slot must name the SURFACE call `box_centered`, not the \
         lowered `box` kind the eval layer reports.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

// ── Task 5750 (units-length η): MODIFY + SWEEP LENGTH slots, end to end ──────
//
// PRD `docs/prds/v0_6/units-length-gate-completion.md` boundary row 4.

/// SIGNAL — a bare `fillet` radius is rejected, naming `radius` with the hint.
///
/// The target is fully dimensioned so the only thing that can fire is the
/// fillet slot: without that, a `box` rejection from the primitive slots would
/// satisfy a loose count assertion and this row would never actually exercise
/// the modify family.
#[test]
fn fillet_bare_radius_is_rejected_naming_radius() {
    let compiled = compile_struct_body("    let f = fillet(box(10mm, 10mm, 10mm), 1)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare fillet radius.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "fillet: radius argument expects Length, got Int; \
         pass a dimensioned length such as `5mm`"
    );
}

/// SIGNAL — `chamfer_asymmetric` is rejected once per MAGNITUDE.
///
/// Both `d1` and `d2`, for the same reason `box` is diagnosed on all three
/// axes: the eval layer reads the pair in one grouped call so the author fixes
/// the line in a single edit, and the compile layer must not degrade that.
#[test]
fn chamfer_asymmetric_bare_distances_are_rejected_once_each() {
    let compiled = compile_struct_body(
        "    let sel = edges(b)\n    let c = chamfer_asymmetric(b, sel, 1, 2)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    let messages: Vec<&str> = errors.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "chamfer_asymmetric: d1 argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
            "chamfer_asymmetric: d2 argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`",
        ],
        "both asymmetric setbacks must be diagnosed in one compile.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// SIGNAL — a bare `extrude` distance is rejected, from the SWEEP family.
#[test]
fn extrude_bare_distance_is_rejected_naming_distance() {
    let compiled = compile_struct_body("    let e = extrude(rectangle(10mm, 10mm), 20)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 ArgTypeMismatch for a bare extrude distance.\n\
         All diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "extrude: distance argument expects Length, got Int; \
         pass a dimensioned length such as `5mm`"
    );
}

/// BOUNDARY ok — the dimensioned controls for both families emit nothing.
///
/// Paired with the three signals above for the same reason every bare fixture
/// in the eval-layer e2e files carries a control: without it, a slot that fired
/// unconditionally would satisfy all three.
#[test]
fn dimensioned_modify_and_sweep_args_give_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let f = fillet(box(10mm, 10mm, 10mm), 1mm)\n\
         \x20   let e = extrude(rectangle(10mm, 10mm), 20mm)\n\
         \x20   let s = shell(b, 2mm)\n\
         \x20   let p = pipe(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 10mm), 1mm)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "fully dimensioned modify/sweep calls must emit no ArgTypeMismatch, got: {:#?}",
        errors
    );
}

/// BOUNDARY ok — `fillet`'s 3-arg curated form does NOT slot its edge SELECTOR.
///
/// The concrete false positive the arity guard exists to prevent: `radius`
/// moves from index 1 to index 2 between the two overloads, so an
/// arity-agnostic `radius@1` slot would demand a Length of the `edges`
/// selector — an ArgTypeMismatch on correct code. The magnitude here is
/// dimensioned, so the ONLY thing that could fire is that false positive.
#[test]
fn fillet_curated_form_does_not_reject_its_edge_selector() {
    let compiled =
        compile_struct_body("    let sel = edges(b)\n    let f = fillet(b, sel, 2mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "fillet(target, edges, radius) must not slot its edge selector — that is \
         the false positive the arity guard exists to prevent; got: {:#?}",
        errors
    );
}

// ── Task 5750 amendment: the nested-argument double-walk ─────────────────────

/// SIGNAL — a bare-argument call NESTED as a geometry argument reports each of
/// its slots exactly ONCE.
///
/// `extrude(circle(4), 12mm)` is the single most common authoring shape a
/// nested primitive/profile takes, and before this amendment it rendered
/// `circle: radius …` TWICE: a nested geometry argument is walked by
/// `compile_expr` more than once, so every diagnostic emitted from the
/// type-inference walk was duplicated. That double-walk PRE-DATES task 5750
/// (`extrude(circle(nope), 12mm)` still reports `unresolved name` three
/// times), but before this leaf no primitive or profile had a slot, so no
/// nested inner call could emit an `ArgTypeMismatch` at all — landing the
/// slots made a latent defect user-visible on the dominant shape.
///
/// `emit_mismatch` therefore drops an exact (code, span, message) duplicate.
/// This test is the pin: it must NOT be relaxed to "at least 1".
#[test]
fn nested_profile_in_a_sweep_reports_its_bare_radius_exactly_once() {
    let compiled = compile_struct_body("    let e = extrude(circle(4), 12mm)\n");
    let errors = arg_type_mismatch_errors(&compiled);
    assert_eq!(
        errors.len(),
        1,
        "a nested `circle(4)` must report its bare radius exactly once, not \
         once per compile-walk.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].message,
        "circle: radius argument expects Length, got Int; \
         pass a dimensioned length such as `5mm`"
    );
}

/// BOUNDARY ok — the dedup drops only EXACT duplicates, never a distinct
/// sibling diagnostic.
///
/// `box(1, 2, 3)` nested as a sweep target shares one call span across all
/// three of its slots, so a dedup keyed on the span alone (or on the code
/// alone) would collapse three real errors into one. Each message names a
/// different axis, so all three must survive.
#[test]
fn nested_primitive_keeps_one_diagnostic_per_axis() {
    let compiled = compile_struct_body("    let f = fillet(box(1, 2, 3), 1mm)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "box: width argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "box: height argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "box: depth argument expects Length, got Int; pass a dimensioned length such as `5mm`",
        ],
        "all three axes must survive the duplicate drop.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

// ── Task 5750 amendment: message-level pins for the ORIGIN/PIVOT rows ────────

/// SIGNAL — a bare `revolve` axis ORIGIN is rejected, naming `ox`/`oy`/`oz`.
///
/// The straddle row: the origin is a point in space (gated), while the axis
/// DIRECTION `0, 0, 1` and the `90` angle in this same call are legitimately
/// bare and must stay silent. Three errors, not six or seven.
#[test]
fn revolve_bare_origin_is_rejected_naming_the_origin_components() {
    let compiled = compile_struct_body(
        "    let profile = rectangle(10mm, 10mm)\n\
         \x20   let r = revolve(profile, 0, 0, 0, 0, 0, 1, 90)\n",
    );
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "revolve: ox argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "revolve: oy argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "revolve: oz argument expects Length, got Int; pass a dimensioned length such as `5mm`",
        ],
        "only the axis ORIGIN is gated — the direction and the angle must stay \
         silent.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// SIGNAL — a bare `rotate_around` PIVOT is rejected, naming `px`/`py`/`pz`.
///
/// Same straddle shape as `revolve`'s, on the TRANSFORM row: the pivot is
/// gated, the axis direction and the angle are not.
#[test]
fn rotate_around_bare_pivot_is_rejected_naming_the_pivot_components() {
    let compiled = compile_struct_body("    let r = rotate_around(b, 0, 0, 0, 0, 0, 1, 90)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "rotate_around: px argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "rotate_around: py argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "rotate_around: pz argument expects Length, got Int; pass a dimensioned length such as `5mm`",
        ],
        "only the PIVOT is gated — the axis direction and the angle must stay \
         silent.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

// ── Task 5662: mirror / circular_pattern ORIGIN triples, end to end ──────────
//
// PRD `docs/prds/v0_6/units-length-gate-completion.md`, the pattern-origin row
// task 5652 deferred and this task closes. Same straddle shape as the `revolve`
// and `rotate_around` rows above: the origin is a point in space (gated), the
// direction components beside it are dimensionless unit vectors (never gated).

/// SIGNAL — a bare 7-arg `mirror` plane ORIGIN is rejected, naming `ox`/`oy`/`oz`.
///
/// Three errors, not six: the plane NORMAL `1, 0, 0` in this same call is a
/// dimensionless unit vector and must stay silent.
#[test]
fn mirror_bare_origin_is_rejected_naming_the_origin_components() {
    let compiled = compile_struct_body("    let m = mirror(b, 0, 0, 0, 1, 0, 0)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "mirror: ox argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "mirror: oy argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "mirror: oz argument expects Length, got Int; pass a dimensioned length such as `5mm`",
        ],
        "only the plane ORIGIN is gated — the plane normal must stay \
         silent.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// SIGNAL — a bare 9-arg `circular_pattern` axis ORIGIN is rejected, naming
/// `ox`/`oy`/`oz`.
///
/// Three errors, not seven: the axis DIRECTION `0, 0, 1`, the Int `count` and
/// the `60deg` angle in this same call must all stay silent. The angle belongs
/// to `docs/prds/v0_6/angle-units-surface-convergence.md` by binding seam
/// decree, so its silence here is a scope boundary, not an oversight.
#[test]
fn circular_pattern_bare_origin_is_rejected_naming_the_origin_components() {
    let compiled =
        compile_struct_body("    let p = circular_pattern(b, 12, 0, 0, 0, 0, 1, 6, 60deg)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(
        messages,
        vec![
            "circular_pattern: ox argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "circular_pattern: oy argument expects Length, got Int; pass a dimensioned length such as `5mm`",
            "circular_pattern: oz argument expects Length, got Int; pass a dimensioned length such as `5mm`",
        ],
        "only the axis ORIGIN is gated — the direction, the count and the angle \
         must stay silent.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// BOUNDARY ok — dimensioned scalar origins on both builtins produce NO
/// `ArgTypeMismatch`.
///
/// A no-error guard that holds both BEFORE and after the slots land (this
/// file's Case-2/4/5 convention): before, because there is no slot to fire;
/// after, because the argument is correct. Its job is to prove the new arms
/// reject the bare form specifically, not the shape of the call.
#[test]
fn dimensioned_pattern_origins_give_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let m = mirror(b, 0mm, 0mm, 0mm, 1, 0, 0)\n\
         \x20   let p = circular_pattern(b, 12mm, 0mm, 0mm, 0, 0, 1, 6, 60deg)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "dimensioned origin components must not trip any slot.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// BOUNDARY ok — the task-5745 decoded-VALUE forms produce NO
/// `ArgTypeMismatch`, because their arities expose no slots at all.
///
/// Also a no-error guard holding both before and after — but the one that
/// matters most, because index 1 EXISTS in both of these calls, holding a
/// `Plane` / an `Axis`. It is the arity guard on each arm, not the
/// `compiled_args.get(index)` bounds check, that keeps them quiet; an
/// arity-agnostic `ox@1 LENGTH` slot would demand a Length of a Plane here, on
/// correct code.
#[test]
fn pattern_value_forms_give_no_arg_type_mismatch() {
    let compiled = compile_struct_body(
        "    let m = mirror(b, plane_xy(0mm))\n\
         \x20   let p = circular_pattern(b, axis_z(point3(0mm, 0mm, 0mm)), 6, 60deg)\n",
    );
    let errors = arg_type_mismatch_errors(&compiled);
    assert!(
        errors.is_empty(),
        "the decoded-value forms expose no slots, so no ArgTypeMismatch may \
         fire.\nAll diagnostics: {:#?}",
        compiled.diagnostics
    );
}

/// MEASURED DIVERGENCE — `circular_pattern`'s compile slot names the SURFACE
/// builtin, while the eval layer names the LOWERED KIND; `mirror` does not
/// diverge at all.
///
/// The twin of `centered_alias_slots_name_the_surface_builtin_not_the_lowered_kind`,
/// and the second instance of that class. `circular_pattern` lowers to
/// `PatternKind::Circular`, whose `Display` — the eval layer's `kind_label` — is
/// `"circular"` (`crates/reify-compiler/src/types.rs:1748`), so eval says
/// `circular:` where this layer says `circular_pattern:`. `PatternKind::Mirror`
/// displays as `"mirror"` (types.rs:1749), so for `mirror` the two layers agree
/// byte-for-byte and decision D9's "byte-identical" wording holds unmodified.
///
/// Both halves are pinned — the divergence AND its absence — because a reader
/// who saw only the divergence might "fix" it in the wrong direction, by
/// teaching the compile layer to report the lowered kind for both.
#[test]
fn circular_pattern_slot_names_the_surface_builtin_not_the_lowered_kind() {
    let compiled =
        compile_struct_body("    let p = circular_pattern(b, 12, 0, 0, 0, 0, 1, 6, 60deg)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for message in &messages {
        assert!(
            message.starts_with("circular_pattern: "),
            "the compile slot must name the SURFACE call `circular_pattern`, not \
             the lowered `circular` kind the eval layer reports; got {message:?}"
        );
    }
    assert_eq!(messages.len(), 3, "expected the origin triple: {messages:#?}");

    // The negative half: `mirror` is NOT an alias, so no prefix divergence.
    let compiled = compile_struct_body("    let m = mirror(b, 0, 0, 0, 1, 0, 0)\n");
    let messages: Vec<&str> = arg_type_mismatch_errors(&compiled)
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for message in &messages {
        assert!(
            message.starts_with("mirror: "),
            "`PatternKind::Mirror` displays as \"mirror\", so the compile and eval \
             layers must agree byte-for-byte here; got {message:?}"
        );
    }
    assert_eq!(messages.len(), 3, "expected the origin triple: {messages:#?}");
}
