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

/// 3-vector cross product `a × b`.
fn vec3_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
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

    /// Compose two spatial transforms: `self.compose(&other)` is the 6×6
    /// matrix product `self · other` (apply `other` first, then `self`).
    ///
    /// Straightforward triple-nested-loop multiply on the dense `[f64; 36]`
    /// storage — Featherstone §5.1 notes a dense representation is sufficient
    /// for the small mechanism sizes targeted in v0.3, so no sparse/linalg
    /// dependency is warranted.
    pub fn compose(&self, other: &Self) -> Self {
        let mut m = [0.0; 36];
        for i in 0..6 {
            for j in 0..6 {
                let mut acc = 0.0;
                for k in 0..6 {
                    acc += self.0[i * 6 + k] * other.0[k * 6 + j];
                }
                m[i * 6 + j] = acc;
            }
        }
        SpatialTransform6(m)
    }

    /// The inverse spatial transform, via the Featherstone closed form
    /// (no general-purpose 6×6 inversion needed).
    ///
    /// For `X(r, E) = [[E, 0]; [−r̃·E, E]]` the inverse is
    /// `X(−Eᵀr, Eᵀ) = [[Eᵀ, 0]; [Eᵀ·r̃, Eᵀ]]`. Working directly from the
    /// stored blocks: let `E` be the top-left block and `BL = −r̃·E` the
    /// bottom-left block. Then `r̃ = −BL·Eᵀ`, so the inverse bottom-left
    /// block is `Eᵀ·r̃ = −Eᵀ·BL·Eᵀ`. This exploits `E` being orthogonal
    /// (`Eᵀ = E⁻¹`) — the defining property of a rotation block.
    pub fn inverse(&self) -> Self {
        let mut e = [[0.0; 3]; 3];
        let mut bl = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                e[i][j] = self.0[i * 6 + j];
                bl[i][j] = self.0[(i + 3) * 6 + j];
            }
        }
        // Eᵀ
        let mut et = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                et[i][j] = e[j][i];
            }
        }
        // Inverse bottom-left block: −Eᵀ·BL·Eᵀ.
        let etbl_et = mat3_mul(mat3_mul(et, bl), et);

        let mut m = [0.0; 36];
        for i in 0..3 {
            for j in 0..3 {
                m[i * 6 + j] = et[i][j]; // top-left Eᵀ
                // top-right 0 (left as initialized)
                m[(i + 3) * 6 + j] = -etbl_et[i][j]; // bottom-left −Eᵀ·BL·Eᵀ
                m[(i + 3) * 6 + (j + 3)] = et[i][j]; // bottom-right Eᵀ
            }
        }
        SpatialTransform6(m)
    }

    /// Apply the transform to a spatial vector: the row-major 6×6 · 6
    /// matrix-vector product `result[i] = Σₖ self[i,k] · v[k]`.
    ///
    /// Used by the RNEA forward pass `v_i = X_{p→i}·v_p + S_i·q̇_i`.
    pub fn apply(&self, v: &SpatialVector6) -> SpatialVector6 {
        let a = v.as_array();
        let mut out = [0.0; 6];
        for (out_i, row) in out.iter_mut().zip(self.0.chunks_exact(6)) {
            *out_i = row.iter().zip(a.iter()).map(|(m, x)| m * x).sum();
        }
        SpatialVector6::from_array(out)
    }
}

/// Spatial rigid-body inertia as a 6×6 symmetric matrix, stored row-major
/// as `[f64; 36]`. Used as `f_i = I_i·a_i + cross_f(v_i, I_i·v_i)` in the
/// RNEA backward pass (Featherstone (2008) §5.2).
///
/// `PartialEq` is bit-wise on the underlying `f64`s — numerical comparisons
/// in tests use the entrywise tolerance helper, never derived equality.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialInertia6([f64; 36]);

impl SpatialInertia6 {
    /// Build the spatial inertia of a rigid body from its mass, center-of-mass
    /// position (expressed in the body frame relative to the frame origin),
    /// and rotational inertia tensor `Ī` about the COM in body axes.
    ///
    /// Featherstone (2008) *Rigid Body Dynamics Algorithms* Eq. 2.63:
    ///
    /// ```text
    /// I_6 = [[ Ī + m·c̃·c̃ᵀ,  m·c̃     ],
    ///        [ m·c̃ᵀ,         m·I_3   ]]
    /// ```
    ///
    /// where `c̃` is the skew-symmetric matrix of `com` (so `c̃ᵀ = −c̃`).
    /// The resulting matrix is symmetric (the top-left block is symmetric
    /// because `Ī` is symmetric and `c̃·c̃ᵀ` is symmetric, and the off-diagonal
    /// blocks are mutual transposes).
    pub fn from_mass_com_inertia(
        mass: f64,
        com: [f64; 3],
        inertia: [[f64; 3]; 3],
    ) -> Self {
        let c = skew(com);
        // c̃ᵀ via in-place transpose.
        let mut ct = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                ct[i][j] = c[j][i];
            }
        }
        // m·c̃·c̃ᵀ.
        let c_ct = mat3_mul(c, ct);

        let mut m = [0.0; 36];
        for i in 0..3 {
            for j in 0..3 {
                // Top-left: Ī + m·c̃·c̃ᵀ
                m[i * 6 + j] = inertia[i][j] + mass * c_ct[i][j];
                // Top-right: m·c̃
                m[i * 6 + (j + 3)] = mass * c[i][j];
                // Bottom-left: m·c̃ᵀ
                m[(i + 3) * 6 + j] = mass * ct[i][j];
                // Bottom-right: m·I_3
                m[(i + 3) * 6 + (j + 3)] = if i == j { mass } else { 0.0 };
            }
        }
        SpatialInertia6(m)
    }

    /// The raw row-major 6×6 storage.
    pub fn as_matrix(&self) -> [f64; 36] {
        self.0
    }

    /// Apply the inertia to a spatial vector: the row-major 6×6 · 6
    /// matrix-vector product `result[i] = Σₖ self[i,k] · v[k]`.
    ///
    /// Used by the RNEA backward pass `f_i = I_i·a_i + cross_f(v_i, I_i·v_i)`.
    pub fn apply(&self, v: &SpatialVector6) -> SpatialVector6 {
        let a = v.as_array();
        let mut out = [0.0; 6];
        for (out_i, row) in out.iter_mut().zip(self.0.chunks_exact(6)) {
            *out_i = row.iter().zip(a.iter()).map(|(m, x)| m * x).sum();
        }
        SpatialVector6::from_array(out)
    }
}

/// Spatial cross product on motion vectors, Featherstone (2008) Eq. 2.31:
///
/// ```text
/// cross_m(v, w) = [ω_v × ω_w,
///                  ω_v × v_w + v_v × ω_w]
/// ```
///
/// Used by the RNEA forward pass to accumulate Coriolis/centrifugal terms,
/// e.g. `a_i = X·a_p + S·q̈ + cross_m(v_i, S·q̇_i)` (Featherstone §5.2).
pub fn cross_m(v: &SpatialVector6, w: &SpatialVector6) -> SpatialVector6 {
    let omega_v = v.angular();
    let lin_v = v.linear();
    let omega_w = w.angular();
    let lin_w = w.linear();
    let ang = vec3_cross(omega_v, omega_w);
    let a = vec3_cross(omega_v, lin_w);
    let b = vec3_cross(lin_v, omega_w);
    SpatialVector6::from_angular_linear(ang, [a[0] + b[0], a[1] + b[1], a[2] + b[2]])
}

/// Spatial cross product on force vectors (the dual of `cross_m` on motions),
/// Featherstone (2008) Eq. 2.32:
///
/// ```text
/// cross_f(v, f) = [ω_v × τ_f + v_v × F_f,
///                  ω_v × F_f]
/// ```
///
/// where `v` is a motion vector and `f` is a force vector whose spatial layout
/// reuses `SpatialVector6` storage but interprets `[0..3]` as torque τ and
/// `[3..6]` as force F.
///
/// Satisfies the Featherstone (2008) §2.11 motion/force duality
/// `<cross_m(v, w), f> = −<w, cross_f(v, f)>` under the standard 6-vector
/// dot product `<(ω, v), (τ, F)> = ω·τ + v·F`. Used by the RNEA backward
/// pass `f_i = I_i·a_i + cross_f(v_i, I_i·v_i) + transmitted`
/// (Featherstone §5.2).
pub fn cross_f(v: &SpatialVector6, f: &SpatialVector6) -> SpatialVector6 {
    let omega_v = v.angular();
    let lin_v = v.linear();
    let tau_f = f.angular();
    let force_f = f.linear();
    let a = vec3_cross(omega_v, tau_f);
    let b = vec3_cross(lin_v, force_f);
    let lin = vec3_cross(omega_v, force_f);
    SpatialVector6::from_angular_linear([a[0] + b[0], a[1] + b[1], a[2] + b[2]], lin)
}

#[cfg(test)]
mod tests {
    use super::quat_to_rotation_matrix;
    use crate::orientation::quat_rotate;

    /// Cross-checks the private `quat_to_rotation_matrix` against the
    /// project's `orientation::quat_rotate` (`q·(0,v)·q*`). Downstream RBD
    /// correctness hinges on `Frame3.rotation` matching the
    /// `Value::Orientation` `(w, x, y, z)` convention and on the rotation
    /// matrix being the active rotation `quat_rotate` implements — but the
    /// integration test (`tests/spatial_tests.rs`) can reach neither the
    /// private fn nor the `pub(crate)` `quat_rotate`. This in-crate check
    /// locks the convention so a future edit to either side cannot silently
    /// drift it. (Non-axis-aligned quaternions so every E entry is exercised.)
    #[test]
    fn quat_to_rotation_matrix_matches_orientation_quat_rotate() {
        const TOL: f64 = 1e-12;
        // (w, x, y, z) unit quaternions, none axis-aligned.
        let quats: [[f64; 4]; 2] = [
            {
                let n = (1.0f64 + 0.25 + 0.0625 + 0.5625).sqrt();
                [1.0 / n, 0.5 / n, 0.25 / n, -0.75 / n]
            },
            {
                let n = (0.04f64 + 0.36 + 0.09 + 0.49).sqrt();
                [0.2 / n, 0.6 / n, -0.3 / n, 0.7 / n]
            },
        ];
        let vs: [[f64; 3]; 3] = [[1.0, 2.0, 3.0], [-0.5, 1.5, -2.25], [0.3, -0.7, 0.9]];

        for q in quats {
            let e = quat_to_rotation_matrix(q);
            for v in vs {
                let ev = [
                    e[0][0] * v[0] + e[0][1] * v[1] + e[0][2] * v[2],
                    e[1][0] * v[0] + e[1][1] * v[1] + e[1][2] * v[2],
                    e[2][0] * v[0] + e[2][1] * v[1] + e[2][2] * v[2],
                ];
                let (rx, ry, rz) =
                    quat_rotate((q[0], q[1], q[2], q[3]), v[0], v[1], v[2]);
                assert!(
                    (ev[0] - rx).abs() < TOL
                        && (ev[1] - ry).abs() < TOL
                        && (ev[2] - rz).abs() < TOL,
                    "convention drift: E·v = {ev:?} but quat_rotate = \
                     {:?} for q={q:?}, v={v:?}",
                    (rx, ry, rz)
                );
            }
        }
    }
}
