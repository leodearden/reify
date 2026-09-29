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
//!   (5) GRADUALISM — a `Scalar<Q>` metric/radius inside a dimension-kinded
//!       generic fn (`fn f<Q: Dimension>(...)`) draws no `ArgTypeMismatch`
//!       (PRD decision-6, `Type::ScalarParam`).
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
    let module =
        compile_structure("    param a : Axis\n    param b : Axis\n    let r = concentric(a, b)\n");
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
    let module =
        compile_structure("    param a : Axis\n    param b : Axis\n    let r = angle(a, b, 5mm)\n");
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

// ── (5) GRADUALISM — Type::ScalarParam metric defers, never poisons ─────────

/// A `Scalar<Q>` metric inside a dimension-kinded generic fn (`fn f<Q: Dimension>
/// (..., theta: Scalar<Q>)`) must not draw a unit-layer `ArgTypeMismatch`: the
/// metric's family (scalar) is known but its dimension is unresolved until
/// instantiation, so the check must defer silently — mirroring the `TypeParam`
/// gradualism the checker already grants.
///
/// Compiled as top-level fns (NOT wrapped in `structure S { … }` via
/// `compile_structure`): a dimension-kinded generic fn is a top-level
/// declaration and does not fit the structure-member wrapper.
///
/// VERIFIED RED against the base-commit binary: `target/debug/reify check` on
/// these exact three fns emits `angle: metric argument expects Angle, got
/// Scalar<Q>`, `distance: metric argument expects Length, got Scalar<Q>`,
/// `offset: metric argument expects Length, got Scalar<Q>`.
#[test]
fn scalar_param_metric_in_generic_fn_emits_no_arg_type_mismatch() {
    let source = r#"
fn drive_angle<Q: Dimension>(a: Axis, b: Axis, theta: Scalar<Q>) -> Relation { angle(a, b, theta) }
fn drive_distance<Q: Dimension>(p1: Point3<Length>, p2: Point3<Length>, d: Scalar<Q>) -> Relation { distance(p1, p2, d) }
fn drive_offset<Q: Dimension>(pa: Plane, pb: Plane, d: Scalar<Q>) -> Relation { offset(pa, pb, d) }
"#;
    let module = compile_source_with_stdlib(source);

    let mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "a Scalar<Q> metric in a dimension-kinded generic fn must NOT emit \
         ArgTypeMismatch (gradualism skip on ScalarParam); got: {:#?}",
        mismatches
    );

    // Half two of the contract: the checker is a pure diagnostic side-effect
    // that never changes inference — each fn's body must still type as
    // Type::Relation, not poison to Type::Error.
    for fn_name in ["drive_angle", "drive_distance", "drive_offset"] {
        let f = module
            .functions
            .iter()
            .find(|f| f.name == fn_name)
            .unwrap_or_else(|| panic!("{fn_name} function should be compiled"));
        assert_eq!(
            f.body.result_expr.result_type,
            Type::Relation,
            "{fn_name}'s body must type as Type::Relation, not poison; got: {:?}",
            f.body.result_expr.result_type
        );
    }
}

/// A `Scalar<Q>` radius on `tangent` (cylinder/plane one-radius form and
/// cylinder/cylinder two-radii form) must not draw a unit-layer
/// `ArgTypeMismatch`, mirroring the metric-slot gradualism above —
/// `check_tangent_operands` has its own separate radius-slot match, so this is
/// a distinct code path from `scalar_param_metric_in_generic_fn_emits_no_arg_type_mismatch`.
/// Also asserts zero `TangentOperandsUnsupported`, so the widened skip cannot
/// be credited to an unrelated suppression.
///
/// Kept in this file rather than moved beside its sibling radius-slot tests in
/// `tests/harness_relate/tangent_operand_check_tests.rs`
/// (`radius_slot_with_the_wrong_dimension_is_a_unit_mismatch`,
/// `second_radius_slot_dimension_is_policed_too`) — that file is outside this
/// task's assigned scope. Cross-referenced here so the split reads as
/// deliberate, not an oversight.
///
/// VERIFIED RED against the base-commit binary: `target/debug/reify check` on
/// `fn drive_tangent<Q: Dimension>(a: Axis, p: Plane, r: Scalar<Q>) -> Relation
/// { tangent(a, p, r) }` emits `tangent: metric argument expects Length, got
/// Scalar<Q>`.
#[test]
fn scalar_param_tangent_radius_in_generic_fn_emits_no_arg_type_mismatch() {
    let source = r#"
fn drive_tangent_cyl_plane<Q: Dimension>(a: Axis, p: Plane, r: Scalar<Q>) -> Relation { tangent(a, p, r) }
fn drive_tangent_cyl_cyl<Q: Dimension>(a: Axis, b: Axis, r1: Scalar<Q>, r2: Scalar<Q>) -> Relation { tangent(a, b, r1, r2) }
"#;
    let module = compile_source_with_stdlib(source);

    let bad: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && matches!(
                    d.code,
                    Some(DiagnosticCode::ArgTypeMismatch)
                        | Some(DiagnosticCode::TangentOperandsUnsupported)
                )
        })
        .collect();
    assert!(
        bad.is_empty(),
        "a Scalar<Q> radius on tangent must NOT emit ArgTypeMismatch or \
         TangentOperandsUnsupported (gradualism skip on ScalarParam); got: {:#?}",
        bad
    );

    for fn_name in ["drive_tangent_cyl_plane", "drive_tangent_cyl_cyl"] {
        let f = module
            .functions
            .iter()
            .find(|f| f.name == fn_name)
            .unwrap_or_else(|| panic!("{fn_name} function should be compiled"));
        assert_eq!(
            f.body.result_expr.result_type,
            Type::Relation,
            "{fn_name}'s body must type as Type::Relation, not poison; got: {:?}",
            f.body.result_expr.result_type
        );
    }
}
