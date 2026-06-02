use std::collections::BTreeMap;

use reify_core::DimensionVector;
use reify_ir::{Value, quaternion_is_finite};

use crate::helpers::tensor_components_f64;

/// Inner validator shared by [`decompose_vec3`] and [`decompose_point3`].
///
/// Validates that `items` contains exactly three components with a single
/// shared dimension, all numeric and finite, and returns the three `f64`
/// values together with their common [`DimensionVector`].
///
/// Returns `None` when:
/// - `items.len() != 3`,
/// - the three components carry mixed dimensions, or
/// - any component is non-numeric or non-finite.
fn decompose_xyz3(items: &[Value]) -> Option<([f64; 3], DimensionVector)> {
    if items.len() != 3 {
        return None;
    }
    let dim = items[0].dimension();
    if items[1].dimension() != dim || items[2].dimension() != dim {
        return None;
    }
    let (a, b, c) = match (items[0].as_f64(), items[1].as_f64(), items[2].as_f64()) {
        (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
        _ => return None,
    };
    Some(([a, b, c], dim))
}

/// Decompose a `Value::Vector` of exactly three components carrying a single
/// shared dimension into its three finite f64 components and that dimension.
///
/// Returns `None` (which callers map to `Value::Undef`) when:
/// - `v` is not a `Value::Vector` of length 3,
/// - the three components have mixed dimensions, or
/// - any component is non-numeric or non-finite.
///
/// Used by `decompose_transform` for the translation field and by
/// `transform_exp` to validate the `angular` / `linear` fields of the input
/// twist `Map`.  Delegates the length/dimension/finite checks to
/// [`decompose_xyz3`].
fn decompose_vec3(v: &Value) -> Option<([f64; 3], DimensionVector)> {
    let items = match v {
        Value::Vector(items) => items,
        _ => return None,
    };
    decompose_xyz3(items)
}

/// Decompose a `Value::Point` of exactly three components carrying a single
/// shared dimension into its three finite f64 components and that dimension.
///
/// Returns `None` (which callers map to `Value::Undef`) when:
/// - `v` is not a `Value::Point` of length 3,
/// - the three components have mixed dimensions, or
/// - any component is non-numeric or non-finite.
///
/// Used by `eval_geometry` for `"project"` (to decode both the point argument
/// and the frame origin) and by `frame_to_frame` (to decode each frame's
/// origin).  Delegates the length/dimension/finite checks to
/// [`decompose_xyz3`].
fn decompose_point3(v: &Value) -> Option<([f64; 3], DimensionVector)> {
    let items = match v {
        Value::Point(items) => items,
        _ => return None,
    };
    decompose_xyz3(items)
}

/// `(w, x, y, z)` quaternion components extracted from a `Value::Orientation`.
type QuatComponents = (f64, f64, f64, f64);

/// Decomposed `Value::Transform`: rotation quaternion components, the three
/// translation f64 components, and the shared dimension carried on the
/// translation vector.
type DecomposedTransform = (QuatComponents, [f64; 3], DimensionVector);

/// Decompose a `Value::Transform` into its quaternion components, three
/// translation f64 components, and the shared dimension carried on the
/// translation vector.
///
/// Returns `None` (which callers map to `Value::Undef`) when:
/// - `v` is not a `Value::Transform`,
/// - `rotation` is not an `Orientation` or has non-finite components,
/// - `translation` is not a `Vector` of exactly three components,
/// - the three translation components have mixed dimensions, or
/// - any component is non-numeric or non-finite.
///
/// This consolidates the destructure-and-validate pattern shared by
/// `transform_compose`, `transform_inverse`, `transform_log`, and
/// `transform_exp`.
fn decompose_transform(v: &Value) -> Option<DecomposedTransform> {
    let (rotation, translation) = match v {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        _ => return None,
    };
    let (rw, rx, ry, rz) = match rotation {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        _ => return None,
    };
    if !quaternion_is_finite(rw, rx, ry, rz) {
        return None;
    }
    let (t, dim) = decompose_vec3(translation)?;
    Some(((rw, rx, ry, rz), t, dim))
}

/// Minimum acceptable squared norm for an input quaternion accepted by
/// `normalize_quat_input` (= (1e-12)²; see that function's doc for rationale).
const INPUT_QUAT_NORM_SQ_MIN: f64 = 1e-24;

/// Normalize a quaternion tuple `(w, x, y, z)` to unit length using the shared
/// `1e-24` squared-norm gate, returning `None` if the quaternion is too small or
/// if its squared norm is non-finite.
///
/// The `1e-24` threshold (= `(1e-12)²`) is intentionally looser than `f64::EPSILON`
/// (`~2.22e-16`): it accepts raw input quaternions whose norm is as small as `~1e-12`
/// and normalises them, while still rejecting genuinely-zero or denormal-risking inputs.
/// The previous `f64::EPSILON` gate rejected anything with norm < `~1.5e-8`, which was
/// needlessly strict — dividing by a `1e-12` norm still yields finite, well-scaled unit
/// components.
///
/// The `!norm_sq.is_finite()` check additionally rejects overflow inputs where
/// `norm_sq = ±∞` (e.g. `Orientation { w: 1e200, … }` where `1e200² = ∞`). Without
/// this check the subsequent `q / ∞ = 0.0` collapse would silently emit a zero
/// quaternion, which is invalid. This is what makes `transform_log`,
/// `transform_inverse`, and `transform_compose` symmetric on overflow input without
/// requiring per-site defensive renormalizes.
///
/// `is_finite()` also rejects NaN (defensive): all current call sites pass through
/// `decompose_transform`'s `quaternion_is_finite` check, so NaN cannot reach this
/// helper in practice — but future callers that bypass `decompose_transform` are
/// covered automatically.
///
/// Called by `transform_log`, `transform_inverse`, and `transform_compose` for
/// input-side quaternion normalization, unifying three formerly near-identical blocks.
fn normalize_quat_input(q: (f64, f64, f64, f64)) -> Option<(f64, f64, f64, f64)> {
    let (w, x, y, z) = q;
    let norm_sq = w * w + x * x + y * y + z * z;
    if !norm_sq.is_finite() || norm_sq < INPUT_QUAT_NORM_SQ_MIN {
        return None;
    }
    let norm = norm_sq.sqrt();
    Some((w / norm, x / norm, y / norm, z / norm))
}

/// Build a translation/twist component preserving the carried dimension:
/// `DIMENSIONLESS → Value::Real(v)`, otherwise `Value::Scalar { si_value, dim }`.
///
/// This consolidates the inline closure shared by `transform_compose`,
/// `transform_inverse`, `transform_log`, and `transform_exp`.
fn make_dimensioned_component(dim: DimensionVector, value: f64) -> Value {
    if dim.is_dimensionless() {
        Value::Real(value)
    } else {
        Value::Scalar {
            si_value: value,
            dimension: dim,
        }
    }
}

pub(crate) fn eval_geometry(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Determinacy predicates (stubs) ---
        "determined" => Value::Undef,
        "undetermined" => Value::Undef,
        "constrained" => Value::Undef,
        "partially_determined" => Value::Undef,

        // --- Frame constructors ---
        "frame3_identity" => {
            if args.is_empty() {
                Value::Frame {
                    origin: Box::new(Value::Point(vec![
                        Value::length(0.0),
                        Value::length(0.0),
                        Value::length(0.0),
                    ])),
                    basis: Box::new(Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                }
            } else {
                Value::Undef
            }
        }
        "frame3" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let origin = &args[0];
            let basis = &args[1];
            match origin {
                Value::Point(components) if components.len() == 3 => {}
                _ => return Some(Value::Undef),
            }
            if !matches!(basis, Value::Orientation { .. }) {
                return Some(Value::Undef);
            }
            Value::Frame {
                origin: Box::new(origin.clone()),
                basis: Box::new(basis.clone()),
            }
        }

        // --- Transform constructors ---
        "transform3" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let rotation = &args[0];
            let translation = &args[1];
            if !matches!(rotation, Value::Orientation { .. }) {
                return Some(Value::Undef);
            }
            match translation {
                Value::Vector(components) if components.len() == 3 => {}
                _ => return Some(Value::Undef),
            }
            Value::Transform {
                rotation: Box::new(rotation.clone()),
                translation: Box::new(translation.clone()),
            }
        }
        "transform3_identity" => {
            if args.is_empty() {
                Value::Transform {
                    rotation: Box::new(Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }),
                    translation: Box::new(Value::Vector(vec![
                        Value::length(0.0),
                        Value::length(0.0),
                        Value::length(0.0),
                    ])),
                }
            } else {
                Value::Undef
            }
        }

        // --- Affine map constructors ---
        // `Value::AffineMap` is a general 3D affine map x ↦ linear·x + translation,
        // where `linear` is a dimensionless row-major 3×3 and `translation` carries
        // Length (SI meters). All arms follow the transform3 convention: bad arity /
        // types / dimensions return `Value::Undef`.
        "affine_identity" => {
            if args.is_empty() {
                Value::AffineMap {
                    linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    translation: [0.0, 0.0, 0.0],
                }
            } else {
                Value::Undef
            }
        }
        "affine_scale" => {
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            // Each factor must be dimensionless (G6 dimensionless-linear-part
            // contract), numeric, finite, and non-zero. Negative factors are valid
            // orientation-reversing reflections (det<0); a zero factor is degenerate
            // (det=0, non-invertible) and rejected.
            let mut factors = [0.0_f64; 3];
            for (i, arg) in args.iter().enumerate() {
                if !arg.dimension().is_dimensionless() {
                    return Some(Value::Undef);
                }
                match arg.as_f64() {
                    Some(v) if v.is_finite() && v != 0.0 => factors[i] = v,
                    _ => return Some(Value::Undef),
                }
            }
            Value::AffineMap {
                linear: [
                    [factors[0], 0.0, 0.0],
                    [0.0, factors[1], 0.0],
                    [0.0, 0.0, factors[2]],
                ],
                translation: [0.0, 0.0, 0.0],
            }
        }
        // `affine_shear_AB(k)` sets the single off-diagonal cell `linear[A][B] = k`
        // (output axis A receives += k·input axis B), e.g. `affine_shear_xy` →
        // `linear[0][1] = k` (x' = x + k·y). The diagonal stays 1, so det = 1
        // (volume-preserving). Exactly one dimensionless, finite scalar argument;
        // otherwise `Value::Undef`.
        "affine_shear_xy" | "affine_shear_xz" | "affine_shear_yx" | "affine_shear_yz"
        | "affine_shear_zx" | "affine_shear_zy" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            if !args[0].dimension().is_dimensionless() {
                return Some(Value::Undef);
            }
            let k = match args[0].as_f64() {
                Some(v) if v.is_finite() => v,
                _ => return Some(Value::Undef),
            };
            let (row, col) = match name {
                "affine_shear_xy" => (0, 1),
                "affine_shear_xz" => (0, 2),
                "affine_shear_yx" => (1, 0),
                "affine_shear_yz" => (1, 2),
                "affine_shear_zx" => (2, 0),
                "affine_shear_zy" => (2, 1),
                _ => unreachable!(),
            };
            let mut linear = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            linear[row][col] = k;
            Value::AffineMap {
                linear,
                translation: [0.0, 0.0, 0.0],
            }
        }
        // `affine_translate(dx, dy, dz)`: identity linear part with the three
        // components stored as the translation in SI units (meters for Length).
        // Requires exactly three numeric, finite components sharing one dimension
        // (decompose_xyz3 contract); otherwise `Value::Undef`.
        "affine_translate" => {
            let (t, _dim) = match decompose_xyz3(args) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            Value::AffineMap {
                linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: t,
            }
        }

        // --- Transform operations ---
        "frame_to_frame" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (origin_from, basis_from) = match &args[0] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            let (origin_to, basis_to) = match &args[1] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            let q_from = match basis_from {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let q_to = match basis_to {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let ([fx, fy, fz], f_dim) = match decompose_point3(origin_from) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let ([tx, ty, tz], t_dim) = match decompose_point3(origin_to) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // R = R_to * conj(R_from)
            let r = quat_mul(q_to, quat_conj(q_from));
            match normalize_quaternion(r.0, r.1, r.2, r.3) {
                Some(rot_val) => {
                    // t = origin_to - R * origin_from
                    if f_dim != t_dim {
                        return Some(Value::Undef);
                    }
                    let dim = f_dim;
                    let r_norm = match &rot_val {
                        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                        _ => unreachable!(),
                    };
                    let (rfx, rfy, rfz) = quat_rotate(r_norm, fx, fy, fz);
                    let trans = Value::Vector(vec![
                        Value::Scalar {
                            si_value: tx - rfx,
                            dimension: dim,
                        },
                        Value::Scalar {
                            si_value: ty - rfy,
                            dimension: dim,
                        },
                        Value::Scalar {
                            si_value: tz - rfz,
                            dimension: dim,
                        },
                    ]);
                    Value::Transform {
                        rotation: Box::new(rot_val),
                        translation: Box::new(trans),
                    }
                }
                None => Value::Undef,
            }
        }

        "transform_exp" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let angular_val = match map.get(&Value::String("angular".to_string())) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let linear_val = match map.get(&Value::String("linear".to_string())) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Extract angular: must be Vector3<DIMENSIONLESS>.
            let (ang_comps, ang_dim) = match decompose_vec3(angular_val) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            if ang_dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let (wx, wy, wz) = (ang_comps[0], ang_comps[1], ang_comps[2]);
            // Extract linear: must be Vector3 with a single shared dimension.
            //
            // Twist linear convention (polymorphic, mirrored on transform_log):
            //   • LENGTH       — canonical (matches Twist type in the doc reference)
            //   • DIMENSIONLESS — accepted for unit-less twists / numerical work
            //   • Any other dim (ANGLE, MASS, …) → rejected as Undef
            //
            // transform_log applies the identical LENGTH|DIMENSIONLESS gate and
            // preserves the dimension on output, so the log↔exp round-trip is
            // symmetric on both accept and reject.
            let (lin_comps, lin_dim) = match decompose_vec3(linear_val) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            if lin_dim != DimensionVector::LENGTH && lin_dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let (lx, ly, lz) = (lin_comps[0], lin_comps[1], lin_comps[2]);
            // Compute R = orient_exp(angular).
            let theta_sq = wx * wx + wy * wy + wz * wz;
            let theta = theta_sq.sqrt();
            const EPS: f64 = 1e-12;
            let r_val = if theta < EPS {
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }
            } else {
                let half = theta / 2.0;
                let s = half.sin() / theta;
                match normalize_quaternion(half.cos(), s * wx, s * wy, s * wz) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                }
            };
            // Compute V * linear, where V is the SE(3) left Jacobian:
            //   V = I + ((1−cos|ω|)/|ω|²) [ω]× + ((|ω|−sin|ω|)/|ω|³) [ω]×²
            // For |ω| ≈ 0, use Taylor: V ≈ I + 0.5*[ω]× + (1/6)*[ω]×² + ...
            let (a_coef, b_coef) = if theta < 1.0e-4 {
                // Taylor series:
                //   (1 − cos|ω|)/|ω|² ≈ 1/2 − |ω|²/24 + |ω|⁴/720 − ...
                //   (|ω| − sin|ω|)/|ω|³ ≈ 1/6 − |ω|²/120 + ...
                (0.5 - theta_sq / 24.0, 1.0 / 6.0 - theta_sq / 120.0)
            } else {
                (
                    (1.0 - theta.cos()) / theta_sq,
                    (theta - theta.sin()) / (theta_sq * theta),
                )
            };
            // [ω]× linear = ω × linear.
            let cx = wy * lz - wz * ly;
            let cy = wz * lx - wx * lz;
            let cz = wx * ly - wy * lx;
            // [ω]×² linear = ω × (ω × linear).
            let ccx = wy * cz - wz * cy;
            let ccy = wz * cx - wx * cz;
            let ccz = wx * cy - wy * cx;
            let tx = lx + a_coef * cx + b_coef * ccx;
            let ty = ly + a_coef * cy + b_coef * ccy;
            let tz = lz + a_coef * cz + b_coef * ccz;
            if !tx.is_finite() || !ty.is_finite() || !tz.is_finite() {
                return Some(Value::Undef);
            }
            Value::Transform {
                rotation: Box::new(r_val),
                translation: Box::new(Value::Vector(vec![
                    make_dimensioned_component(lin_dim, tx),
                    make_dimensioned_component(lin_dim, ty),
                    make_dimensioned_component(lin_dim, tz),
                ])),
            }
        }

        "transform_log" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (r_q, t, t_dim) = match decompose_transform(&args[0]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Transform translation convention (polymorphic, mirrored on transform_exp):
            //   • LENGTH        — canonical (matches Transform type in the doc reference)
            //   • DIMENSIONLESS — accepted for unit-less transforms / numerical work
            //   • Any other dim (ANGLE, MASS, …) → rejected as Undef
            //
            // transform_exp applies the identical LENGTH|DIMENSIONLESS gate on the
            // twist linear field and preserves the dimension on output, so the
            // log↔exp round-trip is symmetric on both accept and reject.
            if t_dim != DimensionVector::LENGTH && t_dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let (tx, ty, tz) = (t[0], t[1], t[2]);
            // Compute angular = orient_log(R): rotation vector ω.
            let (rw, rx, ry, rz) = r_q;
            // Normalize quaternion first (1e-24 gate — see normalize_quat_input).
            let (nw, nx, ny, nz) = match normalize_quat_input((rw, rx, ry, rz)) {
                Some(q) => q,
                None => return Some(Value::Undef),
            };
            // Canonicalize quaternion sign: q and -q represent the same SO(3)
            // rotation. Flipping when nw < 0 ensures the small-angle Taylor
            // branch always sees nw ≈ +1 (so ω = +2*(nx,ny,nz) for q≈identity,
            // not −2*(nx,ny,nz) for q≈−identity). The general atan2 branch
            // still produces the correct magnitude either way, but the sign
            // of the rotation axis matches the canonical hemisphere only
            // after this flip — so we apply it for both branches.
            let (nw, nx, ny, nz) = if nw < 0.0 {
                (-nw, -nx, -ny, -nz)
            } else {
                (nw, nx, ny, nz)
            };
            let v_norm = (nx * nx + ny * ny + nz * nz).sqrt();
            const EPS: f64 = 1e-12;
            let (wx, wy, wz) = if v_norm < EPS {
                // Near-identity → ω ≈ 2*(x,y,z) (Taylor leading order).
                (2.0 * nx, 2.0 * ny, 2.0 * nz)
            } else {
                let angle = 2.0 * v_norm.atan2(nw);
                let scale = angle / v_norm;
                (scale * nx, scale * ny, scale * nz)
            };
            if !wx.is_finite() || !wy.is_finite() || !wz.is_finite() {
                return Some(Value::Undef);
            }
            // Compute V_inv * t where V is the SE(3) left Jacobian.
            // |ω|² is the squared magnitude of the rotation vector.
            let theta_sq = wx * wx + wy * wy + wz * wz;
            let theta = theta_sq.sqrt();
            // Apply V_inv = I − 0.5*[ω]× + α*[ω]×², where
            //   α = 1/|ω|² − cot(|ω|/2)/(2|ω|).
            // For small |ω|, use Taylor: α ≈ 1/12 + |ω|²/720 + ...
            // Use the small-angle Taylor when theta < ~1e-4 to keep FP accurate.
            let alpha = if theta < 1.0e-4 {
                1.0 / 12.0 + theta_sq / 720.0
            } else {
                let half = theta / 2.0;
                let cot_half = half.cos() / half.sin();
                1.0 / theta_sq - cot_half / (2.0 * theta)
            };
            // [ω]× t = ω × t (cross product).
            let cx = wy * tz - wz * ty;
            let cy = wz * tx - wx * tz;
            let cz = wx * ty - wy * tx;
            // [ω]×² t = ω × (ω × t).
            let ccx = wy * cz - wz * cy;
            let ccy = wz * cx - wx * cz;
            let ccz = wx * cy - wy * cx;
            let lx = tx - 0.5 * cx + alpha * ccx;
            let ly = ty - 0.5 * cy + alpha * ccy;
            let lz = tz - 0.5 * cz + alpha * ccz;
            if !lx.is_finite() || !ly.is_finite() || !lz.is_finite() {
                return Some(Value::Undef);
            }
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("angular".to_string()),
                Value::Vector(vec![Value::Real(wx), Value::Real(wy), Value::Real(wz)]),
            );
            m.insert(
                Value::String("linear".to_string()),
                Value::Vector(vec![
                    make_dimensioned_component(t_dim, lx),
                    make_dimensioned_component(t_dim, ly),
                    make_dimensioned_component(t_dim, lz),
                ]),
            );
            Value::Map(m)
        }

        "transform_inverse" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (r_q, t, t_dim) = match decompose_transform(&args[0]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Normalize R first (1e-24 gate — see normalize_quat_input).
            let r_n = match normalize_quat_input(r_q) {
                Some(q) => q,
                None => return Some(Value::Undef),
            };
            // Inverse rotation = conjugate (for unit quaternion).
            // r_n is guaranteed unit by normalize_quat_input; quat_conj of a
            // unit quaternion is unit, so no renormalize is needed here.
            let r_inv = quat_conj(r_n);
            debug_assert!(quaternion_is_finite(r_inv.0, r_inv.1, r_inv.2, r_inv.3));
            let r_inv_val = Value::Orientation {
                w: r_inv.0,
                x: r_inv.1,
                y: r_inv.2,
                z: r_inv.3,
            };
            // Inverse translation: t_inv = -R^-1 * t.
            let (rtx, rty, rtz) = quat_rotate(r_inv, t[0], t[1], t[2]);
            Value::Transform {
                rotation: Box::new(r_inv_val),
                translation: Box::new(Value::Vector(vec![
                    make_dimensioned_component(t_dim, -rtx),
                    make_dimensioned_component(t_dim, -rty),
                    make_dimensioned_component(t_dim, -rtz),
                ])),
            }
        }

        "transform_compose" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (r1_q, t1, t1_dim) = match decompose_transform(&args[0]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let (r2_q, t2, t2_dim) = match decompose_transform(&args[1]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            if t1_dim != t2_dim {
                return Some(Value::Undef);
            }
            // Normalize R1 and R2 symmetrically (matches operator-level semantics in reify-expr;
            // 1e-24 gate — see normalize_quat_input).
            let r1_n = match normalize_quat_input(r1_q) {
                Some(q) => q,
                None => return Some(Value::Undef),
            };
            let r2_n = match normalize_quat_input(r2_q) {
                Some(q) => q,
                None => return Some(Value::Undef),
            };
            // R = R1 * R2 (Hamilton product). r1_n and r2_n are unit by construction;
            // quat_mul of unit quaternions is unit (modulo FP rounding).
            let composed_r = quat_mul(r1_n, r2_n);
            debug_assert!(quaternion_is_finite(
                composed_r.0,
                composed_r.1,
                composed_r.2,
                composed_r.3
            ));
            let r_val = Value::Orientation {
                w: composed_r.0,
                x: composed_r.1,
                y: composed_r.2,
                z: composed_r.3,
            };
            // t = R1 * t2 + t1.
            let (rt2x, rt2y, rt2z) = quat_rotate(r1_n, t2[0], t2[1], t2[2]);
            Value::Transform {
                rotation: Box::new(r_val),
                translation: Box::new(Value::Vector(vec![
                    make_dimensioned_component(t1_dim, rt2x + t1[0]),
                    make_dimensioned_component(t1_dim, rt2y + t1[1]),
                    make_dimensioned_component(t1_dim, rt2z + t1[2]),
                ])),
            }
        }

        // --- Plane constructors ---
        "plane_xy" => make_plane(args, 2, [0.0, 0.0, 1.0]),
        "plane_xz" => make_plane(args, 1, [0.0, 1.0, 0.0]),
        "plane_yz" => make_plane(args, 0, [1.0, 0.0, 0.0]),

        // --- Axis constructors ---
        "axis_x" => make_axis(args, [1.0, 0.0, 0.0]),
        "axis_y" => make_axis(args, [0.0, 1.0, 0.0]),
        "axis_z" => make_axis(args, [0.0, 0.0, 1.0]),

        // --- BoundingBox constructors ---
        "bbox" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let min = &args[0];
            let max = &args[1];
            let min_comps = match min {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
            };
            let max_comps = match max {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
            };
            let min_dim = min_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            let max_dim = max_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            if min_dim != max_dim {
                return Some(Value::Undef);
            }
            Value::BoundingBox {
                min: Box::new(min.clone()),
                max: Box::new(max.clone()),
            }
        }

        // --- BoundingBox accessors ---
        "bbox_size" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Some(Value::Undef);
                    }
                    let make_component = |v: f64| -> Value {
                        if dim.is_dimensionless() {
                            Value::Real(v)
                        } else {
                            Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            }
                        }
                    };
                    Value::Vector(vec![
                        make_component(max_vals[0] - min_vals[0]),
                        make_component(max_vals[1] - min_vals[1]),
                        make_component(max_vals[2] - min_vals[2]),
                    ])
                }
                _ => Value::Undef,
            }
        }
        "bbox_center" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Some(Value::Undef);
                    }
                    let make_component = |v: f64| -> Value {
                        if dim.is_dimensionless() {
                            Value::Real(v)
                        } else {
                            Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            }
                        }
                    };
                    Value::Point(vec![
                        make_component((min_vals[0] + max_vals[0]) / 2.0),
                        make_component((min_vals[1] + max_vals[1]) / 2.0),
                        make_component((min_vals[2] + max_vals[2]) / 2.0),
                    ])
                }
                _ => Value::Undef,
            }
        }

        // --- Point/Vector constructors ---
        "point2" => construct_point_or_vector(args, 2, true),
        "point3" => construct_point_or_vector(args, 3, true),
        "vec2" => construct_point_or_vector(args, 2, false),
        "vec3" => construct_point_or_vector(args, 3, false),

        // --- Field operations (stubs) ---
        "sample" => Value::Undef,
        "gradient" => Value::Undef,
        "divergence" => Value::Undef,
        "curl" => Value::Undef,

        // --- Frame projection ---
        // project(point: Point3<L>, to: Frame<3>) -> Point3<L>
        //   = inverse(basis) · (point − origin)
        // project(vector: Vector3<L>, to: Frame<3>) -> Vector3<L>
        //   = inverse(basis) · vector   (translation-invariant; no origin subtraction)
        "project" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            // Decode the second argument as a Frame.
            let (origin, basis_val) = match &args[1] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            // Extract basis quaternion components.
            let (bw, bx, by, bz) = match basis_val {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            // Compute inverse basis: normalize (1e-24 gate) then conjugate.
            let q_inv = match normalize_quat_input((bw, bx, by, bz)) {
                Some(qn) => quat_conj(qn),
                None => return Some(Value::Undef),
            };
            // Dispatch on the first argument type.
            match &args[0] {
                Value::Point(_) => {
                    let (p, p_dim) = match decompose_point3(&args[0]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let (o, o_dim) = match decompose_point3(origin) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    // Subtracting across different dimensions is meaningless
                    // (mirrors frame_to_frame's f_dim != t_dim guard, geometry.rs:279-281).
                    if p_dim != o_dim {
                        return Some(Value::Undef);
                    }
                    // Translate then inverse-rotate.
                    let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
                    let (rx, ry, rz) = quat_rotate(q_inv, d[0], d[1], d[2]);
                    if !rx.is_finite() || !ry.is_finite() || !rz.is_finite() {
                        return Some(Value::Undef);
                    }
                    Value::Point(vec![
                        make_dimensioned_component(p_dim, rx),
                        make_dimensioned_component(p_dim, ry),
                        make_dimensioned_component(p_dim, rz),
                    ])
                }
                Value::Vector(_) => {
                    // Vectors are translation-invariant: inverse-rotate only, no origin subtraction.
                    let (v, v_dim) = match decompose_vec3(&args[0]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    };
                    let (rx, ry, rz) = quat_rotate(q_inv, v[0], v[1], v[2]);
                    if !rx.is_finite() || !ry.is_finite() || !rz.is_finite() {
                        return Some(Value::Undef);
                    }
                    Value::Vector(vec![
                        make_dimensioned_component(v_dim, rx),
                        make_dimensioned_component(v_dim, ry),
                        make_dimensioned_component(v_dim, rz),
                    ])
                }
                _ => Value::Undef,
            }
        }

        _ => return None,
    })
}

/// Validate args for a point/vector constructor and return `Value::Point` or `Value::Vector`.
fn construct_point_or_vector(args: &[Value], expected_n: usize, is_point: bool) -> Value {
    if args.len() != expected_n {
        return Value::Undef;
    }
    if !args.iter().all(|a| a.as_f64().is_some()) {
        return Value::Undef;
    }
    let first_dim = match args.first() {
        Some(v) => v.dimension(),
        None => return Value::Undef,
    };
    if !args.iter().all(|a| a.dimension() == first_dim) {
        return Value::Undef;
    }
    if is_point {
        Value::Point(args.to_vec())
    } else {
        Value::Vector(args.to_vec())
    }
}

/// Build a Plane from a single offset argument.
fn make_plane(args: &[Value], offset_index: usize, normal: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let offset_val = &args[0];
    let offset_f = match offset_val.as_f64() {
        Some(v) => v,
        None => return Value::Undef,
    };
    if !offset_f.is_finite() {
        return Value::Undef;
    }
    let dim = offset_val.dimension();
    let make_zero = || -> Value {
        if dim.is_dimensionless() {
            Value::Real(0.0)
        } else {
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            }
        }
    };
    let offset_component = offset_val.clone();
    let zero = make_zero();
    let mut comps = [zero.clone(), zero.clone(), zero];
    comps[offset_index] = offset_component;
    let origin = Value::Point(comps.to_vec());
    let normal_vec = Value::Vector(vec![
        Value::Real(normal[0]),
        Value::Real(normal[1]),
        Value::Real(normal[2]),
    ]);
    Value::Plane {
        origin: Box::new(origin),
        normal: Box::new(normal_vec),
    }
}

/// Build an Axis from a single Point3 origin argument.
fn make_axis(args: &[Value], direction: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match &args[0] {
        Value::Point(comps) if comps.len() == 3 => {}
        _ => return Value::Undef,
    }
    let dir_vec = Value::Vector(vec![
        Value::Real(direction[0]),
        Value::Real(direction[1]),
        Value::Real(direction[2]),
    ]);
    Value::Axis {
        origin: Box::new(args[0].clone()),
        direction: Box::new(dir_vec),
    }
}

// Quaternion helpers used by frame_to_frame — re-imported from orientation module.
use crate::orientation::{normalize_quaternion, quat_conj, quat_mul, quat_rotate};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::construct_point_or_vector;
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "determined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "undetermined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "constrained stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "partially_determined stub should return Undef, got {:?}",
            result
        );
    }

    // --- Field operation stubs (step-25) ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        // gradient(field) on a scalar field should return Undef (stub).
        let field = Value::Field {
            domain_type: reify_core::Type::StructureRef("Point3".into()),
            codomain_type: reify_core::Type::length(),
            source: reify_ir::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(
            result.is_undef(),
            "gradient stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_core::Type::StructureRef("Point3".into()),
            codomain_type: reify_core::Type::StructureRef("Vector3".into()),
            source: reify_ir::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(
            result.is_undef(),
            "divergence stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_core::Type::StructureRef("Point3".into()),
            codomain_type: reify_core::Type::StructureRef("Vector3".into()),
            source: reify_ir::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(
            result.is_undef(),
            "curl stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: reify_core::Type::StructureRef("Point3".into()),
            codomain_type: reify_core::Type::length(),
            source: reify_ir::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
    }

    // --- non-numeric args → Undef ---

    #[test]
    fn point3_non_numeric_undef() {
        // point3(String, Scalar, Scalar) → Undef
        let args = vec![
            Value::String("hello".to_string()),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args).is_undef(),
            "non-numeric first arg must return Undef"
        );
    }

    #[test]
    fn vec2_non_numeric_undef() {
        // vec2(Bool, Bool) → Undef
        let args = vec![Value::Bool(true), Value::Bool(false)];
        assert!(
            eval_builtin("vec2", &args).is_undef(),
            "Bool args must return Undef"
        );
    }

    // --- wrong arg count → Undef ---

    #[test]
    fn point3_wrong_arg_count_undef() {
        // point3 with 2 args → Undef
        let args2 = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args2).is_undef(),
            "point3 with 2 args must be Undef"
        );
        // point3 with 0 args → Undef
        assert!(
            eval_builtin("point3", &[]).is_undef(),
            "point3 with 0 args must be Undef"
        );
        // point3 with 4 args → Undef
        let args4 = vec![
            Value::Real(1.0),
            Value::Real(2.0),
            Value::Real(3.0),
            Value::Real(4.0),
        ];
        assert!(
            eval_builtin("point3", &args4).is_undef(),
            "point3 with 4 args must be Undef"
        );
    }

    #[test]
    fn point2_wrong_arg_count_undef() {
        // point2 with 3 args → Undef
        let args3 = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        assert!(
            eval_builtin("point2", &args3).is_undef(),
            "point2 with 3 args must be Undef"
        );
        // point2 with 1 arg → Undef
        assert!(
            eval_builtin("point2", &[Value::Real(1.0)]).is_undef(),
            "point2 with 1 arg must be Undef"
        );
    }

    #[test]
    fn vec3_wrong_arg_count_undef() {
        assert!(
            eval_builtin("vec3", &[]).is_undef(),
            "vec3 with 0 args must be Undef"
        );
        let args2 = vec![Value::Real(1.0), Value::Real(2.0)];
        assert!(
            eval_builtin("vec3", &args2).is_undef(),
            "vec3 with 2 args must be Undef"
        );
    }

    #[test]
    fn vec2_wrong_arg_count_undef() {
        assert!(
            eval_builtin("vec2", &[]).is_undef(),
            "vec2 with 0 args must be Undef"
        );
        let args3 = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        assert!(
            eval_builtin("vec2", &args3).is_undef(),
            "vec2 with 3 args must be Undef"
        );
    }

    // --- dimension mismatch → Undef ---

    #[test]
    fn point3_dimension_mismatch_undef() {
        // point3(Scalar(1,LENGTH), Scalar(2,MASS), Scalar(3,LENGTH)) → Undef
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn vec3_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::MASS,
            },
        ];
        assert!(
            eval_builtin("vec3", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn point2_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
        ];
        assert!(
            eval_builtin("point2", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn vec2_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("vec2", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    // --- dimensionless components ---

    #[test]
    fn point3_dimensionless() {
        // point3(Real(1.0), Real(2.0), Real(3.0)) → Value::Point with Real components preserved
        let args = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        let result = eval_builtin("point3", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(&items[0], Value::Real(v) if (*v - 1.0).abs() < 1e-12));
                assert!(matches!(&items[1], Value::Real(v) if (*v - 2.0).abs() < 1e-12));
                assert!(matches!(&items[2], Value::Real(v) if (*v - 3.0).abs() < 1e-12));
            }
            other => panic!("expected Point with Real components, got {:?}", other),
        }
    }

    // --- vec2 ---

    #[test]
    fn vec2_basic() {
        // vec2(9.0, 10.0) → Value::Vector([Real(9.0), Real(10.0)])
        let args = vec![Value::Real(9.0), Value::Real(10.0)];
        let result = eval_builtin("vec2", &args);
        match result {
            Value::Vector(ref items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Value::Real(v) if (*v - 9.0).abs() < 1e-12));
                assert!(matches!(&items[1], Value::Real(v) if (*v - 10.0).abs() < 1e-12));
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    // --- point2 ---

    #[test]
    fn point2_basic() {
        // point2(7m, 8m) → Value::Point([Scalar(7,L), Scalar(8,L)])
        let args = vec![
            Value::Scalar {
                si_value: 7.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 8.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("point2", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 2);
                assert_scalar_approx!(items[0].clone(), 7.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 8.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    // --- vec3 ---

    #[test]
    fn vec3_basic() {
        // vec3(4m, 5m, 6m) → Value::Vector([Scalar(4,L), Scalar(5,L), Scalar(6,L)])
        let args = vec![
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 6.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("vec3", &args);
        match result {
            Value::Vector(ref items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 4.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 5.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[2].clone(), 6.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    // --- point3 ---

    #[test]
    fn point3_basic() {
        // point3(1m, 2m, 3m) → Value::Point([Scalar(1,L), Scalar(2,L), Scalar(3,L)])
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("point3", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 1.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 2.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[2].clone(), 3.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    // --- Semantic distinction: point vs vector ---

    #[test]
    fn point_vector_semantic_distinction() {
        // point2 and vec2 with identical args must produce distinct Value variants
        let a = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        };

        let p2 = eval_builtin("point2", &[a.clone(), b.clone()]);
        let v2 = eval_builtin("vec2", &[a.clone(), b.clone()]);

        // point2 must produce Value::Point
        assert!(
            matches!(&p2, Value::Point(items) if items.len() == 2),
            "expected Value::Point(2), got {:?}",
            p2
        );

        // vec2 must produce Value::Vector
        assert!(
            matches!(&v2, Value::Vector(items) if items.len() == 2),
            "expected Value::Vector(2), got {:?}",
            v2
        );

        // point2(a,b) != vec2(a,b) — different variants
        assert_ne!(p2, v2, "point2 and vec2 with identical args must differ");

        // point3 vs vec3
        let c = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        let p3 = eval_builtin("point3", &[a.clone(), b.clone(), c.clone()]);
        let v3 = eval_builtin("vec3", &[a.clone(), b.clone(), c.clone()]);

        assert!(
            matches!(&p3, Value::Point(items) if items.len() == 3),
            "expected Value::Point(3), got {:?}",
            p3
        );
        assert!(
            matches!(&v3, Value::Vector(items) if items.len() == 3),
            "expected Value::Vector(3), got {:?}",
            v3
        );
        assert_ne!(p3, v3, "point3 and vec3 with identical args must differ");

        // content_hash: Point and Vector with same components produce different hashes
        assert_ne!(
            p2.content_hash(),
            v2.content_hash(),
            "point2 and vec2 content_hash must differ"
        );
        assert_ne!(
            p3.content_hash(),
            v3.content_hash(),
            "point3 and vec3 content_hash must differ"
        );

        // Display: point(...) vs vec(...)
        let p2_display = format!("{}", p2);
        let v2_display = format!("{}", v2);
        assert!(
            p2_display.starts_with("point("),
            "Point2 Display should start with 'point(', got {:?}",
            p2_display
        );
        assert!(
            v2_display.starts_with("vec("),
            "Vector2 Display should start with 'vec(', got {:?}",
            v2_display
        );
    }

    // ── construct_point_or_vector edge cases (task 398, step-11) ──────────────

    #[test]
    fn construct_point_or_vector_empty_args_returns_undef() {
        // When expected_n=0 and args=[], should return Undef, not panic.
        let result = construct_point_or_vector(&[], 0, true);
        assert!(
            result.is_undef(),
            "expected Undef for empty args with expected_n=0, got {:?}",
            result
        );

        let result = construct_point_or_vector(&[], 0, false);
        assert!(
            result.is_undef(),
            "expected Undef for empty vector args with expected_n=0, got {:?}",
            result
        );
    }

    // ── frame3 tests (step-5) ────────────────────────────────────────────────

    fn make_point3_len() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_identity_orientation() -> Value {
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    #[test]
    fn frame3_valid_args_returns_frame() {
        let origin = make_point3_len();
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*b, basis);
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_stores_origin_and_basis_correctly() {
        let origin = Value::Point(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let basis = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin, "origin should be stored exactly");
                assert_eq!(*b, basis, "basis should be stored exactly");
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_no_args_returns_undef() {
        assert!(eval_builtin("frame3", &[]).is_undef());
    }

    #[test]
    fn frame3_one_arg_returns_undef() {
        assert!(eval_builtin("frame3", &[make_point3_len()]).is_undef());
    }

    #[test]
    fn frame3_three_args_returns_undef() {
        let o = make_point3_len();
        let b = make_identity_orientation();
        assert!(eval_builtin("frame3", &[o.clone(), b.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn frame3_non_point_first_arg_returns_undef() {
        let basis = make_identity_orientation();
        // First arg is Real, not Point
        assert!(eval_builtin("frame3", &[Value::Real(1.0), basis]).is_undef());
    }

    #[test]
    fn frame3_non_orientation_second_arg_returns_undef() {
        let origin = make_point3_len();
        // Second arg is Real, not Orientation
        assert!(eval_builtin("frame3", &[origin, Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn frame3_point2_origin_returns_undef() {
        // Point2 (wrong component count) should be rejected
        let origin_2d = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_2d, basis]).is_undef());
    }

    #[test]
    fn frame3_point4_origin_returns_undef() {
        // Point4 (wrong component count) should be rejected
        let origin_4d = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0),
        ]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_4d, basis]).is_undef());
    }

    #[test]
    fn frame3_dimensionless_point3_is_accepted() {
        // Point3 with dimensionless (Real) components is accepted
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame for dimensionless Point3 origin, got {:?}",
            result
        );
    }

    // ── frame3_identity tests (step-7) ────────────────────────────────────────

    #[test]
    fn frame3_identity_no_args_returns_frame() {
        let result = eval_builtin("frame3_identity", &[]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame, got {:?}",
            result
        );
    }

    #[test]
    fn frame3_identity_origin_is_zero_length_point3() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { origin, .. } => {
                let expected_origin = Value::Point(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *origin, expected_origin,
                    "identity origin should be zero Point3<Length>"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_basis_is_identity_quaternion() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { basis, .. } => {
                let expected_basis = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *basis, expected_basis,
                    "identity basis should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
        assert!(
            eval_builtin(
                "frame3_identity",
                &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]
            )
            .is_undef()
        );
        assert!(
            eval_builtin(
                "frame3_identity",
                &[
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0)
                ]
            )
            .is_undef()
        );
    }

    // ── transform3 tests (step-5) ─────────────────────────────────────────────

    fn make_vec3_length() -> Value {
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    #[test]
    fn transform3_valid_args_returns_transform() {
        let rotation = make_identity_orientation();
        let translation = make_vec3_length();
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation);
                assert_eq!(*t, translation);
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_stores_rotation_and_translation_correctly() {
        let rotation = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = Value::Vector(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation, "rotation should be stored exactly");
                assert_eq!(*t, translation, "translation should be stored exactly");
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_no_args_returns_undef() {
        assert!(eval_builtin("transform3", &[]).is_undef());
    }

    #[test]
    fn transform3_one_arg_returns_undef() {
        assert!(eval_builtin("transform3", &[make_identity_orientation()]).is_undef());
    }

    #[test]
    fn transform3_three_args_returns_undef() {
        let r = make_identity_orientation();
        let t = make_vec3_length();
        assert!(eval_builtin("transform3", &[r.clone(), t.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn transform3_non_orientation_first_arg_returns_undef() {
        // First arg is Real, not Orientation
        assert!(eval_builtin("transform3", &[Value::Real(1.0), make_vec3_length()]).is_undef());
    }

    #[test]
    fn transform3_non_vector_second_arg_returns_undef() {
        // Second arg is Real, not Vector
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), Value::Real(1.0)]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_point3_second_arg_returns_undef() {
        // Second arg is Point3, not Vector3
        let pt3 = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), pt3]).is_undef());
    }

    #[test]
    fn transform3_orientation_second_arg_returns_undef() {
        // Second arg is Orientation, not Vector3
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), make_identity_orientation()]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_vector2_translation_returns_undef() {
        // Vector2 (wrong component count) should be rejected
        let vec2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), vec2]).is_undef());
    }

    #[test]
    fn transform3_dimensionless_vector3_is_accepted() {
        // Vector3 with dimensionless (Real) components is accepted
        let translation = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin(
            "transform3",
            &[make_identity_orientation(), translation.clone()],
        );
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform for dimensionless Vector3 translation, got {:?}",
            result
        );
    }

    // ── transform3_identity tests (step-7) ────────────────────────────────────

    #[test]
    fn transform3_identity_no_args_returns_transform() {
        let result = eval_builtin("transform3_identity", &[]);
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform, got {:?}",
            result
        );
    }

    #[test]
    fn transform3_identity_rotation_is_identity_quaternion() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { rotation, .. } => {
                let expected = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *rotation, expected,
                    "identity rotation should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_translation_is_zero_length_vector3() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { translation, .. } => {
                let expected = Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *translation, expected,
                    "identity translation should be zero Vector3<Length>"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("transform3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(
            eval_builtin("transform3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef()
        );
    }

    // ── axis_z tests (step-5) ────────────────────────────────────────────────

    fn make_point3_length() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point2_length() -> Value {
        Value::Point(vec![Value::length(1.0), Value::length(2.0)])
    }

    #[test]
    fn axis_z_with_point3_returns_axis() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        assert!(
            matches!(result, Value::Axis { .. }),
            "expected Value::Axis, got {:?}",
            result
        );
    }

    #[test]
    fn axis_z_stores_origin_correctly() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_direction_is_z() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(1.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_no_args_returns_undef() {
        assert!(eval_builtin("axis_z", &[]).is_undef());
    }

    #[test]
    fn axis_z_real_arg_returns_undef() {
        assert!(eval_builtin("axis_z", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn axis_z_point2_returns_undef() {
        assert!(eval_builtin("axis_z", &[make_point2_length()]).is_undef());
    }

    #[test]
    fn axis_z_vector3_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("axis_z", &[vec3]).is_undef());
    }

    // ── axis_x / axis_y tests (step-7) ───────────────────────────────────────

    #[test]
    fn axis_x_direction_is_x() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_x", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(1.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_y_direction_is_y() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_y", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(1.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_x_no_args_returns_undef() {
        assert!(eval_builtin("axis_x", &[]).is_undef());
    }

    #[test]
    fn axis_y_two_args_returns_undef() {
        assert!(eval_builtin("axis_y", &[make_point3_length(), make_point3_length()]).is_undef());
    }

    #[test]
    fn axis_x_with_dimensionless_point3() {
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin("axis_x", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    // ── bbox tests (step-9) ──────────────────────────────────────────────────

    fn make_point3_min() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point3_max() -> Value {
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ])
    }

    #[test]
    fn bbox_with_two_point3_returns_bounding_box() {
        let result = eval_builtin("bbox", &[make_point3_min(), make_point3_max()]);
        assert!(
            matches!(result, Value::BoundingBox { .. }),
            "expected BoundingBox, got {:?}",
            result
        );
    }

    #[test]
    fn bbox_stores_min_and_max() {
        let min = make_point3_min();
        let max = make_point3_max();
        let result = eval_builtin("bbox", &[min.clone(), max.clone()]);
        match result {
            Value::BoundingBox { min: mn, max: mx } => {
                assert_eq!(*mn, min);
                assert_eq!(*mx, max);
            }
            other => panic!("expected BoundingBox, got {:?}", other),
        }
    }

    #[test]
    fn bbox_mismatched_dimensions_returns_undef() {
        let min = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let max = Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        assert!(eval_builtin("bbox", &[min, max]).is_undef());
    }

    #[test]
    fn bbox_non_point_arg_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[vec3, pt3]).is_undef());
    }

    #[test]
    fn bbox_point2_returns_undef() {
        let pt2 = make_point2_length();
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[pt2, pt3]).is_undef());
    }

    #[test]
    fn bbox_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox", &[]).is_undef());
        assert!(eval_builtin("bbox", &[make_point3_min()]).is_undef());
        assert!(
            eval_builtin(
                "bbox",
                &[make_point3_min(), make_point3_max(), make_point3_min()]
            )
            .is_undef()
        );
    }

    #[test]
    fn bbox_one_point_one_vector_returns_undef() {
        let pt3 = make_point3_min();
        let vec3 = Value::Vector(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ]);
        assert!(eval_builtin("bbox", &[pt3, vec3]).is_undef());
    }

    // ── bbox_size / bbox_center tests (step-11) ──────────────────────────────

    fn make_bbox() -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(2.0),
                Value::length(3.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::length(4.0),
                Value::length(6.0),
                Value::length(9.0),
            ])),
        }
    }

    #[test]
    fn bbox_size_returns_correct_vector() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → size=(3m,4m,6m)
        let result = eval_builtin("bbox_size", &[make_bbox()]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(3.0));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn bbox_center_returns_correct_point() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → center=(2.5m,4m,6m)
        let result = eval_builtin("bbox_center", &[make_bbox()]);
        match result {
            Value::Point(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(2.5));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    #[test]
    fn bbox_size_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_size", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_center_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_center", &[Value::Undef]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_size_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_size", &[]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_center_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_center", &[]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_size_dimensionless_bbox() {
        let bbox = Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::Real(2.0),
                Value::Real(4.0),
                Value::Real(6.0),
            ])),
        };
        let result = eval_builtin("bbox_size", &[bbox]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps[0], Value::Real(2.0));
                assert_eq!(comps[1], Value::Real(4.0));
                assert_eq!(comps[2], Value::Real(6.0));
            }
            other => panic!("expected Vector of Reals, got {:?}", other),
        }
    }

    // ── plane_xz / plane_yz tests (step-3) ───────────────────────────────────

    #[test]
    fn plane_xz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xz_correct_origin_and_normal() {
        // plane_xz(3mm) → origin=(0m, 3mm, 0m), normal=(0,1,0)
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.0), "x should be 0m");
                        assert_eq!(comps[1], Value::length(0.003), "y should be 3mm");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(0.0));
                        assert_eq!(comps[1], Value::Real(1.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_yz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_yz_correct_origin_and_normal() {
        // plane_yz(7mm) → origin=(7mm, 0m, 0m), normal=(1,0,0)
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.007), "x should be 7mm");
                        assert_eq!(comps[1], Value::length(0.0), "y should be 0m");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(1.0));
                        assert_eq!(comps[1], Value::Real(0.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xz_no_args_returns_undef() {
        assert!(eval_builtin("plane_xz", &[]).is_undef());
    }

    #[test]
    fn plane_yz_no_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[]).is_undef());
    }

    #[test]
    fn plane_xz_nan_returns_undef() {
        assert!(eval_builtin("plane_xz", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_yz_two_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    // ── plane_xy tests (step-1) ───────────────────────────────────────────────

    #[test]
    fn plane_xy_with_length_offset_returns_plane() {
        // plane_xy(5mm) → Plane with origin=(0m,0m,5mm) and normal=(0,0,1)
        let offset = Value::length(0.005); // 5mm in SI (meters)
        let result = eval_builtin("plane_xy", &[offset]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xy_with_length_offset_correct_origin() {
        let offset = Value::length(0.005); // 5mm
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { origin, .. } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3, "origin should be 3D");
                        // x=0m, y=0m, z=5mm
                        assert_eq!(comps[0], Value::length(0.0), "origin.x should be 0m");
                        assert_eq!(comps[1], Value::length(0.0), "origin.y should be 0m");
                        assert_eq!(comps[2], Value::length(0.005), "origin.z should be 5mm");
                    }
                    other => panic!("origin should be Point, got {:?}", other),
                }
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_with_length_offset_correct_normal() {
        let offset = Value::length(0.005);
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { normal, .. } => match *normal {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3, "normal should be 3D");
                    assert_eq!(comps[0], Value::Real(0.0), "normal.x should be 0");
                    assert_eq!(comps[1], Value::Real(0.0), "normal.y should be 0");
                    assert_eq!(comps[2], Value::Real(1.0), "normal.z should be 1");
                }
                other => panic!("normal should be Vector, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_no_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[]).is_undef());
    }

    #[test]
    fn plane_xy_bool_arg_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Bool(true)]).is_undef());
    }

    #[test]
    fn plane_xy_two_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    #[test]
    fn plane_xy_nan_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_xy_inf_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::INFINITY)]).is_undef());
    }

    #[test]
    fn plane_xy_real_zero_produces_dimensionless_origin() {
        // plane_xy(Real(0.0)) → dimensionless origin with Real(0.0) components
        let result = eval_builtin("plane_xy", &[Value::Real(0.0)]);
        match result {
            Value::Plane { origin, .. } => match *origin {
                Value::Point(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Point, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    // ── step-7: frame_to_frame tests ─────────────────────────────────────────

    /// Helper: build a Frame with given origin (LENGTH) and orientation.
    fn make_frame(ox: f64, oy: f64, oz: f64, orientation: Value) -> Value {
        Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(ox),
                Value::length(oy),
                Value::length(oz),
            ])),
            basis: Box::new(orientation),
        }
    }

    /// Helper: 90-degree Z rotation quaternion.
    fn make_rot90z() -> Value {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        }
    }

    /// frame_to_frame(F, F) should return an identity transform.
    #[test]
    fn frame_to_frame_same_gives_identity() {
        let f = make_frame(5.0, 3.0, 1.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[f.clone(), f]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(origin_frame, translated_frame) gives pure translation.
    #[test]
    fn frame_to_frame_translated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(5.0, 0.0, 0.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Translation = (5,0,0)
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 5.0).abs() < 1e-10, "tx = {tx}, expected 5");
                        assert!(ty.abs() < 1e-10, "ty = {ty}, expected 0");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(identity_frame, rotated_frame) gives pure rotation.
    #[test]
    fn frame_to_frame_rotated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // 90Z rotation
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame with both rotation and translation.
    /// From: origin=(1,0,0), identity rotation
    /// To: origin=(0,0,0), 90Z rotation
    /// R = R_to * conj(R_from) = 90Z * identity = 90Z
    /// t = origin_to - R * origin_from = (0,0,0) - 90Z*(1,0,0) = (0,0,0) - (0,1,0) = (0,-1,0)
    #[test]
    fn frame_to_frame_general() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-10, "tx = {tx}, expected 0");
                        assert!((ty + 1.0).abs() < 1e-10, "ty = {ty}, expected -1");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Wrong argument count or non-Frame args return Undef.
    #[test]
    fn frame_to_frame_wrong_args_undef() {
        // No args
        assert!(eval_builtin("frame_to_frame", &[]).is_undef());
        // One arg
        let f = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(eval_builtin("frame_to_frame", std::slice::from_ref(&f)).is_undef());
        // Three args
        assert!(eval_builtin("frame_to_frame", &[f.clone(), f.clone(), f.clone()]).is_undef());
        // Non-Frame args
        assert!(eval_builtin("frame_to_frame", &[Value::Real(1.0), f.clone()]).is_undef());
        assert!(eval_builtin("frame_to_frame", &[f, Value::Real(1.0)]).is_undef());
    }

    /// frame_to_frame with NaN in origin_from x-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_from_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_from should return Undef"
        );
    }

    /// frame_to_frame with NaN in origin_to y-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_to_returns_undef() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(0.0),
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_to should return Undef"
        );
    }

    /// frame_to_frame with mixed-dimension origin (length, angle, length) should return Undef.
    #[test]
    fn frame_to_frame_mixed_dimension_origin_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::angle(0.0), // dimension mismatch within same origin
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "mixed-dimension origin should return Undef"
        );
    }

    /// frame_to_frame with mismatched origin dimensions (LENGTH vs ANGLE) returns Undef.
    #[test]
    fn frame_to_frame_mismatched_origin_dimensions_undef() {
        // from-frame: LENGTH-dimensioned origin
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        // to-frame: ANGLE-dimensioned origin
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::angle(1.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(eval_builtin("frame_to_frame", &[from, to]).is_undef());
    }

    // ── transform_compose tests (step-15) ────────────────────────────────────

    /// Helper: build a Transform from rotation quaternion and translation (LENGTH).
    fn make_transform(rotation: Value, tx: f64, ty: f64, tz: f64) -> Value {
        Value::Transform {
            rotation: Box::new(rotation),
            translation: Box::new(Value::Vector(vec![
                Value::length(tx),
                Value::length(ty),
                Value::length(tz),
            ])),
        }
    }

    /// transform_compose(identity, T) == T
    #[test]
    fn transform_compose_identity_left() {
        let id = eval_builtin("transform3_identity", &[]);
        let t = make_transform(make_rot90z(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_compose", &[id, t.clone()]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose(T, identity) == T
    #[test]
    fn transform_compose_identity_right() {
        let id = eval_builtin("transform3_identity", &[]);
        let t = make_transform(make_rot90z(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_compose", &[t.clone(), id]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Pure translation composition: (R=I, t=[1,0,0]) * (R=I, t=[0,2,0]) == (R=I, t=[1,2,0]).
    #[test]
    fn transform_compose_pure_translation() {
        let t1 = make_transform(make_identity_orientation(), 1.0, 0.0, 0.0);
        let t2 = make_transform(make_identity_orientation(), 0.0, 2.0, 0.0);
        let result = eval_builtin("transform_compose", &[t1, t2]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Translation rotated by R1: (R=90Z, t=0) * (R=I, t=[1,0,0]) == (R=90Z, t=[0,1,0]).
    /// Composition formula: t = R1*t2 + t1 = 90Z*(1,0,0) + 0 = (0,1,0).
    #[test]
    fn transform_compose_rotation_then_translation() {
        let t1 = make_transform(make_rot90z(), 0.0, 0.0, 0.0);
        let t2 = make_transform(make_identity_orientation(), 1.0, 0.0, 0.0);
        let result = eval_builtin("transform_compose", &[t1, t2]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-12, "tx = {tx}, expected 0");
                        assert!((ty - 1.0).abs() < 1e-12, "ty = {ty}, expected 1");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose(T1, T2) numerically matches the (R1*R2, R1*t2+t1)
    /// formula used by the `Transform * Transform` operator in reify-expr.
    ///
    /// This test does NOT invoke the operator code path itself — `eval_mul`
    /// is private to reify-expr and not callable from this crate's unit
    /// tests. Instead, it asserts numeric equivalence with the same algebra,
    /// using the same shared helpers (quat_mul / quat_rotate). The
    /// kinematic_stdlib_smoke E2E test in `crates/reify-eval/tests` is the
    /// place that drives the actual operator path through the eval pipeline
    /// and compares against `transform_compose`'s output.
    #[test]
    fn transform_compose_matches_named_function_formula() {
        // Use transform3_identity-derived inputs that don't already pre-normalize quaternions.
        let q1 = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let q2 = Value::Orientation {
            w: 0.5,
            x: -0.5,
            y: 0.5,
            z: 0.5,
        };
        let t1 = make_transform(q1, 1.0, 2.0, 3.0);
        let t2 = make_transform(q2, 4.0, 5.0, 6.0);
        let composed = eval_builtin("transform_compose", &[t1.clone(), t2.clone()]);
        // Mirror the exact algebra used by the operator-level path:
        //   R = normalize(q1) * q2
        //   t = normalize(q1) * t2 + t1  (vector rotation)
        // Construct the expected result component-by-component and compare.
        let q1_t = (0.5, 0.5, 0.5, 0.5);
        let q2_t = (0.5, -0.5, 0.5, 0.5);
        // q1 already has norm 1 → no-op.
        let (rw, rx, ry, rz) = super::quat_mul(q1_t, q2_t);
        let norm = (rw * rw + rx * rx + ry * ry + rz * rz).sqrt();
        let (rw, rx, ry, rz) = (rw / norm, rx / norm, ry / norm, rz / norm);
        let (rt2x, rt2y, rt2z) = super::quat_rotate(q1_t, 4.0, 5.0, 6.0);
        match composed {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, rw, rx, ry, rz, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(
                            (tx - (rt2x + 1.0)).abs() < 1e-12,
                            "tx = {tx}, expected {}",
                            rt2x + 1.0
                        );
                        assert!(
                            (ty - (rt2y + 2.0)).abs() < 1e-12,
                            "ty = {ty}, expected {}",
                            rt2y + 2.0
                        );
                        assert!(
                            (tz - (rt2z + 3.0)).abs() < 1e-12,
                            "tz = {tz}, expected {}",
                            rt2z + 3.0
                        );
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose with wrong arg count → Undef.
    #[test]
    fn transform_compose_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_compose", &[]).is_undef());
        assert!(eval_builtin("transform_compose", std::slice::from_ref(&t)).is_undef());
        assert!(eval_builtin("transform_compose", &[t.clone(), t.clone(), t.clone()]).is_undef());
    }

    /// transform_compose with non-Transform arg → Undef.
    #[test]
    fn transform_compose_non_transform_arg_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_compose", &[Value::Real(1.0), t.clone()]).is_undef());
        assert!(eval_builtin("transform_compose", &[t, Value::Real(1.0)]).is_undef());
    }

    /// transform_compose with mixed-dimension translations → Undef.
    /// (LENGTH translation in T1, ANGLE translation in T2)
    #[test]
    fn transform_compose_mixed_dimension_translations_returns_undef() {
        let t1 = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::length(1.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        let t2 = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::angle(0.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
        };
        assert!(eval_builtin("transform_compose", &[t1, t2]).is_undef());
    }

    /// transform_compose with an overflow-corner quaternion (w = 1e200, x=y=z=0) → Undef.
    ///
    /// Overflow trace:
    /// - `decompose_transform` accepts: every component (1e200, 0, 0, 0) is finite.
    /// - `normalize_quat_input`: `norm_sq = 1e200² = ∞`. The gate `!norm_sq.is_finite()`
    ///   fires for the first operand, returning `None`.
    /// - `transform_compose` returns `Undef` immediately, before `quat_mul` is called.
    #[test]
    fn transform_compose_overflow_quaternion_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 0.0, 0.0, 0.0);
        assert!(
            eval_builtin("transform_compose", &[bad_t.clone(), bad_t]).is_undef(),
            "expected Undef for overflow-corner quaternion but got a non-Undef result"
        );
    }

    /// Same overflow corner as above but with a non-zero translation `(1.0, 2.0, 3.0)`.
    ///
    /// Confirms the rotation gate in `normalize_quat_input` (not coincidental zero
    /// translation) is what produces Undef. The gate fires before `quat_mul` ever
    /// sees the quaternion, so translation magnitude is irrelevant.
    #[test]
    fn transform_compose_overflow_quaternion_nonzero_translation_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 1.0, 2.0, 3.0);
        assert!(
            eval_builtin("transform_compose", &[bad_t.clone(), bad_t]).is_undef(),
            "expected Undef for overflow-corner quaternion with non-zero translation"
        );
    }

    // ── transform_inverse tests (step-17) ────────────────────────────────────

    /// transform_inverse(identity) == identity.
    #[test]
    fn transform_inverse_identity_is_identity() {
        let id = eval_builtin("transform3_identity", &[]);
        let result = eval_builtin("transform_inverse", &[id]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-12, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_inverse((R=90Z, t=[1,0,0])) has R = -90Z (conjugate of 90Z) and t = -R^-1 * (1,0,0) = (0,1,0).
    /// Computation: R^-1 = conj(R) = (s, 0, 0, -s). R^-1 * (1,0,0) = quat_rotate(R^-1, (1,0,0)) = (0,-1,0).
    /// t_inv = -R^-1 * t = -(0,-1,0) = (0,1,0).
    #[test]
    fn transform_inverse_90z_with_translation() {
        let t = make_transform(make_rot90z(), 1.0, 0.0, 0.0);
        let result = eval_builtin("transform_inverse", &[t]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, -s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-12, "tx = {tx}, expected 0");
                        assert!((ty - 1.0).abs() < 1e-12, "ty = {ty}, expected 1");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// inverse(inverse(T)) ≈ T (round-trip with sign_insensitive on rotation).
    #[test]
    fn transform_inverse_round_trip() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q.clone(), 1.5, 2.5, -3.5);
        let inv = eval_builtin("transform_inverse", std::slice::from_ref(&t));
        let back = eval_builtin("transform_inverse", &[inv]);
        match back {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 0.5, 0.5, 0.5, 0.5, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.5).abs() < 1e-12, "tx = {tx}, expected 1.5");
                        assert!((ty - 2.5).abs() < 1e-12, "ty = {ty}, expected 2.5");
                        assert!((tz - (-3.5)).abs() < 1e-12, "tz = {tz}, expected -3.5");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// compose(T, inverse(T)) ≈ identity for an arbitrary T.
    #[test]
    fn transform_inverse_compose_t_inv_t_is_identity() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q, 1.5, 2.5, -3.5);
        let inv = eval_builtin("transform_inverse", std::slice::from_ref(&t));
        let composed = eval_builtin("transform_compose", &[t, inv]);
        match composed {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_inverse with wrong arg count → Undef.
    #[test]
    fn transform_inverse_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_inverse", &[]).is_undef());
        assert!(eval_builtin("transform_inverse", &[t.clone(), t]).is_undef());
    }

    /// transform_inverse with non-Transform arg → Undef.
    #[test]
    fn transform_inverse_non_transform_arg_returns_undef() {
        assert!(eval_builtin("transform_inverse", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("transform_inverse", &[make_identity_orientation()]).is_undef());
    }

    /// transform_inverse with an overflow-corner quaternion (w = 1e200, x=y=z=0) → Undef.
    ///
    /// Overflow trace:
    /// - `decompose_transform` accepts: every component (1e200, 0, 0, 0) is finite.
    /// - `normalize_quat_input`: `norm_sq = 1e200² = ∞`. The gate `!norm_sq.is_finite()`
    ///   fires, returning `None`.
    /// - `transform_inverse` returns `Undef` immediately, before `quat_conj` is called.
    #[test]
    fn transform_inverse_overflow_quaternion_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 0.0, 0.0, 0.0);
        assert!(
            eval_builtin("transform_inverse", std::slice::from_ref(&bad_t)).is_undef(),
            "expected Undef for overflow-corner quaternion but got a non-Undef result"
        );
    }

    /// Same overflow corner as above but with a non-zero translation `(1.0, 2.0, 3.0)`.
    ///
    /// Confirms the rotation gate in `normalize_quat_input` (not coincidental zero
    /// translation) is what produces Undef. The gate fires before `quat_rotate` ever
    /// sees the quaternion, so translation magnitude is irrelevant.
    #[test]
    fn transform_inverse_overflow_quaternion_nonzero_translation_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 1.0, 2.0, 3.0);
        assert!(
            eval_builtin("transform_inverse", std::slice::from_ref(&bad_t)).is_undef(),
            "expected Undef for overflow-corner quaternion with non-zero translation"
        );
    }

    // ── transform_log tests (step-19) ────────────────────────────────────────

    /// Helper: extract a Vector3's three f64 components from a Map's value at `key`.
    fn map_vec3_components(map: &Value, key: &str) -> [f64; 3] {
        let map_inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = map_inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => [
                items[0].as_f64().unwrap(),
                items[1].as_f64().unwrap(),
                items[2].as_f64().unwrap(),
            ],
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// Helper: dimension of a Map's vector value at `key`.
    fn map_vec3_dim(map: &Value, key: &str) -> DimensionVector {
        let map_inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = map_inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => items[0].dimension(),
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// transform_log(identity) == Map { angular=[0,0,0] DIMENSIONLESS, linear=[0,0,0] LENGTH }.
    #[test]
    fn transform_log_identity_is_zero_twist() {
        let id = eval_builtin("transform3_identity", &[]);
        let result = eval_builtin("transform_log", &[id]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        for (i, v) in ang.iter().enumerate() {
            assert!(v.abs() < 1e-12, "angular[{i}] = {v}, expected 0");
        }
        for (i, v) in lin.iter().enumerate() {
            assert!(v.abs() < 1e-12, "linear[{i}] = {v}, expected 0");
        }
        assert_eq!(
            map_vec3_dim(&result, "angular"),
            DimensionVector::DIMENSIONLESS
        );
        assert_eq!(map_vec3_dim(&result, "linear"), DimensionVector::LENGTH);
    }

    /// Pure translation: T=(identity, [1,2,3] m) → angular=[0,0,0], linear=[1,2,3].
    /// (When ω=0, V=I, so V_inv*t = t.)
    #[test]
    fn transform_log_pure_translation() {
        let t = make_transform(make_identity_orientation(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        for (i, v) in ang.iter().enumerate() {
            assert!(v.abs() < 1e-12, "angular[{i}] = {v}, expected 0");
        }
        assert!(
            (lin[0] - 1.0).abs() < 1e-12,
            "linear[0] = {}, expected 1",
            lin[0]
        );
        assert!(
            (lin[1] - 2.0).abs() < 1e-12,
            "linear[1] = {}, expected 2",
            lin[1]
        );
        assert!(
            (lin[2] - 3.0).abs() < 1e-12,
            "linear[2] = {}, expected 3",
            lin[2]
        );
        assert_eq!(
            map_vec3_dim(&result, "angular"),
            DimensionVector::DIMENSIONLESS
        );
        assert_eq!(map_vec3_dim(&result, "linear"), DimensionVector::LENGTH);
    }

    /// Pure 90°z rotation, no translation: angular=[0,0,π/2], linear=[0,0,0].
    #[test]
    fn transform_log_pure_rotation() {
        let t = make_transform(make_rot90z(), 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        let expected_z = std::f64::consts::FRAC_PI_2;
        assert!(ang[0].abs() < 1e-12, "angular[0] = {}, expected 0", ang[0]);
        assert!(ang[1].abs() < 1e-12, "angular[1] = {}, expected 0", ang[1]);
        assert!(
            (ang[2] - expected_z).abs() < 1e-12,
            "angular[2] = {}, expected π/2",
            ang[2]
        );
        for (i, v) in lin.iter().enumerate() {
            assert!(v.abs() < 1e-12, "linear[{i}] = {v}, expected 0");
        }
    }

    /// Small-rotation transform: angular components match rotation vector linearly.
    /// For a small angle ε about axis (0,0,1) with no translation, angular ≈ [0, 0, ε].
    #[test]
    fn transform_log_small_rotation() {
        let eps: f64 = 1e-6;
        // Build a small-z rotation manually: q = (cos(eps/2), 0, 0, sin(eps/2)).
        let half = eps / 2.0;
        let q = Value::Orientation {
            w: half.cos(),
            x: 0.0,
            y: 0.0,
            z: half.sin(),
        };
        let t = make_transform(q, 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        // angular[2] should be ≈ eps within ~1e-12.
        assert!(ang[0].abs() < 1e-10, "angular[0] = {}, expected ~0", ang[0]);
        assert!(ang[1].abs() < 1e-10, "angular[1] = {}, expected ~0", ang[1]);
        assert!(
            (ang[2] - eps).abs() < 1e-12,
            "angular[2] = {}, expected {}",
            ang[2],
            eps
        );
    }

    /// Negated-quaternion (w ≈ -1) input represents the same rotation as
    /// identity, so transform_log must canonicalize the sign before computing
    /// ω. Without canonicalization, the small-angle Taylor branch (v_norm < EPS)
    /// would emit ω ≈ −2*(nx,ny,nz) — wrong-signed for q whose nw is exactly 0
    /// or near −1. After the canonical sign-flip, both q and −q yield the same
    /// rotation vector.
    #[test]
    fn transform_log_negated_identity_quaternion_canonicalizes_sign() {
        // q = (-1, 0, 0, 0): identity rotation in the "negative hemisphere".
        let q = Value::Orientation {
            w: -1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let t = make_transform(q, 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        for (i, v) in ang.iter().enumerate() {
            assert!(
                v.abs() < 1e-12,
                "negated-identity angular[{i}] = {v}, expected 0 after sign canonicalization"
            );
        }
    }

    /// Slightly-perturbed q with w near −1 (near-identity in the negative
    /// hemisphere): after canonicalization (flip all components so nw > 0),
    /// nx becomes −small, so ω = (2·nx, 0, 0) ≈ (−2·small, 0, 0). ang[0]
    /// is *negative*, not positive.
    #[test]
    fn transform_log_near_negative_identity_canonicalizes_axis_sign() {
        // Construct q such that nw ≈ −1 + tiny, with small (x,y,z) of definite sign.
        let small = 1e-10_f64;
        let w0 = -(1.0 - small * small / 2.0); // ≈ -1 + tiny
        let q = Value::Orientation {
            w: w0,
            x: small,
            y: 0.0,
            z: 0.0,
        };
        let t = make_transform(q, 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        // After canonicalization (flip sign so nw > 0), nx becomes −small, so
        // ω = (2·nx, 0, 0) = (−2·small, 0, 0). Verify the sign (ang[0] < 0)
        // and the magnitude (|ang[0]| ≈ 2·small ≈ 2e-10).
        // Note: small = 1e-10 > EPS = 1e-12, so this test deliberately stays
        // in the atan2 branch of transform_log — the Taylor branch (v_norm < EPS)
        // is exercised by the negated-identity test above where v_norm = 0.
        // ω ≈ 2·nx is the leading-order result of the atan2 formula
        // (angle = 2·atan2(v_norm, nw); scale = angle/v_norm → 2 as nw → 1),
        // not the Taylor approximation.
        assert!(
            ang[0] < 0.0,
            "angular[0] = {}, expected negative after canonicalization-driven sign flip",
            ang[0]
        );
        assert!(
            (ang[0].abs() - 2.0 * small).abs() < 1e-15,
            "|angular[0]| = {}, expected ≈ {}",
            ang[0].abs(),
            2.0 * small
        );
        assert!(ang[1].abs() < 1e-15, "angular[1] = {}, expected 0", ang[1]);
        assert!(ang[2].abs() < 1e-15, "angular[2] = {}, expected 0", ang[2]);
    }

    /// transform_log with wrong arg count → Undef.
    #[test]
    fn transform_log_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_log", &[]).is_undef());
        assert!(eval_builtin("transform_log", &[t.clone(), t]).is_undef());
    }

    /// transform_log with non-Transform arg → Undef.
    #[test]
    fn transform_log_non_transform_arg_returns_undef() {
        assert!(eval_builtin("transform_log", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("transform_log", &[make_identity_orientation()]).is_undef());
    }

    /// transform_log with an overflow-corner quaternion (w = 1e200, x=y=z=0) → Undef.
    ///
    /// Overflow trace (pre-fix):
    /// - `decompose_transform` accepts: every component (1e200, 0, 0, 0) is finite.
    /// - `normalize_quat_input`: `norm_sq = 1e200² = ∞`, which is NOT `< 1e-24`, so the gate
    ///   accepts and returns `(1e200/∞, 0/∞, 0/∞, 0/∞) = (0, 0, 0, 0)`.
    /// - `v_norm = 0 < EPS=1e-12` → Taylor branch → `(wx,wy,wz) = (0,0,0)`, finite.
    /// - `theta = 0` → small-angle alpha branch → `lx,ly,lz = tx,ty,tz`, finite.
    /// - Emits `Map { angular=[0,0,0], linear=t }` — a non-Undef result (BUG).
    ///
    /// After fix: `normalize_quat_input` additionally rejects non-finite `norm_sq` via
    /// `!norm_sq.is_finite()`, so the helper returns `None` and `transform_log` returns Undef.
    #[test]
    fn transform_log_overflow_quaternion_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 0.0, 0.0, 0.0);
        assert!(
            eval_builtin("transform_log", std::slice::from_ref(&bad_t)).is_undef(),
            "expected Undef for overflow-corner quaternion but got a non-Undef result"
        );
    }

    /// Same overflow corner as above but with a non-zero translation `(1.0, 2.0, 3.0)`.
    ///
    /// With zero translation the zero-norm rotation cannot independently produce
    /// non-finite output via the linear part. This sibling test confirms that the
    /// rotation-side gate in `normalize_quat_input` short-circuits before any linear
    /// computation sees the collapsed `(0,0,0,0)` rotation — it is the rotation gate
    /// that produces Undef, not coincidental zero translation.
    #[test]
    fn transform_log_overflow_quaternion_nonzero_translation_returns_undef() {
        let bad_rot = Value::Orientation {
            w: 1e200,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bad_t = make_transform(bad_rot, 1.0, 2.0, 3.0);
        assert!(
            eval_builtin("transform_log", std::slice::from_ref(&bad_t)).is_undef(),
            "expected Undef for overflow-corner quaternion with non-zero translation"
        );
    }

    /// Degenerate-quaternion gate boundary: r_norm_sq in [1e-24, f64::EPSILON) is accepted.
    ///
    /// The threshold was bumped from `f64::EPSILON` (~2.22e-16) to `1e-24` so that
    /// quaternions with r_norm_sq down to (1e-12)² are accepted and normalised rather
    /// than rejected as Undef. This test pins the new lower boundary:
    /// `r_norm_sq = 1e-20` was previously rejected under `f64::EPSILON`; it must now
    /// succeed for `transform_log`, `transform_inverse`, and `transform_compose`.
    #[test]
    fn degenerate_quat_small_norm_above_1e24_gate_accepted() {
        // Quaternion (1e-10, 0, 0, 0): r_norm_sq = 1e-20, in [1e-24, f64::EPSILON).
        // Normalises to the identity quaternion, so every operation returns the
        // zero twist / identity transform / etc. — just not Undef.
        let small_quat = Value::Orientation {
            w: 1e-10_f64,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let t = Value::Transform {
            rotation: Box::new(small_quat),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        assert!(
            !eval_builtin("transform_log", std::slice::from_ref(&t)).is_undef(),
            "transform_log must accept r_norm_sq=1e-20 (≥ 1e-24 gate)"
        );
        assert!(
            !eval_builtin("transform_inverse", std::slice::from_ref(&t)).is_undef(),
            "transform_inverse must accept r_norm_sq=1e-20 (≥ 1e-24 gate)"
        );
        assert!(
            !eval_builtin("transform_compose", &[t.clone(), t]).is_undef(),
            "transform_compose must accept r_norm_sq=1e-20 (≥ 1e-24 gate) on both operands"
        );
    }

    /// Degenerate-quaternion gate boundary: zero-norm quaternion → Undef.
    ///
    /// Complements `degenerate_quat_small_norm_above_1e24_gate_accepted`: a quaternion
    /// with all components zero (r_norm_sq = 0 < 1e-24) must return Undef from all
    /// three functions.
    #[test]
    fn degenerate_quat_zero_norm_returns_undef() {
        let zero_quat = Value::Orientation {
            w: 0.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let t_zero = Value::Transform {
            rotation: Box::new(zero_quat),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        assert!(
            eval_builtin("transform_log", std::slice::from_ref(&t_zero)).is_undef(),
            "transform_log must reject zero-norm quaternion"
        );
        assert!(
            eval_builtin("transform_inverse", std::slice::from_ref(&t_zero)).is_undef(),
            "transform_inverse must reject zero-norm quaternion"
        );
        assert!(
            eval_builtin("transform_compose", &[t_zero.clone(), t_zero]).is_undef(),
            "transform_compose must reject zero-norm quaternion"
        );
    }

    /// Helper: build a w-only Transform with the given r_norm_sq and assert
    /// that all three gated builtins (transform_log, transform_inverse,
    /// transform_compose) produce Undef iff `expect_undef` is true.
    ///
    /// Using a w-only quaternion makes r_norm_sq = w² trivially predictable,
    /// avoiding multi-component cancellation that could perturb the actual
    /// norm computed by the implementation.
    ///
    /// **ULP-gap assumption (boundary tests):** this helper sets `w =
    /// r_norm_sq.sqrt()`, so the implementation re-derives r_norm_sq as
    /// `w*w`.  f64 round-trip error at magnitude ~1e-24 is ~2.22e-40
    /// (relative ULP ~2.22e-16), which is ~12 orders of magnitude smaller
    /// than the 0.1% gap (1e-27) used by the boundary test values
    /// (1.001e-24 / 0.999e-24).  **Do not tighten that gap** without
    /// switching to exact quaternion components instead of going through sqrt.
    fn assert_quat_norm_sq_outcome(r_norm_sq: f64, expect_undef: bool) {
        let w = r_norm_sq.sqrt();
        let small_quat = Value::Orientation {
            w,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let t = Value::Transform {
            rotation: Box::new(small_quat),
            translation: Box::new(Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        let label = if expect_undef { "reject" } else { "accept" };
        assert_eq!(
            eval_builtin("transform_log", std::slice::from_ref(&t)).is_undef(),
            expect_undef,
            "transform_log must {label} r_norm_sq={r_norm_sq:e}"
        );
        assert_eq!(
            eval_builtin("transform_inverse", std::slice::from_ref(&t)).is_undef(),
            expect_undef,
            "transform_inverse must {label} r_norm_sq={r_norm_sq:e}"
        );
        assert_eq!(
            eval_builtin("transform_compose", &[t.clone(), t]).is_undef(),
            expect_undef,
            "transform_compose must {label} r_norm_sq={r_norm_sq:e} on both operands"
        );
    }

    /// 1e-24 gate boundary (just above): r_norm_sq ≈ 1.001e-24 must be accepted.
    ///
    /// Pins the tight upper side of the 1e-24 gate. A quaternion with
    /// r_norm_sq = 1.001e-24 (0.1% above the threshold) must succeed — not
    /// return Undef — for transform_log, transform_inverse, and transform_compose.
    /// Together with `degenerate_quat_norm_just_below_1e24_gate_returns_undef`,
    /// any off-by-a-percentage-point refactor of the gate will fail at least
    /// one of these two tests (the 1e-20 test above would not catch that).
    #[test]
    fn degenerate_quat_norm_just_above_1e24_gate_accepted() {
        assert_quat_norm_sq_outcome(1.001e-24, false);
    }

    /// 1e-24 gate boundary (just below): r_norm_sq ≈ 0.999e-24 must return Undef.
    ///
    /// Pins the tight lower side of the 1e-24 gate. A quaternion with
    /// r_norm_sq = 0.999e-24 (0.1% below the threshold) must return Undef
    /// for transform_log, transform_inverse, and transform_compose.
    /// Complements `degenerate_quat_norm_just_above_1e24_gate_accepted` so
    /// together the pair catches any off-by-a-percentage-point refactor of
    /// the gate that the 1e-20 / zero tests above would miss.
    #[test]
    fn degenerate_quat_norm_just_below_1e24_gate_returns_undef() {
        assert_quat_norm_sq_outcome(0.999e-24, true);
    }

    /// transform_log with ANGLE-dimension translation → Undef (matches transform_exp gate).
    ///
    /// transform_exp rejects twist.linear with ANGLE dimension (see
    /// `transform_exp_linear_wrong_dimension_returns_undef`). This test
    /// pins the symmetric gate in transform_log: a Transform whose
    /// translation is ANGLE-dimensioned must also return Undef so that
    /// neither end of the log↔exp round-trip silently accepts
    /// untranslatable inputs.
    #[test]
    fn transform_log_angle_dim_translation_returns_undef() {
        let t = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::angle(0.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
        };
        assert!(eval_builtin("transform_log", &[t]).is_undef());
    }

    /// transform_log with MASS-dimension translation → Undef.
    ///
    /// The gate is `t_dim != LENGTH && t_dim != DIMENSIONLESS`. MASS flows through the
    /// same rejection branch as ANGLE, ensuring the test suite covers more than one
    /// non-accepted dimension so a future narrowing of the gate (e.g. adding ANGLE as a
    /// special case) cannot silently pass.
    #[test]
    fn transform_log_mass_dim_translation_returns_undef() {
        let t = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::MASS,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::MASS,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::MASS,
                },
            ])),
        };
        assert!(eval_builtin("transform_log", &[t]).is_undef());
    }

    /// transform_log with DIMENSIONLESS translation → accepted (non-Undef).
    ///
    /// DIMENSIONLESS is the second accepted dimension alongside LENGTH. This positive
    /// case pins that the gate does NOT reject dimensionless translations, complementing
    /// the `transform_exp_zero_twist_is_identity` test which already verifies the
    /// round-trip for the DIMENSIONLESS case.
    #[test]
    fn transform_log_dimensionless_translation_returns_non_undef() {
        let t = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
        };
        assert!(
            !eval_builtin("transform_log", &[t]).is_undef(),
            "transform_log must accept DIMENSIONLESS translation"
        );
    }

    // ── transform_exp tests (step-21) ────────────────────────────────────────

    /// Helper: build a twist Map with given angular & linear vectors.
    fn make_twist(angular: [f64; 3], linear: [f64; 3], linear_dim: DimensionVector) -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![
                Value::Real(angular[0]),
                Value::Real(angular[1]),
                Value::Real(angular[2]),
            ]),
        );
        let make_lin = |v: f64| -> Value {
            if linear_dim.is_dimensionless() {
                Value::Real(v)
            } else {
                Value::Scalar {
                    si_value: v,
                    dimension: linear_dim,
                }
            }
        };
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![
                make_lin(linear[0]),
                make_lin(linear[1]),
                make_lin(linear[2]),
            ]),
        );
        Value::Map(m)
    }

    /// transform_exp(zero twist) == identity transform.
    #[test]
    fn transform_exp_zero_twist_is_identity() {
        let zero = make_twist([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], DimensionVector::LENGTH);
        let result = eval_builtin("transform_exp", &[zero]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-12, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp(angular=[0,0,π/2], linear=0) → 90°z rotation, zero translation.
    #[test]
    fn transform_exp_pure_rotation() {
        let twist = make_twist(
            [0.0, 0.0, std::f64::consts::FRAC_PI_2],
            [0.0, 0.0, 0.0],
            DimensionVector::LENGTH,
        );
        let result = eval_builtin("transform_exp", &[twist]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp(angular=0, linear=[1,2,3]) → (identity, [1,2,3] m).
    #[test]
    fn transform_exp_pure_translation() {
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], DimensionVector::LENGTH);
        let result = eval_builtin("transform_exp", &[twist]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                        // Verify dimension is LENGTH.
                        assert_eq!(items[0].dimension(), DimensionVector::LENGTH);
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Round-trip: transform_log(transform_exp(twist)) ≈ twist for several non-trivial twists.
    #[test]
    fn transform_exp_log_round_trip() {
        let twists = [
            ([0.1, 0.2, 0.3], [1.0, 2.0, 3.0]),
            ([0.5, -0.3, 0.7], [-1.0, 0.5, 2.0]),
            ([0.01, 0.02, 0.03], [0.1, 0.1, 0.1]),
        ];
        for (i, (ang, lin)) in twists.iter().enumerate() {
            let twist = make_twist(*ang, *lin, DimensionVector::LENGTH);
            let t = eval_builtin("transform_exp", &[twist]);
            let back = eval_builtin("transform_log", &[t]);
            let ang_back = map_vec3_components(&back, "angular");
            let lin_back = map_vec3_components(&back, "linear");
            for j in 0..3 {
                assert!(
                    (ang_back[j] - ang[j]).abs() < 1e-10,
                    "case {i}: angular[{j}] = {}, expected {}",
                    ang_back[j],
                    ang[j]
                );
                assert!(
                    (lin_back[j] - lin[j]).abs() < 1e-10,
                    "case {i}: linear[{j}] = {}, expected {}",
                    lin_back[j],
                    lin[j]
                );
            }
        }
    }

    /// Round-trip: transform_exp(transform_log(T)) ≈ T (with sign_insensitive on rotation).
    #[test]
    fn transform_log_exp_round_trip() {
        // Use a non-axis-aligned rotation to exercise the general case.
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q.clone(), 1.5, -2.5, 3.0);
        let twist = eval_builtin("transform_log", std::slice::from_ref(&t));
        let back = eval_builtin("transform_exp", &[twist]);
        match back {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 0.5, 0.5, 0.5, 0.5, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.5).abs() < 1e-10, "tx = {tx}, expected 1.5");
                        assert!((ty - (-2.5)).abs() < 1e-10, "ty = {ty}, expected -2.5");
                        assert!((tz - 3.0).abs() < 1e-10, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp with Map missing "angular" key → Undef.
    #[test]
    fn transform_exp_missing_angular_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with Map missing "linear" key → Undef.
    #[test]
    fn transform_exp_missing_linear_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::Real(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with non-DIMENSIONLESS angular dimension → Undef.
    #[test]
    fn transform_exp_angular_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]), // LENGTH instead of DIMENSIONLESS
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with non-LENGTH linear dimension → Undef.
    #[test]
    fn transform_exp_linear_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::Real(0.0); 3]),
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]), // ANGLE instead of LENGTH
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with NaN component → Undef.
    #[test]
    fn transform_exp_nan_angular_returns_undef() {
        let twist = make_twist(
            [f64::NAN, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            DimensionVector::LENGTH,
        );
        assert!(eval_builtin("transform_exp", &[twist]).is_undef());
    }

    #[test]
    fn transform_exp_inf_linear_returns_undef() {
        let twist = make_twist(
            [0.0, 0.0, 0.0],
            [f64::INFINITY, 0.0, 0.0],
            DimensionVector::LENGTH,
        );
        assert!(eval_builtin("transform_exp", &[twist]).is_undef());
    }

    /// transform_exp with wrong arg count → Undef.
    #[test]
    fn transform_exp_wrong_arg_count_returns_undef() {
        let twist = make_twist([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("transform_exp", &[]).is_undef());
        assert!(eval_builtin("transform_exp", &[twist.clone(), twist]).is_undef());
    }

    // ── step-1/2: project(point, Frame<3>) tests ─────────────────────────────

    /// project(point3(1,2,3 m), frame(origin=(1,0,0 m), identity)) → Point ≈ [0,2,3 m].
    /// Subtracts origin before (no) rotation; also pin that output components carry LENGTH.
    #[test]
    fn project_point_identity_basis_subtracts_origin() {
        let point = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let frame = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let result = eval_builtin("project", &[point, frame]);
        assert_vector3_approx!(Point, result, [0.0, 2.0, 3.0]);
        // Also verify the first component carries LENGTH dimension (not dimensionless).
        match eval_builtin("project", &[
            Value::Point(vec![
                Value::length(1.0),
                Value::length(2.0),
                Value::length(3.0),
            ]),
            make_frame(1.0, 0.0, 0.0, make_identity_orientation()),
        ]) {
            Value::Point(ref items) => {
                assert_scalar_approx!(items[0].clone(), 0.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    /// project(point3(1,1,0 m), frame(origin=(1,0,0 m), rot90z)) → Point ≈ [1,0,0 m].
    ///
    /// d = (1,1,0) − (1,0,0) = (0,1,0).
    /// inverse(rot90z) = rot(−90°Z).  Rotating (0,1,0) by −90°Z → (1,0,0).
    /// Discriminates that origin subtraction happens BEFORE the inverse rotation.
    #[test]
    fn project_point_rotated_frame_subtract_then_inverse_rotate() {
        let point = Value::Point(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(0.0),
        ]);
        let frame = make_frame(1.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("project", &[point, frame]);
        assert_vector3_approx!(Point, result, [1.0, 0.0, 0.0]);
    }

    // ── step-3/4: project(vector, Frame<3>) tests ─────────────────────────────

    /// project(vec3(1,2,3 m), frame(origin=(1,0,0 m), identity)) → Vector ≈ [1,2,3 m].
    /// Origin is NOT subtracted for vectors (translation-invariant).
    #[test]
    fn project_vector_identity_basis_keeps_components() {
        let vec3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let frame = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let result = eval_builtin("project", &[vec3, frame]);
        assert_vector3_approx!(Vector, result, [1.0, 2.0, 3.0]);
    }

    /// project(vec3(1,0,0 m), frame(origin=0, rot90z)) → Vector ≈ [0,−1,0 m].
    ///
    /// inverse(rot90z) = rot(−90°Z).  Rotating (1,0,0) by −90°Z → (0,−1,0).
    #[test]
    fn project_vector_rotated_frame_inverse_rotates() {
        let vec3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let frame = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("project", &[vec3, frame]);
        assert_vector3_approx!(Vector, result, [0.0, -1.0, 0.0]);
    }

    /// project(vec3, frame(origin=(7,8,9), identity)) == project(same vec3, frame(origin=(0,0,0), identity)).
    /// Both ≈ [1,2,3] — pins translation-invariance: origin must NOT be subtracted.
    #[test]
    fn project_vector_ignores_frame_origin() {
        let vec3_a = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let vec3_b = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let frame_far = make_frame(7.0, 8.0, 9.0, make_identity_orientation());
        let frame_zero = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let r1 = eval_builtin("project", &[vec3_a, frame_far]);
        let r2 = eval_builtin("project", &[vec3_b, frame_zero]);
        assert_vector3_approx!(Vector, r1, [1.0, 2.0, 3.0]);
        assert_vector3_approx!(Vector, r2, [1.0, 2.0, 3.0]);
    }

    // ── step-5/6: project rejection tests ────────────────────────────────────

    /// Structural rejections: wrong arg count, non-Frame 2nd arg, non-Point/Vector 1st arg,
    /// wrong-length arg, degenerate basis, NaN component.
    #[test]
    fn project_rejections_return_undef() {
        let pt = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let v3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        let frame = make_frame(0.0, 0.0, 0.0, make_identity_orientation());

        // --- wrong arg count ---
        assert!(eval_builtin("project", &[]).is_undef(), "no args");
        assert!(eval_builtin("project", std::slice::from_ref(&pt)).is_undef(), "one arg");
        assert!(
            eval_builtin("project", &[pt.clone(), frame.clone(), Value::Real(0.0)]).is_undef(),
            "three args"
        );

        // --- non-Frame 2nd arg ---
        assert!(
            eval_builtin("project", &[pt.clone(), Value::Real(1.0)]).is_undef(),
            "2nd arg Real"
        );
        assert!(
            eval_builtin("project", &[pt.clone(), pt.clone()]).is_undef(),
            "2nd arg Point"
        );
        assert!(
            eval_builtin("project", &[v3.clone(), v3.clone()]).is_undef(),
            "2nd arg Vector"
        );

        // --- arg[0] neither Point nor Vector ---
        assert!(
            eval_builtin("project", &[Value::Real(1.0), frame.clone()]).is_undef(),
            "1st arg Real"
        );

        // --- arg[0] wrong length (2 components) ---
        let pt2 = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        let v2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        assert!(
            eval_builtin("project", &[pt2, frame.clone()]).is_undef(),
            "Point2"
        );
        assert!(
            eval_builtin("project", &[v2, frame.clone()]).is_undef(),
            "Vector2"
        );

        // --- degenerate basis (zero quaternion) ---
        let degenerate_frame = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(Value::Orientation {
                w: 0.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
        };
        assert!(
            eval_builtin("project", &[pt.clone(), degenerate_frame.clone()]).is_undef(),
            "degenerate basis for Point"
        );
        assert!(
            eval_builtin("project", &[v3.clone(), degenerate_frame]).is_undef(),
            "degenerate basis for Vector"
        );

        // --- non-finite point component (NaN x) ---
        let nan_pt = Value::Point(vec![
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::LENGTH,
            },
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("project", &[nan_pt, frame.clone()]).is_undef(),
            "NaN point component"
        );
    }

    /// project(point in LENGTH, frame with DIMENSIONLESS origin) → Undef.
    ///
    /// Subtracting LENGTH si_values from DIMENSIONLESS si_values is meaningless;
    /// the cross-dimension guard (deferred to step-6) must reject this.
    /// Currently (before step-6) the guard is absent, so this test is RED.
    #[test]
    fn project_point_origin_dimension_mismatch_undef() {
        // Point3 in LENGTH
        let pt = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        // Frame with dimensionless (Real) origin
        let dimensionless_frame = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(
            eval_builtin("project", &[pt, dimensionless_frame]).is_undef(),
            "point/origin dimension mismatch should be Undef"
        );
    }

    // ── affine_identity / affine_scale tests (step-1) ─────────────────────────

    /// Identity 3×3 matrix used as the expected `linear` part for several
    /// affine-constructor tests.
    const IDENTITY_3X3: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    /// Extract `(linear, translation)` from a `Value::AffineMap`, or panic.
    fn expect_affine(v: Value) -> ([[f64; 3]; 3], [f64; 3]) {
        match v {
            Value::AffineMap {
                linear,
                translation,
            } => (linear, translation),
            other => panic!("expected Value::AffineMap, got {:?}", other),
        }
    }

    #[test]
    fn affine_identity_no_args_returns_identity_map() {
        let (linear, translation) = expect_affine(eval_builtin("affine_identity", &[]));
        assert_eq!(linear, IDENTITY_3X3, "affine_identity linear must be I");
        assert_eq!(
            translation,
            [0.0, 0.0, 0.0],
            "affine_identity translation must be 0"
        );
    }

    #[test]
    fn affine_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("affine_identity", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("affine_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
    }

    #[test]
    fn affine_scale_diagonal_factors() {
        let args = [Value::Real(2.0), Value::Real(3.0), Value::Real(4.0)];
        let (linear, translation) = expect_affine(eval_builtin("affine_scale", &args));
        assert_eq!(
            linear,
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]],
            "affine_scale must place factors on the diagonal"
        );
        assert_eq!(
            translation,
            [0.0, 0.0, 0.0],
            "affine_scale translation must be 0"
        );
    }

    #[test]
    fn affine_scale_negative_factor_accepted() {
        // A negative factor is a valid orientation-reversing reflection (det<0).
        let args = [Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)];
        let (linear, _) = expect_affine(eval_builtin("affine_scale", &args));
        assert_eq!(linear[0][0], -1.0, "negative scale factor must be accepted");
    }

    #[test]
    fn affine_scale_wrong_arity_returns_undef() {
        assert!(eval_builtin("affine_scale", &[]).is_undef(), "0 args");
        assert!(
            eval_builtin("affine_scale", &[Value::Real(2.0)]).is_undef(),
            "1 arg"
        );
        assert!(
            eval_builtin("affine_scale", &[Value::Real(2.0), Value::Real(3.0)]).is_undef(),
            "2 args"
        );
        assert!(
            eval_builtin(
                "affine_scale",
                &[
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0),
                    Value::Real(5.0)
                ]
            )
            .is_undef(),
            "4 args"
        );
    }

    #[test]
    fn affine_scale_zero_factor_returns_undef() {
        // A zero factor is degenerate (det=0, non-invertible) and must be rejected.
        assert!(
            eval_builtin(
                "affine_scale",
                &[Value::Real(0.0), Value::Real(1.0), Value::Real(1.0)]
            )
            .is_undef(),
            "zero scale factor must be Undef"
        );
    }

    #[test]
    fn affine_scale_dimensioned_factor_returns_undef() {
        // A dimensioned factor violates the G6 dimensionless-linear-part contract.
        assert!(
            eval_builtin(
                "affine_scale",
                &[Value::length(2.0), Value::Real(1.0), Value::Real(1.0)]
            )
            .is_undef(),
            "dimensioned scale factor must be Undef"
        );
    }

    // ── affine_shear_* tests (step-3) ─────────────────────────────────────────

    /// The six shear constructors paired with their documented target cell per
    /// the `affine_shear_AB(k)` → `linear[A][B]` convention.
    const SHEAR_CASES: [(&str, usize, usize); 6] = [
        ("affine_shear_xy", 0, 1),
        ("affine_shear_xz", 0, 2),
        ("affine_shear_yx", 1, 0),
        ("affine_shear_yz", 1, 2),
        ("affine_shear_zx", 2, 0),
        ("affine_shear_zy", 2, 1),
    ];

    /// Build the expected shear `linear` matrix: identity with `k` at `[row][col]`.
    fn shear_linear(row: usize, col: usize, k: f64) -> [[f64; 3]; 3] {
        let mut m = IDENTITY_3X3;
        m[row][col] = k;
        m
    }

    #[test]
    fn affine_shear_places_k_at_documented_cell() {
        let k = 0.5;
        for (name, row, col) in SHEAR_CASES {
            let (linear, translation) = expect_affine(eval_builtin(name, &[Value::Real(k)]));
            assert_eq!(
                linear,
                shear_linear(row, col, k),
                "{name} must place k at linear[{row}][{col}], identity elsewhere"
            );
            assert_eq!(translation, [0.0, 0.0, 0.0], "{name} translation must be 0");
        }
    }

    #[test]
    fn affine_shear_dimensioned_k_returns_undef() {
        for (name, _, _) in SHEAR_CASES {
            assert!(
                eval_builtin(name, &[Value::length(0.5)]).is_undef(),
                "{name} with dimensioned k must be Undef"
            );
        }
    }

    #[test]
    fn affine_shear_wrong_arity_returns_undef() {
        for (name, _, _) in SHEAR_CASES {
            assert!(eval_builtin(name, &[]).is_undef(), "{name} 0 args");
            assert!(
                eval_builtin(name, &[Value::Real(1.0), Value::Real(2.0)]).is_undef(),
                "{name} 2 args"
            );
        }
    }

    // ── affine_translate tests (step-5) ───────────────────────────────────────

    #[test]
    fn affine_translate_length_components_stored_si_meters() {
        // affine_translate(5mm, 0, 0) → identity linear, translation [0.005, 0, 0] m.
        let args = [
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ];
        let (linear, translation) = expect_affine(eval_builtin("affine_translate", &args));
        assert_eq!(linear, IDENTITY_3X3, "affine_translate linear must be I");
        assert_eq!(
            translation,
            [0.005, 0.0, 0.0],
            "affine_translate translation must be SI meters"
        );
    }

    #[test]
    fn affine_translate_mixed_dimensions_returns_undef() {
        let args = [
            Value::length(1.0),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
            Value::length(3.0),
        ];
        assert!(
            eval_builtin("affine_translate", &args).is_undef(),
            "mixed-dimension components must be Undef"
        );
    }

    #[test]
    fn affine_translate_non_numeric_or_non_finite_returns_undef() {
        // Non-numeric component.
        let bad = [
            Value::String("x".to_string()),
            Value::length(0.0),
            Value::length(0.0),
        ];
        assert!(
            eval_builtin("affine_translate", &bad).is_undef(),
            "non-numeric component must be Undef"
        );
        // Non-finite component.
        let nan = [Value::Real(f64::NAN), Value::Real(0.0), Value::Real(0.0)];
        assert!(
            eval_builtin("affine_translate", &nan).is_undef(),
            "non-finite component must be Undef"
        );
    }

    #[test]
    fn affine_translate_wrong_arity_returns_undef() {
        assert!(eval_builtin("affine_translate", &[]).is_undef(), "0 args");
        assert!(
            eval_builtin("affine_translate", &[Value::length(1.0)]).is_undef(),
            "1 arg"
        );
        assert!(
            eval_builtin("affine_translate", &[Value::length(1.0), Value::length(2.0)]).is_undef(),
            "2 args"
        );
        assert!(
            eval_builtin(
                "affine_translate",
                &[
                    Value::length(1.0),
                    Value::length(2.0),
                    Value::length(3.0),
                    Value::length(4.0)
                ]
            )
            .is_undef(),
            "4 args"
        );
    }

    // ── affine_map tests (step-7) ─────────────────────────────────────────────

    /// Build a `Value::Matrix` of `Value::Real` rows from a row-major `[[f64;3];3]`.
    fn matrix3x3(data: [[f64; 3]; 3]) -> Value {
        Value::Matrix(
            data.iter()
                .map(|row| row.iter().map(|&x| Value::Real(x)).collect())
                .collect(),
        )
    }

    #[test]
    fn affine_map_builds_from_matrix_and_vector() {
        let m = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
        let translation_arg = Value::Vector(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let (linear, translation) =
            expect_affine(eval_builtin("affine_map", &[matrix3x3(m), translation_arg]));
        assert_eq!(
            linear, m,
            "affine_map linear must match the input matrix row-major"
        );
        assert_eq!(
            translation,
            [0.005, 0.0, 0.0],
            "affine_map translation must be SI meters"
        );
    }

    #[test]
    fn affine_map_non_3x3_matrix_returns_undef() {
        let translation_arg = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        // 2×2 matrix
        let m2x2 = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        assert!(
            eval_builtin("affine_map", &[m2x2, translation_arg.clone()]).is_undef(),
            "2x2 matrix must be Undef"
        );
        // 3×2 matrix
        let m3x2 = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
            vec![Value::Real(0.0), Value::Real(0.0)],
        ]);
        assert!(
            eval_builtin("affine_map", &[m3x2, translation_arg]).is_undef(),
            "3x2 matrix must be Undef"
        );
    }

    #[test]
    fn affine_map_dimensioned_linear_returns_undef() {
        // Linear part with Length elements violates the dimensionless contract.
        let m = Value::Matrix(vec![
            vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(1.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(0.0), Value::length(1.0)],
        ]);
        let translation_arg = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("affine_map", &[m, translation_arg]).is_undef(),
            "dimensioned linear part must be Undef"
        );
    }

    #[test]
    fn affine_map_translation_not_vec3_returns_undef() {
        let m = matrix3x3(IDENTITY_3X3);
        // Vector2 translation
        let v2 = Value::Vector(vec![Value::length(0.0), Value::length(0.0)]);
        assert!(
            eval_builtin("affine_map", &[m.clone(), v2]).is_undef(),
            "non-3 Vector translation must be Undef"
        );
        // Non-vector translation
        assert!(
            eval_builtin("affine_map", &[m, Value::Real(0.0)]).is_undef(),
            "non-Vector translation must be Undef"
        );
    }

    #[test]
    fn affine_map_wrong_arity_returns_undef() {
        let m = matrix3x3(IDENTITY_3X3);
        assert!(eval_builtin("affine_map", &[]).is_undef(), "0 args");
        assert!(eval_builtin("affine_map", &[m.clone()]).is_undef(), "1 arg");
        let translation_arg = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("affine_map", &[m, translation_arg, Value::Real(0.0)]).is_undef(),
            "3 args"
        );
    }
}
