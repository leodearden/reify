//! process-DFM δ end-to-end CI test (task #4275, PRD §7).
//!
//! **(a) ALWAYS-ON** (no kernel):
//!   - `std_process_dfm_compiles_error_clean` — the example parses and compiles
//!     with no Error-severity diagnostics.
//!   - `std_process_dfm_scalar_constraints_satisfied_violated` — under a no-kernel
//!     `make_simple_engine().check()`, the SCALAR DFM constraints (Manufacturable,
//!     BendManufacturable, DrawManufacturable, DraftManufacturable) report the
//!     expected Satisfied/Violated set. The geometry-backed FitsBuildVolume and
//!     FeatureManufacturable constraints are Indeterminate without a kernel and are
//!     intentionally NOT asserted here.
//!   - `std_process_dfm_build_volume_constraints_declared` — the two FitsBuildVolume
//!     constraint entries are present (as Indeterminate) in the no-kernel result.
//!
//! **(b) DEFERRED to the build-DAG cutover** —
//!   `std_process_dfm_build_volume_flip_and_severity_diagnostics` is `#[ignore]`d:
//!   the geometry-backed OK→VIOLATED flip + I/W/E_DFM_BUILD_VOLUME severity
//!   diagnostics require the UnifiedDag post-geometry constraint re-check, which is
//!   not the default scheduler until the human-gated cutover (#4362). See the
//!   test's doc comment for the full rationale; the canonical flip proof is owned
//!   by task η #4360.
//!
//! The example is also covered for compile-cleanliness by
//! `crates/reify-compiler/tests/examples_smoke.rs` (directory walk).

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::{ConstraintCheckEntry, Engine};
use reify_ir::{ExportFormat, Satisfaction};
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

const STD_PROCESS_DFM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/process/std_process_dfm.ri"
);

fn read_dfm_example() -> String {
    std::fs::read_to_string(STD_PROCESS_DFM_PATH)
        .expect("examples/process/std_process_dfm.ri should exist (task 4275 step-2)")
}

// ── (a) ALWAYS-ON: compile-cleanliness ───────────────────────────────────────

/// ALWAYS-ON: example file parses and compiles with no Error-severity diagnostics.
///
/// RED: the example file does not exist yet → `read_to_string` panics.
/// GREEN: `examples/process/std_process_dfm.ri` is authored (step-2).
#[test]
fn std_process_dfm_compiles_error_clean() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/process/std_process_dfm.ri should compile with no Error-severity \
         diagnostics; got:\n{:#?}",
        errors_only(&compiled)
    );
}

// ── (a) ALWAYS-ON: scalar DFM constraint satisfaction ────────────────────────

/// ALWAYS-ON: scalar DFM constraints report the expected Satisfied/Violated set
/// under `make_simple_engine().check()` (no geometry kernel required).
///
/// Asserts at minimum one Satisfied and one Violated entry for each of:
/// `Manufacturable`, `BendManufacturable`, `DrawManufacturable`, `DraftManufacturable`.
///
/// Does NOT assert global eval-Error-cleanliness: geometry-backed constraints
/// (FitsBuildVolume, FeatureManufacturable) are Indeterminate without a kernel;
/// direct `fits_build_volume(bounding_box(box(...)), ...)` let-bindings yield
/// `Value::Undef` (kernel absent → `bounding_box` unresolvable) and emit advisory
/// `E_DFM_BUILD_VOLUME` diagnostics that do NOT constitute a test failure here.
#[test]
fn std_process_dfm_scalar_constraints_satisfied_violated() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    // Belt-and-suspenders compile guard (canonical check is `std_process_dfm_compiles_error_clean`).
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    let result = engine.check(&compiled);

    // Helper: collect constraint results whose label starts with `def_name`.
    // Relies on the label format "DefName#inst[pred]" from the Reify compiler.
    // `starts_with("Manufacturable")` matches ONLY the pure Manufacturable def
    // (BendManufacturable starts with "Bend", DrawManufacturable with "Draw", etc.).
    let entries_for = |def_name: &str| -> Vec<_> {
        result
            .constraint_results
            .iter()
            .filter(|e| {
                e.label
                    .as_deref()
                    .map(|l| l.starts_with(def_name))
                    .unwrap_or(false)
            })
            .collect()
    };

    // ── Manufacturable ────────────────────────────────────────────────────────
    {
        let mfg = entries_for("Manufacturable");
        assert!(
            !mfg.is_empty(),
            "expected Manufacturable constraint results; got none (check example file)"
        );
        assert!(
            mfg.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one Manufacturable Satisfied; got:\n{:#?}",
            mfg.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            mfg.iter().any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one Manufacturable Violated; got:\n{:#?}",
            mfg.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── BendManufacturable ────────────────────────────────────────────────────
    {
        let bend = entries_for("BendManufacturable");
        assert!(
            !bend.is_empty(),
            "expected BendManufacturable constraint results; got none"
        );
        assert!(
            bend.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one BendManufacturable Satisfied; got:\n{:#?}",
            bend.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            bend.iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one BendManufacturable Violated; got:\n{:#?}",
            bend.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── DrawManufacturable ────────────────────────────────────────────────────
    {
        let draw = entries_for("DrawManufacturable");
        assert!(
            !draw.is_empty(),
            "expected DrawManufacturable constraint results; got none"
        );
        assert!(
            draw.iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DrawManufacturable Satisfied; got:\n{:#?}",
            draw.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draw.iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one DrawManufacturable Violated; got:\n{:#?}",
            draw.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }

    // ── DraftManufacturable ───────────────────────────────────────────────────
    {
        let draft = entries_for("DraftManufacturable");
        assert!(
            !draft.is_empty(),
            "expected DraftManufacturable constraint results; got none"
        );
        assert!(
            draft
                .iter()
                .any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DraftManufacturable Satisfied; got:\n{:#?}",
            draft
                .iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draft
                .iter()
                .any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one DraftManufacturable Violated; got:\n{:#?}",
            draft
                .iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }
}

// ── (b) OCCT-GATED: build-volume flip + severity bridge ──────────────────────

/// Helper: find the first constraint entry in `results` matching `entity` +
/// label starting with `label_prefix`.
fn find_fvb_entry(
    results: &[ConstraintCheckEntry],
    entity: &str,
    label_prefix: &str,
) -> Option<ConstraintCheckEntry> {
    results
        .iter()
        .find(|e| {
            e.id.entity == entity
                && e.label
                    .as_deref()
                    .map(|l| l.starts_with(label_prefix))
                    .unwrap_or(false)
        })
        .cloned()
}

/// ALWAYS-ON presence guard: the two `FitsBuildVolume` constraint entries
/// (`FittingPart`, `OversizedPart`) appear in the no-kernel `check()` result as
/// `Satisfaction::Indeterminate`. This proves the example declares the
/// build-volume constraints without requiring a geometry kernel on the CI host.
///
/// The geometry-backed OK→VIOLATED *flip* and the severity-tagged diagnostics
/// are exercised by `std_process_dfm_build_volume_flip_and_severity_diagnostics`
/// (deferred — see its `#[ignore]`).
#[test]
fn std_process_dfm_build_volume_constraints_declared() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    let mut no_kernel_engine = make_simple_engine();
    let no_kernel_result = no_kernel_engine.check(&compiled);

    assert!(
        find_fvb_entry(
            &no_kernel_result.constraint_results,
            "FittingPart",
            "FitsBuildVolume"
        )
        .is_some(),
        "expected FittingPart FitsBuildVolume constraint entry in no-kernel result; \
         check the build-volume section of examples/process/std_process_dfm.ri; got:\n{:#?}",
        no_kernel_result.constraint_results
    );
    assert!(
        find_fvb_entry(
            &no_kernel_result.constraint_results,
            "OversizedPart",
            "FitsBuildVolume"
        )
        .is_some(),
        "expected OversizedPart FitsBuildVolume constraint entry in no-kernel result; \
         got:\n{:#?}",
        no_kernel_result.constraint_results
    );
}

/// DEFERRED (build-DAG cutover): the geometry-backed `FitsBuildVolume` OK→VIOLATED
/// flip and the severity-tagged diagnostics.
///
/// Under the DEFAULT (`LegacyMultiPass`) `Engine::build` scheduler the
/// `FitsBuildVolume` predicate `fits_build_volume(bounding_box(part), …)` is
/// evaluated in the pre-geometry constraint pass — `bounding_box(part)` is still
/// `Undef`, so the constraint folds to `Indeterminate` and never flips, even with
/// OCCT present. The post-geometry constraint re-check that flips it to a definite
/// `Satisfied`/`Violated` is the UnifiedDag executor path (ε task #4358, on main),
/// which is NOT the default: the `unified-dag` cargo feature only makes it
/// *selectable* via `REIFY_BUILD_SCHEDULER`; it becomes the default at the
/// human-gated cutover (task ι #4362).
///
/// This test is therefore `#[ignore]`d until that cutover. The canonical flip
/// proof under the unified scheduler is owned by `dfm_fits_build_volume_4275_e2e`
/// (task η #4360). When the default flips (#4362), drop the `#[ignore]`.
///
/// When run (`cargo test -- --ignored`, under the unified scheduler, with OCCT)
/// it asserts:
/// - `FittingPart`'s `FitsBuildVolume` → `Satisfaction::Satisfied`
///   (100×100×150 mm part fits the 220×220×250 mm FDM build envelope).
/// - `OversizedPart`'s `FitsBuildVolume` → `Satisfaction::Violated`
///   (250 mm on X exceeds the 220 mm envelope).
/// - `result.diagnostics` contains a `Severity::Warning` with `"W_DFM_BUILD_VOLUME"`
///   (default 2-arg `FitsBuildVolume` violation from `OversizedPart`).
/// - `result.diagnostics` contains a `Severity::Error` with `"E_DFM_BUILD_VOLUME"`
///   (3-arg `DFMSeverity.Error` direct call in `DFMSeverityBridge`).
/// - `result.diagnostics` contains a `Severity::Info` with `"I_DFM_BUILD_VOLUME"`
///   (3-arg `DFMSeverity.Info` direct call in `DFMSeverityBridge`).
#[test]
#[ignore = "blocked on #4362 — build-volume flip needs the UnifiedDag post-geometry constraint re-check, default-off until the build-DAG cutover; canonical flip proof owned by #4360"]
fn std_process_dfm_build_volume_flip_and_severity_diagnostics() {
    let source = read_dfm_example();
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile check failed:\n{:#?}",
        errors_only(&compiled)
    );

    // OCCT-GATED: build-volume flip and severity diagnostics.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping OCCT-gated FitsBuildVolume assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // FittingPart: 100×100×150 mm fits the 220×220×250 mm FDM build envelope.
    let fitting = find_fvb_entry(&result.constraint_results, "FittingPart", "FitsBuildVolume")
        .unwrap_or_else(|| {
            panic!(
                "FittingPart FitsBuildVolume absent from build result; got:\n{:#?}",
                result.constraint_results
            )
        });
    assert_eq!(
        fitting.satisfaction,
        Satisfaction::Satisfied,
        "FittingPart (100×100×150 mm) should fit the 220×220×250 mm envelope; got {:?}",
        fitting.satisfaction
    );

    // OversizedPart: 250 mm on X exceeds the 220 mm envelope.
    let oversized = find_fvb_entry(
        &result.constraint_results,
        "OversizedPart",
        "FitsBuildVolume",
    )
    .unwrap_or_else(|| {
        panic!(
            "OversizedPart FitsBuildVolume absent from build result; got:\n{:#?}",
            result.constraint_results
        )
    });
    assert_eq!(
        oversized.satisfaction,
        Satisfaction::Violated,
        "OversizedPart (250×100×100 mm) should exceed the 220 mm X-axis envelope; got {:?}",
        oversized.satisfaction
    );

    // W_DFM_BUILD_VOLUME — from OversizedPart's 2-arg FitsBuildVolume (default Warning).
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.message.contains("W_DFM_BUILD_VOLUME")),
        "expected a Warning diagnostic containing 'W_DFM_BUILD_VOLUME' \
         (from OversizedPart FitsBuildVolume); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // E_DFM_BUILD_VOLUME — from DFMSeverityBridge DFMSeverity.Error direct call.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("E_DFM_BUILD_VOLUME")),
        "expected an Error diagnostic containing 'E_DFM_BUILD_VOLUME' \
         (from DFMSeverityBridge DFMSeverity.Error); diagnostics:\n{:#?}",
        result.diagnostics
    );

    // I_DFM_BUILD_VOLUME — from DFMSeverityBridge DFMSeverity.Info direct call.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Info && d.message.contains("I_DFM_BUILD_VOLUME")),
        "expected an Info diagnostic containing 'I_DFM_BUILD_VOLUME' \
         (from DFMSeverityBridge DFMSeverity.Info); diagnostics:\n{:#?}",
        result.diagnostics
    );
}
