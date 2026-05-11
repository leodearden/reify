//! Sweep step: 2D mesh × K layers → 3D wedge/hex connectivity.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #7.
//!
//! This module turns a 2D cross-section mesh (produced by task 2987's
//! [`crate::mesh_swept_profile_2d`] → [`crate::Mesh2d`]) into a 3D
//! wedge/hex mesh by replicating the 2D node grid across K+1 layers and
//! emitting one element per (face, layer) pair.
//!
//! # Canonical element node orderings
//!
//! - **Wedge6 (PRI6):** `[b0, b1, b2, t0, t1, t2]` — bottom-face CCW,
//!   then top-face in the same cyclic order. Matches
//!   `elements::wedge_p1::WedgeP1` node numbering.
//! - **Hex8:** `[b0, b1, b2, b3, t0, t1, t2, t3]` — bottom-face CCW,
//!   then top-face in the same cyclic order. Matches
//!   `elements::hex_p1::HexP1` node numbering.
//!
//! Both orderings produce det J > 0 when the 2D mesher emits CCW faces
//! (as Gmsh does by convention).
//!
//! # Node layout
//!
//! Node `(layer ℓ, base i)` lives at global flat index `ℓ * n_base + i`.
//! Layer ℓ=0 is the "bottom" (origin) plane; layer ℓ=K is the "top" plane.

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parameters that describe the sweep trajectory.
///
/// All coordinates are in the same profile-local frame as the input [`crate::Mesh2d`].
/// The 2D mesh's `[x, y]` plane embeds at z=0; sweep directions are relative
/// to that frame.
#[derive(Debug, Clone)]
pub enum SweepParams {
    /// Straight extrusion along a constant axis direction.
    ///
    /// `axis` need not be unit-length — it is normalised internally. `length`
    /// is the total extrusion distance along `axis`.
    Extrude {
        /// Direction of extrusion (any non-zero vector).
        axis: [f64; 3],
        /// Total extrusion distance (must be > 0 and finite).
        length: f64,
    },
    /// Rotation of the profile around a line in 3D.
    ///
    /// The axis line passes through `axis_origin` in the direction `axis_dir`.
    /// `angle` is the total rotation in radians (must be > 0 and finite).
    Revolve {
        /// A point on the rotation axis.
        axis_origin: [f64; 3],
        /// Direction of the rotation axis (any non-zero vector).
        axis_dir: [f64; 3],
        /// Total rotation angle in radians (must be > 0 and finite).
        angle: f64,
    },
    /// Single-profile straight-path loft (Phase A semantics = Extrude).
    ///
    /// PRD Phase A restricts `SweepLinear` to `LineSegment`-pathed sweeps,
    /// which are geometrically identical to [`SweepParams::Extrude`]. The
    /// variant is kept distinct to preserve diagnostic-routing contracts
    /// (PRD task #11 emits different fallback messages per variant).
    SweepLinear {
        /// Direction of travel (any non-zero vector).
        axis: [f64; 3],
        /// Total path length (must be > 0 and finite).
        length: f64,
    },
}

/// 3D wedge/hex mesh produced by [`sweep_2d_mesh_to_3d`].
///
/// `vertices` is a flat `[x0,y0,z0, x1,y1,z1, …]` buffer (stride 3, `f32`).
/// `connectivity` carries the element index buffer — Wedge or Hex depending
/// on the 2D input element shape.
#[derive(Debug, Clone)]
pub struct SweptMesh3d {
    /// Flat 3D vertex buffer `[x,y,z, …]`, stride 3, in `f32`.
    pub vertices: Vec<f32>,
    /// Element connectivity — Wedge or Hex depending on input mesh shape.
    pub connectivity: SweptConnectivity,
    /// Number of element layers (K). The vertex buffer has `(K+1) * n_base`
    /// nodes; the connectivity has `K * n_faces` elements.
    pub layers: usize,
}

/// Element connectivity for a swept 3D mesh.
///
/// Index ordering follows the canonical PRI6 / hex8 orderings documented in
/// `elements/wedge_p1.rs` and `elements/hex_p1.rs`: bottom face first (CCW),
/// then top face in the same cyclic order.
#[derive(Debug, Clone)]
pub enum SweptConnectivity {
    /// Wedge (PRI6) connectivity.  `indices.len() % 6 == 0`.
    /// Each element: `[b0, b1, b2, t0, t1, t2]`.
    Wedge { indices: Vec<u32> },
    /// Hex8 connectivity.  `indices.len() % 8 == 0`.
    /// Each element: `[b0, b1, b2, b3, t0, t1, t2, t3]`.
    Hex { indices: Vec<u32> },
}

/// Errors returned by [`sweep_2d_mesh_to_3d`].
#[derive(Debug, Clone)]
pub enum SweepError {
    /// The input [`crate::Mesh2d`] has no vertices or no faces.
    EmptyMesh2d,
    /// `layers == 0` — a zero-layer sweep produces no elements.
    InvalidLayerCount,
    /// The sweep axis (or revolution axis direction) has Euclidean norm < 1e-12.
    DegenerateAxis,
    /// The sweep magnitude (length or angle) is zero, negative, or non-finite.
    DegenerateMagnitude,
}

/// Warning emitted when the swept mesh has fewer than `min_layers` elements
/// through the sweep direction.
///
/// Mirrors the struct shape of `reify_kernel_gmsh::through_thickness::ThroughThicknessSweepWarning`
/// but without the region-index / thickness fields (this is per-body, not
/// per-region, and layer count is known directly from input).
#[derive(Debug, Clone)]
pub struct ThroughThicknessSweepWarning {
    /// Actual number of element layers in the swept mesh.
    pub layer_count: usize,
    /// Minimum acceptable layers (typically 2).
    pub min_layers: usize,
    /// Human-readable diagnostic message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // step-1: surface compilation check — each line constructs one variant /
    // struct field.  A missing variant, renamed field, or wrong type fails
    // to compile before a single assertion is evaluated.

    #[test]
    fn sweep_params_has_required_variants() {
        let _extrude = SweepParams::Extrude {
            axis: [0.0_f64, 0.0, 1.0],
            length: 1.0_f64,
        };
        let _revolve = SweepParams::Revolve {
            axis_origin: [0.0_f64, 0.0, 0.0],
            axis_dir: [0.0_f64, 1.0, 0.0],
            angle: std::f64::consts::FRAC_PI_2,
        };
        let _linear = SweepParams::SweepLinear {
            axis: [0.0_f64, 0.0, 1.0],
            length: 2.0_f64,
        };
    }

    #[test]
    fn sweep_error_has_required_variants() {
        let _empty = SweepError::EmptyMesh2d;
        let _invalid = SweepError::InvalidLayerCount;
        let _axis = SweepError::DegenerateAxis;
        let _mag = SweepError::DegenerateMagnitude;
    }

    #[test]
    fn swept_connectivity_has_required_variants() {
        let _wedge = SweptConnectivity::Wedge {
            indices: vec![0_u32, 1, 2, 3, 4, 5],
        };
        let _hex = SweptConnectivity::Hex {
            indices: vec![0_u32, 1, 2, 3, 4, 5, 6, 7],
        };
    }

    #[test]
    fn swept_mesh3d_has_required_fields() {
        let mesh = SweptMesh3d {
            vertices: vec![0.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0],
            connectivity: SweptConnectivity::Wedge { indices: vec![0_u32] },
            layers: 1_usize,
        };
        assert_eq!(mesh.layers, 1);
        assert_eq!(mesh.vertices.len(), 6);
    }

    #[test]
    fn through_thickness_warning_has_required_fields() {
        let w = ThroughThicknessSweepWarning {
            layer_count: 1_usize,
            min_layers: 2_usize,
            message: "test warning".to_string(),
        };
        assert_eq!(w.layer_count, 1);
        assert_eq!(w.min_layers, 2);
        assert!(w.message.contains("test"));
    }
}
