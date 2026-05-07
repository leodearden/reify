//! Integration test for `OcctKernelHandle::fillet_with_history` —
//! the v0.2 persistent-naming-v2 local-feature history-tracking primitive
//! for `BRepFilletAPI_MakeFillet` (task 2655, step-1/step-2).
//!
//! Exercises the FFI primitive that wraps `BRepFilletAPI_MakeFillet::Modified()`,
//! `Generated()`, and `IsDeleted()` and exposes the per-parent face/edge
//! correspondence for face and edge topology.
//!
//! Mirrors the structure of `boolean_op_history_integration.rs`: gated on
//! `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip without
//! linker errors.

#![cfg(has_occt)]

mod common;

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

/// Fillet radius: 1 mm. Small enough that every edge gets a fillet face
/// without geometric collapse on a 10mm cube.
const FILLET_RADIUS_M: f64 = 1.0e-3;

/// `BRepFilletAPI_MakeFillet` history exposes Modified/Generated/Deleted for
/// each parent face and edge. Full assertion spec is in
/// `common::run_local_feature_reports_face_records`; see that function's
/// doc-comment for the block-by-block description.
///
/// Compilation/linkage of this test pins step-2: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn fillet_with_history_reports_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    common::run_local_feature_reports_face_records(
        &kernel,
        FILLET_RADIUS_M,
        |id, r| kernel.fillet_with_history(id, r),
        "fillet",
    );
}

/// `fillet_with_history` must reject non-`BRepKind::Solid` input handles with
/// a descriptive `OperationFailed` error mentioning "Solid" or "BRepKind".
///
/// Rationale: `BRepFilletAPI_MakeFillet` iterates parent edges of a Solid;
/// passing a Face or Edge would either crash inside OCCT or silently produce a
/// misclassified result (the output is always stored as `BRepKind::Solid`).
/// The up-front kind guard added in task 2821 step-4 makes this rejection
/// explicit and message-checked (esc-2655-26 issue #4).
///
/// Exercises both `BRepKind::Face` and `BRepKind::Edge` to protect against a
/// future refactor that whitelists one non-Solid kind (esc-2655-26 suggestion #5 /
/// task 2821 amendment).
#[test]
fn fillet_with_history_rejects_non_solid_input() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    common::run_local_feature_rejects_non_solid_input(
        &kernel,
        FILLET_RADIUS_M,
        |id, r| kernel.fillet_with_history(id, r),
        "fillet_with_history",
    );
}
