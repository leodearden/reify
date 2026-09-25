//! End-to-end regression lock for task 5747 (units-length ζ, R8 half) — a BARE
//! (dimensionless) TRANSLATION inside a `Transform<3>` must be REJECTED at
//! eval/build with a `Severity::Error` carrying
//! `DiagnosticCode::DimensionedArgRejected` that NAMES THE OFFENDING COORDINATE,
//! rather than dropping the op behind the generic
//! `'transform' arg is not a valid Transform<3>`.
//!
//! WHAT ζ ACTUALLY CHANGES HERE — read this before adding a row. Both fixtures
//! below ALREADY exit 1 on the pre-ζ tree (measured 2026-08-25:
//! `apply_transform(b, transform3(orient_identity(), vec3(5, 0, 0)))` printed
//! `warning: apply_transform dropped: 'transform' arg is not a valid Transform<3>`
//! plus `error: failed to compile geometry operation: …`). So R8's signal is the
//! DIAGNOSTIC, not the exit code, and NO row here may assert an exit-code flip.
//! What each row asserts instead is (a) a coded units Error naming
//! `translation.x`, and (b) that the generic shape message is GONE. The genuine
//! 0→1 exit-code change is R12's, asserted at the CLI in
//! `crates/reify-cli/tests/harness_cli/cli_affine_eval.rs`.
//!
//! WHY BOUNDARY ROW 7'S `Scalar{DIMENSIONLESS}` TWIN IS NOT A FIXTURE HERE.
//! That value shape is not expressible from `.ri` source. Probed, not guessed:
//! `let r = 5mm / 1mm` then `vec3(r, …)` still produced the pre-ζ GENERIC shape
//! rejection, which proves the division collapsed to `Value::Real` — had it
//! yielded a DIMENSIONLESS `Scalar`, the pre-ζ code would have ACCEPTED it and
//! the build would have exited 0. Its home is therefore the unit row
//! `compile_geometry_op_apply_transform_translation_follows_the_three_state_contract`
//! in `crates/reify-eval/src/geometry_ops/tests.rs`, which constructs the
//! `reify_ir::Value::Scalar { dimension: DIMENSIONLESS }` directly. Writing an
//! `.ri` fixture anyway would silently exercise the `Real` path twice while
//! claiming to cover the twin.
//!
//! WHY `Engine::build` AND NOT `Engine::eval` (decision D8): `compile_geometry_op`
//! — the chokepoint this task gates — runs on build. `engine_eval` mints
//! symbolic `GeometryHandle`s and never reaches the kernel, so the gate's
//! user-visible surface is `BuildResult.diagnostics`. Carried verbatim from
//! β/5743's `primitive_profile_length_units_e2e.rs`, whose harness this copies.
//!
//! WHY EVERY BARE FIXTURE IS PAIRED WITH A DIMENSIONED CONTROL: without the
//! control, a "no op reached the kernel" assertion can pass VACUOUSLY — the op
//! absent because compilation broke, not because the eval gate dropped it. The
//! pair is inseparable; do not delete "the redundant half".

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
///
/// Extended past β's copy with the assertion that carries ζ's R8 signal: the
/// pre-ζ generic `not a valid Transform<3>` message must be ABSENT. Without it
/// the test would pass on a tree that merely added a units Error beside the old
/// shape warning, which is not what ζ did.
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
        "{label}: a bare translation must produce at least one Severity::Error \
         carrying DimensionedArgRejected; got: {diagnostics:?}"
    );

    for needle in needles {
        assert!(
            coded.iter().any(|d| d.message.contains(needle)),
            "{label}: no coded Error message contained {needle:?}; got: {:?}",
            coded.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("not a valid Transform<3>")),
        "{label}: THE R8 SIGNAL — the pre-ζ generic shape message must be GONE on \
         the units path; got: {diagnostics:?}"
    );

    let built: Vec<_> = ops.iter().filter(|op| is_target(op)).collect();
    assert!(
        built.is_empty(),
        "{label}: the op must be DROPPED, not silently built with an SI-metre \
         translation; got {} matching ops: {built:?}",
        built.len()
    );
}

fn is_apply_transform(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::ApplyTransform { .. })
}

fn is_arbitrary_pattern(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::ArbitraryPattern { .. })
}

// ---------------------------------------------------------------------------
// Row 1 / Row 2 — apply_transform: the headline pair
// ---------------------------------------------------------------------------

/// BARE `vec3(5, 0, 0)` inside a `transform3` → a coded Error naming
/// `translation.x`, and no `ApplyTransform` op reaches the kernel.
///
/// Needling the ANCHORED `"translation.x argument expects"` shape rather than
/// the bare coordinate name is what makes this test able to see the difference:
/// a stray mention of `translation` in some other diagnostic would satisfy a
/// loose `contains("translation")`.
#[test]
fn bare_apply_transform_translation_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "apply_transform(b, transform3(orient_identity(), vec3(5, 0, 0)))",
        r#"
        structure def BareApplyTransform {
            let b = box(20mm, 20mm, 10mm)
            let moved = apply_transform(b, transform3(orient_identity(), vec3(5, 0, 0)))
        }
        "#,
        &[
            "apply_transform",
            "translation.x argument expects",
            "Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_apply_transform,
    );
}

/// The control that keeps the row above from passing vacuously: the SAME
/// transform with DIMENSIONED literals builds with ZERO Error diagnostics and
/// exactly ONE `ApplyTransform` op, whose SI translation is unchanged by the
/// gate (0.005 / 0 / 0 metres).
#[test]
fn dimensioned_apply_transform_builds_one_op_with_unchanged_si_translation() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimApplyTransform {
            let b = box(20mm, 20mm, 10mm)
            let moved = apply_transform(b, transform3(orient_identity(), vec3(5mm, 0mm, 0mm)))
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned apply_transform must build with zero Error diagnostics; \
         got: {errors:?}"
    );

    let applied: Vec<_> = ops.iter().filter(|op| is_apply_transform(op)).collect();
    assert_eq!(
        applied.len(),
        1,
        "a dimensioned apply_transform must emit exactly one ApplyTransform op; \
         got: {applied:?}"
    );

    let GeometryOp::ApplyTransform { translation, .. } = applied[0] else {
        unreachable!("filtered to ApplyTransform above");
    };
    assert_eq!(
        *translation,
        [0.005, 0.0, 0.0],
        "the gate must not re-scale an accepted LENGTH translation"
    );
}

// ---------------------------------------------------------------------------
// Row 3 / Row 4 — arbitrary_pattern's LIST form: the same pair
// ---------------------------------------------------------------------------

/// The list form decodes each element through the same gate, and has its OWN
/// pre-ζ element message — so it needs its own pair, not a shared one.
#[test]
fn bare_arbitrary_pattern_list_translation_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "arbitrary_pattern(b, [transform3(orient_identity(), vec3(5, 0, 0))])",
        r#"
        structure def BareArbitraryPattern {
            let b = box(2mm, 2mm, 10mm)
            let patterned = arbitrary_pattern(
                b,
                [transform3(orient_identity(), vec3(5, 0, 0))]
            )
        }
        "#,
        &[
            "arbitrary_pattern",
            "translation.x argument expects",
            "Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_arbitrary_pattern,
    );
}

/// The inseparable control for the row above.
#[test]
fn dimensioned_arbitrary_pattern_list_builds_one_op_with_unchanged_si_translation() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimArbitraryPattern {
            let b = box(2mm, 2mm, 10mm)
            let patterned = arbitrary_pattern(
                b,
                [transform3(orient_identity(), vec3(5mm, 0mm, 0mm))]
            )
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned arbitrary_pattern must build with zero Error diagnostics; \
         got: {errors:?}"
    );

    let patterned: Vec<_> = ops.iter().filter(|op| is_arbitrary_pattern(op)).collect();
    assert_eq!(
        patterned.len(),
        1,
        "a dimensioned arbitrary_pattern must emit exactly one ArbitraryPattern op; \
         got: {patterned:?}"
    );

    let GeometryOp::ArbitraryPattern { transforms, .. } = patterned[0] else {
        unreachable!("filtered to ArbitraryPattern above");
    };
    assert_eq!(
        transforms.len(),
        1,
        "one list element in, one transform out; got: {transforms:?}"
    );
    assert_eq!(
        transforms[0].1,
        [0.005, 0.0, 0.0],
        "the gate must not re-scale an accepted LENGTH translation"
    );
}
