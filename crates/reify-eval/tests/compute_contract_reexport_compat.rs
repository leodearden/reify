//! Cross-crate re-export compatibility pins for task A (#4934): reify-eval
//! extracts the pure `ComputeFn` contract value types into the OCCT-free
//! `reify-compute-contract` foundation crate and re-exports every moved type
//! at its original public paths (INV-2 / BT-3).
//!
//! Each cluster below adds an IDENTITY assertion — a fn typed on
//! `reify_compute_contract::X` invoked with a value built via the
//! `reify_eval::` re-export path — which only compiles if the re-export is
//! the SAME type, not an accidental duplicate definition. A mere
//! `use`/existence check would miss that failure mode.
//!
//! This file grows with each extraction step; see `.task/plan.json` steps
//! 1/3/5. The pre-existing `tests/compute_dispatch_registry.rs::_seam_pin_api_surface`
//! is the INV-2 regression guard for the compute-dispatch cluster and must
//! stay green, unchanged, throughout this extraction.

// ── step-1: CancellationHandle ───────────────────────────────────────────

/// Identity seam: a fn parameter typed on
/// `reify_compute_contract::CancellationHandle` accepts a value constructed
/// via `reify_eval::CancellationHandle::new()`. This only compiles if the
/// `reify_eval` re-export is the SAME type as the compute-contract
/// definition, not a duplicate.
fn _cc_identity(_: reify_compute_contract::CancellationHandle) {}

#[test]
fn cancellation_handle_reexport_is_identity_not_duplicate() {
    _cc_identity(reify_eval::CancellationHandle::new());
}

#[test]
fn cancellation_handle_new_is_not_cancelled() {
    let h = reify_eval::CancellationHandle::new();
    assert!(!h.is_cancelled(), "a fresh handle must not be cancelled");
}

#[test]
fn cancellation_handle_cancel_transitions_false_to_true() {
    let h = reify_eval::CancellationHandle::new();
    assert!(!h.is_cancelled(), "must start false");
    h.cancel();
    assert!(
        h.is_cancelled(),
        "is_cancelled() must be true after cancel()"
    );
}

#[test]
fn cancellation_handle_clone_shares_flag() {
    let h = reify_eval::CancellationHandle::new();
    let clone = h.clone();
    clone.cancel();
    assert!(
        h.is_cancelled(),
        "cancelling a clone must be observed by the original handle \
         (they share the same underlying Arc<AtomicBool>)"
    );
}

// ── step-3: compute-dispatch cluster ─────────────────────────────────────
// ComputeFn, ComputeOutcome, DispatchError, RealizedContent,
// RealizationReadHandle, StructuredComputeDetail.

fn _compute_fn_identity(_: reify_compute_contract::ComputeFn) {}
fn _compute_outcome_identity(_: reify_compute_contract::ComputeOutcome) {}
fn _dispatch_error_identity(_: reify_compute_contract::DispatchError) {}
fn _realized_content_identity(_: reify_compute_contract::RealizedContent) {}
fn _realization_read_handle_identity(_: reify_compute_contract::RealizationReadHandle) {}
fn _structured_compute_detail_identity(_: reify_compute_contract::StructuredComputeDetail) {}

/// A concrete trampoline written entirely in terms of `reify_eval::` re-export
/// paths. Coercing it to `reify_compute_contract::ComputeFn` below only
/// type-checks if every parameter/return type in the signature is the SAME
/// type as compute-contract's own definition, not a look-alike duplicate.
fn identity_trampoline(
    value_inputs: &[reify_ir::Value],
    _realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &reify_ir::Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    reify_eval::ComputeOutcome::Completed {
        result: value_inputs.first().cloned().unwrap_or(reify_ir::Value::Undef),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

#[test]
fn compute_fn_reexport_is_identity_not_duplicate() {
    let f: reify_compute_contract::ComputeFn = identity_trampoline;
    _compute_fn_identity(f);
}

#[test]
fn compute_outcome_variants_reexport_is_identity_not_duplicate() {
    _compute_outcome_identity(reify_eval::ComputeOutcome::Completed {
        result: reify_ir::Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    });
    _compute_outcome_identity(reify_eval::ComputeOutcome::Cancelled);
    _compute_outcome_identity(reify_eval::ComputeOutcome::Failed {
        diagnostics: vec![],
        structured_detail: vec![],
    });
}

#[test]
fn dispatch_error_variants_reexport_is_identity_not_duplicate() {
    _dispatch_error_identity(reify_eval::DispatchError::Cancelled);
    _dispatch_error_identity(reify_eval::DispatchError::Failed(vec![], vec![]));
}

#[test]
fn structured_compute_detail_fea_reexport_is_identity_not_duplicate() {
    let detail = reify_eval::StructuredComputeDetail::Fea(
        reify_solver_elastic::FeaDiagnosticDetail::Unconstrained {
            rigid_body_modes: vec![reify_solver_elastic::DofDirection::TranslationX],
        },
    );
    _structured_compute_detail_identity(detail);
}

fn make_sdf() -> reify_ir::SampledField {
    reify_ir::SampledField {
        name: "test".to_string(),
        kind: reify_ir::SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![1.0],
        spacing: vec![1.0],
        axis_grids: vec![vec![0.0, 1.0]],
        interpolation: reify_ir::InterpolationKind::Linear,
        data: vec![0.0, 1.0],
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

fn make_surface_mesh() -> reify_ir::Mesh {
    reify_ir::Mesh {
        vertices: vec![],
        indices: vec![],
        normals: None,
    }
}

fn make_volume_mesh(boundary: Option<reify_ir::BoundaryAssociation>) -> reify_ir::VolumeMesh {
    reify_ir::VolumeMesh {
        vertices: vec![],
        tet_indices: vec![],
        element_order: reify_ir::ElementOrderTag::P1,
        normals: None,
        boundary,
    }
}

#[test]
fn realized_content_sdf_reexport_is_identity_and_handle_accessors_are_honest() {
    let content = reify_eval::RealizedContent::Sdf(std::sync::Arc::new(make_sdf()));
    _realized_content_identity(content.clone());

    let h = reify_eval::RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("f", 0),
        reify_core::ContentHash(3),
        Some(content),
    );
    _realization_read_handle_identity(h.clone());
    assert!(h.sdf().is_some(), "sdf() must be Some for Sdf content");
    assert!(h.surface_mesh().is_none());
    assert!(h.volume_mesh().is_none());
    assert!(h.boundary().is_none(), "Sdf content has no boundary");
}

#[test]
fn realized_content_surface_mesh_reexport_is_identity_and_handle_accessors_are_honest() {
    let content = reify_eval::RealizedContent::SurfaceMesh(std::sync::Arc::new(make_surface_mesh()));
    _realized_content_identity(content.clone());

    let h = reify_eval::RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("s", 0),
        reify_core::ContentHash(2),
        Some(content),
    );
    assert!(h.surface_mesh().is_some());
    assert!(h.sdf().is_none());
    assert!(h.volume_mesh().is_none());
    assert!(h.boundary().is_none(), "SurfaceMesh content has no boundary");
}

#[test]
fn realized_content_volume_mesh_reexport_is_identity_and_handle_accessors_are_honest() {
    let content = reify_eval::RealizedContent::VolumeMesh(std::sync::Arc::new(make_volume_mesh(None)));
    _realized_content_identity(content.clone());

    let h = reify_eval::RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("v", 0),
        reify_core::ContentHash(1),
        Some(content),
    );
    assert!(h.volume_mesh().is_some());
    assert!(h.sdf().is_none());
    assert!(h.surface_mesh().is_none());
    assert!(h.boundary().is_none(), "VolumeMesh with boundary=None");
}

#[test]
fn realization_read_handle_boundary_accessor_surfaces_threaded_association() {
    let mut b = reify_ir::BoundaryAssociation::default();
    b.associate(
        0,
        reify_ir::NodeAttachment::OnFace(reify_ir::GeometryHandleId(7)),
    );

    let h = reify_eval::RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("b", 0),
        reify_core::ContentHash(10),
        Some(reify_eval::RealizedContent::VolumeMesh(std::sync::Arc::new(
            make_volume_mesh(Some(b.clone())),
        ))),
    );
    assert_eq!(
        h.boundary(),
        Some(&b),
        "boundary() must return the threaded BoundaryAssociation by reference"
    );

    let empty = reify_eval::RealizationReadHandle::new(
        reify_core::RealizationNodeId::new("b", 1),
        reify_core::ContentHash(0),
        None,
    );
    assert!(empty.content().is_none());
    assert!(empty.boundary().is_none());
}
