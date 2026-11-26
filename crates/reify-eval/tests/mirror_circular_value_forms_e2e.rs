//! End-to-end tests for mirror and circular_pattern with value-form args (Plane/Axis).
//!
//! Step-5 (RED → GREEN after step-6): mirror value form, back-compat, wrong-variant rejection.
//! Step-7 (RED → GREEN after step-8): circular_pattern value form, wrong-variant.
//!
//! Both scalar component forms are retired: `mirror`'s 7-arg one by
//! direction-typing ι-a (task 9646), `circular_pattern`'s 9-arg one by ι-c
//! (task 9648). `circular_pattern(target, axis, count, angle)` is now its only
//! form, so a 9-arg call is an arity error naming it (pinned at compile by
//! `circular_pattern_arg_count_diagnostic_has_span_label` in reify-compiler).
//!
//! # Where an origin is gated
//!
//! An origin reaches these builtins only inside a decoded value — a `Plane`, an
//! `Axis`, or `mirror`'s `Point3` — and no compile-layer slot kind describes
//! one, so a bare origin component is caught at EVAL
//! (`DiagnosticCode::DimensionedArgRejected`, tasks 5214 / 5350 / 5745) and the
//! op is DROPPED, so nothing reaches the kernel. Every row here therefore
//! compiles clean on the strict `parse_and_compile`. Why that exclusion is
//! structural rather than a gap is stated once, on the `circular_pattern` arm
//! of `builtin_arg_slots` in `crates/reify-compiler/src/builtin_signatures.rs`.

use reify_core::Severity;
use reify_ir::GeometryOp;
use reify_test_support::{build_against_mock_kernel, parse_and_compile};

// ── step-5: mirror consumer tests ─────────────────────────────────────────────

/// (a) Value form: mirror(box, plane_xy(0mm)) builds with zero Error diagnostics
/// and emits exactly one Mirror op with plane_origin ≈ [0,0,0] and plane_normal ≈ [0,0,1].
///
/// RED today: parse_and_compile panics — 2-arg mirror fails compile ("expects 7 arguments").
/// GREEN after step-6.
#[test]
fn mirror_value_form_plane_xy_builds_and_emits_correct_mirror_op() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_xy(0mm))
        }
    "#;

    let compiled = parse_and_compile(source);
    let (diagnostics, ops) = build_against_mock_kernel(compiled);

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for mirror value form, got: {:?}",
        error_diags
    );

    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::Mirror { .. }))
        .collect();
    assert_eq!(
        mirror_ops.len(),
        1,
        "expected exactly one Mirror op, got {}",
        mirror_ops.len()
    );

    match mirror_ops[0] {
        GeometryOp::Mirror {
            plane_origin,
            plane_normal,
            ..
        } => {
            assert!(
                plane_origin[0].abs() < 1e-9,
                "plane_origin[0] should be 0, got {}",
                plane_origin[0]
            );
            assert!(
                plane_origin[1].abs() < 1e-9,
                "plane_origin[1] should be 0, got {}",
                plane_origin[1]
            );
            assert!(
                plane_origin[2].abs() < 1e-9,
                "plane_origin[2] should be 0 (plane_xy at z=0mm), got {}",
                plane_origin[2]
            );
            assert!(
                plane_normal[0].abs() < 1e-9,
                "plane_normal[0] should be 0, got {}",
                plane_normal[0]
            );
            assert!(
                plane_normal[1].abs() < 1e-9,
                "plane_normal[1] should be 0, got {}",
                plane_normal[1]
            );
            assert!(
                (plane_normal[2] - 1.0).abs() < 1e-9,
                "plane_normal[2] should be 1.0 (Z-axis for plane_xy), got {}",
                plane_normal[2]
            );
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// (b) The point-direction form mirror(box, point3(0mm, 0mm, 0mm), dir(1, 0, 0)) builds
/// without errors and emits Mirror with plane_normal ≈ [1,0,0].
#[test]
fn mirror_point_direction_form_emits_correct_plane() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, point3(0mm, 0mm, 0mm), dir(1, 0, 0))
        }
    "#;

    let compiled = parse_and_compile(source);
    let (diagnostics, ops) = build_against_mock_kernel(compiled);

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for the point-direction mirror, got: {:?}",
        error_diags
    );

    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::Mirror { .. }))
        .collect();
    assert_eq!(mirror_ops.len(), 1, "expected exactly one Mirror op");

    match mirror_ops[0] {
        GeometryOp::Mirror { plane_normal, .. } => {
            assert!(
                (plane_normal[0] - 1.0).abs() < 1e-9,
                "plane_normal[0] should be 1.0, got {}",
                plane_normal[0]
            );
            assert!(
                plane_normal[1].abs() < 1e-9,
                "plane_normal[1] should be 0, got {}",
                plane_normal[1]
            );
            assert!(
                plane_normal[2].abs() < 1e-9,
                "plane_normal[2] should be 0, got {}",
                plane_normal[2]
            );
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// (c) Wrong-variant rejection (H signal): mirror(box, axis_z(...)) must produce an
/// Error diagnostic because axis_z yields Value::Axis not Value::Plane.  No Mirror op.
///
/// RED today: parse_and_compile panics — 2-arg mirror fails compile ("expects 7 arguments").
/// GREEN after step-6: 2-arg compiles (value form); eval rejects Axis → Error diagnostic.
#[test]
fn mirror_wrong_variant_axis_rejected_with_error_diagnostic() {
    let source = r#"
        structure def S {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, axis_z(point3(0mm, 0mm, 0mm)))
        }
    "#;

    let compiled = parse_and_compile(source);
    let (diagnostics, ops) = build_against_mock_kernel(compiled);

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic for wrong-variant axis→mirror, got: {:?}",
        diagnostics
    );

    let mirror_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::Mirror { .. }))
        .collect();
    assert!(
        mirror_ops.is_empty(),
        "expected NO Mirror op when Axis is passed where Plane is required, got {} Mirror op(s)",
        mirror_ops.len()
    );
}

// ── step-7: circular_pattern consumer tests ───────────────────────────────────

/// (a) Value form: circular_pattern(box, axis_z(point3(0,0,0)), 6, 60deg) builds
/// with zero Error diagnostics and emits exactly one CircularPattern with
/// axis_dir ≈ [0,0,1] (within 1e-9) and count == 6.
///
/// RED at step-7: parse_and_compile panicked — the 4-arg circular_pattern
/// failed compile. GREEN after step-8.
#[test]
fn circular_pattern_value_form_axis_z_emits_correct_op() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(0mm, 0mm, 0mm)), 6, 60deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let (diagnostics, ops) = build_against_mock_kernel(compiled);

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "expected zero Error diagnostics for circular_pattern value form, got: {:?}",
        error_diags
    );

    let cp_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::CircularPattern { .. }))
        .collect();
    assert_eq!(
        cp_ops.len(),
        1,
        "expected exactly one CircularPattern op, got {}",
        cp_ops.len()
    );

    match cp_ops[0] {
        GeometryOp::CircularPattern {
            axis_origin,
            axis_dir,
            count,
            ..
        } => {
            assert!(
                axis_origin[0].abs() < 1e-9,
                "axis_origin[0] should be 0, got {}",
                axis_origin[0]
            );
            assert!(
                axis_origin[1].abs() < 1e-9,
                "axis_origin[1] should be 0, got {}",
                axis_origin[1]
            );
            assert!(
                axis_origin[2].abs() < 1e-9,
                "axis_origin[2] should be 0, got {}",
                axis_origin[2]
            );
            assert!(
                axis_dir[0].abs() < 1e-9,
                "axis_dir[0] should be 0, got {}",
                axis_dir[0]
            );
            assert!(
                axis_dir[1].abs() < 1e-9,
                "axis_dir[1] should be 0, got {}",
                axis_dir[1]
            );
            assert!(
                (axis_dir[2] - 1.0).abs() < 1e-9,
                "axis_dir[2] should be 1.0 (Z-axis), got {}",
                axis_dir[2]
            );
            assert_eq!(*count, 6, "count should be 6, got {}", count);
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}

/// (c) Wrong-variant rejection: circular_pattern(box, plane_xy(0mm), 6, 60deg) must
/// produce an Error diagnostic because plane_xy yields Value::Plane, not Value::Axis.
/// No CircularPattern op should be emitted.
///
/// RED at step-7: parse_and_compile panicked — the 4-arg circular_pattern
/// failed compile. GREEN after step-8: 4-arg compiles; eval rejects Plane →
/// Error diagnostic.
#[test]
fn circular_pattern_wrong_variant_plane_rejected_with_error_diagnostic() {
    let source = r#"
        structure def S {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, plane_xy(0mm), 6, 60deg)
        }
    "#;

    let compiled = parse_and_compile(source);
    let (diagnostics, ops) = build_against_mock_kernel(compiled);

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected at least one Error diagnostic for wrong-variant plane→circular_pattern, got: {:?}",
        diagnostics
    );

    let cp_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, GeometryOp::CircularPattern { .. }))
        .collect();
    assert!(
        cp_ops.is_empty(),
        "expected NO CircularPattern op when Plane is passed where Axis is required, got {} op(s)",
        cp_ops.len()
    );
}

// ── units-length δ (task 5745): the decoded VALUE-FORM origin gate ────────────
//
// PRD §6 boundary rows 5 and 6, at the outermost seam. Until δ the value form
// BYPASSED the very gate its scalar sibling enforced: `mirror(b, plane_yz(10))`
// reached the kernel as a 10 SI-**metre** plane offset, exit 0, zero
// diagnostics, while the since-retired `mirror(b, 10, 0, 0, 1, 0, 0)` was
// rejected at ox/oy/oz.
// That is the 1000× hazard this PRD exists for, arriving through the one route
// an author is most likely to take.
//
// These rows drive the real parser and unit system (not hand-built
// `CompiledExpr` fixtures, which the `geometry_ops/tests.rs` unit rows cover),
// so they would catch a regression introduced anywhere between the `.ri`
// surface and the kernel call.

/// Build `source` against a mock kernel and return `(diagnostics, mirror_ops)`.
///
/// Returns the diagnostics THEMSELVES rather than a count: the δ rows below
/// assert on the specific
/// `ArgRejection` diagnostic — its `DiagnosticCode` and its wording — not merely
/// that "some Error was emitted". That distinction matters, because the
/// op-compile Error which accompanies every rejection ALSO names the argument
/// and the word "Length", so a looser shape would pass without the gate ever
/// having fired.
#[track_caller]
fn build_value_form(
    source: &str,
    want: fn(&GeometryOp) -> bool,
) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form_compiled(parse_and_compile(source), want)
}

/// The kernel half of [`build_value_form`], shared with its bare counterpart.
#[track_caller]
fn build_value_form_compiled(
    compiled: reify_compiler::CompiledModule,
    want: fn(&GeometryOp) -> bool,
) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    let (diagnostics, ops) = build_against_mock_kernel(compiled);
    let kept: Vec<GeometryOp> = ops.into_iter().filter(want).collect();
    (diagnostics, kept)
}

/// `build_value_form` narrowed to `Mirror` ops.
#[track_caller]
fn build_mirror(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form(source, |op| matches!(op, GeometryOp::Mirror { .. }))
}

/// `build_value_form` narrowed to `CircularPattern` ops.
#[track_caller]
fn build_circular(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_value_form(source, |op| matches!(op, GeometryOp::CircularPattern { .. }))
}

/// PRD §6 row 5 — the BYPASS, closed: a bare `plane_yz(10)` origin must be
/// REJECTED and the op DROPPED, never silently built with a 10 SI-metre plane
/// offset.
///
/// The VERDICT is unchanged by units-length ε (task 5746, R11): the op is still
/// dropped and a `DimensionedArgRejected` `Severity::Error` is still emitted.
/// The SPEAKER moved. `plane_yz(10)` is now `Value::Undef` at the PRODUCER, so
/// `decode_plane` takes its wrong-variant arm and pushes nothing, and the
/// surviving rejection is `make_plane`'s. Its argument name moved with it, from
/// the synthesized origin component `ox` to `offset` — the argument the author
/// actually wrote, and the vocabulary `make_plane` itself uses. MEASURED on this
/// source after ε:
///     plane_yz: offset argument expects Length, got Int; pass a dimensioned
///     length such as `5mm`
///
/// δ's consumer-side `ox`/`oy`/`oz` gate is NOT retired by that move: it stays
/// reachable through the five construction-datum producers ε deliberately leaves
/// dimension-polymorphic (`plane_through`, `midplane`, the arity-2 `offset`,
/// `axis_through`, `frame_at`), and its axis-side twin below still reads `ox`
/// for its own reason.
#[test]
fn mirror_value_form_bare_plane_origin_drops_op_with_error() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def BarePlaneMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_yz(10))
        }
        "#,
    );

    let rejection = diagnostics.iter().find(|d| {
        d.severity == reify_core::Severity::Error
            && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)
    });
    let rejection = rejection.unwrap_or_else(|| {
        panic!(
            "a bare `plane_yz(10)` origin must produce the shared \
             DimensionedArgRejected Error; got: {:?}",
            diagnostics
        )
    });
    assert!(
        rejection.message.contains("argument expects Length"),
        "the wording must come from `ArgRejection::message`, shared with the \
         named-arg and variadic routes; got: {}",
        rejection.message
    );
    assert!(
        rejection
            .message
            .contains("plane_yz: offset argument expects Length"),
        "the rejection must be the PRODUCER's, naming the builtin and the `offset` \
         argument the author wrote; got: {}",
        rejection.message
    );
    assert!(
        mirror_ops.is_empty(),
        "a bare-origin value-form mirror must be DROPPED, not silently built \
         with a 10 SI-metre plane origin; emitted Mirror ops: {:?}",
        mirror_ops
    );
}

/// PRD §6 row 6 — the positive control that keeps row 5 from passing vacuously,
/// and the compatibility promise: the DIMENSIONED value form still builds, and
/// is geometrically identical to what `plane_yz(0.01)` produced before δ.
///
/// `1e-12` rather than `==` because the parser computes `10 * 1e-3`; the f64
/// error is at most ~1 ulp (order 1e-18 at this magnitude), so this clears it by
/// six orders of magnitude while staying far tighter than the 1000× defect being
/// guarded against. It is the same tolerance the sibling
/// `decode_plane_producer_round_trip_*` unit tests use for this same quantity.
#[test]
fn mirror_value_form_dimensioned_plane_origin_builds_unchanged() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def MmPlaneMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_yz(10mm))
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the dimensioned value form must build with ZERO Error diagnostics; \
         got: {:?}",
        errors
    );
    assert_eq!(
        mirror_ops.len(),
        1,
        "expected exactly one Mirror op to reach the kernel, got {:?}",
        mirror_ops
    );
    match &mirror_ops[0] {
        GeometryOp::Mirror {
            plane_origin,
            plane_normal,
            ..
        } => {
            for (i, expected) in [0.01_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (plane_origin[i] - expected).abs() < 1e-12,
                    "plane_yz(10mm) must reach the kernel with plane_origin[{i}] \
                     = {expected} SI metres (NOT {} m); got {:?}",
                    expected * 1000.0,
                    plane_origin
                );
            }
            for (i, expected) in [1.0_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (plane_normal[i] - expected).abs() < 1e-12,
                    "plane_yz's normal is the dimensionless unit vector [1,0,0] \
                     and is NOT gated; got {:?}",
                    plane_normal
                );
            }
        }
        other => panic!("expected GeometryOp::Mirror, got {:?}", other),
    }
}

/// The point form's bare-origin rejections speak the one shared wording.
///
/// δ's no-regression lock was on the 7-arg SCALAR form, whose `ox`/`oy`/`oz`
/// rejections proved the decoded-value route joined the shared chokepoint
/// rather than forking a second copy of the text. Direction-typing ι-a (task
/// 9646) retired that form for `mirror(b, point3(..), dir(..))`, whose origin
/// goes through `accept_length_point3` — the same chokepoint, under the
/// `origin.x`/`origin.y`/`origin.z` names of the argument the author wrote. The
/// `Point3` origin has no compile slot, so the source compiles clean on the
/// strict helper and only the eval-layer `DimensionedArgRejected` fires.
#[test]
fn mirror_point_form_bare_origin_rejections_use_the_shared_wording() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def BarePointMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, point3(10, 0, 0), dir(1, 0, 0))
        }
        "#,
    );

    for name in ["origin.x", "origin.y", "origin.z"] {
        let expected = format!(
            "mirror: {name} argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`"
        );
        assert!(
            diagnostics.iter().any(|d| d.message == expected
                && d.severity == reify_core::Severity::Error
                && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)),
            "the point form's `{name}` rejection must use the shared wording; \
             expected {expected:?}, got: {:?}",
            diagnostics
        );
    }
    assert!(
        mirror_ops.is_empty(),
        "the point-form bare origin must be DROPPED; got {:?}",
        mirror_ops
    );
}

/// The point-form positive control: the same origin, dimensioned, builds clean.
/// Paired with the row above so neither can pass vacuously.
#[test]
fn mirror_point_form_dimensioned_origin_builds_clean() {
    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def MmPointMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, point3(10mm, 0mm, 0mm), dir(1, 0, 0))
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected zero Errors, got: {:?}", errors);
    assert_eq!(mirror_ops.len(), 1, "got {:?}", mirror_ops);
}

/// PRD §6 row 5, axis half — the route that once BYPASSED the gate the retired
/// 9-arg scalar form enforced, and since direction-typing ι-c the only route.
///
/// A bare `axis_z(point3(12, 0, 0))` origin must be rejected and the op DROPPED,
/// never silently built with a 12 SI-**metre** axis origin — 1000× a plausible
/// 12 mm offset. Placing the rotation axis 12 metres out silently reproduces the
/// scalar form's headline defect through a different door, which is exactly what
/// made the two forms' disagreement worth closing.
#[test]
fn circular_pattern_value_form_bare_axis_origin_drops_op_with_error() {
    let (diagnostics, circular_ops) = build_circular(
        r#"
        structure def BareAxisRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(12, 0, 0)), 6, 60deg)
        }
        "#,
    );

    let rejection = diagnostics
        .iter()
        .find(|d| {
            d.severity == reify_core::Severity::Error
                && d.code == Some(reify_core::DiagnosticCode::DimensionedArgRejected)
        })
        .unwrap_or_else(|| {
            panic!(
                "a bare `axis_z(point3(12, 0, 0))` origin must produce the shared \
                 DimensionedArgRejected Error; got: {:?}",
                diagnostics
            )
        });
    assert!(
        rejection.message.contains("argument expects Length"),
        "wording must come from `ArgRejection::message`; got: {}",
        rejection.message
    );
    assert!(
        rejection.message.contains("ox"),
        "the rejection must name the ORIGIN component `ox`; got: {}",
        rejection.message
    );
    assert!(
        circular_ops.is_empty(),
        "a bare-origin value-form circular_pattern must be DROPPED, not silently \
         built with a 12 SI-metre axis origin; emitted ops: {:?}",
        circular_ops
    );
}

/// PRD §6 row 6, axis half — the positive control that keeps the row above from
/// passing vacuously, and the compatibility promise: the DIMENSIONED value form
/// still builds and still reaches the kernel at `[0.012, 0, 0]`, not `[12, 0, 0]`.
///
/// `1e-12` because the parser computes `12 * 1e-3`, so the f64 error is at most
/// ~1 ulp (order 1e-18 here), six orders below this bound and far tighter than
/// the 1000× defect guarded against.
#[test]
fn circular_pattern_value_form_dimensioned_axis_origin_builds_unchanged() {
    let (diagnostics, circular_ops) = build_circular(
        r#"
        structure def MmAxisRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_z(point3(12mm, 0mm, 0mm)), 6, 60deg)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "the dimensioned value form must build with ZERO Error diagnostics; \
         got: {:?}",
        errors
    );
    assert_eq!(
        circular_ops.len(),
        1,
        "expected exactly one CircularPattern op, got {:?}",
        circular_ops
    );
    match &circular_ops[0] {
        GeometryOp::CircularPattern {
            axis_origin,
            axis_dir,
            ..
        } => {
            for (i, expected) in [0.012_f64, 0.0, 0.0].into_iter().enumerate() {
                assert!(
                    (axis_origin[i] - expected).abs() < 1e-12,
                    "axis_origin[{i}] must be {expected} SI metres (NOT {} m); \
                     got {:?}",
                    expected * 1000.0,
                    axis_origin
                );
            }
            for (i, expected) in [0.0_f64, 0.0, 1.0].into_iter().enumerate() {
                assert!(
                    (axis_dir[i] - expected).abs() < 1e-12,
                    "axis_z's direction is the dimensionless unit vector [0,0,1] \
                     and is NOT gated; got {:?}",
                    axis_dir
                );
            }
        }
        other => panic!("expected GeometryOp::CircularPattern, got {:?}", other),
    }
}

/// The stdlib-minted datum half of direction-typing B17: an `axis_through` /
/// `plane_through` datum feeds `circular_pattern` / `mirror` end to end, and the
/// op carries the unit direction / normal. Pinned across the producer switch
/// from a bare `Vector` to a `Direction` (task 9638).
#[test]
fn circular_pattern_and_mirror_accept_axis_through_and_plane_through_datums() {
    let errors_of = |diagnostics: &[reify_core::Diagnostic]| -> Vec<reify_core::Diagnostic> {
        diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .cloned()
            .collect()
    };
    let assert_unit_z = |v: &[f64; 3], what: &str| {
        for (i, expected) in [0.0_f64, 0.0, 1.0].into_iter().enumerate() {
            assert!(
                (v[i] - expected).abs() < 1e-12,
                "{what} must be the unit [0,0,1]; got {v:?}"
            );
        }
    };

    let (diagnostics, circular_ops) = build_circular(
        r#"
        structure def ThroughAxisRing {
            let b = box(2mm, 2mm, 2mm)
            let p = circular_pattern(b, axis_through(point3(0mm, 0mm, 0mm), point3(0mm, 0mm, 10mm)), 4, 90deg)
        }
        "#,
    );
    assert!(errors_of(&diagnostics).is_empty(), "got: {diagnostics:?}");
    match circular_ops.as_slice() {
        [GeometryOp::CircularPattern { axis_dir, .. }] => assert_unit_z(axis_dir, "axis_dir"),
        other => panic!("expected exactly one CircularPattern op, got {other:?}"),
    }

    let (diagnostics, mirror_ops) = build_mirror(
        r#"
        structure def ThroughPlaneMirror {
            let b = box(10mm, 10mm, 10mm)
            let m = mirror(b, plane_through(point3(0mm, 0mm, 0mm), point3(10mm, 0mm, 0mm), point3(0mm, 10mm, 0mm)))
        }
        "#,
    );
    assert!(errors_of(&diagnostics).is_empty(), "got: {diagnostics:?}");
    match mirror_ops.as_slice() {
        [GeometryOp::Mirror { plane_normal, .. }] => assert_unit_z(plane_normal, "plane_normal"),
        other => panic!("expected exactly one Mirror op, got {other:?}"),
    }
}

// ── direction-typing ι-a (task 9646): mirror(geo, origin, normal) ──────────

/// The Direction form and the plane form survive retirement of the scalar
/// form, so their equality is a durable cross-form check: `mirror(b,
/// point3(0mm, 0mm, 0mm), dir(1, 0, 0))` reaches the kernel as exactly the op
/// `mirror(b, plane_yz(0mm))` does, bit for bit.
#[test]
fn mirror_direction_form_equals_the_plane_form() {
    let mirror_op = |call: &str| -> ([f64; 3], [f64; 3]) {
        let (diagnostics, ops) = build_mirror(&format!(
            "structure def S {{\n    let b = box(10mm, 10mm, 10mm)\n    let m = {call}\n}}\n"
        ));
        assert!(
            diagnostics.iter().all(|d| d.severity != Severity::Error),
            "{call}: unexpected errors: {diagnostics:?}"
        );
        match ops.as_slice() {
            [
                GeometryOp::Mirror {
                    plane_origin,
                    plane_normal,
                    ..
                },
            ] => (*plane_origin, *plane_normal),
            other => panic!("{call}: expected one Mirror op, got {other:?}"),
        }
    };
    let (direction_origin, direction_normal) =
        mirror_op("mirror(b, point3(0mm, 0mm, 0mm), dir(1, 0, 0))");
    let (plane_origin, plane_normal) = mirror_op("mirror(b, plane_yz(0mm))");
    assert_eq!(
        direction_origin.map(f64::to_bits),
        plane_origin.map(f64::to_bits),
        "origin"
    );
    assert_eq!(
        direction_normal.map(f64::to_bits),
        plane_normal.map(f64::to_bits),
        "normal"
    );
}
