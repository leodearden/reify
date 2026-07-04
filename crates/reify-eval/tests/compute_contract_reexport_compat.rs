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

// ── step-5: ElasticResult / ShellChannels FEA-result cluster ────────────
// The orphan rule forces this island (ElasticResult, ShellChannels, the
// PersistentlyCacheable impl, the PartialElasticResult drift-guard `From`
// impls, and the shared f64-slab codec) to move together; see
// `.task/plan.json` step-6 and the design_decisions entry on the shared
// codec. This cluster is fed via the `reify_eval::persistent_cache::`
// re-export path, since that is where these types currently live.

fn _elastic_result_identity(_: reify_compute_contract::ElasticResult) {}
fn _shell_channels_identity(_: reify_compute_contract::ShellChannels) {}

fn make_shell_channels() -> reify_eval::persistent_cache::ShellChannels {
    // Bit-scrambled payload (same idiom as the existing persistent_cache.rs
    // shell-channels round-trip test) so every byte of every f64 is
    // non-trivial; a native-byte or wrong-len regression would surface here.
    let top: Vec<f64> = (0..9u64)
        .map(|i| f64::from_bits(i.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xABAD_1DEA_0BAD_F00D))
        .collect();
    let bottom: Vec<f64> = (0..9u64)
        .map(|i| f64::from_bits(i.wrapping_mul(0x6C62_272E_07BB_0142) ^ 0xC0DE_FACE_DEAD_C0DE))
        .collect();
    let frame: Vec<f64> = (0..9u64)
        .map(|i| f64::from_bits(i.wrapping_mul(0xD737_E5B5_2727_2727) ^ 0x1234_5678_9ABC_DEF0))
        .collect();
    reify_eval::persistent_cache::ShellChannels { top, bottom, frame }
}

/// Populates every field, including the v3 grid/derivative channels and a
/// `Some(ShellChannels)` tail, so the round-trip test below exercises the
/// full moved encode/decode path in one pass.
fn make_elastic_result_with_shell_and_grid() -> reify_eval::persistent_cache::ElasticResult {
    reify_eval::persistent_cache::ElasticResult {
        displacement: vec![1.0, -2.5, std::f64::consts::PI, 0.0, 1e-9, 4.25],
        stress: vec![100e6, -50e6, 0.0, 250e6, 75e6, -125e6],
        max_von_mises: 250e6,
        converged: true,
        iterations: 17,
        solve_time_ms: 4321,
        shell_channels: Some(make_shell_channels()),
        grid_bounds_min: [0.0, -1.0, 0.5],
        grid_bounds_max: [1.0, 0.3, 0.1],
        grid_counts: [2, 3, 4],
        divergence: vec![1e-5, 2e-5, 3e-5],
        gradient: (0..9u64).map(|i| i as f64 * 1e-3).collect(),
        curl: vec![0.1, 0.2, 0.3],
        aposteriori: None,
    }
}

#[test]
fn elastic_result_and_shell_channels_reexport_is_identity_not_duplicate() {
    let er = make_elastic_result_with_shell_and_grid();
    _shell_channels_identity(er.shell_channels.clone().unwrap());
    _elastic_result_identity(er);
}

/// Casts both paths to the same `fn(&[f64]) -> f64` pointer type and compares
/// addresses — this only holds if `reify_eval::persistent_cache::
/// max_deflection_magnitude` is a re-export of the compute-contract item, not
/// a look-alike duplicate definition compiled from a copy-pasted body.
#[test]
fn max_deflection_magnitude_reexport_is_identity_not_duplicate() {
    let contract_fn: fn(&[f64]) -> f64 = reify_compute_contract::max_deflection_magnitude;
    let eval_fn: fn(&[f64]) -> f64 = reify_eval::persistent_cache::max_deflection_magnitude;
    assert_eq!(
        contract_fn as usize, eval_fn as usize,
        "reify_eval::persistent_cache::max_deflection_magnitude must be the SAME fn item as \
         reify_compute_contract::max_deflection_magnitude, not a duplicate definition"
    );
}

#[test]
fn elastic_result_with_shell_channels_and_grid_round_trips_byte_exact_and_reserializes_identically()
 {
    use reify_eval::persistent_cache::{ElasticResult, PersistentlyCacheable};

    let original = make_elastic_result_with_shell_and_grid();
    let mut bytes_a: Vec<u8> = Vec::new();
    original.serialize_to_writer(&mut bytes_a).unwrap();

    let decoded = ElasticResult::deserialize_from_reader(&mut &bytes_a[..]).unwrap();
    assert_eq!(
        decoded, original,
        "round-tripped value must equal the original (PartialEq)"
    );

    let mut bytes_b: Vec<u8> = Vec::new();
    decoded.serialize_to_writer(&mut bytes_b).unwrap();
    assert_eq!(
        bytes_a, bytes_b,
        "re-serializing a decoded value must be byte-identical"
    );
}

#[test]
fn elastic_result_from_partial_by_ref_and_by_value_use_documented_neutral_defaults() {
    use reify_solver_elastic::progressive::PartialElasticResult;

    let partial = PartialElasticResult {
        displacement: vec![1.0, 2.0, 3.0],
        stress: vec![4.0, 5.0],
        max_von_mises: 123.5,
        converged: true,
        iterations: 7,
    };

    let by_ref: reify_eval::persistent_cache::ElasticResult = (&partial).into();
    assert_eq!(by_ref.displacement, vec![1.0, 2.0, 3.0]);
    assert_eq!(by_ref.stress, vec![4.0, 5.0]);
    assert_eq!(by_ref.max_von_mises, 123.5);
    assert!(by_ref.converged);
    assert_eq!(by_ref.iterations, 7);
    assert_eq!(
        by_ref.solve_time_ms, 0,
        "solve_time_ms must default to 0 for a partial snapshot"
    );
    assert!(
        by_ref.shell_channels.is_none(),
        "shell_channels must default to None for tet-only solver"
    );

    // By-value variant moves displacement/stress instead of cloning them.
    let by_value: reify_eval::persistent_cache::ElasticResult = partial.into();
    assert_eq!(by_value.displacement, vec![1.0, 2.0, 3.0]);
    assert_eq!(by_value.stress, vec![4.0, 5.0]);
    assert_eq!(by_value.solve_time_ms, 0);
    assert!(by_value.shell_channels.is_none());
}

#[test]
fn max_deflection_on_known_stride_3_buffer_matches_hand_computed_l2_norms() {
    // Node 0: (3,4,0) -> L2 norm 5.0. Node 1: (1,1,1) -> L2 norm sqrt(3) < 5.0.
    let displacement = vec![3.0, 4.0, 0.0, 1.0, 1.0, 1.0];
    let expected = 5.0_f64;
    assert_eq!(
        reify_compute_contract::max_deflection_magnitude(&displacement),
        expected
    );

    let er = reify_eval::persistent_cache::ElasticResult {
        displacement: displacement.clone(),
        stress: vec![],
        max_von_mises: 0.0,
        converged: true,
        iterations: 0,
        solve_time_ms: 0,
        shell_channels: None,
        grid_bounds_min: [0.0; 3],
        grid_bounds_max: [0.0; 3],
        grid_counts: [0; 3],
        divergence: Vec::new(),
        gradient: Vec::new(),
        curl: Vec::new(),
        aposteriori: None,
    };
    assert_eq!(
        er.max_deflection(),
        expected,
        "ElasticResult::max_deflection must delegate to max_deflection_magnitude"
    );
}
