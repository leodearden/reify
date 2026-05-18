//! Featherstone 6D spatial-vector primitives.
//!
//! Implements the spatial-vector core from `docs/prds/v0_3/rigid-body-dynamics.md`
//! §10 Phase 1 (RBD-γ), consumed by RBD-δ (motion subspace) and RBD-ε (RNEA).
//! All math is pure-Rust `f64` numerics — no Reify-level `Value` dispatch and
//! no heavyweight linalg dependency (triple-nested-loop multiply on `[f64; N]`
//! is plenty fast for the small mechanism sizes targeted in v0.3).
//!
//! # Conventions (Featherstone, *Rigid Body Dynamics Algorithms*, 2008)
//!
//! * **Spatial-vector ordering** (§2.4 motion-vector convention): angular
//!   first, linear second — `[ω_x, ω_y, ω_z, v_x, v_y, v_z]`. The PRD §5.1
//!   inline literal `[ω; v]` matches. Spatial *force* vectors reuse the same
//!   storage but interpret `[0..3]` as torque τ and `[3..6]` as force F.
//! * **Matrix storage**: 6×6 transforms / inertias are row-major `[f64; 36]`.
//! * **Quaternions**: `(w, x, y, z)` unit-quat ordering, scalar first, matching
//!   `reify_types::Value::Orientation`.

/// A 6D spatial vector in Featherstone motion-vector ordering
/// `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` (angular first, linear second).
///
/// Used for both spatial *motion* vectors (velocity, acceleration) and spatial
/// *force* vectors (where `[0..3]` is torque τ and `[3..6]` is force F); the
/// interpretation is fixed by the operator, not the storage.
///
/// `PartialEq` is bit-wise on the underlying `f64`s — numerical comparisons in
/// tests use an entrywise tolerance helper, never derived equality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialVector6([f64; 6]);

impl SpatialVector6 {
    /// The zero spatial vector (six zeros).
    pub fn zero() -> Self {
        SpatialVector6([0.0; 6])
    }

    /// Construct from a raw `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` array.
    pub fn from_array(a: [f64; 6]) -> Self {
        SpatialVector6(a)
    }

    /// Construct from separate angular and linear 3-vectors.
    pub fn from_angular_linear(angular: [f64; 3], linear: [f64; 3]) -> Self {
        SpatialVector6([
            angular[0], angular[1], angular[2], linear[0], linear[1], linear[2],
        ])
    }

    /// The raw `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` storage.
    pub fn as_array(&self) -> [f64; 6] {
        self.0
    }

    /// The angular part `[ω_x, ω_y, ω_z]` (indices `0..3`).
    pub fn angular(&self) -> [f64; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// The linear part `[v_x, v_y, v_z]` (indices `3..6`).
    pub fn linear(&self) -> [f64; 3] {
        [self.0[3], self.0[4], self.0[5]]
    }
}

/// A rigid-body pose: a local pure-Rust mirror of Reify's
/// `reify_types::Value::Frame { origin: Point3<LENGTH>, basis: Orientation }`.
///
/// Spatial primitives are consumed by RNEA at speed; they operate on raw `f64`
/// rather than paying a match-and-unbox cost on every call, so this struct
/// carries the `Value::Frame` payload at the f64 level:
///
/// * `rotation` — a `(w, x, y, z)` unit quaternion, scalar first, matching the
///   ordering of `Value::Orientation`.
/// * `translation` — meters, with the Reify `LENGTH` dimension stripped.
///
/// A future adapter `Frame3::from_value_frame(&Value) -> Option<Self>` (the
/// eval-side wiring that bridges `Value::Frame` ↔ `Frame3`) is filed under
/// RBD-ε, where the RNEA call sites actually need it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame3 {
    rotation: [f64; 4],
    translation: [f64; 3],
}

impl Frame3 {
    /// The identity pose: unit quaternion `(1, 0, 0, 0)` and zero translation.
    pub fn identity() -> Self {
        Frame3 {
            rotation: [1.0, 0.0, 0.0, 0.0],
            translation: [0.0, 0.0, 0.0],
        }
    }

    /// Construct from a `(w, x, y, z)` quaternion and a meters translation.
    pub fn new(rotation: [f64; 4], translation: [f64; 3]) -> Self {
        Frame3 {
            rotation,
            translation,
        }
    }

    /// The `(w, x, y, z)` rotation quaternion.
    pub fn rotation(&self) -> [f64; 4] {
        self.rotation
    }

    /// The translation `[x, y, z]` in meters.
    pub fn translation(&self) -> [f64; 3] {
        self.translation
    }
}

// ── Private 3×3 helpers ──────────────────────────────────────────────────────

/// Rotation matrix `E` for a `(w, x, y, z)` unit quaternion.
///
/// Uses the standard active-rotation formula (consistent with the project's
/// `orientation::quat_rotate`, which computes `q·(0,v)·q*`):
///
/// ```text
/// E = [[1−2(y²+z²),  2(xy−wz),   2(xz+wy)],
///      [2(xy+wz),    1−2(x²+z²), 2(yz−wx)],
///      [2(xz−wy),    2(yz+wx),   1−2(x²+y²)]]
/// ```
///
/// The input is assumed unit; defensive renormalization is layered in by a
/// later RBD-γ step if the random-sample capstone exposes non-unit drift.
fn quat_to_rotation_matrix(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// Skew-symmetric (cross-product) matrix `ṽ` of a 3-vector, such that
/// `ṽ · u == v × u`:
///
/// ```text
/// skew([x, y, z]) = [[0, −z,  y],
///                    [z,  0, −x],
///                    [−y, x,  0]]
/// ```
fn skew(v: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = v;
    [[0.0, -z, y], [z, 0.0, -x], [-y, x, 0.0]]
}

/// 3×3 · 3×3 matrix product (row-major nested arrays).
fn mat3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

/// A 6×6 spatial (Plücker) transform in Featherstone block form, stored
/// row-major as `[f64; 36]`.
///
/// `PartialEq` is bit-wise on the underlying `f64`s — numerical comparisons in
/// tests use the entrywise tolerance helper, never derived equality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialTransform6([f64; 36]);

impl SpatialTransform6 {
    /// The raw row-major 6×6 storage.
    pub fn as_matrix(&self) -> [f64; 36] {
        self.0
    }

    /// Build the spatial transform of a rigid-body pose, per Featherstone
    /// (2008) *Rigid Body Dynamics Algorithms* Eq. 2.24:
    ///
    /// ```text
    /// X(r, E) = [[E,      0 ],
    ///            [−r̃·E,  E ]]
    /// ```
    ///
    /// where `E` is the rotation matrix of `f.rotation` and `r̃` is the
    /// skew-symmetric matrix of the translation `r = f.translation`.
    pub fn from_frame3(f: &Frame3) -> Self {
        let e = quat_to_rotation_matrix(f.rotation());
        let r_tilde = skew(f.translation());
        let rte = mat3_mul(r_tilde, e); // r̃·E

        let mut m = [0.0; 36];
        for i in 0..3 {
            for j in 0..3 {
                // Top-left: E
                m[i * 6 + j] = e[i][j];
                // Top-right: 0 (left as initialized).
                // Bottom-left: −r̃·E
                m[(i + 3) * 6 + j] = -rte[i][j];
                // Bottom-right: E
                m[(i + 3) * 6 + (j + 3)] = e[i][j];
            }
        }
        SpatialTransform6(m)
    }
}
