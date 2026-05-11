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
// Pure helpers
// ---------------------------------------------------------------------------

/// Derive the number of element layers from the sweep distance and element size.
///
/// Returns `round(sweep_distance / mesh_size).max(min_layers)`.
///
/// # Defensive handling
///
/// Any non-positive or non-finite value in `sweep_distance` or `mesh_size`
/// causes the function to return `min_layers` directly. This matches the
/// expected behaviour when called from PRD task #9's `ElasticOptions` wiring:
/// if `mesh_size` was unset (0 or negative) or the geometry produced a
/// degenerate distance, we fall through to the minimum.
///
/// # PRD contract
///
/// From `docs/prds/v0_3/hex-wedge-meshing.md` task #7:
/// `K = max(min_layers, round(sweep_distance / mesh_size))`.
/// Task #9 wires this via `ElasticOptions.mesh_size` and
/// `ElasticOptions.sweep_subdivisions`.
pub fn derive_layer_count(sweep_distance: f64, mesh_size: f64, min_layers: usize) -> usize {
    if sweep_distance.is_finite()
        && mesh_size.is_finite()
        && sweep_distance > 0.0
        && mesh_size > 0.0
    {
        let raw = (sweep_distance / mesh_size).round();
        raw.max(min_layers as f64) as usize
    } else {
        min_layers
    }
}

/// Check whether the swept mesh has enough elements through its thickness.
///
/// Returns `None` when `layers >= min_layers` (acceptable).  Returns
/// `Some(warning)` when too few layers were produced, with a human-readable
/// message that names the two knobs (`mesh_size`, `sweep_subdivisions`) a
/// caller can adjust.
///
/// The diagnostic vocabulary (`mesh_size`, `sweep_subdivisions`) is locked by
/// test assertions — preserve those exact substrings when editing the message.
///
/// Mirrors the pattern in `reify_kernel_gmsh::through_thickness::through_thickness_check`
/// but as a standalone one-liner (no bin-detection needed since K is an input).
pub fn check_sweep_through_thickness(
    layers: usize,
    min_layers: usize,
) -> Option<ThroughThicknessSweepWarning> {
    if layers >= min_layers {
        return None;
    }
    Some(ThroughThicknessSweepWarning {
        layer_count: layers,
        min_layers,
        message: format!(
            "swept body has only {layers} elements through the sweep direction; \
             expected at least {min_layers}. \
             Decrease mesh_size or set an explicit sweep_subdivisions.",
        ),
    })
}

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

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

const DEGENERATE_TOL: f64 = 1e-12;

/// Validate sweep inputs before any allocation.
///
/// Returns `Ok(unit_axis)` — the normalised axis vector — or a `SweepError`.
///
/// # Debug-only contract checks
///
/// In debug builds this function also enforces the `Mesh2d` buffer-layout invariants
/// documented in `crate::Mesh2d` (see `mesher.rs:46-61`):
///
/// - `vertices.len()` must be a multiple of 2 (stride-2 `[x, y, …]` buffer).
/// - `indices.len()` must be a multiple of 3 (Triangle) or 4 (Quad).
///
/// Violations trigger a `debug_assert!` panic — not a `SweepError` — because a
/// malformed `Mesh2d` is an upstream construction bug, not a runtime input error.
/// The asserts are elided in release builds (zero hot-path cost), consistent with
/// the pattern used in `shell_boundary.rs`, `tet.rs`, `progressive.rs`, etc.
fn validate_sweep_inputs(
    mesh2d: &crate::Mesh2d,
    params: &SweepParams,
    layers: usize,
) -> Result<[f64; 3], SweepError> {
    // 1. Empty mesh check (with debug-only stride invariant assertions per variant).
    let (verts_empty, faces_empty) = match mesh2d {
        crate::Mesh2d::Triangle { vertices, indices } => {
            debug_assert!(
                vertices.len() % 2 == 0,
                "Mesh2d::Triangle vertices.len() ({}) must be a multiple of 2 \
                 (stride-2 [x,y,...] buffer)",
                vertices.len(),
            );
            debug_assert!(
                indices.len() % 3 == 0,
                "Mesh2d::Triangle indices.len() ({}) must be a multiple of 3 \
                 (stride-3 face list)",
                indices.len(),
            );
            (vertices.is_empty(), indices.is_empty())
        }
        crate::Mesh2d::Quad { vertices, indices } => {
            debug_assert!(
                vertices.len() % 2 == 0,
                "Mesh2d::Quad vertices.len() ({}) must be a multiple of 2 \
                 (stride-2 [x,y,...] buffer)",
                vertices.len(),
            );
            debug_assert!(
                indices.len() % 4 == 0,
                "Mesh2d::Quad indices.len() ({}) must be a multiple of 4 \
                 (stride-4 face list)",
                indices.len(),
            );
            (vertices.is_empty(), indices.is_empty())
        }
    };
    if verts_empty || faces_empty {
        return Err(SweepError::EmptyMesh2d);
    }

    // 2. Layer count.
    if layers == 0 {
        return Err(SweepError::InvalidLayerCount);
    }

    // 3. Axis + magnitude per variant.
    match params {
        SweepParams::Extrude { axis, length } | SweepParams::SweepLinear { axis, length } => {
            let norm = norm3(*axis);
            if norm < DEGENERATE_TOL {
                return Err(SweepError::DegenerateAxis);
            }
            if !length.is_finite() || *length <= 0.0 {
                return Err(SweepError::DegenerateMagnitude);
            }
            Ok([axis[0] / norm, axis[1] / norm, axis[2] / norm])
        }
        SweepParams::Revolve { axis_dir, angle, .. } => {
            let norm = norm3(*axis_dir);
            if norm < DEGENERATE_TOL {
                return Err(SweepError::DegenerateAxis);
            }
            if !angle.is_finite() || *angle <= 0.0 {
                return Err(SweepError::DegenerateMagnitude);
            }
            Ok([axis_dir[0] / norm, axis_dir[1] / norm, axis_dir[2] / norm])
        }
    }
}

/// Euclidean norm of a 3-vector.
#[inline]
fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Sweep a 2D cross-section mesh into a 3D wedge/hex mesh.
///
/// # Arguments
///
/// * `mesh2d` — the 2D cross-section mesh (from `mesh_swept_profile_2d`).
///   `Mesh2d::Triangle` produces wedge elements; `Mesh2d::Quad` produces hex.
/// * `params` — sweep trajectory (extrude, revolve, or linear sweep).
/// * `layers` — number of element layers K.  Use [`derive_layer_count`] to
///   compute K from mesh size and sweep distance.
///
/// # Returns
///
/// `Ok(SweptMesh3d)` with `(K+1) * n_base` vertices (stride 3, `f32`) and
/// `K * n_faces` elements.  Returns `Err(SweepError)` on degenerate inputs.
///
/// # Node layout
///
/// Node `(layer ℓ, base i)` → global index `ℓ * n_base + i`.
/// Layer 0 is at the profile origin; layer K is at the sweep end.
pub fn sweep_2d_mesh_to_3d(
    mesh2d: &crate::Mesh2d,
    params: &SweepParams,
    layers: usize,
) -> Result<SweptMesh3d, SweepError> {
    let unit_axis = validate_sweep_inputs(mesh2d, params, layers)?;

    match mesh2d {
        crate::Mesh2d::Triangle { vertices, indices } => {
            let verts3d = build_vertices(vertices, params, &unit_axis, layers);
            let conn = build_swept_connectivity(indices, vertices.len() / 2, layers, 3);
            Ok(SweptMesh3d {
                vertices: verts3d,
                connectivity: SweptConnectivity::Wedge { indices: conn },
                layers,
            })
        }
        crate::Mesh2d::Quad { vertices, indices } => {
            let verts3d = build_vertices(vertices, params, &unit_axis, layers);
            let conn = build_swept_connectivity(indices, vertices.len() / 2, layers, 4);
            Ok(SweptMesh3d {
                vertices: verts3d,
                connectivity: SweptConnectivity::Hex { indices: conn },
                layers,
            })
        }
    }
}

/// Build the 3D vertex buffer by replicating the 2D base layer K+1 times.
///
/// The 2D node `[x, y]` embeds at `z=0` in 3D; each layer applies a
/// transform derived from `params`. Per-layer constants (translation offset
/// for Extrude/SweepLinear, trig values for Revolve) are hoisted out of the
/// inner node loop to avoid redundant computation across base nodes.
fn build_vertices(
    verts2d: &[f32],
    params: &SweepParams,
    unit_axis: &[f64; 3],
    layers: usize,
) -> Vec<f32> {
    let n_base = verts2d.len() / 2;
    let mut out = Vec::with_capacity(n_base * (layers + 1) * 3);

    match params {
        SweepParams::Extrude { length, .. } | SweepParams::SweepLinear { length, .. } => {
            let [ax, ay, az] = *unit_axis;
            for layer in 0..=layers {
                // Per-layer translation offset — constant across all base nodes.
                let d = layer as f64 / layers as f64 * length;
                let (dx, dy, dz) = (ax * d, ay * d, az * d);
                for i in 0..n_base {
                    let x2 = verts2d[i * 2] as f64;
                    let y2 = verts2d[i * 2 + 1] as f64;
                    out.push((x2 + dx) as f32);
                    out.push((y2 + dy) as f32);
                    out.push(dz as f32);
                }
            }
        }
        SweepParams::Revolve { axis_origin, angle, .. } => {
            let [kx, ky, kz] = *unit_axis;
            let [ox, oy, oz] = *axis_origin;
            // pz is constant: the 2D mesh sits at z=0 in the profile frame.
            let pz_const = -oz;
            for layer in 0..=layers {
                // Per-layer trig — constant across all base nodes for this layer.
                //
                // Negate the angle so positive revolution sweeps from the profile's
                // local +x toward the 3D +z direction (cylindrical-coordinate
                // convention used throughout the CAD pipeline). The standard
                // right-hand Rodrigues formula around +y would go from +x toward -z;
                // negating θ reverses the sweep to +x → +z, matching the test
                // contract locked in step-15 and PRD task #8 surface-extraction
                // assumptions.
                let theta = -(layer as f64 / layers as f64 * angle);
                let (sin_t, cos_t) = theta.sin_cos();
                let one_m_cos = 1.0 - cos_t;
                for i in 0..n_base {
                    let px = verts2d[i * 2] as f64 - ox;
                    let py = verts2d[i * 2 + 1] as f64 - oy;
                    let kdotp = kx * px + ky * py + kz * pz_const;
                    // Rodrigues: R(θ) p = p cosθ + (k × p) sinθ + k (k·p)(1 − cosθ)
                    let cx = px * cos_t + (ky * pz_const - kz * py) * sin_t
                        + kx * kdotp * one_m_cos;
                    let cy = py * cos_t + (kz * px - kx * pz_const) * sin_t
                        + ky * kdotp * one_m_cos;
                    let cz = pz_const * cos_t + (kx * py - ky * px) * sin_t
                        + kz * kdotp * one_m_cos;
                    out.push((cx + ox) as f32);
                    out.push((cy + oy) as f32);
                    out.push((cz + oz) as f32);
                }
            }
        }
    }
    out
}

/// Build element connectivity by sweeping a 2D face list across K layers.
///
/// Each (layer k, face f with `face_arity` nodes) emits one element with
/// `face_arity * 2` indices: the bottom-face node indices (same order as
/// the 2D face), then the top-face node indices in the same order.
///
/// Used for both wedge (`face_arity = 3`) and hex (`face_arity = 4`) meshes.
fn build_swept_connectivity(
    indices: &[u32],
    n_base: usize,
    layers: usize,
    face_arity: usize,
) -> Vec<u32> {
    let n_faces = indices.len() / face_arity;
    let elem_size = face_arity * 2;
    let mut conn = Vec::with_capacity(layers * n_faces * elem_size);
    for k in 0..layers {
        let base_off = (k * n_base) as u32;
        let top_off = ((k + 1) * n_base) as u32;
        for face in indices.chunks_exact(face_arity) {
            for &idx in face {
                conn.push(base_off + idx);
            }
            for &idx in face {
                conn.push(top_off + idx);
            }
        }
    }
    conn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mesh2d;

    // Unit triangle fixture for validation tests.
    fn unit_triangle() -> Mesh2d {
        Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
        }
    }

    // Unit square quad fixture.
    fn unit_quad() -> Mesh2d {
        Mesh2d::Quad {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
            indices: vec![0, 1, 2, 3],
        }
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

    // step-7: sweep_2d_mesh_to_3d validation pre-pass tests

    #[test]
    fn sweep_rejects_empty_vertices() {
        // (a) empty vertices → EmptyMesh2d
        let empty_verts = Mesh2d::Triangle { vertices: vec![], indices: vec![] };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let r = sweep_2d_mesh_to_3d(&empty_verts, &params, 1);
        assert!(matches!(r, Err(SweepError::EmptyMesh2d)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_empty_indices() {
        // (b) vertices present but no faces → EmptyMesh2d
        let no_faces = Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![],
        };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let r = sweep_2d_mesh_to_3d(&no_faces, &params, 1);
        assert!(matches!(r, Err(SweepError::EmptyMesh2d)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_zero_layers() {
        // (c) layers=0 → InvalidLayerCount
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 0);
        assert!(matches!(r, Err(SweepError::InvalidLayerCount)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_zero_axis() {
        // (d) Extrude zero axis → DegenerateAxis
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 0.0], length: 1.0 };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 1);
        assert!(matches!(r, Err(SweepError::DegenerateAxis)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_zero_length() {
        // (e) Extrude zero length → DegenerateMagnitude
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 0.0 };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 1);
        assert!(matches!(r, Err(SweepError::DegenerateMagnitude)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_nan_length() {
        // (f) Extrude NaN length → DegenerateMagnitude
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: f64::NAN };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 1);
        assert!(matches!(r, Err(SweepError::DegenerateMagnitude)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_revolve_zero_axis_dir() {
        // (g) Revolve zero axis_dir → DegenerateAxis
        let params = SweepParams::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 0.0],
            angle: 1.0,
        };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 1);
        assert!(matches!(r, Err(SweepError::DegenerateAxis)), "got: {r:?}");
    }

    #[test]
    fn sweep_rejects_revolve_zero_angle() {
        // (h) Revolve zero angle → DegenerateMagnitude
        let params = SweepParams::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 1.0, 0.0],
            angle: 0.0,
        };
        let r = sweep_2d_mesh_to_3d(&unit_triangle(), &params, 1);
        assert!(matches!(r, Err(SweepError::DegenerateMagnitude)), "got: {r:?}");
    }

    // step-15: Revolve around y-axis by π/2 with K=2

    #[test]
    fn revolve_triangle_y_axis_pi_over_2_k2() {
        // Profile sits in the x>0 half-plane so revolution traces a positive arc.
        let mesh2d = Mesh2d::Triangle {
            vertices: vec![1.0_f32, 0.0, 2.0, 0.0, 1.0, 1.0],
            indices: vec![0, 1, 2],
        };
        let params = SweepParams::Revolve {
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 1.0, 0.0],
            angle: std::f64::consts::FRAC_PI_2,
        };
        let mesh = sweep_2d_mesh_to_3d(&mesh2d, &params, 2).expect("should succeed");

        assert_eq!(mesh.layers, 2);
        // 3 node planes × 3 verts × 3 coords = 27
        assert_eq!(mesh.vertices.len(), 27, "vertices.len()");

        let eps = 1e-5_f32;

        // Bottom layer (θ=0): nodes match input (x, y, 0)
        // node 0: (1,0,0)
        assert!((mesh.vertices[0] - 1.0).abs() < eps, "bot node0 x={}", mesh.vertices[0]);
        assert!((mesh.vertices[1] - 0.0).abs() < eps, "bot node0 y={}", mesh.vertices[1]);
        assert!((mesh.vertices[2] - 0.0).abs() < eps, "bot node0 z={}", mesh.vertices[2]);
        // node 1: (2,0,0)
        assert!((mesh.vertices[3] - 2.0).abs() < eps, "bot node1 x={}", mesh.vertices[3]);
        assert!((mesh.vertices[4] - 0.0).abs() < eps);
        assert!((mesh.vertices[5] - 0.0).abs() < eps);
        // node 2: (1,1,0)
        assert!((mesh.vertices[6] - 1.0).abs() < eps);
        assert!((mesh.vertices[7] - 1.0).abs() < eps);
        assert!((mesh.vertices[8] - 0.0).abs() < eps);

        // Middle layer (θ=π/4): (1,0,0) → (cos(π/4), 0, sin(π/4)) ≈ (0.7071, 0, 0.7071)
        let c45 = (std::f64::consts::FRAC_PI_4.cos()) as f32;
        let s45 = (std::f64::consts::FRAC_PI_4.sin()) as f32;
        // node 3 (middle, base 0): (cos45, 0, sin45)
        assert!((mesh.vertices[9] - c45).abs() < eps, "mid node0 x");
        assert!((mesh.vertices[10] - 0.0).abs() < eps, "mid node0 y");
        assert!((mesh.vertices[11] - s45).abs() < eps, "mid node0 z");

        // Top layer (θ=π/2): (1,0,0) → (cos(π/2), 0, sin(π/2)) ≈ (0, 0, 1)
        let c90 = (std::f64::consts::FRAC_PI_2.cos()) as f32;
        let s90 = (std::f64::consts::FRAC_PI_2.sin()) as f32;
        // node 6 (top, base 0): (0, 0, 1)
        assert!((mesh.vertices[18] - c90).abs() < eps, "top node0 x");
        assert!((mesh.vertices[19] - 0.0).abs() < eps, "top node0 y");
        assert!((mesh.vertices[20] - s90).abs() < eps, "top node0 z");
        // node 8 (top, base 2): (1,1,0) → (0, 1, 1) at θ=π/2
        assert!((mesh.vertices[24] - 0.0).abs() < eps, "top node2 x");
        assert!((mesh.vertices[25] - 1.0).abs() < eps, "top node2 y");
        assert!((mesh.vertices[26] - 1.0).abs() < eps, "top node2 z");

        // Connectivity: 2 wedges
        match &mesh.connectivity {
            SweptConnectivity::Wedge { indices } => {
                assert_eq!(indices.len(), 2 * 6, "2 wedges × 6 indices");
            }
            other => panic!("expected Wedge, got {other:?}"),
        }
    }

    // step-17: SweepLinear produces byte-identical output to Extrude (Phase A contract)

    #[test]
    fn sweep_linear_equals_extrude_same_axis_length() {
        // Phase A contract: SweepLinear on a LineSegment path IS Extrude.
        // Use the K=3 triangle fixture so the full layer loop is exercised.
        let mesh2d = unit_triangle();
        let axis = [0.0_f64, 0.0, 1.0];
        let length = 3.0_f64;

        let extrude_params = SweepParams::Extrude { axis, length };
        let linear_params = SweepParams::SweepLinear { axis, length };

        let extrude_mesh = sweep_2d_mesh_to_3d(&mesh2d, &extrude_params, 3)
            .expect("Extrude should succeed");
        let linear_mesh = sweep_2d_mesh_to_3d(&mesh2d, &linear_params, 3)
            .expect("SweepLinear should succeed");

        // Vertices must be byte-identical (same f32 arithmetic from identical inputs).
        assert_eq!(
            extrude_mesh.vertices, linear_mesh.vertices,
            "SweepLinear must produce identical vertices to Extrude (Phase A contract)"
        );

        assert_eq!(
            extrude_mesh.layers, linear_mesh.layers,
            "layers field must agree"
        );

        // Connectivity indices must be byte-identical.
        match (&extrude_mesh.connectivity, &linear_mesh.connectivity) {
            (SweptConnectivity::Wedge { indices: ei }, SweptConnectivity::Wedge { indices: li }) => {
                assert_eq!(ei, li, "SweepLinear wedge indices must match Extrude");
            }
            _ => panic!("expected Wedge connectivity for both Extrude and SweepLinear"),
        }
    }

    // step-13: K>1 extrude — pins the layer-dimension generalisation

    #[test]
    fn extrude_unit_triangle_k3() {
        let mesh2d = unit_triangle();
        let params = SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 3.0,
        };
        let mesh = sweep_2d_mesh_to_3d(&mesh2d, &params, 3).expect("should succeed");

        assert_eq!(mesh.layers, 3);
        // 4 node planes × 3 base verts × 3 coords = 36
        assert_eq!(mesh.vertices.len(), 36, "vertices.len()");

        let eps = 1e-6_f32;
        // Layer 0: z=0.0
        assert!((mesh.vertices[2] - 0.0).abs() < eps);
        // Layer 1 (offset 9): z=1.0
        assert!((mesh.vertices[9 + 2] - 1.0).abs() < eps);
        // Layer 2 (offset 18): z=2.0
        assert!((mesh.vertices[18 + 2] - 2.0).abs() < eps);
        // Layer 3 (offset 27): z=3.0
        assert!((mesh.vertices[27 + 2] - 3.0).abs() < eps);

        // Connectivity: 3 wedges
        match &mesh.connectivity {
            SweptConnectivity::Wedge { indices } => {
                assert_eq!(indices.len(), 3 * 6, "3 wedges × 6 indices");
                // First wedge: layer 0→1
                assert_eq!(&indices[0..6], &[0_u32, 1, 2, 3, 4, 5]);
                // Second wedge: layer 1→2
                assert_eq!(&indices[6..12], &[3_u32, 4, 5, 6, 7, 8]);
                // Third wedge: layer 2→3
                assert_eq!(&indices[12..18], &[6_u32, 7, 8, 9, 10, 11]);
            }
            other => panic!("expected Wedge, got {other:?}"),
        }
    }

    // step-11: Extrude single CCW unit-square quad, K=1

    #[test]
    fn extrude_unit_quad_k1() {
        let mesh2d = unit_quad();
        let params = SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 0.5,
        };
        let mesh = sweep_2d_mesh_to_3d(&mesh2d, &params, 1).expect("should succeed");

        assert_eq!(mesh.layers, 1);
        // 2 layers × 4 verts × 3 coords = 24
        assert_eq!(mesh.vertices.len(), 24, "vertices.len()");

        let eps = 1e-6_f32;
        // Bottom layer z=0
        assert!((mesh.vertices[2] - 0.0).abs() < eps); // z of node 0
        assert!((mesh.vertices[5] - 0.0).abs() < eps); // z of node 1
        // Top layer z=0.5
        assert!((mesh.vertices[14] - 0.5).abs() < eps); // z of node 4 (=12+2)
        assert!((mesh.vertices[23] - 0.5).abs() < eps); // z of node 7 (=21+2)

        // Connectivity: one hex [0,1,2,3, 4,5,6,7]
        match &mesh.connectivity {
            SweptConnectivity::Hex { indices } => {
                assert_eq!(indices, &vec![0_u32, 1, 2, 3, 4, 5, 6, 7]);
            }
            other => panic!("expected Hex, got {other:?}"),
        }
    }

    // step-9: Extrude single CCW unit-triangle, K=1

    #[test]
    fn extrude_unit_triangle_k1() {
        let mesh2d = unit_triangle();
        let params = SweepParams::Extrude {
            axis: [0.0, 0.0, 1.0],
            length: 0.5,
        };
        let mesh = sweep_2d_mesh_to_3d(&mesh2d, &params, 1).expect("should succeed");

        assert_eq!(mesh.layers, 1);
        // 2 layers × 3 base verts × 3 coords = 18
        assert_eq!(mesh.vertices.len(), 18, "vertices.len()");

        // Bottom layer at z=0
        let eps = 1e-6_f32;
        // node 0: (0,0,0)
        assert!((mesh.vertices[0] - 0.0).abs() < eps);
        assert!((mesh.vertices[1] - 0.0).abs() < eps);
        assert!((mesh.vertices[2] - 0.0).abs() < eps);
        // node 1: (1,0,0)
        assert!((mesh.vertices[3] - 1.0).abs() < eps);
        assert!((mesh.vertices[4] - 0.0).abs() < eps);
        assert!((mesh.vertices[5] - 0.0).abs() < eps);
        // node 2: (0,1,0)
        assert!((mesh.vertices[6] - 0.0).abs() < eps);
        assert!((mesh.vertices[7] - 1.0).abs() < eps);
        assert!((mesh.vertices[8] - 0.0).abs() < eps);

        // Top layer at z=0.5
        // node 3: (0,0,0.5)
        assert!((mesh.vertices[9] - 0.0).abs() < eps);
        assert!((mesh.vertices[10] - 0.0).abs() < eps);
        assert!((mesh.vertices[11] - 0.5).abs() < eps);
        // node 5: (0,1,0.5)
        assert!((mesh.vertices[15] - 0.0).abs() < eps);
        assert!((mesh.vertices[16] - 1.0).abs() < eps);
        assert!((mesh.vertices[17] - 0.5).abs() < eps);

        // Connectivity: one wedge [0,1,2, 3,4,5]
        match &mesh.connectivity {
            SweptConnectivity::Wedge { indices } => {
                assert_eq!(indices, &vec![0_u32, 1, 2, 3, 4, 5]);
            }
            other => panic!("expected Wedge, got {other:?}"),
        }
    }

    // step-5: check_sweep_through_thickness unit tests

    #[test]
    fn check_through_thickness_ok_cases() {
        // (a) exactly at min_layers boundary → None (OK)
        assert!(check_sweep_through_thickness(2, 2).is_none());
        // (b) well above min_layers → None
        assert!(check_sweep_through_thickness(10, 2).is_none());
    }

    #[test]
    fn check_through_thickness_warning_cases() {
        // (c) layers=1 < min_layers=2 → Some warning
        let w = check_sweep_through_thickness(1, 2).expect("should warn");
        assert_eq!(w.layer_count, 1);
        assert_eq!(w.min_layers, 2);
        assert!(w.message.contains("1"), "message: {}", w.message);
        assert!(w.message.contains("mesh_size"), "message: {}", w.message);
        assert!(w.message.contains("sweep_subdivisions"), "message: {}", w.message);
        // (d) layers=0 → Some warning
        let w0 = check_sweep_through_thickness(0, 2).expect("should warn on zero layers");
        assert_eq!(w0.layer_count, 0);
        assert_eq!(w0.min_layers, 2);
    }

    // step-3: derive_layer_count unit tests
    // Contract: round(sweep_distance / mesh_size).max(min_layers)
    // with defensive handling of zero, negative, or non-finite inputs.

    #[test]
    fn derive_layer_count_basic_cases() {
        // (a) round(1.0/0.5) = round(2.0) = 2 → max(2, 2) = 2
        assert_eq!(derive_layer_count(1.0, 0.5, 2), 2);
        // (b) round(2.5/1.0) = round(2.5); 2.5_f64.round() == 3 (Rust rounds
        //     half-values away from zero), so result is max(3, 2) = 3
        assert_eq!(derive_layer_count(2.5, 1.0, 2), 3);
        // (c) round(0.1/1.0) = round(0.1) = 0 → max(0, 2) = 2
        assert_eq!(derive_layer_count(0.1, 1.0, 2), 2);
        // (d) round(10.0/1.0) = 10 → max(10, 2) = 10
        assert_eq!(derive_layer_count(10.0, 1.0, 2), 10);
    }

    #[test]
    fn derive_layer_count_defensive_cases() {
        // (e) mesh_size = 0 → fall through to min_layers
        assert_eq!(derive_layer_count(1.0, 0.0, 2), 2);
        // (f) negative distance → min_layers
        assert_eq!(derive_layer_count(-1.0, 1.0, 2), 2);
        // (g) NaN distance → min_layers
        assert_eq!(derive_layer_count(f64::NAN, 1.0, 2), 2);
        // mesh_size = NaN → min_layers
        assert_eq!(derive_layer_count(1.0, f64::NAN, 2), 2);
        // negative mesh_size → min_layers
        assert_eq!(derive_layer_count(1.0, -1.0, 2), 2);
    }

    // step-3: derive_layer_count MAX_LAYERS upper-bound tests.
    //
    // Reference `super::MAX_LAYERS` symbolically so the test survives any future
    // adjustment to the constant without requiring literal value updates here.

    #[test]
    fn derive_layer_count_clamps_pathological_inputs() {
        // (a) finite-huge raw (1e25 / 1.0 = 1e25 >> MAX_LAYERS) → clamped to MAX_LAYERS.
        assert_eq!(derive_layer_count(1.0e25, 1.0, 2), super::MAX_LAYERS);

        // (b) +∞ raw (f64::MAX / f64::MIN_POSITIVE overflows to +∞) → clamped to MAX_LAYERS.
        assert_eq!(derive_layer_count(f64::MAX, f64::MIN_POSITIVE, 2), super::MAX_LAYERS);

        // (c) raw > MAX_LAYERS by a large factor (2^25 > 2^20) → clamped to MAX_LAYERS.
        assert_eq!(
            derive_layer_count((1u64 << 25) as f64, 1.0, 2),
            super::MAX_LAYERS,
        );

        // (d) min_layers floor is irrelevant when the clamp wins: result is still MAX_LAYERS.
        assert_eq!(derive_layer_count(1.0e25, 1.0, 5), super::MAX_LAYERS);
    }

    // step-1: debug-only `#[should_panic]` tests for malformed Mesh2d shape invariants.
    //
    // Each test is gated by `#[cfg(debug_assertions)]` because `debug_assert!` is
    // elided in release builds — without the gate, `#[should_panic]` would falsely
    // fail under `cargo test --release` (same rationale as
    // `shell_boundary.rs:542-573` and `crates/reify-eval/src/kernel_registry.rs:915-933`).

    /// Triangle with indices.len() % 3 != 0 must debug-panic naming `indices.len()`.
    ///
    /// Gated `#[cfg(debug_assertions)]`: `debug_assert!` is elided in release builds;
    /// without the gate `#[should_panic]` would falsely fail under `cargo test --release`.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Mesh2d::Triangle indices.len()")]
    fn validate_sweep_inputs_panics_on_triangle_bad_indices_len() {
        // indices has 4 elements — 4 % 3 != 0.  Vertices are valid (len=8, 8%2==0)
        // so the vertices-stride check passes and the indices-stride check fires.
        let mesh = Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.5, 0.5],
            indices: vec![0, 1, 2, 3],
        };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let _ = sweep_2d_mesh_to_3d(&mesh, &params, 1);
    }

    /// Quad with indices.len() % 4 != 0 must debug-panic naming `indices.len()`.
    ///
    /// Gated `#[cfg(debug_assertions)]`: see sibling `_triangle_bad_indices_len` for rationale.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Mesh2d::Quad indices.len()")]
    fn validate_sweep_inputs_panics_on_quad_bad_indices_len() {
        // indices has 5 elements — 5 % 4 != 0.  Vertices are valid (len=10, 10%2==0)
        // so the vertices-stride check passes and the indices-stride check fires.
        let mesh = Mesh2d::Quad {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.5, 0.5],
            indices: vec![0, 1, 2, 3, 4],
        };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let _ = sweep_2d_mesh_to_3d(&mesh, &params, 1);
    }

    /// Triangle with vertices.len() % 2 != 0 must debug-panic naming `vertices.len()`.
    ///
    /// Gated `#[cfg(debug_assertions)]`: see sibling `_triangle_bad_indices_len` for rationale.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Mesh2d::Triangle vertices.len()")]
    fn validate_sweep_inputs_panics_on_triangle_bad_vertices_len() {
        // vertices has 3 elements — 3 % 2 != 0; the vertices-stride check fires first.
        let mesh = Mesh2d::Triangle {
            vertices: vec![0.0_f32, 0.0, 1.0],
            indices: vec![0, 1, 2],
        };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let _ = sweep_2d_mesh_to_3d(&mesh, &params, 1);
    }

    /// Quad with vertices.len() % 2 != 0 must debug-panic naming `vertices.len()`.
    ///
    /// Gated `#[cfg(debug_assertions)]`: see sibling `_triangle_bad_indices_len` for rationale.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Mesh2d::Quad vertices.len()")]
    fn validate_sweep_inputs_panics_on_quad_bad_vertices_len() {
        // vertices has 5 elements — 5 % 2 != 0; the vertices-stride check fires first.
        let mesh = Mesh2d::Quad {
            vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 1.0],
            indices: vec![0, 1, 2, 3],
        };
        let params = SweepParams::Extrude { axis: [0.0, 0.0, 1.0], length: 1.0 };
        let _ = sweep_2d_mesh_to_3d(&mesh, &params, 1);
    }
}

/// Compile-time signature pins for `sweep`'s public function items.
///
/// If a function's parameter types or return type changes, this module fails
/// to compile before any test runs — behavioral tests alone would not catch
/// a signature change that happens to preserve the same observable behaviour.
#[cfg(test)]
mod surface_pins {
    use super::*;
    use crate::Mesh2d;

    #[test]
    fn function_signatures_compile() {
        let _: fn(f64, f64, usize) -> usize = derive_layer_count;
        let _: fn(usize, usize) -> Option<ThroughThicknessSweepWarning> =
            check_sweep_through_thickness;
        let _: fn(&Mesh2d, &SweepParams, usize) -> Result<SweptMesh3d, SweepError> =
            sweep_2d_mesh_to_3d;
    }
}
