//! End-to-end regression lock for task 5214 — the headline behaviour:
//! a BARE (dimensionless) length-semantic pattern argument must be REJECTED at
//! eval/build, producing a `Severity::Error` diagnostic and DROPPING the op,
//! rather than silently reading the bare number as SI **metres**.
//!
//! Before the fix, `linear_pattern_2d(..., spacing1: 20, ..., spacing2: 20)`
//! silently placed instances 20 SI **metres** apart (1000× a plausible 20 mm
//! pitch) — the root cause of the litter-tray "holes vanish" symptom, where a
//! bare-spacing grid scatters cutting tools hundreds of metres from the plate
//! so the difference-sieve removes ~1 hole per pattern. The eval-layer gate
//! (`eval_named_arg_length`) now fails closed.
//!
//! Error-diagnostic assertion modelled on `mirror_circular_value_forms_e2e.rs`;
//! emitted-op inspection (`operations_ref`) modelled on
//! `arbitrary_pattern_transform_e2e.rs`.
//!
//! ## Charter, widened by units-length λ (task 5755)
//!
//! This file is now the `Engine::build` leaf signal for LENGTH-semantic
//! geometry arguments generally, not the pattern spacings alone — same
//! headline behaviour, same mock-kernel harness, three subjects:
//!
//! 1. task 5214's original bare pattern spacings (`linear_pattern` /
//!    `linear_pattern_2d`), above;
//! 2. λ's DIAGNOSTIC LABEL probe — the label a real build shows the author must
//!    be the builtin they typed (PRD §6 boundary row 18, decision D7);
//! 3. λ's `isosurface(..., iso)` gate (decision D12), at the bottom.
//!
//! (3) LIVES HERE rather than in a file of its own because
//! `scripts/check-harness-baseline-registration.sh` rejects a newly-ADDED
//! standalone `crates/<c>/tests/<f>.rs` binary outright — task 5265's
//! anti-re-accretion ratchet serving PRD `merge-gate-compile-cost.md`, whose
//! whole point is to cut the merge-gate LINK count. Minting a new
//! `harness_<subsystem>` root to hold four tests would clear the gate's letter
//! while adding exactly the compile unit it exists to prevent. Co-locating λ's
//! two `Engine::build` halves costs nothing and reads better.

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source, parse_and_compile,
};

/// Compile a source whose pattern spacing is deliberately BARE.
///
/// Task 5652 added a compile-LAYER `ArgTypeMismatch` Error for bare pattern
/// spacing, so these sources no longer compile clean and `parse_and_compile`
/// (which hard-asserts zero Error diagnostics) would panic before eval ever
/// runs. The non-asserting `compile_source` keeps this file testing what it
/// exists to test: task 5214's EVAL-layer gate.
///
/// The assertions below are what make that switch a TIGHTENING rather than a
/// loosening. They keep BOTH halves of what `parse_and_compile` used to give:
///
/// 1. The expected compile-layer `ArgTypeMismatch` really is emitted, so this
///    file cannot silently stop noticing if task 5652's gate regresses.
/// 2. It is the ONLY Error-severity compile diagnostic. Without this, ANY
///    unrelated compile Error would go unnoticed — and if e.g. `box(…)` or
///    `linear_pattern_2d(…)` stopped lowering, each caller's "no pattern op
///    reached the kernel" assertion would then hold VACUOUSLY (op absent
///    because compilation broke, not because task 5214's eval gate dropped it),
///    which is precisely the wrong-reason pass this file exists to prevent.
///
/// Each caller's eval-layer assertions still run, because
/// `check_builtin_arg_types` is anti-cascade: lowering is untouched, so the op
/// is still emitted and must still be DROPPED at eval.
fn compile_bare_spacing(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "a bare pattern spacing must ALSO be rejected at compile time (task 5652 \
         ArgTypeMismatch), not only at eval; got no Error diagnostics in: {:?}",
        compiled.diagnostics
    );
    assert!(
        errors
            .iter()
            .all(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)),
        "ArgTypeMismatch must be the ONLY compile Error in this fixture, else the \
         callers' \"no pattern op reached the kernel\" assertions could pass \
         because compilation broke rather than because the eval gate dropped the \
         op; unexpected errors: {:?}",
        errors
            .iter()
            .filter(|d| d.code != Some(DiagnosticCode::ArgTypeMismatch))
            .collect::<Vec<_>>()
    );
    compiled
}

/// BARE `20` spacings on `linear_pattern_2d` → the op is dropped: at least one
/// `Severity::Error` diagnostic is emitted and NO `LinearPattern2D` op reaches
/// the kernel (it is NOT silently built with 20 SI-metre spacing).
#[test]
fn linear_pattern_2d_bare_spacing_drops_op_with_error() {
    let source = r#"
        structure def BareSpacingGrid {
            let grid = linear_pattern_2d(
                box(10mm, 10mm, 10mm),
                1, 0, 0, 3, 20,
                0, 1, 0, 3, 20
            )
        }
    "#;

    let compiled = compile_bare_spacing(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "bare (dimensionless) linear_pattern_2d spacings must produce at least \
         one Error diagnostic; got diagnostics: {:?}",
        result.diagnostics
    );

    let ops = ops_ref.lock().unwrap();
    let pattern_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::LinearPattern2D { .. }))
        .collect();
    assert!(
        pattern_ops.is_empty(),
        "a bare-spacing linear_pattern_2d must be DROPPED, not silently built \
         with 20 SI-metre spacing; emitted LinearPattern2D ops: {:?}",
        pattern_ops.len()
    );
}

/// Build `source` against a mock kernel and return
/// `(error_diagnostic_count, matching_op_count)`, where an op matches when
/// `is_pattern` accepts it. Shared by the 1D `linear_pattern` pair below, whose
/// two cases differ only in the source and the expected counts.
fn build_and_count(
    compiled: &reify_compiler::CompiledModule,
    is_pattern: fn(&GeometryOp) -> bool,
) -> (usize, usize) {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(compiled, ExportFormat::Step);

    let error_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let ops = ops_ref.lock().unwrap();
    let op_count = ops.iter().filter(|r| is_pattern(&r.op)).count();
    (error_count, op_count)
}

/// The 1D `linear_pattern` needs its own e2e lock, not just the unit-level one:
/// its spacing is read at a SEPARATE call site from `linear_pattern_2d`'s
/// spacing1/spacing2, so a regression there would slip past the 2D e2e above.
///
/// BARE `20` spacing → at least one `Severity::Error` and NO `LinearPattern` op
/// reaches the kernel; DIMENSIONED `20mm` → zero Errors and exactly one op
/// (the positive control that keeps the rejection case from passing vacuously).
#[test]
fn linear_pattern_1d_bare_spacing_drops_op_dimensioned_builds() {
    let is_linear = |op: &GeometryOp| matches!(op, GeometryOp::LinearPattern { .. });

    let (bare_errors, bare_ops) = build_and_count(
        &compile_bare_spacing(
            r#"
        structure def BareSpacingRow {
            let row = linear_pattern(box(10mm, 10mm, 10mm), 1, 0, 0, 3, 20)
        }
        "#,
        ),
        is_linear,
    );
    assert!(
        bare_errors > 0,
        "a bare (dimensionless) linear_pattern spacing must produce at least one \
         Error diagnostic; got {bare_errors}"
    );
    assert_eq!(
        bare_ops, 0,
        "a bare-spacing linear_pattern must be DROPPED, not silently built with \
         20 SI-metre spacing; emitted LinearPattern ops: {bare_ops}"
    );

    // The dimensioned control keeps the STRICT `parse_and_compile`: it must
    // still compile with zero Error diagnostics, which is what proves the new
    // compile-layer slot does not fire on valid code.
    let (dim_errors, dim_ops) = build_and_count(
        &parse_and_compile(
            r#"
        structure def DimSpacingRow {
            let row = linear_pattern(box(10mm, 10mm, 10mm), 1, 0, 0, 3, 20mm)
        }
        "#,
        ),
        is_linear,
    );
    assert_eq!(
        dim_errors, 0,
        "a dimensioned 20mm linear_pattern spacing must build with zero Error \
         diagnostics; got {dim_errors}"
    );
    assert_eq!(
        dim_ops, 1,
        "a dimensioned linear_pattern must emit exactly one LinearPattern op; \
         got {dim_ops}"
    );
}

/// Positive control / contrast: the SAME grid with DIMENSIONED `20mm` spacings
/// builds cleanly — zero Error diagnostics and exactly one `LinearPattern2D`
/// op reaches the kernel. This guards the rejection test above against a
/// vacuous pass (op absent for an unrelated reason).
#[test]
fn linear_pattern_2d_dimensioned_spacing_builds_op() {
    let source = r#"
        structure def DimSpacingGrid {
            let grid = linear_pattern_2d(
                box(10mm, 10mm, 10mm),
                1, 0, 0, 3, 20mm,
                0, 1, 0, 3, 20mm
            )
        }
    "#;

    let compiled = parse_and_compile(source);
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        error_diags.is_empty(),
        "dimensioned 20mm spacings must build with zero Error diagnostics, got: {:?}",
        error_diags
    );

    let ops = ops_ref.lock().unwrap();
    let pattern_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::LinearPattern2D { .. }))
        .collect();
    assert_eq!(
        pattern_ops.len(),
        1,
        "dimensioned linear_pattern_2d must emit exactly one LinearPattern2D op, \
         got: {}",
        pattern_ops.len()
    );
}

/// units-length λ (task 5755, PRD §6 boundary row 18) — the LABEL a real
/// `Engine::build` shows the author must be the builtin they TYPED.
///
/// WHY THIS FIXTURE and not the bare-`20` one used everywhere above: the
/// bare-spacing route is SHADOWED by task 5652's compile-layer `ArgTypeMismatch`
/// slot, whose message is minted by `ArgRejection::message` from the DSL
/// builtin name — already `linear_pattern`, and never routed through
/// `PatternKind::Display`. So a bare-spacing source cannot fail on the label
/// and is worthless as a fixture here. An UNBOUND `param s : Length` folds to
/// `Value::Undef` at build instead, reaching `required_length_arg`'s
/// `Unresolved` arm — the one caller-facing message where
/// `PatternKind::Display` is the sole producer of the label token.
///
/// Reachability was MEASURED on the pre-change tree (task 5755 pre-1), not
/// assumed: this exact source compiles with ZERO diagnostics and builds to
/// exactly one `Severity::Error`, `"failed to compile geometry operation:
/// argument 'spacing' for linear is unresolved (Undef)"`, with no
/// `LinearPattern` op reaching the kernel. RED until the λ `Display` flip
/// turns `for linear` into `for linear_pattern`.
///
/// The `failed to compile geometry operation: <err>` wrapper shape is already
/// proven observable from a real `Engine::build` by
/// `unified_dag_geometry_executors.rs`'s `"argument 'radius' for fillet is
/// unresolved (Undef)"` assertion.
#[test]
fn unresolved_spacing_diagnostic_names_the_dsl_builtin_not_the_variant_nickname() {
    let source = r#"
        structure def LabelProbe {
            param s : Length
            let b = box(10mm, 10mm, 10mm)
            let p = linear_pattern(b, 1, 0, 0, 3, s)
        }
    "#;

    // STRICT `parse_and_compile`: an unbound param is legal `.ri`, so this
    // fixture must compile with zero Error diagnostics. That is what keeps the
    // assertions below from passing because compilation broke.
    let compiled = parse_and_compile(source);

    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(&compiled, ExportFormat::Step);

    let error_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        error_diags.len(),
        1,
        "an unresolved spacing must produce EXACTLY ONE Error diagnostic (no \
         cascade); got: {:?}",
        result.diagnostics
    );
    let msg = &error_diags[0].message;
    assert!(
        msg.contains("failed to compile geometry operation"),
        "the eval-layer Err must surface through the build-loop wrapper; got: {msg:?}"
    );
    assert!(
        msg.contains("argument 'spacing' for linear_pattern is unresolved (Undef)"),
        "the diagnostic must name `linear_pattern` — the builtin the author \
         actually typed and can grep for — not the `PatternKind::Linear` variant \
         nickname `linear`; got: {msg:?}"
    );

    let ops = ops_ref.lock().unwrap();
    let pattern_ops: Vec<_> = ops
        .iter()
        .filter(|r| matches!(&r.op, GeometryOp::LinearPattern { .. }))
        .collect();
    assert!(
        pattern_ops.is_empty(),
        "an unresolved-spacing linear_pattern must be DROPPED, not built; \
         emitted LinearPattern ops: {}",
        pattern_ops.len()
    );
}

// ---------------------------------------------------------------------------
// units-length λ (task 5755), decision D12 — `isosurface(..., iso)`.
//
// WHY AN E2E AT ALL, given `geometry_ops/tests.rs` already tables the three
// Contract C states at `compile_geometry_op`: `isosurface` has NO entry in
// `builtin_arg_slots`, so unlike the pattern spacings above there is no
// compile-layer slot shadowing it — the eval-layer gate is the ONLY thing
// standing between a bare `iso: 5` and a 5 SI-**metre** isovalue reaching the
// kernel. A unit test proves the classifier; it does not prove that a bare
// `iso` WRITTEN IN REAL `.ri` SOURCE survives lowering, reaches that
// classifier, and surfaces to the author through the build loop's
// `failed to compile geometry operation: <err>` wrapper.
//
// WHY NOT EXTEND `isosurface_iso_option_e2e.rs`: that file is
// `#[cfg(has_openvdb)]`-gated AND runtime-skips via `occt_available_or_skip`,
// so in a lane without both kernels it contributes ZERO signal. Everything
// below is kernel-INDEPENDENT — the rejection cases are decided before any
// kernel call, and the positive control only needs `MockGeometryKernel` to
// record what it was handed — so it runs everywhere the workspace tests run.
// ---------------------------------------------------------------------------

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
///
/// NAMED for that operand, not `iso_source`, because
/// `crates/reify-eval/tests/isosurface_iso_option_e2e.rs` already owns an
/// `iso_source` building a DIFFERENT fixture (a `#[cfg(has_openvdb)]`
/// narrow-band voxel path with a `param size` knob). The two are deliberately
/// NOT interchangeable — that file needs a real surfaceable grid to compare
/// triangle counts, this one needs the opposite — so they must not share a
/// name that invites a future edit to one to be assumed to apply to both.
fn box_operand_iso_source(arg: &str) -> String {
    format!(
        "structure def IsoUnits {{ let solid = box(10mm, 10mm, 10mm)  \
         let shell = isosurface(solid{arg}) }}"
    )
}

/// What one `isosurface` build produced: its Error diagnostics, its
/// below-Error ones ABOUT THE BUILTIN UNDER TEST, and every
/// `GeometryOp::Surface` that reached the kernel.
///
/// The two diagnostic vectors are split rather than pooled because they answer
/// different questions and must not be able to substitute for one another: the
/// Errors are the CONTRACT (what the author is told is wrong, and what makes
/// `reify eval` exit nonzero), while an advisory is a HINT that must never
/// inflate that count.
struct IsoBuild {
    /// EVERY `Severity::Error` message, unfiltered — the exact-vector
    /// assertions below depend on that, since an unexpected extra Error is
    /// exactly the cascade they exist to catch.
    errors: Vec<String>,
    /// Below-Error messages naming `isosurface`.
    ///
    /// FILTERED, unlike `errors`, and the filter is load-bearing: every build in
    /// this file emits two unrelated `Severity::Warning`s from the mock harness
    /// ("no openvdb kernel registered", "topology-attribute seeding failed"),
    /// because a `Surface` op demands a Voxel repr no mock provides. Those are
    /// this file's fixture talking, not the units gate; pinning them would make
    /// these tests fail on an unrelated harness change they hold no lock on.
    /// Naming the builtin is the discriminator because every diagnostic the
    /// `isosurface` arm itself emits is prefixed with it, and none of the
    /// harness ones are.
    iso_advisories: Vec<String>,
    surfaces: Vec<(f64, bool)>,
}

/// Build `source` against a mock kernel and collect the three.
///
/// STRICT `parse_and_compile` (which hard-asserts zero compile Error
/// diagnostics) is what keeps the rejection cases below from passing
/// vacuously: `isosurface` has no `builtin_arg_slots` row, so every source
/// here — bare `iso` included — must compile clean, and a dropped op can then
/// only mean the EVAL gate dropped it, never that lowering broke.
fn build_isosurface(source: &str) -> IsoBuild {
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
    let iso_advisories: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Error && d.message.contains("isosurface"))
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

    IsoBuild {
        errors,
        iso_advisories,
        surfaces,
    }
}

/// The EXACT two-Error vector a rejected `isosurface` `iso` of runtime type
/// `got` must produce, in order.
///
/// Both halves of the pair are the contract and neither alone is: the typed
/// Contract C rejection says WHAT is wrong and how to fix it, and the
/// build-loop wrapper says the op was consequently DROPPED. Pinning the vector
/// also subsumes the weaker "no cascade" count check — a third Error fails it.
///
/// Single-sourced across the rejection tests below, which previously pasted
/// both literals and so would each have needed editing on any reword of
/// `ArgRejection::message`. It cannot additionally share with
/// `geometry_ops/tests.rs`'s `expected_length_rejection`: that helper lives
/// inside the crate's private test module and the wording's true owner,
/// `arg_acceptance`, is `pub(crate)` — so an integration test can only restate
/// it. Reducing three copies to two is the reachable win; the remaining pair is
/// the unit/integration boundary, not carelessness.
fn expected_iso_rejection(got: &str) -> Vec<String> {
    vec![
        format!(
            "isosurface: iso argument expects Length, got {got}; pass a dimensioned \
             length such as `5mm`"
        ),
        "failed to compile geometry operation: missing or non-Length argument \
         'iso' for isosurface"
            .to_string(),
    ]
}

/// A BARE `iso` written in real `.ri` source must DROP the op, never be built
/// as a 5 SI-**metre** isovalue.
///
/// The 1000x-silent defect this pins is the same class as task 5214's bare
/// pattern spacing: measured on the pre-λ tree, `isosurface(solid, 5)` returned
/// `Ok(Surface { iso_level: 5.0, .. })` with ZERO diagnostics.
///
/// EXACT-VECTOR assertion via `expected_iso_rejection` — see that helper for
/// why both Errors are the contract. Contrast the Undef arm, which is
/// deliberately QUIET at the value layer (D10 / INV-SF-1) and so surfaces the
/// wrapper ALONE — that asymmetry is exactly what this file's single-Error
/// label probe measures.
///
/// A bare number is a spelling the author really typed, so — unlike the `Bool`
/// sibling below — there is nothing to disambiguate: the `advisories` assertion
/// keeps the #6313 positional-binding hint from leaking onto this row, where it
/// would be pure noise.
#[test]
fn bare_iso_drops_surface_op_with_typed_rejection_and_drop_wrapper() {
    let built = build_isosurface(&box_operand_iso_source(", 5"));

    assert_eq!(
        built.errors,
        expected_iso_rejection("Int"),
        "a bare `iso` must surface the typed Contract C rejection AND the \
         op-dropped wrapper, and nothing else"
    );
    assert!(
        built.iso_advisories.is_empty(),
        "a bare number needs no positional-binding hint — the author wrote the \
         value they meant; got: {:?}",
        built.iso_advisories
    );
    assert!(
        built.surfaces.is_empty(),
        "a bare-`iso` isosurface must be DROPPED — `isosurface` has no \
         compile-layer slot, so nothing else stops 5 reaching the kernel as 5 SI \
         metres; emitted Surface ops: {:?}",
        built.surfaces
    );
}

/// Positive control: the SAME fixture with a DIMENSIONED `5mm` builds cleanly —
/// zero Error diagnostics, exactly one `GeometryOp::Surface`, and the isovalue
/// converted to SI metres. Without this, the rejection test above could pass
/// because `isosurface` stopped lowering entirely.
#[test]
fn dimensioned_iso_builds_surface_op_in_si_metres() {
    let built = build_isosurface(&box_operand_iso_source(", 5mm"));

    assert!(
        built.errors.is_empty(),
        "a dimensioned `5mm` iso must build with zero Error diagnostics; got: {:?}",
        built.errors
    );
    assert_eq!(
        built.surfaces.len(),
        1,
        "a dimensioned iso must emit exactly one Surface op; got: {:?}",
        built.surfaces
    );
    assert!(
        (built.surfaces[0].0 - 0.005).abs() < 1e-12,
        "`5mm` must reach the kernel as 0.005 SI metres; got: {}",
        built.surfaces[0].0
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
    let built = build_isosurface(&box_operand_iso_source(""));

    assert!(
        built.errors.is_empty(),
        "absence is the normal expected shape and must emit no Error; got: {:?}",
        built.errors
    );
    assert!(
        built.iso_advisories.is_empty(),
        "\"quiet\" means quiet BELOW Error too — a missing-arg Warning naming \
         `isosurface` here is precisely the regression `optional_length_arg`'s \
         `Ok(None)` arm exists to prevent; got: {:?}",
        built.iso_advisories
    );
    assert_eq!(
        built.surfaces.len(),
        1,
        "a bare `isosurface(g)` must still build exactly one Surface op; got: {:?}",
        built.surfaces
    );
    assert_eq!(
        built.surfaces[0],
        (0.0, false),
        "an ABSENT iso/adaptive keeps the documented (0.0, false) defaults (D12)"
    );
}

/// The reachable source shape behind the `Bool` row of
/// `compile_geometry_op_isosurface_non_length_iso_is_rejected_not_read_as_metres`,
/// pinned here so the failure mode is discoverable rather than folklore.
///
/// `isosurface(g, adaptive: true)` binds `Bool(true)` to the `iso` SLOT — a
/// pre-existing positional-lowering quirk owned by live task #6313.
///
/// Because the Errors then name an argument the author never wrote, and their
/// wording is single-owned upstream and so may not be forked here, a
/// SUPPLEMENTARY advisory carries the actionable part. Its two properties are
/// asserted separately and both matter: it must be PRESENT (or the rejection is
/// unactionable for a real spelling) and it must be BELOW Error severity (or one
/// bad input reports as two failures). `contains`, not equality, on the hint —
/// it is prose, and pinning it verbatim here would just relocate the reword
/// burden this file's `expected_iso_rejection` helper exists to remove.
///
/// This test asserts the CURRENT behaviour, not the desired one, and #6313 is
/// the ADDRESSEE of the rewrite instruction below. When #6313 lands,
/// `adaptive: true` will bind to `adaptive` and this test SHOULD be rewritten
/// to assert a clean build — and the hint, having become unreachable, deleted
/// with it. The assertions deliberately name the positional binding so that
/// rewrite is obviously the right response to the failure rather than a
/// regression to paper over.
#[test]
fn skipped_optional_iso_slot_binds_adaptive_positionally() {
    let built = build_isosurface(&box_operand_iso_source(", adaptive: true"));

    assert_eq!(
        built.errors,
        expected_iso_rejection("Bool"),
        "`isosurface(g, adaptive: true)` currently binds Bool(true) to the `iso` \
         SLOT, so both Errors name `iso` — not the `adaptive` the author actually \
         typed"
    );
    assert_eq!(
        built.iso_advisories.len(),
        1,
        "exactly one supplementary advisory — the misbind hint; got: {:?}",
        built.iso_advisories
    );
    for needle in ["POSITION", "isosurface(g, 0mm, true)", "#6313"] {
        assert!(
            built.iso_advisories[0].contains(needle),
            "the hint must name the cause, a working spelling, and the owning \
             task; missing {needle:?} in: {:?}",
            built.iso_advisories[0]
        );
    }
    assert!(
        built.surfaces.is_empty(),
        "the misbound call drops the op entirely (pre-λ it warned and still built \
         at iso 0.0); emitted Surface ops: {:?}",
        built.surfaces
    );
}
