//! Unified-only OCCT acceptance e2e for DFM build-volume fit (η; closes esc-4275-38).
//!
//! Under `BuildScheduler::UnifiedDag` with a real OCCT kernel, the `let proc = FdmPrinter()`
//! cross-let form must resolve the `FitsBuildVolume` constraint to a **DEFINITE** verdict
//! (never `Indeterminate`) that **flips** with the actual OCCT bounding boxes:
//!
//! - `part = box(50mm, 50mm, 50mm)` fits inside `FdmPrinter.build_volume = box(200mm, 200mm, 200mm)` →
//!   `Satisfaction::Satisfied`
//! - `part = box(300mm, 300mm, 300mm)` exceeds the build volume on every axis →
//!   `Satisfaction::Violated`
//!
//! Both must be **DEFINITE** (`!= Indeterminate`), proving that ε's cross-let
//! `bounding_box(proc.build_volume)` fold reaches a real OCCT verdict under unified.
//!
//! **Gate:** `#[cfg_attr(not(feature = "unified-dag"), ignore)]` — the cross-let fold is
//! unified-only; the legacy default stays `Indeterminate`. Run with:
//! `cargo test -p reify-eval --features unified-dag dfm_fits_build_volume_4275_e2e`

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{BuildResult, BuildScheduler, Engine};
use reify_ir::{ExportFormat, Satisfaction};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

#[test]
#[cfg_attr(not(feature = "unified-dag"), ignore)]
fn dfm_fits_build_volume_4275_e2e() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping dfm_fits_build_volume_4275_e2e: OCCT not available");
        return;
    }

    // FdmPrinter MUST be declared before Part (declaration order is topological
    // for the cross-let snapshot seed — mirrors ε's unified_dag_cross_sub_build_volume_constraint_is_definite).
    let source_fits = make_source("50mm, 50mm, 50mm");
    let source_exceeds = make_source("300mm, 300mm, 300mm");

    // (a) FITS: box(50, 50, 50) is entirely inside box(200, 200, 200) → Satisfied.
    let result_fits = build_occt_unified(&source_fits);
    let sat_fits = fits_build_volume_satisfaction(&result_fits);
    assert_ne!(
        sat_fits,
        Satisfaction::Indeterminate,
        "FITS build must be DEFINITE (not Indeterminate); constraint_results={:?}",
        result_fits.constraint_results
    );
    assert_eq!(
        sat_fits,
        Satisfaction::Satisfied,
        "box(50,50,50) fits inside FdmPrinter build_volume box(200,200,200) → Satisfied; \
         got {:?}; constraint_results={:?}",
        sat_fits,
        result_fits.constraint_results
    );

    // (b) EXCEEDS: box(300, 300, 300) exceeds the build volume on every axis → Violated.
    let result_exceeds = build_occt_unified(&source_exceeds);
    let sat_exceeds = fits_build_volume_satisfaction(&result_exceeds);
    assert_ne!(
        sat_exceeds,
        Satisfaction::Indeterminate,
        "EXCEEDS build must be DEFINITE (not Indeterminate); constraint_results={:?}",
        result_exceeds.constraint_results
    );
    assert_eq!(
        sat_exceeds,
        Satisfaction::Violated,
        "box(300,300,300) exceeds FdmPrinter build_volume box(200,200,200) → Violated; \
         got {:?}; constraint_results={:?}",
        sat_exceeds,
        result_exceeds.constraint_results
    );
}

/// Build `source` template through stdlib + `UnifiedDag` + real OCCT kernel.
///
/// Compiles with the stdlib prelude (so `import std.process` / `FitsBuildVolume` /
/// `Adding` resolve), asserts no error-severity diagnostics, then runs on a fresh
/// `Engine` with `set_build_scheduler(UnifiedDag)` and returns the full `BuildResult`.
fn build_occt_unified(source: &str) -> BuildResult {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(reify_kernel_occt::OcctKernelHandle::spawn())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    engine.build(&compiled, ExportFormat::Step)
}

/// Locate the single `FitsBuildVolume` constraint result and return its satisfaction.
///
/// Matches on the `.label` field containing `"FitsBuildVolume"` (the stdlib def's
/// instantiation is labelled `"FitsBuildVolume#0[0]"`). Mirrors
/// `fits_build_volume_satisfaction` in `unified_dag_geometry_executors.rs:728`.
/// Panics with the full constraint list if no such entry is present.
fn fits_build_volume_satisfaction(result: &BuildResult) -> Satisfaction {
    result
        .constraint_results
        .iter()
        .find(|e| {
            e.label
                .as_deref()
                .is_some_and(|l| l.contains("FitsBuildVolume"))
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a FitsBuildVolume constraint result, got: {:?}",
                result.constraint_results
            )
        })
        .satisfaction
}

/// Build the shared FdmPrinter+Part source with `part_dims` substituted.
fn make_source(part_dims: &str) -> String {
    format!(
        r#"
import std.process

structure def FdmPrinter : Adding {{
    param duration           : Time   = 60min
    param cost               : Money  = 10USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 45deg
}}

structure Part {{
    let proc = FdmPrinter()
    let part = box({part_dims})
    constraint FitsBuildVolume(proc: proc, part: part)
}}
"#
    )
}
