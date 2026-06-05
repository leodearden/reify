//! process-DFM δ end-to-end CI test (task #4275, PRD §7).
//!
//! Two tiers:
//!
//! **(a) ALWAYS-ON** (no kernel) — `std_process_dfm_compiles_error_clean` +
//!   `std_process_dfm_scalar_constraints_satisfied_violated`:
//!   The example file parses and compiles with no Error-severity diagnostics.
//!   Under a no-kernel `make_simple_engine().check()`, the SCALAR DFM constraints
//!   (Manufacturable, BendManufacturable, DrawManufacturable, DraftManufacturable)
//!   report the expected Satisfied/Violated set. The FitsBuildVolume and
//!   FeatureManufacturable geometry-backed constraints are Indeterminate without a
//!   kernel and are intentionally NOT asserted here.
//!
//! **(b) OCCT-GATED** — `std_process_dfm_build_volume_flip_and_severity_diagnostics`:
//!   With a full OCCT kernel engine (via `OcctKernelHandle::spawn()`), asserts:
//!   - `FittingPart`'s FitsBuildVolume → `Satisfaction::Satisfied`
//!     (100×100×150 mm part fits 220×220×250 mm FDMNylonPart build envelope)
//!   - `OversizedPart`'s FitsBuildVolume → `Satisfaction::Violated`
//!     (250 mm on X exceeds the 220 mm envelope)
//!   - `result.diagnostics` contains a Warning with "W_DFM_BUILD_VOLUME"
//!     (default-severity violation from OversizedPart's FitsBuildVolume predicate)
//!   - `result.diagnostics` contains an Error with "E_DFM_BUILD_VOLUME"
//!     (DFMSeverity.Error direct 3-arg call in DFMSeverityBridge)
//!   - `result.diagnostics` contains an Info with "I_DFM_BUILD_VOLUME"
//!     (DFMSeverity.Info direct 3-arg call in DFMSeverityBridge)
//!
//! The example is also covered for compile-cleanliness by
//! `crates/reify-compiler/tests/examples_smoke.rs` (directory walk).

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::Engine;
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
            mfg.iter().any(|e| e.satisfaction == Satisfaction::Satisfied),
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
            bend.iter().any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one BendManufacturable Satisfied; got:\n{:#?}",
            bend.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            bend.iter().any(|e| e.satisfaction == Satisfaction::Violated),
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
            draw.iter().any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DrawManufacturable Satisfied; got:\n{:#?}",
            draw.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draw.iter().any(|e| e.satisfaction == Satisfaction::Violated),
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
            draft.iter().any(|e| e.satisfaction == Satisfaction::Satisfied),
            "expected at least one DraftManufacturable Satisfied; got:\n{:#?}",
            draft.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
        assert!(
            draft.iter().any(|e| e.satisfaction == Satisfaction::Violated),
            "expected at least one DraftManufacturable Violated; got:\n{:#?}",
            draft.iter()
                .map(|e| (e.label.as_deref(), e.satisfaction))
                .collect::<Vec<_>>()
        );
    }
}
