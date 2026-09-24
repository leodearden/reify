//! End-to-end regression lock for task 5744 (units-length γ) — the headline
//! behaviour for the MODIFY and SWEEP families: a BARE (dimensionless)
//! magnitude must be REJECTED at eval/build, producing a `Severity::Error`
//! diagnostic carrying `DiagnosticCode::DimensionedArgRejected` and DROPPING
//! the op, rather than silently reading the bare number as SI **metres**.
//!
//! COVERAGE — READ THIS BEFORE AUDITING e2e BREADTH. γ gates TWELVE magnitude
//! slots; this file carries `.ri`-source → build → kernel rows for FOUR
//! REPRESENTATIVE builtins only: `fillet` (the PRD's headline, §6 boundary
//! row 4), `chamfer`, `chamfer_asymmetric` (the all-at-once group read) and
//! `extrude` (the sweep family's representative). It does NOT exercise
//! `shell`, `thicken`, `zone_slab`, `offset_solid`, `offset_curve`,
//! `extrude_symmetric` or `pipe` — those are pinned at the UNIT level by
//! `GAMMA_MODIFY_SLOTS` / `GAMMA_SWEEP_SLOTS` in
//! `crates/reify-eval/src/geometry_ops/tests.rs`, whose three-state tables
//! cover every slot's accepted / rejected / undefined behaviour. That split is
//! deliberate and safe because all twelve slots share ONE chokepoint —
//! `required_length_value` over `accept_length_value` — so the per-slot risk is
//! "was this slot wired to the chokepoint at all", which the unit table answers
//! directly, while what an e2e row uniquely adds (the source→kernel path, and
//! that the op really does not reach the kernel) is a property of the path, not
//! of the individual builtin.
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
//! NO LONGER TRUE, AND UPDATED BY THE TASK THAT INVALIDATED IT (task 5750,
//! units-length η). This file used to record that the modify and sweep
//! magnitudes had no compile-layer LENGTH slot, so the STRICT
//! `parse_and_compile` worked for BARE sources here. η landed those slots, so
//! the strict helper now PANICS on every bare fixture below before eval ever
//! runs.
//!
//! What replaces it is NOT a loosening. `compile_bare_length` swaps in the
//! lenient `compile_source` and then re-asserts both halves the strict helper
//! used to give: that the compile-layer `ArgTypeMismatch` really IS emitted,
//! and that it is the ONLY Error-severity compile diagnostic — which is what
//! keeps every "no op reached the kernel" assertion below from passing
//! VACUOUSLY.
//!
//! The eval gate is still proven to fire INDEPENDENTLY, by CODE rather than by
//! the compile layer being silent: the compile diagnostic carries
//! `DiagnosticCode::ArgTypeMismatch` while the build diagnostic carries
//! `DiagnosticCode::DimensionedArgRejected`, and `assert_rejected` filters on
//! the latter. That is PRD decision D2's two-layer observability, asserted from
//! this side by
//! [`bare_fillet_source_carries_both_layers_with_distinct_codes`].

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source, parse_and_compile,
};

/// Build `source` against a mock kernel, returning the build diagnostics and
/// every `GeometryOp` that reached the kernel.
///
/// `operations_ref()` is captured BEFORE the kernel moves into the `Engine` —
/// the only ordering that lets the emitted ops be inspected afterwards.
fn build_capturing_ops(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_compiled(parse_and_compile(source))
}

/// The BARE-source counterpart of [`build_capturing_ops`] (task 5750).
///
/// Task η gave every modify and sweep magnitude a compile-layer LENGTH slot, so
/// the bare sources in this file no longer compile clean and the strict
/// `parse_and_compile` — which hard-asserts zero Error diagnostics — would panic
/// before eval ever ran. Same shape as the helper task 5750 introduced in
/// `primitive_profile_length_units_e2e.rs`, itself modelled on task 5652's
/// `compile_bare_spacing` in `crates/reify-eval/tests/pattern_spacing_units_e2e.rs`.
fn build_capturing_ops_bare(source: &str) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
    build_compiled(compile_bare_length(source))
}

/// Compile a source whose magnitude arguments are deliberately BARE.
///
/// Asserts (i) the compile-layer `ArgTypeMismatch` really is emitted, so this
/// file cannot silently stop noticing if task η's slots regress, and (ii) it is
/// the ONLY Error-severity compile diagnostic, so an unrelated compile Error
/// cannot make a caller's "no op reached the kernel" assertion hold for the
/// wrong reason.
///
/// The eval-layer assertions still run afterwards because
/// `check_builtin_arg_types` is anti-cascade: it touches only `diagnostics` and
/// never lowering, so the op is still emitted and must still be DROPPED at
/// build by task 5744's gate.
fn compile_bare_length(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "a bare modify/sweep magnitude must ALSO be rejected at compile time \
         (task 5750 ArgTypeMismatch), not only at eval; got no Error diagnostics \
         in: {:?}",
        compiled.diagnostics
    );
    assert!(
        errors
            .iter()
            .all(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)),
        "ArgTypeMismatch must be the ONLY compile Error in this fixture, else the \
         callers' \"no op reached the kernel\" assertions could pass because \
         compilation broke rather than because the eval gate dropped the op; \
         unexpected errors: {:?}",
        errors
            .iter()
            .filter(|d| d.code != Some(DiagnosticCode::ArgTypeMismatch))
            .collect::<Vec<_>>()
    );
    compiled
}

/// The kernel half, shared by the strict and bare compile paths.
fn build_compiled(
    compiled: reify_compiler::CompiledModule,
) -> (Vec<reify_core::Diagnostic>, Vec<GeometryOp>) {
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
    // BARE by construction — every caller passes a source whose magnitude is
    // deliberately undimensioned, so it takes the lenient compile path (task
    // 5750). The dimensioned CONTROLS keep the strict `build_capturing_ops`,
    // which is what still proves they compile clean.
    let (diagnostics, ops) = build_capturing_ops_bare(source);

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

/// BOTH layers reject the bare fillet radius, and they stay INDEPENDENTLY
/// OBSERVABLE because their `DiagnosticCode`s differ.
///
/// REWRITTEN by task 5750 (units-length η), which owns this row per the PRD §8
/// charter. Its predecessor,
/// `bare_fillet_source_compiles_strictly_so_the_rejection_is_the_eval_gate`,
/// asserted that `fillet(.., 1)` COMPILED CLEAN — which was how it proved the
/// rejection under test came from the eval gate rather than from a compile
/// diagnostic. η landed `fillet`'s compile-layer LENGTH slot, so that premise
/// is now false and the assertion had to be replaced rather than deleted: the
/// pair above is only a test of task 5744's chokepoint if the eval gate is
/// still shown to fire on its own account.
///
/// The replacement proves the same thing by a stronger route. Instead of
/// inferring "it must be eval, because compile was silent", it observes BOTH
/// diagnostics directly and pins each to its own layer's code:
/// * compile → `DiagnosticCode::ArgTypeMismatch` (`check_builtin_arg_types`);
/// * build   → `DiagnosticCode::DimensionedArgRejected` (`accept_length_value`).
///
/// That distinction is PRD decision D2, and
/// `crates/reify-core/src/diagnostics.rs` records why the two codes were kept
/// separate rather than shared: sharing one would make "which layer rejected
/// this?" unanswerable from the code alone. This test asserts it from the
/// eval side; `length_slot_rejection_uses_the_compile_layer_code_not_the_eval_layer_one`
/// in `crates/reify-compiler/tests/builtin_arg_signature_tests.rs` asserts it
/// from the compile side.
#[test]
fn bare_fillet_source_carries_both_layers_with_distinct_codes() {
    const SRC: &str = r#"
        structure def BareFillet {
            let body = fillet(box(10mm, 10mm, 10mm), 1)
        }
        "#;

    // (a) COMPILE layer: exactly the ArgTypeMismatch, and nothing else at
    //     Error severity. `compile_bare_length` asserts both halves.
    let compiled = compile_bare_length(SRC);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        compile_errors.len(),
        1,
        "expected exactly one compile-layer Error for the bare fillet radius; \
         got: {compile_errors:?}"
    );
    assert_eq!(
        compile_errors[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "the COMPILE-layer rejection must carry ArgTypeMismatch"
    );
    assert!(
        !compile_errors
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::DimensionedArgRejected)),
        "the compile layer must NOT borrow the eval layer's code, or \
         \"which layer rejected this?\" stops being answerable (PRD D2); \
         got: {compile_errors:?}"
    );

    // (b) EVAL layer: the build gate fires on its own account, with its own
    //     code — the fact the deleted assertion was standing in for.
    let (diagnostics, _ops) = build_capturing_ops_bare(SRC);
    let coded: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::DimensionedArgRejected)
        })
        .collect();
    assert!(
        !coded.is_empty(),
        "the EVAL gate must still reject independently, carrying \
         DimensionedArgRejected — a compile-layer Error does not substitute \
         for it (contract C3: a compile slot never replaces the eval gate); \
         got: {diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)),
        "the BUILD diagnostics must not carry the compile layer's code; \
         got: {diagnostics:?}"
    );
}

fn is_chamfer(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::Chamfer { .. })
}

fn is_chamfer_asymmetric(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::ChamferAsymmetric { .. })
}

// ---------------------------------------------------------------------------
// `chamfer` distance — the buildable modify pair (step-4)
// ---------------------------------------------------------------------------

/// BARE `chamfer(box(10mm,10mm,10mm), 1)` → a coded `DimensionedArgRejected`
/// Error naming the builtin, the `distance` argument and the migration hint,
/// with NO `Chamfer` op reaching the kernel.
///
/// `chamfer`'s 2-arg form is the modify family's buildable e2e vehicle: it takes
/// no `edges` selector, so the whole `.ri` → build → kernel path is exercised
/// against `MockGeometryKernel` without needing curated edge resolution (which
/// this pipeline does not yet offer — see
/// [`dimensioned_chamfer_asymmetric_raises_no_units_rejection`]).
#[test]
fn bare_chamfer_distance_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "chamfer(box(10mm,10mm,10mm), 1)",
        r#"
        structure def BareChamfer {
            let body = chamfer(box(10mm, 10mm, 10mm), 1)
        }
        "#,
        &[
            "chamfer",
            "distance",
            "expects Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_chamfer,
    );
}

/// The inseparable control for the row above: the SAME chamfer with a
/// DIMENSIONED distance builds with ZERO Error diagnostics and exactly ONE
/// `Chamfer` op whose SI distance is still 0.001 — the gate re-wraps the
/// accepted f64 and must not re-scale it.
#[test]
fn dimensioned_chamfer_distance_builds_one_op_with_unchanged_si_distance() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimChamfer {
            let body = chamfer(box(10mm, 10mm, 10mm), 1mm)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned chamfer must build with zero Error diagnostics; got: {errors:?}"
    );

    let chamfers: Vec<_> = ops.iter().filter(|op| is_chamfer(op)).collect();
    assert_eq!(
        chamfers.len(),
        1,
        "a dimensioned chamfer must emit exactly one Chamfer op; got: {chamfers:?}"
    );

    let GeometryOp::Chamfer { distance, .. } = chamfers[0] else {
        unreachable!("filtered to Chamfer above")
    };
    let si = distance
        .as_f64()
        .unwrap_or_else(|| panic!("distance must carry a numeric SI value; got {distance:?}"));
    assert!(
        (si - 0.001).abs() < 1e-12,
        "the gate must not re-scale: distance should stay 0.001 SI metres, got {si}"
    );
}

// ---------------------------------------------------------------------------
// `chamfer_asymmetric` d1 + d2 — ALL FAILURES AT ONCE, end to end
// ---------------------------------------------------------------------------

/// A fully BARE `chamfer_asymmetric(body, edges(body), 1, 2)` must report BOTH
/// `d1` AND `d2` in ONE build.
///
/// The COUNT is the load-bearing half, exactly as in the unit-level
/// `compile_geometry_op_chamfer_asymmetric_reports_both_bare_distances_in_one_build`:
/// a names-only check stays green if the single
/// `required_length_values(["d1","d2"], …)` group read is split back into
/// `?`-chained single-slot calls, because the first call's `?` returns before
/// the second is ever attempted. Only `== 2` catches that regression, and only
/// this row catches it through the real `.ri` → compile → build path.
///
/// SURFACE FORM NOTE: `chamfer_asymmetric` is the exact 4-arg
/// `chamfer_asymmetric(solid, edges, d1, d2)` — the compiler rejects a 3-arg
/// call with an arity diagnostic, which would make this row pass VACUOUSLY (no
/// op, because nothing compiled). The mandatory `edges` selector is why this
/// pair's control below is shaped differently from `chamfer`'s.
#[test]
fn bare_chamfer_asymmetric_reports_both_distances_in_one_build() {
    // Bare `1`/`2` — lenient compile path (task 5750), like `assert_rejected`.
    let (diagnostics, ops) = build_capturing_ops_bare(
        r#"
        structure def BareChamferAsym {
            let body = box(10mm, 10mm, 10mm)
            let out = chamfer_asymmetric(body, edges(body), 1, 2)
        }
        "#,
    );

    let coded: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error && d.code == Some(DiagnosticCode::DimensionedArgRejected)
        })
        .collect();
    assert_eq!(
        coded.len(),
        2,
        "BOTH `d1` and `d2` must be reported in ONE build — this is the half \
         that catches the group read being split back into `?`-chained \
         single-slot calls; got: {diagnostics:?}"
    );
    for slot in ["d1", "d2"] {
        assert!(
            coded.iter().any(|d| {
                d.message.contains(slot)
                    && d.message.contains("expects Length")
                    && d.message
                        .contains("pass a dimensioned length such as `5mm`")
            }),
            "the all-at-once report must name `{slot}` with the wrong-type \
             wording and the migration hint; got: {:?}",
            coded.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    let built: Vec<_> = ops.iter().filter(|op| is_chamfer_asymmetric(op)).collect();
    assert!(
        built.is_empty(),
        "the op must be DROPPED, not silently built with SI-metre distances; \
         got: {built:?}"
    );
}

/// The non-vacuity control for the row above, shaped for what this build
/// pipeline can actually do.
///
/// `chamfer_asymmetric`'s `edges` argument is MANDATORY, and this harness
/// drives a bare `MockGeometryKernel` (`build_capturing_ops` →
/// `build_compiled`) that answers no topology query — so the inline
/// `edges(body)` selector cannot resolve to a concrete edge list here. The
/// DIMENSIONED form is therefore also dropped, but for a wholly unrelated
/// reason that carries its own distinct wording ("the edge selector did not
/// resolve to a concrete edge list", raised by `resolve_curated_edges_p2`).
/// So the control here cannot be "builds one op"; it is instead the sharper
/// claim that the DIMENSIONED form raises NO units rejection at all.
///
/// That is exactly the non-vacuity guarantee the pairing doctrine exists for:
/// it proves the two `DimensionedArgRejected` Errors asserted above are caused
/// by the BARE magnitudes and not by the fixture's shape, the `edges(..)`
/// selector, or the scaffolding limitation. Do not delete "the redundant half".
///
/// Task #5208 note: curated edge selection IS reachable through the production
/// `.ri` pipeline now (inline selectors are pre-hydrated at the realization
/// slot against a REAL kernel), and its unresolved-selector diagnostic was
/// reworded from the old staging notice ("curated edge selection is not yet
/// available on the current build pipeline … tasks 4360/4358") into the
/// actionable message pinned below. What still blocks this row from the
/// ordinary "builds one op" form is purely the bare mock kernel, not a missing
/// capability; upgrading it needs a real-OCCT rewrite of `build_compiled`.
#[test]
fn dimensioned_chamfer_asymmetric_raises_no_units_rejection() {
    let (diagnostics, _ops) = build_capturing_ops(
        r#"
        structure def DimChamferAsym {
            let body = box(10mm, 10mm, 10mm)
            let out = chamfer_asymmetric(body, edges(body), 1mm, 2mm)
        }
        "#,
    );

    let coded: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DimensionedArgRejected))
        .collect();
    assert!(
        coded.is_empty(),
        "a DIMENSIONED chamfer_asymmetric must raise no units rejection; got: {coded:?}"
    );

    // Pin the reason the op is nonetheless absent, so this control cannot
    // silently start passing for a NEW reason (e.g. the fixture ceasing to
    // compile) without the wording changing too. The needle is the stable
    // clause of `resolve_curated_edges_p2`'s unresolved-selector `Err`, and it
    // is deliberately NOT the whole sentence — see the #5208 note above for why
    // the previous "curated edge selection is not yet available" wording is
    // gone.
    assert!(
        diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && d.message
                    .contains("did not resolve to a concrete edge list")
        }),
        "the dimensioned form must still be dropped because the bare \
         MockGeometryKernel cannot resolve `edges(body)` to a concrete edge \
         list — if this harness gains a real kernel, upgrade this control to \
         the ordinary `builds one op` form; got: {diagnostics:?}"
    );
}

fn is_extrude(op: &GeometryOp) -> bool {
    matches!(op, GeometryOp::Extrude { .. })
}

// ---------------------------------------------------------------------------
// `extrude` distance — the sweep family's pair (step-6)
// ---------------------------------------------------------------------------

/// BARE `extrude(rectangle(10mm, 10mm), 20)` → a coded `DimensionedArgRejected`
/// Error naming the builtin, the `distance` argument and the migration hint,
/// with NO `Extrude` op reaching the kernel.
///
/// The pre-gate reading of that source was a 20-METRE extrusion of a 10 mm
/// square — 1000× the intended 20 mm, and silently buildable, because
/// `Value::as_f64` reads a bare `Real` as SI metres and `extrude`'s only
/// existing guard is a degeneracy floor at 1e-12 m that a 20 sails past.
#[test]
fn bare_extrude_distance_drops_the_op_with_a_coded_error() {
    assert_rejected(
        "extrude(rectangle(10mm, 10mm), 20)",
        r#"
        structure def BareExtrude {
            let body = extrude(rectangle(10mm, 10mm), 20)
        }
        "#,
        &[
            "extrude",
            "distance",
            "expects Length",
            "pass a dimensioned length such as `5mm`",
        ],
        is_extrude,
    );
}

/// The inseparable control: the SAME extrude with a DIMENSIONED distance builds
/// with ZERO Error diagnostics and exactly ONE `Extrude` op whose SI distance is
/// still 0.02 — the gate re-wraps the accepted f64 and must not re-scale it.
#[test]
fn dimensioned_extrude_distance_builds_one_op_with_unchanged_si_distance() {
    let (diagnostics, ops) = build_capturing_ops(
        r#"
        structure def DimExtrude {
            let body = extrude(rectangle(10mm, 10mm), 20mm)
        }
        "#,
    );

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a dimensioned extrude must build with zero Error diagnostics; got: {errors:?}"
    );

    let extrudes: Vec<_> = ops.iter().filter(|op| is_extrude(op)).collect();
    assert_eq!(
        extrudes.len(),
        1,
        "a dimensioned extrude must emit exactly one Extrude op; got: {extrudes:?}"
    );

    let GeometryOp::Extrude { distance, .. } = extrudes[0] else {
        unreachable!("filtered to Extrude above")
    };
    let si = distance
        .as_f64()
        .unwrap_or_else(|| panic!("distance must carry a numeric SI value; got {distance:?}"));
    assert!(
        (si - 0.02).abs() < 1e-12,
        "the gate must not re-scale: distance should stay 0.02 SI metres, got {si}"
    );
}
