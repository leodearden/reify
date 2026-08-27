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
fn build_isosurface(source: &str) -> (Vec<String>, Vec<(f64, bool)>) {
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
    let (errors, surfaces) = build_isosurface(&iso_source(", 5"));

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
    let (errors, surfaces) = build_isosurface(&iso_source(", 5mm"));

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
    let (errors, surfaces) = build_isosurface(&iso_source(""));

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
    let (errors, surfaces) = build_isosurface(&iso_source(", adaptive: true"));

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
