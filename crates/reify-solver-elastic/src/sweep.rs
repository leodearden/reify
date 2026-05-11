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
