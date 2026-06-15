//! End-to-end `reify check` integration tests for the geometric-relation
//! vocabulary (geometric-relations γ, task 4383).
//!
//! Compiles real `.ri` snippets against the stdlib and asserts:
//!   (1) relation calls type-check to `Type::Relation` (concentric / flush /
//!       offset);
//!   (2) B10 — a metric-dimension mismatch (`angle(a, b, 5mm)`) emits
//!       `ArgTypeMismatch`;
//!   (3) B9  — a non-projecting operand (`angle(p1, p2, 30deg)`, Point has no
//!       Direction) emits `DatumProjectionUnavailable`;
//!   (4) REGRESSION — the arity-2 `angle`/`distance` DERIVE forms still type as
//!       `Scalar<Angle>` / `Scalar<Length>` (geometry-query path untouched).
//!
//! Cases 1–3 are RED until step-8 wires the relation arm + `check_relation_arg_types`
//! into `expr.rs`'s `NoUserFunctions` ladder; case 4 is a boundary guard that
//! holds both before and after wiring.

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::{compile_source_with_stdlib, get_let_expr};

/// Wrap `members` in a minimal `structure S { … }` and compile with the full
/// stdlib prelude (so dimensioned literals like `5mm` / `30deg` resolve). The
/// relation builtins themselves are compiler-internal (the `NoUserFunctions`
/// arm), not stdlib `.ri` definitions.
fn compile_structure(members: &str) -> reify_compiler::CompiledModule {
    let source = format!("structure S {{\n{members}\n}}");
    compile_source_with_stdlib(&source)
}

// ── (1) Relation calls type-check to Type::Relation ──────────────────────────

/// `concentric(a, b)` over two `Axis` operands types to `Type::Relation`.
///
/// RED: until the relation arm is wired, `concentric` falls to the first-arg
/// fallback and `r` types as `Axis`.
#[test]
fn concentric_axes_types_as_relation() {
    let module = compile_structure(
        "    param a : Axis\n    param b : Axis\n    let r = concentric(a, b)\n",
    );
    assert_eq!(
        get_let_expr(&module, "r").result_type,
        Type::Relation,
        "concentric(Axis, Axis) must type as Relation"
    );
}

/// `flush(pa, pb)` over two `Plane` operands types to `Type::Relation`.
#[test]
fn flush_planes_types_as_relation() {
    let module = compile_structure(
        "    param pa : Plane\n    param pb : Plane\n    let r = flush(pa, pb)\n",
    );
    assert_eq!(
        get_let_expr(&module, "r").result_type,
        Type::Relation,
        "flush(Plane, Plane) must type as Relation"
    );
}

/// `offset(pa, pb, 5mm)` (two planes + a Length metric) types to `Type::Relation`.
#[test]
fn offset_planes_with_length_types_as_relation() {
    let module = compile_structure(
        "    param pa : Plane\n    param pb : Plane\n    let r = offset(pa, pb, 5mm)\n",
    );
    assert_eq!(
        get_let_expr(&module, "r").result_type,
        Type::Relation,
        "offset(Plane, Plane, Length) must type as Relation"
    );
}

// ── (2) B10 — unit-layer metric mismatch ─────────────────────────────────────

/// `angle(a, b, 5mm)` — the metric must be an `Angle`; a `Length` metric is a
/// B10 unit error. The `Axis` operands lift to `Direction`, so the only
/// diagnostic is the `ArgTypeMismatch` naming "Angle".
///
/// RED: until the checker is wired, no `ArgTypeMismatch` is emitted (arity-3
/// `angle` types as a geometry-query `Angle` with no arg check).
#[test]
fn angle_with_length_metric_emits_arg_type_mismatch() {
    let module = compile_structure(
        "    param a : Axis\n    param b : Axis\n    let r = angle(a, b, 5mm)\n",
    );
    let mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect();
    assert!(
        !mismatches.is_empty(),
        "angle(a, b, 5mm) must emit an ArgTypeMismatch (Length metric where Angle expected).\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
    assert!(
        mismatches[0].message.contains("Angle"),
        "B10 message should name the expected dimension 'Angle': {}",
        mismatches[0].message
    );
}

// ── (3) B9 — kind/projection-layer operand failure ───────────────────────────

/// `angle(p1, p2, 30deg)` — the metric is a correct `Angle`, but a `Point` has
/// no `Direction` projection, so each operand fails to lift: B9
/// `DatumProjectionUnavailable`.
///
/// RED: until the checker is wired, no projection diagnostic is emitted.
#[test]
fn angle_on_points_emits_datum_projection_unavailable() {
    let module = compile_structure(
        "    param p1 : Point3<Length>\n    param p2 : Point3<Length>\n    \
         let r = angle(p1, p2, 30deg)\n",
    );
    let unavailable: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::DatumProjectionUnavailable)
                && d.severity == Severity::Error
        })
        .collect();
    assert!(
        !unavailable.is_empty(),
        "angle(p1, p2, 30deg) must emit a DatumProjectionUnavailable (Point has no Direction).\n\
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

// ── (4) REGRESSION — arity-2 DERIVE forms stay geometry queries ──────────────

/// The arity-2 `angle`/`distance` DERIVE forms are geometry queries, NOT
/// relations: they must keep typing as `Scalar<Angle>` / `Scalar<Length>`. This
/// boundary guard holds both before and after the relation arm is wired (the arm
/// returns `None` for arity-2 `angle`/`distance`, falling through to
/// geometry-query) — and the relation checker must be a no-op on these forms.
#[test]
fn two_arg_angle_distance_stay_geometry_queries() {
    let module = compile_structure(
        "    param a : Axis\n    param b : Axis\n    \
         param p1 : Point3<Length>\n    param p2 : Point3<Length>\n    \
         let ang = angle(a, b)\n    let dist = distance(p1, p2)\n",
    );
    assert_eq!(
        get_let_expr(&module, "ang").result_type,
        Type::angle(),
        "arity-2 angle(a, b) must stay a geometry-query Scalar<Angle>"
    );
    assert_eq!(
        get_let_expr(&module, "dist").result_type,
        Type::length(),
        "arity-2 distance(p1, p2) must stay a geometry-query Scalar<Length>"
    );
    // The relation checker must not fire on the arity-2 DERIVE forms.
    let spurious: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                Some(DiagnosticCode::ArgTypeMismatch)
                    | Some(DiagnosticCode::DatumProjectionUnavailable)
                    | Some(DiagnosticCode::DatumProjectionAmbiguous)
            ) && d.severity == Severity::Error
        })
        .collect();
    assert!(
        spurious.is_empty(),
        "arity-2 angle/distance must draw no relation arg diagnostics, got: {:#?}",
        spurious
    );
}
