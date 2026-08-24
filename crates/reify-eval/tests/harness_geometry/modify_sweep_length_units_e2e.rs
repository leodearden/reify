//! End-to-end regression lock for task 5744 (units-length γ) — the headline
//! behaviour for the MODIFY and SWEEP families: a BARE (dimensionless)
//! magnitude on `fillet`/`chamfer`/`shell`/`thicken`/`offset_*`/`extrude`/
//! `pipe`/`zone_slab` must be REJECTED at eval/build, producing a
//! `Severity::Error` diagnostic carrying `DiagnosticCode::DimensionedArgRejected`
//! and DROPPING the op, rather than silently reading the bare number as SI
//! **metres**.
//!
//! Before the gate, `fillet(solid, 1)` asked for a 1-METRE fillet radius —
//! 1000× a plausible 1 mm blend — because `Value::as_f64` reads a bare `Real`
//! as SI metres. The failure was not even legibly silent: it surfaced
//! downstream as a span-less `BRepFilletAPI_MakeFillet failed` from the kernel,
//! naming neither the argument nor the units mistake. PRD §6 boundary row 4
//! (`docs/prds/v0_6/units-length-gate-completion.md`) is the replacement
//! signal, and this module is where it is pinned.
//!
//! WHY `Engine::build` AND NOT `Engine::eval` (decision D8): `compile_geometry_op`
//! — the chokepoint this task gates — runs on build. `engine_eval` mints
//! symbolic `GeometryHandle`s and never reaches the kernel, so the gate's
//! user-visible surface is `BuildResult.diagnostics`. Harness copied from
//! `primitive_profile_length_units_e2e.rs` (task 5743's own leaf signal), which
//! in turn follows `pattern_spacing_units_e2e.rs` (task 5214's).
//!
//! WHY EVERY BARE FIXTURE IS PAIRED WITH A DIMENSIONED CONTROL: without the
//! control, a "no op reached the kernel" assertion can pass VACUOUSLY — the op
//! absent because compilation broke, not because the eval gate dropped it. The
//! pair is inseparable; do not delete "the redundant half".
//!
//! LIKE ITS MODEL IN ONE RESPECT WORTH RESTATING: the modify and sweep
//! magnitudes have no compile-layer LENGTH slot yet (those arrive with task η —
//! `builtin_arg_slots` deliberately returns empty for `fillet`/`extrude`
//! today), so the STRICT `parse_and_compile` works for BARE sources here and no
//! compile-side workaround is needed. That is asserted rather than assumed: it
//! is what proves the rejection came from the EVAL gate and not from a compile
//! Error.

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
        "{label}: a bare magnitude must produce at least one Severity::Error \
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
        "{label}: the op must be DROPPED, not silently built with an SI-metre \
         magnitude; got {} matching ops: {built:?}",
        built.len()
    );
}

fn is_fillet(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::Fillet { .. })
}

// ---------------------------------------------------------------------------
// PRD §6 boundary row 4 — `fillet` radius: the headline pair
// ---------------------------------------------------------------------------

/// BARE `fillet(box(10mm,10mm,10mm), 1)` → a `Severity::Error` carrying
/// `DimensionedArgRejected` naming the builtin, the `radius` argument, `Length`
/// and the literal migration hint; and NO `Fillet` op reaches the kernel.
///
/// This replaces the pre-gate signal, which was a span-less
/// `BRepFilletAPI_MakeFillet failed` raised by the kernel after it was handed a
/// 1-METRE blend radius for a 10 mm cube — a message that names neither the
/// argument nor the units mistake, and which a manifold-only build would not
/// produce at all.
///
/// Needling the ANCHORED `"expects Length"` shape (β's `WRONG_TYPE_WORDING`)
/// rather than a hand-copied full message is deliberate: the wording's sole
/// owner is `ArgRejection::message`, so a future rewording changes one place.
#[test]
fn bare_fillet_radius_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "fillet(box(10mm,10mm,10mm), 1)",
        r#"
        structure def BareFillet {
            let body = fillet(box(10mm, 10mm, 10mm), 1)
        }
        "#,
        &[
            "fillet",
            "radius",
            "expects Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_fillet,
    );
}

/// The control that keeps the row above from passing vacuously: the SAME fillet
/// with a DIMENSIONED radius compiles under the STRICT `parse_and_compile` and
/// builds with ZERO Error diagnostics and exactly ONE `Fillet` op, whose SI
/// radius is unchanged by the gate (0.001 metres).
///
/// The "unchanged" half matters as much as the "green" half: the chokepoint
/// re-wraps the accepted SI f64 back into a LENGTH `Value::Scalar`, and a
/// re-scaling bug there would still produce a green build with a silently wrong
/// part.
#[test]
fn dimensioned_fillet_radius_builds_one_op_with_unchanged_si_radius() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimFillet {
            let body = fillet(box(10mm, 10mm, 10mm), 1mm)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned fillet must build with zero Error diagnostics; got: {errors:?}"
    );

    let fillets: Vec<_> = ops.iter().filter(|op| is_fillet(op)).collect();
    assert_eq!(
        fillets.len(),
        1,
        "a dimensioned fillet must emit exactly one Fillet op; got: {fillets:?}"
    );

    let GeometryOp::Fillet { radius, .. } = fillets[0] else {
        unreachable!("filtered to Fillet above")
    };
    let si = radius
        .as_f64()
        .unwrap_or_else(|| panic!("radius must carry a numeric SI value; got {radius:?}"));
    assert!(
        (si - 0.001).abs() < 1e-12,
        "the gate must not re-scale: radius should stay 0.001 SI metres, got {si}"
    );
}

/// The BARE source is accepted STRICTLY by `parse_and_compile` — i.e. the
/// compile layer raises no Error for `fillet(.., 1)` and the rejection asserted
/// above therefore comes from the EVAL gate, not from a compile diagnostic.
///
/// This is what makes the pair above a test of THIS task's chokepoint. It goes
/// stale, deliberately, when task η lands `fillet`'s compile-layer LENGTH slot
/// — at which point η owns updating it, exactly as the PRD §8 charter says.
#[test]
fn bare_fillet_source_compiles_strictly_so_the_rejection_is_the_eval_gate() {
    let compiled = parse_and_compile(
        r#"
        structure def BareFillet {
            let body = fillet(box(10mm, 10mm, 10mm), 1)
        }
        "#,
    );
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "the bare fillet source must COMPILE clean (no compile-layer LENGTH \
         slot until task η) so the rejection under test is provably the eval \
         gate; got: {compile_errors:?}"
    );
}
