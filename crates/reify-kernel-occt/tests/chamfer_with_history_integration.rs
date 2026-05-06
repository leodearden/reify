//! Integration test for `OcctKernelHandle::chamfer_with_history` —
//! the v0.2 persistent-naming-v2 local-feature history-tracking primitive
//! for `BRepFilletAPI_MakeChamfer` (task 2655, step-5/step-6).
//!
//! Exercises the FFI primitive that wraps `BRepFilletAPI_MakeChamfer::Modified()`,
//! `Generated()`, and `IsDeleted()` and exposes the per-parent face/edge
//! correspondence for face and edge topology.
//!
//! Mirrors the structure of `fillet_with_history_integration.rs` (and
//! `boolean_op_history_integration.rs`): gated on `OCCT_AVAILABLE` and
//! `#![cfg(has_occt)]` so non-OCCT builds skip without linker errors.

#![cfg(has_occt)]

mod common;

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

/// Chamfer distance: 1 mm. Small enough that every edge gets a chamfer face
/// without geometric collapse on a 10mm cube.
const CHAMFER_DISTANCE_M: f64 = 1.0e-3;

/// `BRepFilletAPI_MakeChamfer` history exposes Modified/Generated/Deleted for
/// each parent face and edge. Full assertion spec is in
/// `common::run_local_feature_reports_face_records`; see that function's
/// doc-comment for the block-by-block description.
///
/// Compilation/linkage of this test pins step-6: it would fail to build
/// until the FFI primitive + Rust handle method ship (already done in step-2).
#[test]
fn chamfer_with_history_reports_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    common::run_local_feature_reports_face_records(
        &kernel,
        CHAMFER_DISTANCE_M,
        |id, d| kernel.chamfer_with_history(id, d),
        "chamfer",
    );
}

/// `chamfer_with_history` must reject non-`BRepKind::Solid` input handles with
/// a descriptive `OperationFailed` error mentioning "Solid" or "BRepKind".
///
/// Rationale: `BRepFilletAPI_MakeChamfer` iterates parent edges of a Solid;
/// passing a Face or Edge would either crash inside OCCT or silently produce a
/// misclassified result (the output is always stored as `BRepKind::Solid`).
/// The up-front kind guard added in task 2821 step-4 makes this rejection
/// explicit and message-checked (esc-2655-26 issue #4).
///
/// Exercises both `BRepKind::Face` and `BRepKind::Edge` to protect against a
/// future refactor that whitelists one non-Solid kind (esc-2655-26 suggestion #5 /
/// task 2821 amendment).
#[test]
fn chamfer_with_history_rejects_non_solid_input() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    common::run_local_feature_rejects_non_solid_input(
        &kernel,
        CHAMFER_DISTANCE_M,
        |id, d| kernel.chamfer_with_history(id, d),
        "chamfer_with_history",
    );
}
