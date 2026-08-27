//! End-to-end (`Engine::build`) LENGTH lock for `isosurface(..., iso)` —
//! units-length λ (task 5755, PRD
//! `docs/prds/v0_6/units-length-gate-completion.md`, decision D12).
//!
//! WHY THIS FILE EXISTS AT ALL, given `geometry_ops/tests.rs` already tables
//! the three Contract C states at `compile_geometry_op`: `isosurface` has NO
//! entry in `builtin_arg_slots`, so unlike the pattern spacings there is no
//! compile-layer slot shadowing it — the eval-layer gate is the ONLY one
//! standing between a bare `iso: 5` and a 5 SI-**metre** isovalue reaching the
//! kernel. A unit test at `compile_geometry_op` proves the classifier; it does
//! not prove that a bare `iso` WRITTEN IN REAL `.ri` SOURCE survives lowering,
//! reaches that classifier, and surfaces to the author through the build loop's
//! `failed to compile geometry operation: <err>` wrapper. That whole-path claim
//! is what the sibling label half of λ gets from
//! `pattern_spacing_units_e2e.rs`, and what this file gives the `iso` half.
//!
//! WHY NOT EXTEND `isosurface_iso_option_e2e.rs`: that file is
//! `#[cfg(has_openvdb)]`-gated AND runtime-skips via `occt_available_or_skip`,
//! so in a lane without both kernels it contributes ZERO signal. This file is
//! deliberately kernel-INDEPENDENT: every case below is decided before any
//! kernel call (the rejection cases drop the op; the positive control only
//! needs `MockGeometryKernel` to record what it was handed), so it runs
//! everywhere the workspace tests run. Same reasoning, same mock-kernel harness
//! shape, as `pattern_spacing_units_e2e.rs`.

use reify_core::Severity;
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, parse_and_compile};

/// A `structure def` whose `isosurface(...)` call carries `arg` verbatim after
/// the grid operand — e.g. `", 5"`, `", 5mm"`, `", adaptive: true"`, or `""`
/// to omit the optional args entirely. Single-sources the fixture so the box
/// dimensions cannot drift between cases.
///
/// The grid operand is a plain `box(...)` rather than a voxel grid on purpose:
/// nothing on the path under test inspects the operand's repr
/// (`compile_geometry_op`'s `Isosurface` arm only calls `resolve_geom_ref`),
/// and using a primitive keeps the fixture free of the OpenVDB dependency this
/// file exists to avoid.
fn iso_source(arg: &str) -> String {
    format!(
        "structure def IsoUnits {{ let solid = box(10mm, 10mm, 10mm)  \
         let shell = isosurface(solid{arg}) }}"
    )
}

/// Build `source` against a mock kernel; return its Error diagnostics and every
/// `GeometryOp::Surface` that actually reached the kernel.
///
/// STRICT `parse_and_compile` (which hard-asserts zero compile Error
/// diagnostics) is what keeps the rejection cases below from passing
/// vacuously: `isosurface` has no `builtin_arg_slots` row, so every source
/// here — bare `iso` included — must compile clean, and a dropped op can then
/// only mean the EVAL gate dropped it, never that lowering broke.
fn build(source: &str) -> (Vec<String>, Vec<(f64, bool)>) {
    let compiled = parse_and_compile(source);

    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();

    let ops = ops_ref.lock().unwrap();
    let surfaces: Vec<(f64, bool)> = ops
        .iter()
        .filter_map(|r| match &r.op {
            GeometryOp::Surface {
                iso_level,
                adaptive,
                ..
            } => Some((*iso_level, *adaptive)),
            _ => None,
        })
        .collect();

    (errors, surfaces)
}

/// A BARE `iso` written in real `.ri` source must DROP the op, never be built
/// as a 5 SI-**metre** isovalue.
///
/// The 1000x-silent defect this pins is the same class as task 5214's bare
/// pattern spacing: measured on the pre-λ tree, `isosurface(solid, 5)` returned
/// `Ok(Surface { iso_level: 5.0, .. })` with ZERO diagnostics.
///
/// EXACT-VECTOR assertion, because the pair is the contract and neither half
/// alone is: an INVALID (as opposed to Undef) argument surfaces TWO Errors, and
/// the author needs both — the typed Contract C rejection says WHAT is wrong
/// and how to fix it, the build-loop wrapper says the op was consequently
/// DROPPED. Pinning the vector also subsumes the weaker "no cascade" count
/// check: a third Error would fail it. Contrast the Undef arm, which is
/// deliberately QUIET at the value layer (D10 / INV-SF-1) and so surfaces the
/// wrapper ALONE — that asymmetry is exactly what
/// `pattern_spacing_units_e2e.rs`'s single-Error label probe measures.
#[test]
fn bare_iso_drops_surface_op_with_typed_rejection_and_drop_wrapper() {
    let (errors, surfaces) = build(&iso_source(", 5"));

    assert_eq!(
        errors,
        vec![
            "isosurface: iso argument expects Length, got Int; pass a dimensioned \
             length such as `5mm`"
                .to_string(),
            "failed to compile geometry operation: missing or non-Length argument \
             'iso' for isosurface"
                .to_string(),
        ],
        "a bare `iso` must surface the typed Contract C rejection AND the \
         op-dropped wrapper, and nothing else"
    );
    assert!(
        surfaces.is_empty(),
        "a bare-`iso` isosurface must be DROPPED — `isosurface` has no \
         compile-layer slot, so nothing else stops 5 reaching the kernel as 5 SI \
         metres; emitted Surface ops: {surfaces:?}"
    );
}

/// Positive control: the SAME fixture with a DIMENSIONED `5mm` builds cleanly —
/// zero Error diagnostics, exactly one `GeometryOp::Surface`, and the isovalue
/// converted to SI metres. Without this, the rejection test above could pass
/// because `isosurface` stopped lowering entirely.
#[test]
fn dimensioned_iso_builds_surface_op_in_si_metres() {
    let (errors, surfaces) = build(&iso_source(", 5mm"));

    assert!(
        errors.is_empty(),
        "a dimensioned `5mm` iso must build with zero Error diagnostics; got: {errors:?}"
    );
    assert_eq!(
        surfaces.len(),
        1,
        "a dimensioned iso must emit exactly one Surface op; got: {surfaces:?}"
    );
    assert!(
        (surfaces[0].0 - 0.005).abs() < 1e-12,
        "`5mm` must reach the kernel as 0.005 SI metres; got: {}",
        surfaces[0].0
    );
}

/// D12's ABSENT arm, end-to-end: `isosurface(solid)` — the common shipped form
/// — keeps the deliberate un-gated `iso_level == 0.0` default and stays QUIET.
///
/// This is the anti-over-fixing lock at the build layer. Routing ABSENCE
/// through `required_length_arg` too would push `eval_named_arg`'s missing-arg
/// Warning at every bare `isosurface(g)` call site in the corpus; this test
/// fails the moment someone "tidies" the two halves into one.
#[test]
fn absent_iso_keeps_the_ungated_default_and_stays_quiet() {
    let (errors, surfaces) = build(&iso_source(""));

    assert!(
        errors.is_empty(),
        "absence is the normal expected shape and must emit no Error; got: {errors:?}"
    );
    assert_eq!(
        surfaces.len(),
        1,
        "a bare `isosurface(g)` must still build exactly one Surface op; got: {surfaces:?}"
    );
    assert_eq!(
        surfaces[0],
        (0.0, false),
        "an ABSENT iso/adaptive keeps the documented (0.0, false) defaults (D12)"
    );
}

/// The reachable source shape behind the `Bool` row of
/// `compile_geometry_op_isosurface_non_length_iso_is_rejected_not_read_as_metres`,
/// pinned here so the failure mode is discoverable rather than folklore.
///
/// `isosurface`'s optional args are lowered POSITIONALLY:
/// `reify_compiler::geometry`'s `compile_geometry_call_inner` drops the AST's
/// `arg_names`, and the `"isosurface"` arm binds slot 1 to `iso` and slot 2 to
/// `adaptive`. So a source that SKIPS the first optional arg binds
/// `Bool(true)` to `iso` and is rejected with a message naming an argument the
/// author never wrote — and, since λ, the op is DROPPED rather than built at
/// iso 0.0 as the pre-λ Warning left it.
///
/// This test asserts the CURRENT behaviour, not the desired one. The lowering
/// quirk is pre-existing (λ did not introduce it) and its fix — honouring
/// `arg_names` — lives in `crates/reify-compiler/src/geometry.rs`, a file λ
/// holds no lock on. When that lands, `adaptive: true` will bind to `adaptive`
/// and this test SHOULD be rewritten to assert a clean build; the assertions
/// deliberately name the positional binding so that rewrite is obviously the
/// right response to the failure rather than a regression to paper over.
#[test]
fn skipped_optional_iso_slot_binds_adaptive_positionally() {
    let (errors, surfaces) = build(&iso_source(", adaptive: true"));

    assert_eq!(
        errors,
        vec![
            "isosurface: iso argument expects Length, got Bool; pass a dimensioned \
             length such as `5mm`"
                .to_string(),
            "failed to compile geometry operation: missing or non-Length argument \
             'iso' for isosurface"
                .to_string(),
        ],
        "`isosurface(g, adaptive: true)` currently binds Bool(true) to the `iso` \
         SLOT, so both diagnostics name `iso` — not the `adaptive` the author \
         actually typed"
    );
    assert!(
        surfaces.is_empty(),
        "the misbound call drops the op entirely (pre-λ it warned and still built \
         at iso 0.0); emitted Surface ops: {surfaces:?}"
    );
}
