//! End-to-end regression lock for task 5743 (units-length β) — the headline
//! behaviour: a BARE (dimensionless) dimension on a geometry PRIMITIVE or
//! PROFILE must be REJECTED at eval/build, producing a `Severity::Error`
//! diagnostic carrying `DiagnosticCode::DimensionedArgRejected` and DROPPING
//! the op, rather than silently reading the bare number as SI **metres**.
//!
//! Before the gate, `box(20, 20, 10)` built a 20-METRE box — 1000× a plausible
//! 20 mm part — because `Value::as_f64` reads a bare `Real` as SI metres. The
//! failure is silent: nothing errors, a solid is produced, and the mistake only
//! surfaces downstream as an absurd mass, an empty boolean difference, or a
//! mesh that will not fit the build volume.
//!
//! WHY `Engine::build` AND NOT `Engine::eval` (decision D8): `compile_geometry_op`
//! — the chokepoint this task gates — runs on build. `engine_eval` mints
//! symbolic `GeometryHandle`s and never reaches the kernel, so the gate's
//! user-visible surface is `BuildResult.diagnostics`. Harness modelled on
//! `pattern_spacing_units_e2e.rs` (task 5214's own leaf signal).
//!
//! WHY EVERY BARE FIXTURE IS PAIRED WITH A DIMENSIONED CONTROL: without the
//! control, a "no op reached the kernel" assertion can pass VACUOUSLY — the op
//! absent because compilation broke, not because the eval gate dropped it. The
//! pair is inseparable; do not delete "the redundant half".
//!
//! SIMPLER THAN ITS MODEL IN ONE RESPECT: primitives and profiles have no
//! compile-layer LENGTH slot yet (those arrive with task η — `builtin_arg_slots`
//! deliberately returns empty for `box`/`cylinder` today), so the STRICT
//! `parse_and_compile` works for BARE sources here and no `compile_bare_spacing`
//! workaround is needed. That is asserted rather than assumed: it is what proves
//! the rejection came from the EVAL gate and not from a compile Error.

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

/// Build `source` against a mock kernel, returning the build diagnostics and
/// every `GeometryOp` that reached the kernel.
///
/// `operations_ref()` is captured BEFORE the kernel moves into the `Engine` —
/// the only ordering that lets the emitted ops be inspected afterwards.
fn build_capturing_ops(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);
    let ops = ops_ref
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.op.clone())
        .collect();
    (result.diagnostics, ops)
}

/// The rejection half: assert `source` produces at least one `Severity::Error`
/// carrying `DimensionedArgRejected`, whose message contains every needle, and
/// that NO op matching `is_target` reached the kernel.
fn assert_rejected(
    label: &str,
    source: &str,
    needles: &[&str],
    is_target: fn(&GeometryOp) -> bool,
) {
    let (diagnostics, ops) = build_capturing_ops(source);

    let coded: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::DimensionedArgRejected)
        })
        .collect();
    assert!(
        !coded.is_empty(),
        "{label}: a bare dimension must produce at least one Severity::Error \
         carrying DimensionedArgRejected; got: {diagnostics:?}"
    );

    for needle in needles {
        assert!(
            coded.iter().any(|d| d.message.contains(needle)),
            "{label}: no coded Error message contained {needle:?}; got: {:?}",
            coded.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    let built: Vec<_> = ops.iter().filter(|op| is_target(op)).collect();
    assert!(
        built.is_empty(),
        "{label}: the op must be DROPPED, not silently built with SI-metre \
         dimensions; got {} matching ops: {built:?}",
        built.len()
    );
}

fn is_box(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::Box { .. })
}

fn is_circle_profile(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::CircleProfile { .. })
}

// ---------------------------------------------------------------------------
// Row 1 / Row 2 — box: the headline pair
// ---------------------------------------------------------------------------

/// BARE `box(20, 20, 10)` → a `Severity::Error` carrying
/// `DimensionedArgRejected` for EVERY bare dimension, each naming the builtin,
/// its own argument, `Length` and the literal migration hint; and NO `Box` op
/// reaches the kernel.
///
/// ALL THREE SLOTS, not just `width` (reviewer amendment, task 5743): a box is
/// written as one gesture, so a bare box is bare in every dimension. Reading
/// the three fields through `?`-chained per-field calls would report only
/// `width` and cost the author three edit-build cycles to fix one line — the
/// UX `required_length_values` exists to prevent. Needling the ANCHORED
/// `"{slot} argument expects"` shape rather than the bare slot name is what
/// makes this test able to see the difference: a single `width` rejection plus
/// the op-compile Error would satisfy a loose `contains("height")`.
#[test]
fn bare_box_dimensions_drop_the_op_with_a_coded_error() {
    assert_rejected(
        "box(20, 20, 10)",
        r#"
        structure def BareBox {
            let body = box(20, 20, 10)
        }
        "#,
        &[
            "box",
            "width argument expects",
            "height argument expects",
            "depth argument expects",
            "Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_box,
    );
}

/// The control that keeps the row above from passing vacuously: the SAME box
/// with DIMENSIONED literals compiles under the STRICT `parse_and_compile` and
/// builds with ZERO Error diagnostics and exactly ONE `Box` op, whose SI
/// dimensions are unchanged by the gate (0.02 / 0.02 / 0.01 metres).
#[test]
fn dimensioned_box_builds_one_op_with_unchanged_si_dimensions() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimBox {
            let body = box(20mm, 20mm, 10mm)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned box must build with zero Error diagnostics; got: {errors:?}"
    );

    let boxes: Vec<_> = ops.iter().filter(|op| is_box(op)).collect();
    assert_eq!(
        boxes.len(),
        1,
        "a dimensioned box must emit exactly one Box op; got: {boxes:?}"
    );

    let GeometryOp::Box {
        width,
        height,
        depth,
    } = boxes[0]
    else {
        unreachable!("filtered to Box above")
    };
    for (name, value, expected) in [
        ("width", width, 0.02),
        ("height", height, 0.02),
        ("depth", depth, 0.01),
    ] {
        let si = value
            .as_f64()
            .unwrap_or_else(|| panic!("{name} must carry a numeric SI value; got {value:?}"));
        assert!(
            (si - expected).abs() < 1e-12,
            "the gate must not re-scale: {name} should stay {expected} SI metres, got {si}"
        );
    }
}

/// PRD boundary row 3 / decision D1: bare ZERO is NOT special-cased. `box(0, 0, 0)`
/// is rejected exactly like any other bare dimension.
///
/// This row exists because "zero has no units" is the single most plausible
/// carve-out a future patch might add, and adding it reopens the hazard.
#[test]
fn bare_zero_box_dimensions_are_not_special_cased() {
    assert_rejected(
        "box(0, 0, 0)",
        r#"
        structure def ZeroBox {
            let body = box(0, 0, 0)
        }
        "#,
        &["box", "Length"],
        is_box,
    );
}

// ---------------------------------------------------------------------------
// Row 4 — profile
// ---------------------------------------------------------------------------

/// A bare PROFILE radius is rejected on the same terms as a primitive
/// dimension, and no `CircleProfile` op reaches the kernel.
#[test]
fn bare_profile_radius_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "circle(4)",
        r#"
        structure def BareProfile {
            let body = extrude(circle(4), 12mm)
        }
        "#,
        &["circle", "radius", "Length"],
        is_circle_profile,
    );
}

/// Control for the profile row: `circle(4mm)` builds clean.
#[test]
fn dimensioned_profile_radius_builds_clean() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimProfile {
            let body = extrude(circle(4mm), 12mm)
        }
        "#,
    );
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned circle profile must build with zero Error diagnostics; \
         got: {errors:?}"
    );
    assert!(
        ops.iter().any(is_circle_profile),
        "a dimensioned circle profile must reach the kernel; got: {ops:?}"
    );
}

// ---------------------------------------------------------------------------
// Row 5 — the slice boundary: half_space's normal stays dimensionless
// ---------------------------------------------------------------------------

/// LOAD-BEARING NEGATIVE ASSERTION. `half_space`'s `(px, py, pz)` is a point
/// (length-semantic, gated) but `(nx, ny, nz)` is a dimensionless unit NORMAL
/// and must STILL accept bare numbers.
///
/// This is the exact shape of the shipped `examples/half_space.ri`. Without
/// this row, an over-broad gate that also caught the direction triple would
/// pass every positive test in this file while breaking the corpus — a
/// negative-space assertion is the only thing that can catch over-reach.
#[test]
fn half_space_bare_normal_still_builds_with_a_dimensioned_point() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def HalfSpaceMixed {
            let body = half_space(0mm, 0mm, 0mm, 0, 0, 1)
        }
        "#,
    );
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "half_space's DIRECTION triple is a dimensionless unit vector and must \
         still accept bare numbers — gating it would break examples/half_space.ri; \
         got: {errors:?}"
    );
    assert!(
        ops.iter()
            .any(|op| matches!(op, GeometryOp::HalfSpace { .. })),
        "the half_space op must reach the kernel; got: {ops:?}"
    );
}

// ---------------------------------------------------------------------------
// The desugaring regression battery — the highest-risk item in this leaf
// ---------------------------------------------------------------------------

/// Every compiler desugaring that lowers into a now-gated primitive or profile
/// slot must still build clean from DIMENSIONED source text.
///
/// This is the machine check that dependency α (task 5742) covered its side
/// completely for this slice. α retyped `cylinder_centered`'s dx/dy and
/// `rounded_rect`'s dz to `Value::length(0.0)`, and left the ±0.5 Mul factors
/// as bare dimensionless `Real`s on purpose — `Scalar{LENGTH} × Real` preserves
/// LENGTH, whereas retyping the factor would take the Scalar×Scalar arm and
/// yield `Scalar{AREA}`, which is numerically silent through `as_f64` but
/// rejected here. If this test fails, the fix belongs in
/// `crates/reify-compiler/src/geometry.rs` (α's file), NOT in this leaf.
///
/// `rounded_box`'s arg order is width/depth/height/corner_r — DIFFERENT from
/// `box`'s width/height/depth. Getting that wrong produces a confusing
/// unrelated failure, so it is called out here.
#[test]
fn desugared_primitives_build_clean() {
    for (label, source) in [
        (
            "box_centered",
            r#"structure def D { let body = box_centered(20mm, 20mm, 10mm) }"#,
        ),
        (
            "cylinder_centered",
            r#"structure def D { let body = cylinder_centered(5mm, 20mm) }"#,
        ),
        (
            // width, depth, height, corner_r — NOT box's arg order.
            "rounded_box",
            r#"structure def D { let body = rounded_box(40mm, 30mm, 10mm, 5mm) }"#,
        ),
        (
            "rounded_rect",
            r#"structure def D { let body = extrude(rounded_rect(40mm, 30mm, 5mm), 10mm) }"#,
        ),
        // The four GD&T zone constructors. Per this leaf's desugaring audit these
        // lower to Sweep{Pipe} radii and Modify args, NOT to β-gated primitive or
        // profile slots — so they are a CHEAP REGRESSION GUARD rather than a
        // direct coverage claim. They are kept in the battery because the audit
        // that establishes that is a point-in-time reading of
        // reify-compiler/src/geometry.rs, and a future re-lowering through
        // box/cylinder/rectangle/circle would silently move them into the gate.
        (
            "zone_slab",
            r#"structure def D {
                let f = rectangle(width: 40mm, height: 20mm)
                let s = zone_slab(f, 2mm)
            }"#,
        ),
        (
            "zone_cylinder",
            r#"structure def D {
                let z = zone_cylinder(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 8mm)
            }"#,
        ),
        (
            "zone_annulus",
            r#"structure def D {
                let z = zone_annulus(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 20mm, 4mm, 20mm)
            }"#,
        ),
        (
            "zone_profile",
            r#"structure def D { let z = zone_profile(box(10mm, 10mm, 10mm), 1mm) }"#,
        ),
    ] {
        let (diagnostics, ops) = build_capturing_ops(source);
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "{label} lowers into a now-gated slot and must still build clean from \
             DIMENSIONED source; got: {errors:?}"
        );
        assert!(
            !ops.is_empty(),
            "{label} must emit at least one op; got none"
        );
    }
}
