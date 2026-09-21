use std::collections::BTreeMap;

use reify_core::DimensionVector;
use reify_ir::arg_acceptance::{Acceptance, ArgRejection, accept_arg, length_spec};
use reify_ir::{Value, quaternion_is_finite};

use crate::helpers::tensor_components_f64;
use crate::matrix::{mat3_det, matrix_components_f64};

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

/// The one and only dimension admitted on the linear half of a twist — and,
/// mirrored, on a `Transform`'s translation where it crosses the `transform_log`
/// ↔ `transform_exp` seam.
///
/// RULING #6126 (Leo, 2026-08-07): a twist is
/// `Map { angular: Vector3<…>, linear: Vector3<Length> }`. The linear half carries
/// LENGTH and only LENGTH; every other dimension — DIMENSIONLESS included — is
/// rejected as `Value::Undef` and explained by [`diagnose`].
///
/// Grounds (decision D11 of `docs/prds/v0_6/units-length-gate-completion.md`): after the
/// `Real` → `Scalar{DIMENSIONLESS}` unification, an "also admits DIMENSIONLESS"
/// gate means "also admits bare numbers", which is an affordance for exactly the
/// unit-less numerical work this seam should not silently accept. This closes the
/// last `LENGTH | DIMENSIONLESS` disjunction ON THE log↔exp SEAM — which is the whole
/// of RULING #6126's scope, and is NOT the same as "the transform family is uniformly
/// Length-only".
///
/// SCOPE BOUNDARY, so a future reader does not over-read the line above: `"transform3"`
/// applies NO dimension gate at all to its translation (only a 3-`Vector` shape check),
/// and `transform_compose` / `transform_inverse` propagate whatever dimension they are
/// handed. So `transform3(orient_identity(), vec3(1.0, 2.0, 3.0))` still CONSTRUCTS,
/// and the rejection only surfaces downstream at `transform_log`. That asymmetric seam
/// is deliberate and owned elsewhere: #6089 rules `Transform` translation LENGTH and
/// stamps the constructor arms, and #5747 R12/R8 narrows the affine and pose-decode
/// readers. Closing it here would double-migrate their work.
///
/// This const is the SINGLE source of truth for the admitted DIMENSION, consulted by
/// the `transform_log` eval arm, the `transform_exp` eval arm, and both of
/// [`diagnose`]'s RULING #6126 arms — so the eval gates and the post-`Undef` classifier
/// cannot drift apart (the same hazard the `stackup::parse_chain` / `parse_chain_checked` split
/// answers). [`decompose_twist_component`] plays the identical role for the twist SHAPE
/// the gates are applied to.
///
/// NOT applicable to `joint_jacobian`: its columns share the `{angular, linear}`
/// Map shape but are ∂pose/∂q, not twists (a revolute column's linear part is
/// m/rad), so they must never be held to this constant (#6102 gives them their own
/// structure).
const TWIST_LINEAR_DIM: DimensionVector = DimensionVector::LENGTH;

/// The dimension admitted on the ANGULAR half of a twist.
///
/// RULING #6080: a twist's `angular` half is `Vector3<Angle>` — and ONLY `Angle`. A
/// rotation vector is `axis * angle`, a dimensionless unit axis scaled by an angle, so
/// its magnitude IS an angle in radians; that is what makes d/dt of one an
/// ANGULAR_VELOCITY rather than a FREQUENCY, and what makes `transform_log`'s angular
/// half agree with the `orient_log` it is the SE(3) lift of.
///
/// DIMENSIONLESS was the previously-admitted spelling and is now rejected like any
/// other wrong dimension. It is NOT tolerated for back-compat: DIMENSIONLESS is a
/// SPECIFIC dimension (the zero exponent vector), not a wildcard, so admitting it as a
/// tolerant alias would re-open the hole PRD #5747 decision D11 closed for this very
/// family — the same grounds on which #6126 narrowed the linear half. The breaking
/// change is carried by a diagnostic rather than by silence; see [`diagnose`].
///
/// The const exists so that the value has ONE spelling instead of two, because two
/// co-dependent sites read it and they are 1000 lines apart:
///
/// 1. the `transform_exp` EVAL gate, which rejects a non-admitted angular half; and
/// 2. [`diagnose`]'s `transform_exp` arm, which now EXPLAINS exactly the rejection that
///    eval gate performs (it used to defer to it in silence, when #6080 had not yet
///    ruled), so a twist wrong in both halves is not mis-attributed to `linear`.
///
/// That agreement is only correct while both sites move together. Re-spelling the
/// literal at both sites made it unenforced, and the failure is SILENT in the dangerous
/// direction: move the eval gate alone and the classifier keeps testing for the old
/// dimension, so it mis-classifies every twist whose angular half is newly-valid — a
/// diagnostic regression no test that hardcodes a dimension can see.
/// `diagnose_transform_exp_deferral_tracks_evals_angular_gate` is the behavioural pin:
/// it builds its angular half FROM this const, so it follows the gate wherever the gate
/// moves and goes red if only one site moves.
const TWIST_ANGULAR_DIM: DimensionVector = DimensionVector::ANGLE;

/// Decompose one half of a twist — `Map { angular: Vector3<…>, linear: Vector3<…> }` —
/// into its three finite components and their single shared dimension.
///
/// This is the SINGLE source of truth for the twist SHAPE (the `Value::Map` match and
/// the field key), the way [`TWIST_LINEAR_DIM`] is the single source of truth for the
/// admitted dimension. The `transform_exp` eval arm and [`diagnose`]'s `transform_exp`
/// arm both go through it, so a later change to the key name — or to accepting a
/// `Point` alongside a `Vector` — cannot silently mute the classifier while eval keeps
/// rejecting. Without it both sites independently re-spell `Value::Map` →
/// `map.get(&Value::String("linear".into()))` → [`decompose_vec3`], which is the exact
/// drift the const was introduced to prevent.
///
/// Returns `None` for every SHAPE failure: a non-`Map` argument, a missing key, or a
/// value that is not a 3-`Vector` of finite, single-dimension components. Eval maps
/// that to `Value::Undef`; the classifier maps it to silence (no mis-attribution).
///
/// The eval arm's `angular` lookup is deliberately still spelled inline: #6080 owns the
/// angular half and will restructure that extraction along with its gate, so rewriting
/// it here would only manufacture a merge conflict. [`diagnose`] does route its angular
/// deferral through this helper, and that direction is fail-safe — a key-name drift
/// makes the deferral decline to speak rather than speak wrongly.
fn decompose_twist_component(v: &Value, key: &str) -> Option<([f64; 3], DimensionVector)> {
    let map = match v {
        Value::Map(m) => m,
        _ => return None,
    };
    decompose_vec3(map.get(&Value::String(key.to_string()))?)
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

/// Compute the SE(3) composition `a ∘ b` (Hamilton product for rotation, R_a·t_b + t_a for translation).
///
/// This is the typed, allocation-light extract of the `"transform_compose"` match arm.
/// The FK/Jacobian path calls this directly to avoid a stringly-typed `eval_geometry`
/// dispatch in the hot loop (PRD §7.2 rationale).
///
/// Returns `Value::Undef` for any of the conditions that the named builtin returns Undef:
/// - Either argument is not a `Value::Transform` with a finite `Orientation` and a LENGTH `Vector3`.
/// - The translation dimensions of `a` and `b` differ.
/// - Either quaternion has squared norm below the 1e-24 gate (see `normalize_quat_input`).
pub(crate) fn compose_transforms(a: &Value, b: &Value) -> Value {
    let (r1_q, t1, t1_dim) = match decompose_transform(a) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let (r2_q, t2, t2_dim) = match decompose_transform(b) {
        Some(v) => v,
        None => return Value::Undef,
    };
    if t1_dim != t2_dim {
        return Value::Undef;
    }
    // Normalize R1 and R2 symmetrically (matches operator-level semantics in reify-expr;
    // 1e-24 gate — see normalize_quat_input).
    let r1_n = match normalize_quat_input(r1_q) {
        Some(q) => q,
        None => return Value::Undef,
    };
    let r2_n = match normalize_quat_input(r2_q) {
        Some(q) => q,
        None => return Value::Undef,
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

// --- 3×3 linear algebra helpers for affine algebra (task γ) ---

/// Multiply two 3×3 row-major matrices.
fn mat3_mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut r = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                r[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    r
}

/// Apply a 3×3 row-major matrix to a column vector (matrix · vector).
fn mat3_apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// 3×3 matrix inverse via adjugate / cofactor method.
/// Returns `None` when the determinant is zero or non-finite.
fn affine_mat3_inv(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = mat3_det(m);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [
            (e * i - f * h) * inv_det,
            (c * h - b * i) * inv_det,
            (b * f - c * e) * inv_det,
        ],
        [
            (f * g - d * i) * inv_det,
            (a * i - c * g) * inv_det,
            (c * d - a * f) * inv_det,
        ],
        [
            (d * h - e * g) * inv_det,
            (b * g - a * h) * inv_det,
            (a * e - b * d) * inv_det,
        ],
    ])
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
        // (decompose_xyz3 contract) AND that shared dimension to be LENGTH
        // (task 5747, units-length ζ / R12); otherwise `Value::Undef`.
        //
        // A translation is a DISPLACEMENT, so a bare or wrong-dimension triple
        // was previously read straight into the map's SI-metre translation with
        // the dimension silently discarded — `affine_translate(5kg, 0kg, 0kg)`
        // built a 5-METRE translation at exit 0 (measured pre-ζ).
        //
        // `decompose_xyz3` itself is deliberately NOT tightened: it backs
        // `decompose_vec3` / `decompose_point3`, whose ~15 other callers include
        // `transform_exp`'s ANGULAR twist field and the point/plane/axis
        // constructors, several legitimately non-LENGTH. The gate goes at the
        // CALL SITE. The user-facing rejection is minted by `diagnose` below,
        // which is what turns this `Undef` into a nonzero `reify eval` exit.
        "affine_translate" => {
            let t = match decompose_xyz3(args) {
                Some((t, dim)) if dim == DimensionVector::LENGTH => t,
                _ => return Some(Value::Undef),
            };
            Value::AffineMap {
                linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                translation: t,
            }
        }
        // `affine_map(linear, translation)`: general construction from a 3×3
        // dimensionless matrix (row-major) and a Vector3 translation (stored in SI
        // meters). The linear part must be exactly 3×3 and dimensionless (G6
        // dimensionless-linear-part contract), and the TRANSLATION must be LENGTH
        // (task 5747, units-length ζ / R12); otherwise `Value::Undef`.
        //
        // The two halves are independent and stay that way: D11 keeps the LINEAR
        // part dimensionless-required, so ζ leaves it alone, and the shared
        // classifier checks it FIRST so a dimensioned-linear call is never
        // mislabelled as a translation rejection.
        //
        // The acceptance predicate itself lives in `classify_affine_map_args`,
        // which `diagnose` also consults — the two cannot drift apart, so a
        // rejection here is always paired with the message that explains it.
        // EVERY fault is `Undef` at this layer; only `diagnose` distinguishes
        // them, and only the units one becomes a user-facing rejection.
        "affine_map" => match classify_affine_map_args(args) {
            Ok((linear, translation)) => Value::AffineMap {
                linear,
                translation,
            },
            Err(_) => Value::Undef,
        },
        // `affine_from_transform(t)`: widen a rigid Transform to a general affine
        // map. The rotation quaternion becomes an orthogonal 3×3 (det=+1) whose
        // columns are R·x̂, R·ŷ, R·ẑ (built via quat_rotate on the basis vectors),
        // and the translation passes through in SI meters. The identity quaternion
        // yields the identity matrix exactly. Non-Transform / bad arity → Undef.
        "affine_from_transform" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (q, translation, _dim) = match decompose_transform(&args[0]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Rotation-matrix columns = R applied to each basis vector.
            let (c0x, c0y, c0z) = quat_rotate(q, 1.0, 0.0, 0.0);
            let (c1x, c1y, c1z) = quat_rotate(q, 0.0, 1.0, 0.0);
            let (c2x, c2y, c2z) = quat_rotate(q, 0.0, 0.0, 1.0);
            // Store row-major: linear[row][col], where col_i is the i-th column.
            let linear = [[c0x, c1x, c2x], [c0y, c1y, c2y], [c0z, c1z, c2z]];
            Value::AffineMap {
                linear,
                translation,
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
            // Extract angular: must be Vector3<Angle>.
            //
            // Twist angular convention (RULING #6080): a rotation vector is
            // `axis * angle`, so `angular` carries ANGLE. Every other dimension —
            // DIMENSIONLESS included — returns Undef here AND is explained by
            // `diagnose`, which names the offending dimension rather than leaving a
            // bare OpContractViolation note. `transform_log` emits ANGLE on the same
            // half, so both ends of the log↔exp seam agree on what they admit.
            //
            // The gate is spelled via `TWIST_ANGULAR_DIM` because `diagnose`'s
            // transform_exp arm reads the SAME const and must not drift from it; see
            // that const's doc for the co-dependence.
            //
            // The shape (Map + field key) is read through `decompose_twist_component`,
            // the SAME helper the linear half and `diagnose` use — the restructure the
            // helper's doc reserved for #6080. Every shape failure it folds into `None`
            // (non-Map arg, missing key, not a 3-Vector, mixed or non-finite
            // components) was already Undef here.
            let (ang_comps, ang_dim) = match decompose_twist_component(&args[0], "angular") {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            if ang_dim != TWIST_ANGULAR_DIM {
                return Some(Value::Undef);
            }
            let (wx, wy, wz) = (ang_comps[0], ang_comps[1], ang_comps[2]);
            // Extract linear: must be Vector3 with a single shared dimension.
            //
            // Twist linear convention (RULING #6126): `linear` must be
            // `Vector3<Length>`. Any other dimension — DIMENSIONLESS included — returns
            // Undef here AND is explained by `diagnose`, which names the offending
            // dimension rather than leaving a bare OpContractViolation note.
            //
            // `transform_log` applies the identical `TWIST_LINEAR_DIM` gate to a
            // Transform's translation, so both ends of the log↔exp seam agree on what
            // they admit.
            //
            // The shape (Map + field key) is read through `decompose_twist_component`,
            // the SAME helper `diagnose`'s transform_exp arm uses, so eval and the
            // classifier cannot disagree about what a twist's `linear` half even IS.
            // Every shape failure it folds into `None` — non-Map arg, missing key, not a
            // 3-Vector, mixed or non-finite components — was already Undef here; moving
            // the missing-key check after the angular gate is not observable, since both
            // orders yield Undef.
            let (lin_comps, lin_dim) = match decompose_twist_component(&args[0], "linear") {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            if lin_dim != TWIST_LINEAR_DIM {
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
            // Transform translation convention (RULING #6126): the translation must be
            // Vector3<Length> — LENGTH and nothing else, DIMENSIONLESS included. A
            // non-LENGTH translation returns Undef here AND is explained by `diagnose`,
            // which names the offending dimension rather than leaving a bare
            // OpContractViolation note.
            //
            // `transform_exp` applies the identical `TWIST_LINEAR_DIM` gate to a twist's
            // `linear` field, so both ends of the log↔exp seam agree on what they admit.
            if t_dim != TWIST_LINEAR_DIM {
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
            // The angular half is the rotation vector theta * n_hat, whose magnitude is
            // an angle in radians, so it carries `TWIST_ANGULAR_DIM` (RULING #6080) —
            // built through the SAME helper the linear half uses, so both halves of the
            // emitted twist are constructed the same way.
            m.insert(
                Value::String("angular".to_string()),
                Value::Vector(vec![
                    make_dimensioned_component(TWIST_ANGULAR_DIM, wx),
                    make_dimensioned_component(TWIST_ANGULAR_DIM, wy),
                    make_dimensioned_component(TWIST_ANGULAR_DIM, wz),
                ]),
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
            compose_transforms(&args[0], &args[1])
        }

        // --- Plane constructors ---
        "plane_xy" => make_plane(args, 2, [0.0, 0.0, 1.0]),
        "plane_xz" => make_plane(args, 1, [0.0, 1.0, 0.0]),
        "plane_yz" => make_plane(args, 0, [1.0, 0.0, 0.0]),

        // --- Axis constructors ---
        "axis_x" => make_axis(args, [1.0, 0.0, 0.0]),
        "axis_y" => make_axis(args, [0.0, 1.0, 0.0]),
        "axis_z" => make_axis(args, [0.0, 0.0, 1.0]),

        // --- Construction-datum constructors (geometric-relations η, task 4387) ---
        "midplane" => eval_midplane(args),
        "axis_through" => eval_axis_through(args),
        "plane_through" => eval_plane_through(args),
        "offset" => eval_offset_plane(args),
        "frame_at" => eval_frame_at(args),

        // --- BoundingBox constructors ---
        //
        // A BoundingBox is Length-valued BY CONSTRUCTION (task 6081, ruling
        // from esc-5997-2): `bbox` admits only `Point3<Length>` corners, and
        // both accessors emit `Length` components unconditionally — even for a
        // hand-built or kernel-produced box whose stored corners say otherwise.
        //
        // The quantity polymorphism this replaced was NOT a designed
        // capability: it was incidental generic component-dimension
        // propagation. The old gate only checked that the two corners AGREED,
        // and the accessors merely echoed whatever dimension they found. Do not
        // re-derive it as intentional. The only real producer,
        // `dispatch_bounding_box` (reify-eval/src/geometry_ops.rs), is
        // unconditionally `Point3<Length>`, and the sole `.ri` consumer
        // (examples/differential_field_ops.ri) is metre-valued.
        //
        // DELIBERATE REINTERPRETATION, and the one place where this ruling
        // trades a check for a guarantee. The accessors do not merely decline to
        // propagate a non-Length stored dimension — for a UNIFORMLY non-Length
        // corner (an all-Angle box, say) they RELABEL those magnitudes as metres.
        // A MIXED-dimension stored corner is still rejected outright, because the
        // accessors read corners through `tensor_components_f64`. The relabelling
        // is what makes reify-compiler's static rows `Vector3<Length>` /
        // `Point3<Length>` sound rather than an over-claim: a row true only of
        // constructor-produced boxes would be the very static/runtime
        // disagreement this ruling removes, since `Value::BoundingBox` is also
        // minted by `dispatch_bounding_box` and constructible directly in Rust.
        // The narrowed constructor above is what makes the relabelled input
        // impossible in the first place; a uniformly non-Length stored corner can
        // now only be a PRODUCER bug, and if one ever appears the better answer
        // is to reject it here (with a `diagnose` arm for `bbox_size` /
        // `bbox_center`) rather than to coerce it. Not done today: there is no
        // such producer, and the tests
        // `bbox_size/bbox_center_emits_length_components_for_angle_bbox` pin the
        // coercion as the current, deliberate behaviour.
        //
        // Going monomorphic is the SAFE direction: a later `BoundingBox<Q>`
        // would be a WIDENING, which is the easy one. For scale, the static
        // type is a bare unit variant referenced across six crates
        // (reify-compiler, reify-core, reify-eval, reify-expr, reify-ir,
        // reify-kernel-openvdb) — re-derive the current site list with
        //   grep -rn --include=*.rs -E 'Type::BoundingBox|Type::bounding_box\(\)' crates gui
        // rather than trusting a count quoted here (the house convention on
        // rotting inventory numbers; see `NAMED_DIMENSIONS` in reify-core).
        "bbox" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let min = &args[0];
            let max = &args[1];
            // A BoundingBox is Length-valued by construction (task 6081): both
            // corners must be `Point3<Length>` — UNIFORMLY, in every component.
            // The quantity is therefore read through [`classify_bbox_corner`] and
            // NOT off component 0: a first-component reading admitted
            // `bbox(point3(1m, 2deg, 3m), …)`, whose stored corner is not
            // Length-valued at all, and merely displaced the failure one call
            // downstream to `bbox_size`/`bbox_center` — where, the `bbox` call
            // having SUCCEEDED, the post-Undef classifier never fires and the user
            // gets exactly the silent Undef this ruling exists to remove.
            //
            // This subsumes the older `min_dim != max_dim` gate: any non-Length
            // corner is rejected, so mismatched corners cannot both be Length
            // either. The explanation is emitted by the post-Undef classifier
            // `diagnose` below, not here — and it decodes the corners through the
            // SAME helper, so the two cannot drift in the shape dimension any more
            // than they can in the quantity one.
            for corner in [min, max] {
                if classify_bbox_corner(corner) != BboxCorner::Uniform(DimensionVector::LENGTH) {
                    return Some(Value::Undef);
                }
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
                    // A BoundingBox is Length-valued by construction (task 6081),
                    // so the extent is Length regardless of what the stored
                    // corners carry — the stored dimension is deliberately NOT
                    // propagated. See the DELIBERATE REINTERPRETATION note on the
                    // `bbox` constructor banner above: for a uniformly non-Length
                    // stored corner this RELABELS the magnitudes as metres rather
                    // than rejecting them, which is sound only because such a box
                    // is impossible by construction.
                    let (min_vals, _) = match tensor_components_f64(min) {
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
                    Value::Vector(vec![
                        Value::length(max_vals[0] - min_vals[0]),
                        Value::length(max_vals[1] - min_vals[1]),
                        Value::length(max_vals[2] - min_vals[2]),
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
                    // A BoundingBox is Length-valued by construction (task 6081),
                    // so the centre is Length regardless of what the stored
                    // corners carry — the stored dimension is deliberately NOT
                    // propagated. Same DELIBERATE REINTERPRETATION note as
                    // `bbox_size` above; see the constructor banner.
                    let (min_vals, _) = match tensor_components_f64(min) {
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
                    Value::Point(vec![
                        Value::length((min_vals[0] + max_vals[0]) / 2.0),
                        Value::length((min_vals[1] + max_vals[1]) / 2.0),
                        Value::length((min_vals[2] + max_vals[2]) / 2.0),
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

        // --- Affine algebra free-functions (task γ) ---
        // `affine_compose(a, b) -> AffineMap`: a∘b, apply b first then a.
        // linear = a.linear · b.linear; translation = a.linear · b.translation + a.translation.
        // Left-applied convention (matches OCCT gp_GTrsf::Multiply).
        "affine_compose" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (a_linear, a_trans) = match &args[0] {
                Value::AffineMap { linear, translation } => (*linear, *translation),
                _ => return Some(Value::Undef),
            };
            let (b_linear, b_trans) = match &args[1] {
                Value::AffineMap { linear, translation } => (*linear, *translation),
                _ => return Some(Value::Undef),
            };
            let linear = mat3_mul(a_linear, b_linear);
            let applied = mat3_apply(a_linear, b_trans);
            let translation = [
                applied[0] + a_trans[0],
                applied[1] + a_trans[1],
                applied[2] + a_trans[2],
            ];
            // Guard: reject any non-finite component.
            if linear.iter().any(|row| row.iter().any(|&v| !v.is_finite()))
                || translation.iter().any(|&v| !v.is_finite())
            {
                return Some(Value::Undef);
            }
            Value::AffineMap { linear, translation }
        }

        // `affine_inverse(a) -> Option<AffineMap>`:
        // Invertible (det ≠ 0) → Some(AffineMap{ linear=a.linear⁻¹, translation=−a.linear⁻¹·a.trans }).
        // Singular (det = 0) → Value::Option(None) — NOT Undef, so authors can branch.
        "affine_inverse" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (a_linear, a_trans) = match &args[0] {
                Value::AffineMap { linear, translation } => (*linear, *translation),
                _ => return Some(Value::Undef),
            };
            match affine_mat3_inv(a_linear) {
                None => Value::Option(None),
                Some(inv) => {
                    let applied = mat3_apply(inv, a_trans);
                    let translation = [-applied[0], -applied[1], -applied[2]];
                    if inv.iter().any(|row| row.iter().any(|&v| !v.is_finite()))
                        || translation.iter().any(|&v| !v.is_finite())
                    {
                        return Some(Value::Option(None));
                    }
                    Value::Option(Some(Box::new(Value::AffineMap {
                        linear: inv,
                        translation,
                    })))
                }
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

/// Build a Plane from a single offset argument, which is REQUIRED to be a
/// Length — units-length ε, R11 / decision D4 of
/// `docs/prds/v0_6/units-length-gate-completion.md`.
///
/// The offset is the only dimensioned input, so the whole origin triple
/// consequently carries LENGTH: the two zeros are minted at the gated dimension,
/// and `plane_xy(5mm)` has origin `(0m, 0m, 5mm)`. That mirroring is VERIFIED by
/// `plane_xy_length_zero_mirrors_length_into_origin_and_keeps_normal_bare`
/// rather than argued here, which is why the origin is built from
/// [`make_point3`] at LENGTH instead of from a closure that still carried a
/// DIMENSIONLESS branch the gate has made unreachable. The synthesized normal
/// stays DIMENSIONLESS: a unit vector legitimately has bare components
/// (decision D3), and nothing in ε widens the gate to it.
///
/// SCOPE. ε gates THIS producer family (`plane_xy` / `plane_xz` / `plane_yz`,
/// and [`make_axis`] beside it) and nothing else. The five sibling
/// construction-datum constructors — `midplane`, `axis_through`, `plane_through`,
/// the arity-2 `offset` and `frame_at` (task 4387 / η, the section below) —
/// stay dimension-POLYMORPHIC: they enforce dimension AGREEMENT among their
/// inputs, never LENGTH. `decode_plane`'s consumer-side `ox`/`oy`/`oz` gate
/// (task δ / 5745) therefore stays live and reachable through them, which is why
/// ε shuts R11 at both ends rather than retiring either.
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
    if offset_val.dimension() != DimensionVector::LENGTH {
        return Value::Undef;
    }
    let mut origin_si = [0.0; 3];
    origin_si[offset_index] = offset_f;
    let normal_vec = Value::Vector(vec![
        Value::Real(normal[0]),
        Value::Real(normal[1]),
        Value::Real(normal[2]),
    ]);
    Value::Plane {
        origin: Box::new(make_point3(origin_si, DimensionVector::LENGTH)),
        normal: Box::new(normal_vec),
    }
}

/// Build an Axis from a single `Point3<Length>` origin argument, which is
/// REQUIRED to carry LENGTH — units-length ε, R11 / decision D4 of
/// `docs/prds/v0_6/units-length-gate-completion.md`.
///
/// The origin is decoded through [`decompose_xyz3`] — the SAME helper
/// [`length_group_rejection`] reads, and therefore the same one [`diagnose`]'s
/// axis arm reaches the verdict through. That sharing is load-bearing, not
/// tidiness: a gate and its post-`Undef` classifier that spell the same
/// predicate twice drift ASYMMETRICALLY, leaving eval returning `Undef` while
/// the classifier stays silent — which degrades a loud rejection back to the
/// silent `undef` at exit 0 this PRD exists to close, with no test failing
/// because every case exercises a value both spellings agree on. It is the
/// discipline [`classify_bbox_corner`] and [`classify_affine_map_args`] already
/// establish in this file.
///
/// Reading through that helper also TIGHTENS the gate beyond dimension: the
/// shape-only check it replaces admitted non-numeric, non-finite and
/// mixed-dimension component triples. Those are shape/consistency failures, not
/// units ones, and they now fail closed here just as they long have in
/// [`make_plane`].
///
/// That tightening costs no diagnosability, because NO `.ri` source can reach
/// it — the widened cases are unconstructible one layer earlier, and were
/// equally unreachable before this gate existed. MEASURED against
/// `target/debug/reify`, not argued:
///
/// - `point3(1mm, 2deg, 3mm)` is itself `undef` — [`construct_point_or_vector`]
///   requires ONE shared dimension, so the mixed-dimension `Value::Point` is
///   never built and this function receives `Value::Undef`, which the arity/shape
///   guard has always rejected;
/// - `1mm / 0.0` is itself `undef`, so a non-finite LENGTH component cannot be
///   assembled into a `Point` either.
///
/// Every non-test site that CONSTRUCTS a `Value::Point` in this workspace is
/// dimension-uniform by construction, so the widened cases are reachable only
/// from a HAND-BUILT value in a test. `decode_axis`'s per-coordinate `ox`/`oy`/`oz`
/// attribution for such a triple was unreachable from source for exactly the same
/// reason, so nothing a user could write has lost a diagnostic. This is why
/// [`diagnose`]'s axis arm stays SILENT on them rather than growing a
/// per-component scan: the silence is a no-mis-attribution contract over inputs
/// no author can produce, not a swallowed fault.
///
/// The accepted origin is cloned VERBATIM so a `Point3<Length>` round-trips
/// byte-identically (`decode_axis_producer_round_trip_*`). The synthesized
/// direction stays DIMENSIONLESS: a unit vector legitimately has bare components
/// (decision D3). See [`make_plane`]'s doc for the scope statement the two
/// share.
fn make_axis(args: &[Value], direction: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let Value::Point(comps) = &args[0] else {
        return Value::Undef;
    };
    match decompose_xyz3(comps) {
        Some((_, dim)) if dim == DimensionVector::LENGTH => {}
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

// ── Construction-datum constructor helpers (geometric-relations η, task 4387) ──
// Pure kernel-free value-algebra mirroring make_plane / make_axis: every helper
// returns `Value::Undef` on bad arity / type / dimension-mismatch / degenerate
// (zero-length normal or direction, coincident/collinear points) inputs, per the
// point3 / plane_xy Undef convention. These are the eval side of the compiler's
// `datum_constructor_result_type` family; `offset` is the arity-2 datum constructor
// (the arity-3 `offset` is the γ relation, compiled, never eval'd here).

/// 3-vector cross product.
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Normalize a 3-vector to unit length; `None` for a zero-length or non-finite vector.
fn normalize3(a: [f64; 3]) -> Option<[f64; 3]> {
    let norm = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    let r = [a[0] / norm, a[1] / norm, a[2] / norm];
    if r.iter().all(|v| v.is_finite()) {
        Some(r)
    } else {
        None
    }
}

/// Wrap an SI value in the appropriate numeric `Value` for `dim` (`Real` when
/// dimensionless, else `Scalar`). Mirrors the `make_component` closures in the
/// bbox arms.
fn dimensioned_component(si_value: f64, dim: DimensionVector) -> Value {
    if dim.is_dimensionless() {
        Value::Real(si_value)
    } else {
        Value::Scalar {
            si_value,
            dimension: dim,
        }
    }
}

/// Build a `Value::Point` of three SI components carrying `dim`.
fn make_point3(xyz: [f64; 3], dim: DimensionVector) -> Value {
    Value::Point(vec![
        dimensioned_component(xyz[0], dim),
        dimensioned_component(xyz[1], dim),
        dimensioned_component(xyz[2], dim),
    ])
}

/// Build a dimensionless `Value::Vector` of three `Real` components (a normal/direction).
fn make_real_vec3(xyz: [f64; 3]) -> Value {
    Value::Vector(vec![
        Value::Real(xyz[0]),
        Value::Real(xyz[1]),
        Value::Real(xyz[2]),
    ])
}

/// Decode a `Value::Plane` into (origin[3], origin_dim, normal[3]). `None` for any
/// non-Plane / non-3D / mixed-dimension / non-finite input. Reuses the shared
/// `decompose_point3` (origin) and `decompose_vec3` (normal) validators.
fn decode_plane(v: &Value) -> Option<([f64; 3], DimensionVector, [f64; 3])> {
    let (origin, normal) = match v {
        Value::Plane { origin, normal } => (origin.as_ref(), normal.as_ref()),
        _ => return None,
    };
    let (o, o_dim) = decompose_point3(origin)?;
    // A plane normal may be a dimensionless `Value::Vector` (synthetic, e.g. from
    // `make_real_vec3` / `plane_xy`) OR a `Value::Direction` (how a kernel-realized
    // feature→datum plane carries its normal). Accept both so the construction-datum
    // constructors (`offset` / `midplane`) consume a REALIZED plane — the actual
    // use case — not only a synthetic one.
    let n = match decompose_vec3(normal) {
        Some((n, _)) => n,
        None => decode_direction(normal)?,
    };
    Some((o, o_dim, n))
}

/// Decode a `Value::Direction` into [x,y,z]; `None` for any other value or a
/// non-finite component.
fn decode_direction(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Direction { x, y, z } if x.is_finite() && y.is_finite() && z.is_finite() => {
            Some([*x, *y, *z])
        }
        _ => None,
    }
}

/// Why a construction-datum call cannot be built — the fault vocabulary of the
/// five per-builtin classifiers below, each of which is the ONE acceptance
/// predicate its eval gate and its [`diagnose`] arm both consult (units-length
/// η, task 6591).
///
/// This is the [`AffineMapFault`] / [`BboxCorner`] discipline applied to the
/// construction-datum family, for the reason this file already records: a gate
/// and its post-`Undef` classifier that spell the same predicate twice drift
/// ASYMMETRICALLY, leaving eval returning `Undef` while the classifier stays
/// silent — which degrades a loud rejection back to the silent `undef` at exit 0
/// this PRD exists to close, with no test failing because every case exercises a
/// value both spellings agree on.
///
/// It is not hypothetical for this family. `midplane` and the arity-2 `offset`
/// reject through [`decode_plane`], which validates the NORMAL as well as the
/// origin — so a classifier that destructured `Value::Plane { origin, .. }` and
/// read only the origin would emit a LENGTH rejection for a plane whose actual
/// fault is a malformed normal.
enum DatumFault {
    /// Every SHAPE / CONSISTENCY cause: wrong arity, the wrong `Value` variant,
    /// a component count other than three, a non-numeric or non-finite
    /// component, MIXED component dimensions, or a plane whose NORMAL cannot be
    /// decoded. A shape failure is never blamed on a dimension, so [`diagnose`]
    /// stays SILENT for all of it — matching the `transform3` / `bbox`
    /// convention. Carries no parameter name because none of these causes
    /// belongs to one argument.
    Shape,
    /// A POSITION operand is well-formed and its dimension is not LENGTH. The
    /// ONE fault this family diagnoses.
    NotLength {
        /// The offending parameter as the author wrote it, taken from the
        /// datum-constructor signature block in `reify-compiler/src/units.rs`.
        arg_name: &'static str,
        /// Minted by the shared owner, so the wording is never re-rendered here
        /// (Contract C1 invariant (i)).
        rejection: ArgRejection,
    },
}

/// Classify a `Value::Point` operand that must be a `Point3<Length>`.
///
/// Adds ONLY the LENGTH verdict on top of [`decompose_xyz3`], taking both the
/// verdict and its wording from [`length_group_rejection`], so ONE predicate
/// owns "is this a LENGTH rejection" for the whole crate.
///
/// The message names the whole coordinate group under `arg_name` because
/// `decompose_xyz3` has ALREADY required the three components to share one
/// dimension: when this fires, all three offend identically, so naming the
/// parameter the author actually wrote is complete information rather than a
/// shortcut.
fn classify_datum_point(arg_name: &'static str, v: &Value) -> Result<[f64; 3], DatumFault> {
    let Value::Point(comps) = v else {
        return Err(DatumFault::Shape);
    };
    if let Some(rejection) = length_group_rejection(comps) {
        return Err(DatumFault::NotLength {
            arg_name,
            rejection,
        });
    }
    match decompose_xyz3(comps) {
        Some((xyz, dim)) if dim == DimensionVector::LENGTH => Ok(xyz),
        // `length_group_rejection` said nothing, so this is its OTHER silent
        // cause: a `decompose_xyz3` shape/consistency failure. The non-LENGTH arm
        // is unreachable (the rejection branch above owns it) and falls here
        // deliberately — fail CLOSED, exactly as `classify_affine_map_args` does.
        _ => Err(DatumFault::Shape),
    }
}

/// Classify a `Value::Plane` operand whose ORIGIN must be a `Point3<Length>`,
/// returning its origin and normal components.
///
/// The whole SHAPE question — the `Plane` variant, the origin triple AND the
/// dual `Vector`-or-`Direction` normal — is read through [`decode_plane`], the
/// SAME decoder the eval gates have always used, and it is read BEFORE any
/// dimension is judged. That ordering is what makes a malformed normal a
/// `Shape` fault rather than a `NotLength` one: such a plane never reaches the
/// LENGTH verdict, so its fault is never blamed on its origin's dimension.
///
/// Preserving `decode_plane`'s dual normal acceptance is load-bearing rather
/// than conservative: the one real `.ri` call site of this family
/// (`examples/geometric_relations/construction_datum.ri`) feeds a
/// kernel-realized plane whose normal is a `Value::Direction`, so narrowing the
/// normal decode would break the shipped example while the unit suite stayed
/// green.
fn classify_datum_plane(
    arg_name: &'static str,
    v: &Value,
) -> Result<([f64; 3], [f64; 3]), DatumFault> {
    let (_, _, normal) = decode_plane(v).ok_or(DatumFault::Shape)?;
    let Value::Plane { origin, .. } = v else {
        // Unreachable: `decode_plane` returns `None` for every non-`Plane` value.
        return Err(DatumFault::Shape);
    };
    let o = classify_datum_point(arg_name, origin)?;
    Ok((o, normal))
}

/// Classify a scalar operand that must carry LENGTH — `offset`'s `delta`.
///
/// [`classify_datum_point`] one rank down: the verdict and the wording both come
/// from [`length_scalar_rejection`], and every cause that predicate folds into
/// `None` other than acceptance is a `Shape` fault here.
fn classify_datum_delta(arg_name: &'static str, v: &Value) -> Result<f64, DatumFault> {
    if let Some(rejection) = length_scalar_rejection(v) {
        return Err(DatumFault::NotLength {
            arg_name,
            rejection,
        });
    }
    match v.as_f64() {
        Some(d) if d.is_finite() && v.dimension() == DimensionVector::LENGTH => Ok(d),
        // A non-numeric or non-finite value — `length_scalar_rejection`'s other
        // silent causes. Fail CLOSED, as above.
        _ => Err(DatumFault::Shape),
    }
}

/// A decoded plane: its origin components and its normal components.
type PlaneParts = ([f64; 3], [f64; 3]);

/// Three decoded `Point3<Length>` positions, in argument order — what
/// [`classify_plane_through_args`] yields.
type PointTriple = ([f64; 3], [f64; 3], [f64; 3]);

/// A decoded frame specification: the origin's components, then the x and z
/// direction components — what [`classify_frame_at_args`] yields. The same
/// SHAPE as [`PointTriple`] and a deliberately different NAME: only the first
/// member is a gated position, and the other two are unit vectors that carry no
/// dimension (decision D3).
type FrameParts = ([f64; 3], [f64; 3], [f64; 3]);

/// Decode `midplane(a, b)`'s two planes, or say why they cannot be decoded.
///
/// One of the five per-builtin classifiers that own this family's acceptance.
/// Each is read by BOTH its eval gate and its [`diagnose`] arm — see
/// [`DatumFault`] for why — so guard ORDER is a property of this one function
/// rather than a convention restated at each reader: `a` is classified before
/// `b`, and a both-wrong call therefore names `a` deterministically at both
/// readers (the [`diagnose_bbox_corners`] `min`-before-`max` precedent).
fn classify_midplane_args(args: &[Value]) -> Result<(PlaneParts, PlaneParts), DatumFault> {
    if args.len() != 2 {
        return Err(DatumFault::Shape);
    }
    let a = classify_datum_plane("a", &args[0])?;
    let b = classify_datum_plane("b", &args[1])?;
    Ok((a, b))
}

/// Decode `axis_through(a, b)`'s two points. See [`classify_midplane_args`] for
/// the shared-classifier and guard-order rationale.
fn classify_axis_through_args(args: &[Value]) -> Result<([f64; 3], [f64; 3]), DatumFault> {
    if args.len() != 2 {
        return Err(DatumFault::Shape);
    }
    let a = classify_datum_point("a", &args[0])?;
    let b = classify_datum_point("b", &args[1])?;
    Ok((a, b))
}

/// Decode `plane_through(a, b, c)`'s three points. See
/// [`classify_midplane_args`] for the shared-classifier and guard-order
/// rationale.
fn classify_plane_through_args(args: &[Value]) -> Result<PointTriple, DatumFault> {
    if args.len() != 3 {
        return Err(DatumFault::Shape);
    }
    let a = classify_datum_point("a", &args[0])?;
    let b = classify_datum_point("b", &args[1])?;
    let c = classify_datum_point("c", &args[2])?;
    Ok((a, b, c))
}

/// Decode the arity-2 `offset(p, delta)`. See [`classify_midplane_args`] for the
/// shared-classifier and guard-order rationale: `p` is classified before
/// `delta`, so a call wrong in both names `p`.
///
/// The `args.len() != 2` guard is load-bearing beyond arity hygiene: `offset` is
/// the one arity-OVERLOADED name in this family, and the arity-3 form is the γ
/// RELATION — claimed by an earlier compiler arm and never evaluated in this
/// module. Without the guard, [`diagnose`]'s arm would read `args[1]` on a
/// three-argument call and attribute a datum-constructor units fault to a
/// builtin this gate has no authority over.
fn classify_offset_plane_args(args: &[Value]) -> Result<(PlaneParts, f64), DatumFault> {
    if args.len() != 2 {
        return Err(DatumFault::Shape);
    }
    let plane = classify_datum_plane("p", &args[0])?;
    let delta = classify_datum_delta("delta", &args[1])?;
    Ok((plane, delta))
}

/// Decode `frame_at(o, x, z)`'s origin and its two axis directions.
///
/// Only `o` is a POSITION and only `o` is gated. The x/z operands arrive as
/// `Value::Direction` — three plain `f64` fields, with no dimension to gate —
/// and are decoded for SHAPE alone, because a unit vector legitimately has bare
/// components (decision D3). See [`classify_midplane_args`] for the rest of the
/// rationale.
fn classify_frame_at_args(args: &[Value]) -> Result<FrameParts, DatumFault> {
    if args.len() != 3 {
        return Err(DatumFault::Shape);
    }
    let o = classify_datum_point("o", &args[0])?;
    let x = decode_direction(&args[1]).ok_or(DatumFault::Shape)?;
    let z = decode_direction(&args[2]).ok_or(DatumFault::Shape)?;
    Ok((o, x, z))
}

/// `midplane(a: Plane, b: Plane) -> Plane`: the bisecting plane.
/// normal = normalize(na + nb); origin = midpoint(oa, ob).
///
/// BOTH plane ORIGINS are REQUIRED to be `Point3<Length>` — units-length η
/// (task 6591), the same R11 rule ε gates at [`make_plane`] / [`make_axis`] one
/// family over. `Undef` if either origin is not LENGTH, if either plane cannot
/// be decoded, or if the summed normal is zero (anti-parallel normals).
///
/// That gate REPLACES the dimension-AGREEMENT comparison this function used to
/// make rather than stacking on top of it: requiring LENGTH of both origins
/// accepts exactly the set agreement-plus-LENGTH would, so the comparison is
/// dead code, not a second layer of defence. The invariant is still redundantly
/// enforced where that has value — at the CONSUMER end, by task δ's
/// [`decode_plane`] gate — which is a genuinely independent layer rather than a
/// duplicated predicate one line away.
///
/// The verdict is reached through [`classify_midplane_args`], the SAME predicate
/// [`diagnose`]'s `midplane` arm reads; the degeneracy guard is ordered AFTER
/// it, so a bare-origin call is attributed to its dimension rather than to a
/// degeneracy. The two NORMALS stay dimension-agnostic (decision D3), and the
/// bisector's origin is minted at LENGTH because both inputs now are.
fn eval_midplane(args: &[Value]) -> Value {
    let Ok(((oa, na), (ob, nb))) = classify_midplane_args(args) else {
        return Value::Undef;
    };
    let normal = match normalize3([na[0] + nb[0], na[1] + nb[1], na[2] + nb[2]]) {
        Some(n) => n,
        None => return Value::Undef,
    };
    let mid = [
        (oa[0] + ob[0]) / 2.0,
        (oa[1] + ob[1]) / 2.0,
        (oa[2] + ob[2]) / 2.0,
    ];
    Value::Plane {
        origin: Box::new(make_point3(mid, DimensionVector::LENGTH)),
        normal: Box::new(make_real_vec3(normal)),
    }
}

/// `axis_through(a: Point, b: Point) -> Axis`: origin a, direction normalize(b - a).
///
/// BOTH points are REQUIRED to be `Point3<Length>` — units-length η (task 6591).
/// `Undef` if either is not LENGTH, if either cannot be decoded, or if the two
/// are coincident. See [`eval_midplane`] for why the dimension-AGREEMENT
/// comparison this replaces is deleted rather than kept beside it, and
/// [`classify_axis_through_args`] for the predicate this gate and
/// [`diagnose`]'s arm share.
///
/// The accepted origin is cloned VERBATIM so a `Point3<Length>` round-trips
/// byte-identically; the synthesized direction stays DIMENSIONLESS, since a unit
/// vector legitimately has bare components (decision D3).
fn eval_axis_through(args: &[Value]) -> Value {
    let Ok((pa, pb)) = classify_axis_through_args(args) else {
        return Value::Undef;
    };
    let direction = match normalize3([pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]]) {
        Some(d) => d,
        None => return Value::Undef,
    };
    Value::Axis {
        origin: Box::new(args[0].clone()),
        direction: Box::new(make_real_vec3(direction)),
    }
}

/// `plane_through(a: Point, b: Point, c: Point) -> Plane`: origin a, normal
/// normalize((b-a)×(c-a)).
///
/// ALL THREE points are REQUIRED to be `Point3<Length>` — units-length η (task
/// 6591). `Undef` if any is not LENGTH, if any cannot be decoded, or if the
/// three are collinear. See [`eval_midplane`] for why the dimension-AGREEMENT
/// comparison this replaces is deleted rather than kept beside it, and
/// [`classify_plane_through_args`] for the predicate this gate and
/// [`diagnose`]'s arm share.
///
/// The parameters are `a`/`b`/`c` — the spelling
/// `reify-compiler/src/units.rs`'s signature block and this family's
/// diagnostics use. The accepted origin is cloned VERBATIM; the synthesized
/// normal stays DIMENSIONLESS (decision D3).
fn eval_plane_through(args: &[Value]) -> Value {
    let Ok((a, b, c)) = classify_plane_through_args(args) else {
        return Value::Undef;
    };
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = match normalize3(cross3(u, v)) {
        Some(n) => n,
        None => return Value::Undef,
    };
    Value::Plane {
        origin: Box::new(args[0].clone()),
        normal: Box::new(make_real_vec3(normal)),
    }
}

/// `offset(p: Plane, delta: Length) -> Plane`: shift the origin by `delta` along
/// the unit normal; the normal is unchanged.
///
/// BOTH operands are REQUIRED to carry LENGTH — `p`'s ORIGIN and `delta` itself
/// (units-length η, task 6591). `Undef` on bad arity, a non-LENGTH or
/// undecodable plane origin, a non-LENGTH delta, or a degenerate normal.
///
/// Gating `delta` closes an R12-class SILENT REINTERPRETATION, not merely a
/// missing check: the rule this replaces compared `delta`'s dimension to the
/// origin's, so a bare plane offset by a bare `2` MEASURED as
/// `plane(point(0, 0, 2), …)` — the dimensionless `2` became 2 METRES. See
/// [`eval_midplane`] for why that comparison is deleted rather than kept beside
/// the new guard, and [`classify_offset_plane_args`] for the predicate this gate
/// and [`diagnose`]'s arm share, which also fixes the `p`-before-`delta` order.
///
/// The normal keeps its dimension-agnostic decode, including the
/// `Value::Direction` form a kernel-realized plane carries (decision D3).
fn eval_offset_plane(args: &[Value]) -> Value {
    let Ok(((o, n), delta)) = classify_offset_plane_args(args) else {
        return Value::Undef;
    };
    let n_hat = match normalize3(n) {
        Some(nh) => nh,
        None => return Value::Undef,
    };
    let new_o = [
        o[0] + delta * n_hat[0],
        o[1] + delta * n_hat[1],
        o[2] + delta * n_hat[2],
    ];
    Value::Plane {
        origin: Box::new(make_point3(new_o, DimensionVector::LENGTH)),
        normal: Box::new(make_real_vec3(n_hat)),
    }
}

/// `frame_at(o: Point, x: Direction, z: Direction) -> Frame`:
/// orthonormalize (ŷ = ẑ×x̂, x̂' = ŷ×ẑ) into a basis quaternion; origin o.
/// Reuses the tested `orient_basis` (Shepperd's method + orthonormality guards).
///
/// The ORIGIN is REQUIRED to be a `Point3<Length>` — units-length η (task 6591).
/// This function previously did not check the origin's dimension at all, not
/// even for agreement, so a wholly bare `frame_at` built a bare-origin Frame at
/// exit 0. `Undef` on bad arity / a non-LENGTH or undecodable origin /
/// non-Direction axes / x ∥ z.
///
/// The x and z operands stay UNGATED: they arrive as `Value::Direction`, which
/// is three plain `f64` fields with no dimension to gate, and a unit vector
/// legitimately has bare components (decision D3). See
/// [`classify_frame_at_args`] for the predicate this gate and [`diagnose`]'s arm
/// share. The accepted origin is cloned VERBATIM.
fn eval_frame_at(args: &[Value]) -> Value {
    let Ok((_, x_in, z_in)) = classify_frame_at_args(args) else {
        return Value::Undef;
    };
    let z_hat = match normalize3(z_in) {
        Some(z) => z,
        None => return Value::Undef,
    };
    let y_hat = match normalize3(cross3(z_hat, x_in)) {
        Some(y) => y,
        None => return Value::Undef,
    };
    let x_hat = cross3(y_hat, z_hat);
    let basis = crate::orientation::eval_orientation(
        "orient_basis",
        &[
            make_real_vec3(x_hat),
            make_real_vec3(y_hat),
            make_real_vec3(z_hat),
        ],
    )
    .unwrap_or(Value::Undef);
    if !matches!(basis, Value::Orientation { .. }) {
        return Value::Undef;
    }
    Value::Frame {
        origin: Box::new(args[0].clone()),
        basis: Box::new(basis),
    }
}

// Quaternion helpers used by frame_to_frame — re-imported from orientation module.
use crate::orientation::{normalize_quaternion, quat_conj, quat_mul, quat_rotate};

// The `E_RotationVectorDimension` message is built by ONE shared constructor (#6080) so
// this module's `transform_exp` angular arm and `orientation::diagnose`'s `orient_exp`
// arm cannot drift apart in token, severity or recommended fix. Their only cross-module
// pin would otherwise be the CLI tests' bare `contains("E_RotationVectorDimension")`,
// which stays green through exactly that drift.
use crate::orientation::rotation_vector_dimension_error;

/// Human-readable name for a dimension, for use in diagnostic messages.
///
/// `DimensionVector::canonical_name()` yields `"Length"` / `"Angle"` / `"Mass"` from
/// the `NAMED_DIMENSIONS` table, and deliberately returns `None` for DIMENSIONLESS,
/// whose `Display` renders `"dimensionless"`. This is what lets a message NAME the
/// offending dimension instead of hardcoding one string.
///
/// KNOWN NEAR-DUPLICATE, and the divergence is deliberate — do not blind-unify. Two
/// sibling helpers in `reify-eval` branch on the same `canonical_name()`:
/// `arg_acceptance.rs`'s `value_short_label` and `geometry_ops.rs`'s
/// `scalar_got_label` (whose own doc already notes it is replicated rather than
/// shared). Both label a VALUE, so both suffix `" Scalar"` and render DIMENSIONLESS as
/// `"dimensionless Scalar"` and an unnamed dimension as the generic
/// `"dimensioned Scalar"`. This one labels a DIMENSION, in a sentence that has already
/// named the value ("a twist's `linear` must be Vector3<Length>; got …"), so it
/// diverges on both counts on purpose: the `Scalar` suffix would be redundant AND
/// wrong (the offending component may be a `Value::Real`, never a `Value::Scalar`),
/// and the fallback goes through `Display` so an unnamed dimension still prints its
/// actual exponents rather than the uninformative word "dimensioned". Hoisting a
/// shared `DimensionVector::diagnostic_label()` to `reify-core` is worth doing, but it
/// must carry BOTH renderings rather than collapsing them; that lives outside this
/// crate's scope and is filed as follow-up work.
fn dimension_label(dim: DimensionVector) -> String {
    dim.canonical_name()
        .map(str::to_string)
        .unwrap_or_else(|| dim.to_string())
}

/// Classify a SCALAR value at a position that must carry LENGTH.
///
/// The one predicate that owns "is this a LENGTH rejection" in this module.
/// Returns `Some(rejection)` only when `value` is well-formed enough to BE a
/// units rejection — numeric and FINITE — and its dimension is not LENGTH.
/// `None` otherwise, and each silent cause is silent for its own reason, none of
/// them a units fault:
/// - `Value::Undef` and any other non-numeric value: a type failure, and an
///   unresolved cell is not a wrong one (decision D10);
/// - a NON-FINITE numeric value: a finiteness failure, which the callers' gates
///   reject on their own grounds;
/// - an accepted LENGTH value: nothing to say.
///
/// The rejection is obtained from [`accept_arg`] rather than rendered here, so
/// Contract C1 invariant (i) holds literally: the wording is produced only by
/// `ArgRejection::message`, and there is no hand-rolled rejection string in this
/// crate to drift. `length_rejection_wording_is_the_shared_arg_rejection_template`
/// is the standing guard.
fn length_scalar_rejection(value: &Value) -> Option<ArgRejection> {
    if !value.as_f64().is_some_and(f64::is_finite) {
        return None;
    }
    if value.dimension() == DimensionVector::LENGTH {
        return None;
    }
    match accept_arg(value, &length_spec()) {
        Acceptance::Rejected(rejection) => Some(rejection),
        // Unreachable by construction: the guards above have already accepted the
        // value as numeric and finite (so not `Undefined`) and excluded LENGTH (so
        // not `Accepted`).
        Acceptance::Accepted(_) | Acceptance::Undefined => None,
    }
}

/// Classify a 3-component group that must share ONE LENGTH dimension.
///
/// Returns `Some(rejection)` when the group is well-formed enough to be a UNITS
/// rejection (three numeric, finite components sharing one non-LENGTH dimension),
/// and `None` when there is nothing to say — wrong arity, a non-numeric or
/// non-finite component, MIXED dimensions (a `decompose_xyz3` CONSISTENCY
/// failure, not a LENGTH one), or an accepted LENGTH group.
///
/// The LENGTH verdict and its wording both come from [`length_scalar_rejection`],
/// so ONE predicate owns them rather than two near-identical helpers drifting
/// apart. Delegating is behaviour-preserving: [`decompose_xyz3`] has already
/// established that all three components are numeric, finite and share one
/// dimension, so `items[0].dimension()` IS that shared dimension and the
/// sibling's own LENGTH guard performs exactly the test this helper used to
/// spell itself.
///
/// Only the first component is offered to the sibling because `decompose_xyz3`
/// has ALREADY required all three to share one dimension, so they reject
/// identically — which is also why the one message names the whole triple.
fn length_group_rejection(items: &[Value]) -> Option<ArgRejection> {
    let ([_, _, _], _) = decompose_xyz3(items)?;
    length_scalar_rejection(&items[0])
}

/// Why an `affine_map(linear, translation)` call cannot be built — the fault
/// vocabulary of [`classify_affine_map_args`], which is the ONE acceptance
/// predicate both `eval_geometry`'s arm and [`diagnose`]'s arm consult.
///
/// This is the `TWIST_LINEAR_DIM` / `decompose_twist_component` discipline
/// applied to `affine_map`. Before it, eval spelled the predicate as
/// `matrix_components_f64` + an `nrows`/`ncols`/`dim` check + `decompose_vec3` +
/// a LENGTH comparison, while `diagnose` spelled the same predicate as a
/// `matches!` pattern + a `Value::Vector` destructure + `length_group_rejection`.
/// The two agreed, but only by convention — and a drift is not symmetric: it
/// makes `diagnose` return `None` where eval returns `Undef`, which degrades
/// R12's rejection back to the SILENT `undef` at exit 0 that task 5747 exists to
/// close, with no test failing because every case exercises a value both
/// spellings agree on.
///
/// GUARD ORDER is now a property of one function rather than a convention
/// restated in two places: the LINEAR part is classified FIRST, so a
/// dimensioned-linear call can never be mislabelled as a translation rejection.
enum AffineMapFault {
    /// Wrong arity, a non-matrix or non-3×3 linear part, or a translation that
    /// is not a `Vector` of exactly three numeric, finite components sharing ONE
    /// dimension. A SHAPE/CONSISTENCY failure, never a units one — [`diagnose`]
    /// stays silent for all of it, matching the `transform3` convention.
    Shape,
    /// The 3×3 linear part carries a dimension: D11's pre-existing
    /// dimensionless-linear-part contract, which ζ neither loosened nor
    /// tightened. Silent for a sharper reason than `Shape` — naming
    /// `translation` here would misattribute the fault to the other argument.
    LinearNotDimensionless,
    /// The translation is a well-formed 3-group whose shared dimension is not
    /// LENGTH. The ONE fault R12 diagnoses (task 5747, units-length ζ), carrying
    /// the shared owner's rejection so the wording is never re-rendered here.
    TranslationNotLength(ArgRejection),
}

/// Decode `affine_map`'s two arguments into its linear and translation parts,
/// or say why they cannot be decoded. See [`AffineMapFault`] for the
/// shared-classifier rationale and the guard order it fixes.
fn classify_affine_map_args(args: &[Value]) -> Result<([[f64; 3]; 3], [f64; 3]), AffineMapFault> {
    if args.len() != 2 {
        return Err(AffineMapFault::Shape);
    }
    let Some((nrows, ncols, data, dim)) = matrix_components_f64(&args[0]) else {
        return Err(AffineMapFault::Shape);
    };
    if nrows != 3 || ncols != 3 {
        return Err(AffineMapFault::Shape);
    }
    // FIRST, and before the translation is even looked at: the D11 check.
    if !dim.is_dimensionless() {
        return Err(AffineMapFault::LinearNotDimensionless);
    }
    // `data` is row-major with exactly 9 entries (3×3), guaranteed above.
    let linear = [
        [data[0], data[1], data[2]],
        [data[3], data[4], data[5]],
        [data[6], data[7], data[8]],
    ];
    let Value::Vector(items) = &args[1] else {
        return Err(AffineMapFault::Shape);
    };
    match length_group_rejection(items) {
        Some(rejection) => Err(AffineMapFault::TranslationNotLength(rejection)),
        None => match decompose_xyz3(items) {
            Some((translation, t_dim)) if t_dim == DimensionVector::LENGTH => {
                Ok((linear, translation))
            }
            // `length_group_rejection` said nothing, so this is its OTHER silent
            // cause: a `decompose_xyz3` shape/consistency failure. The non-LENGTH
            // arm is unreachable (the rejection branch above owns it) and falls
            // here deliberately — fail CLOSED, so an unclassifiable translation is
            // still `Undef` rather than silently built into a map.
            _ => Err(AffineMapFault::Shape),
        },
    }
}

/// Pure classifier (post-`Value::Undef` hook) for geometry builtin calls,
/// mirroring `stackup::diagnose` / `fea::diagnose`. `reify-expr`'s `FunctionCall`
/// arm calls this (re-exported as `geometry_diagnose`) when a stdlib builtin
/// returns `Value::Undef`, and pushes any returned `Diagnostic` into the
/// `EvalContext` runtime sink so `reify eval` can print it.
///
/// Names served:
/// - **`affine_scale`** (exactly 3 args), distinguishing its two user-correctable
///   failure causes (the third — arity — stays silent, like the `transform3`
///   convention):
///   - *dimensioned factor* → violates the G6 dimensionless-linear-part contract;
///   - *zero factor* → degenerate (det=0, non-invertible) map.
/// - **`transform_log`** (exactly 1 arg) — a `Transform` whose translation is not
///   `Vector3<Length>` (RULING #6126).
/// - **`transform_exp`** (exactly 1 arg) — BOTH halves of the twist, checked in eval's
///   own order:
///   - an `angular` half that is not `Vector3<Angle>` (RULING #6080) — a rotation
///     vector is `axis * angle`. `DIMENSIONLESS` used to be the accepted spelling and
///     is now rejected like any other wrong dimension, so this diagnostic is the
///     migration mechanism for that breaking change. Its message is built by
///     [`crate::orientation::rotation_vector_dimension_error`], SHARED with that
///     module's `orient_exp` arm, so one fault class reports one way across the whole
///     builtin family.
///   - otherwise, a `linear` half that is not `Vector3<Length>` (RULING #6126).
///
///   Angular is checked FIRST because eval gates it first: a twist wrong in both
///   halves is rejected by the angular gate before the linear one is reached, so
///   blaming `linear` would mis-attribute the failure. This arm used to uphold that by
///   staying silent (it did not own the angular gate); it now upholds it by ORDER.
/// - **`affine_translate`** (exactly 3 args) and **`affine_map`** (exactly 2 args) —
///   a TRANSLATION that is not `Vector3<Length>` (task 5747, units-length ζ / R12).
///   `affine_map`'s LINEAR half is checked FIRST and bails, so a dimensioned-linear
///   call — which fails the pre-existing D11 dimensionless check, not ζ's — is never
///   mislabelled as a translation rejection. That order is fixed in
///   [`classify_affine_map_args`], the one acceptance predicate this arm and the
///   eval arm share, rather than restated in each. The same ordering claim as
///   `transform_exp`'s above, upheld the same way: by the gate's own order, not by
///   a convention restated at each reader.
/// - **`plane_xy`** / **`plane_xz`** / **`plane_yz`** (exactly 1 arg) — an OFFSET
///   that is not a Length, and **`axis_x`** / **`axis_y`** / **`axis_z`** (exactly
///   1 arg) — an ORIGIN that is not a `Point3<Length>` (task 5746, units-length ε
///   / R11). These are the PRODUCER end of the same rule task δ gates at the
///   consumer end. The axis arm names the whole triple `ox/oy/oz` in ONE message,
///   for `affine_translate`'s reason: its decoder has already required the three
///   components to share one dimension, so all three positions offend identically
///   and naming them together is complete information.
/// - **`bbox`** (exactly 2 args) — a corner that is not `Point3<Length>`
///   (task 6081: a BoundingBox is spatial by construction), including one whose
///   components carry MIXED dimensions. Every SHAPE failure stays silent — a
///   non-`Point` argument, a component count other than 3, a non-numeric
///   component — like the arity convention above: a type failure is not a
///   dimension failure.
/// - **`midplane`** / **`axis_through`** / **`plane_through`** / **`offset`**
///   (exactly 2 args) / **`frame_at`** — a POSITION operand that is not
///   `Point3<Length>`, or an `offset` delta that is not a Length (task 6591,
///   units-length η). Each arm is the ε arms' sibling one family over, and each
///   reads the SAME per-builtin classifier its eval gate is built from, so it
///   can neither speak for an argument eval accepted nor stay quiet on one eval
///   rejected on dimension grounds. The offending argument is named as the
///   author wrote it in the signature — `a`/`b`/`c`, `p`/`delta`, `o` — and when
///   several offend the FIRST in argument order is named, deterministically,
///   because the order lives in that one classifier (the
///   [`diagnose_bbox_corners`] `min`-before-`max` precedent). `frame_at`'s x/z
///   and every plane NORMAL stay UNGATED and therefore never named: a unit
///   vector legitimately has bare components (decision D3). The arity-3
///   `offset` — the γ RELATION — is never served, which its classifier enforces
///   rather than this list.
///
/// Invariant: the twist dimension arms consult [`TWIST_LINEAR_DIM`] and
/// [`TWIST_ANGULAR_DIM`] — the SAME consts the eval gates use — and read BOTH twist
/// halves through [`decompose_twist_component`] — the SAME helper the eval arm uses —
/// so the classifier cannot drift away from what eval actually rejects, in either the
/// dimension or the shape dimension of that drift. The `bbox` arm holds the same
/// property the same way: it decodes both corners through
/// [`classify_bbox_corner`] — the SAME helper its own eval gate reads, which is
/// where `DimensionVector::LENGTH` is compared — so the shape half is pinned
/// alongside the quantity half rather than re-derived here.
///
/// The units-length ζ arms are bound the same way, one level down: they route through
/// [`length_group_rejection`], which applies `DimensionVector::LENGTH` to the output of
/// [`decompose_xyz3`] — the SAME helper the `affine_translate` / `affine_map` eval arms
/// decode through, and the same dimension. ε's `axis_*` arm reads that same helper, and
/// so reaches its verdict through the very decoder [`make_axis`]'s gate reads; its
/// `plane_*` arm reads [`length_scalar_rejection`], the sibling predicate that owns the
/// LENGTH comparison `length_group_rejection` now delegates to and that [`make_plane`]'s
/// gate applies to the same argument. (Same VALUE as `TWIST_LINEAR_DIM`, read
/// separately on purpose: that const is scoped to the log↔exp seam by its own doc, and
/// an affine translation is not a twist.)
///
/// Invariant: this hook fires on EVERY `Value::Undef` from these builtins, not just
/// dimension rejections, so each arm stays SILENT (`None`) on every non-dimension
/// cause — wrong arity, wrong argument shape, a degenerate or non-finite
/// quaternion, non-finite components — rather than mis-attributing an unrelated
/// failure to a dimension problem. Concretely: only emit when the decomposition
/// SUCCEEDS and the recovered dimension differs from the admitted one.
///
/// Severity is split by fault class, and the split is deliberate:
///
/// - The `affine_scale` arms are `Warning`, mirroring the existing degenerate-scale
///   rejection in `reify_eval::geometry_ops` (TransformKind::Scale): the offending
///   factor is discarded and evaluation proceeds.
/// - EVERY dimension arm is `Severity::Error` — `transform_log` and BOTH halves of
///   `transform_exp` (RULING #6126 for `linear`, RULING #6080 for `angular`), `bbox`
///   (task 6081), `affine_translate` / `affine_map` (task 5747, units-length ζ,
///   PRD `docs/prds/v0_6/units-length-gate-completion.md` decision D11), and ε's
///   two construction-datum families (task 5746, same PRD, R11 / D4). ONE reason
///   serves all six rather than one argued per family: a wrong dimension
///   is a design-correctness fault and an outright CONSTRUCTION failure — no twist,
///   no BoundingBox, no AffineMap is produced at all — not a drop-and-continue.
///   Per Leo's severity amendment (2026-08-19, via esc-6080-6), `reify eval` must
///   EXIT 1 rather than print and continue; `cmd_eval` gates its exit code on
///   `diagnostics.iter().any(|d| d.severity == Severity::Error)`, so the severity IS
///   the exit code here. The angular half (#6080) landed at that same severity, so
///   one fault class does NOT report two ways across one builtin family — and ζ's
///   two arms joined it there rather than opening a third way.
///
/// `DiagnosticCode` is NOT uniform across the arms, but every DIMENSION arm
/// agrees. All EIGHT — `transform_log`, BOTH halves of `transform_exp`, `bbox`,
/// ζ's `affine_translate` / `affine_map` and ε's `plane_*` / `axis_*` — carry the
/// PRE-EXISTING [`reify_core::DiagnosticCode::DimensionedArgRejected`], which
/// `reify_eval::geometry_ops` already attaches to exactly this fault class (a
/// `Severity::Error` runtime dimension rejection of a positional argument). The
/// `angular` half reaches it through `orientation::rotation_vector_dimension_error`,
/// the shared constructor it splits with `orientation::diagnose`'s `orient_exp`
/// arm, so the code cannot drift between those two either.
///
/// The convergence an earlier revision of this comment DEFERRED — waiting on a
/// `DiagnosticCode::ArgDimensionMismatch` that
/// `docs/prds/v0_6/dimension-checked-readers.md` §6 decision 1 was expected to
/// mint — is DONE here, by task 5791. Under BINDING ruling A7 (Leo,
/// 2026-08-30, esc-5791-3) there was nothing to wait for: that variant is
/// deliberately NOT minted, because one rejection REASON gets one code, and
/// `DimensionedArgRejected` already IS that code. See §6 decision 1's
/// RECONCILIATION block (landed b3ba3228f5). The `bbox` arm, which has carried
/// the code since task 6081 (2026-08-27), was the shipped counter-example that
/// made the point; ζ's two arms are another, carrying it from the start because
/// task 5743 (β) minted it for exactly this chokepoint — its own doc in
/// `reify-core::diagnostics` records it as deliberately distinct from the
/// compile-layer `ArgTypeMismatch`, and mandates ONE shared runtime code rather
/// than per-dimension siblings, which is ruling A7 stated one layer down.
///
/// The `affine_scale` arms deliberately stay `Severity::Warning` and CODE-LESS.
/// They are a different fault class — a dropped factor, drop-and-continue — and
/// ruling A7 converges the DIMENSION reason, not every arm in this function.
///
/// Returns `None` for any other name, wrong arity, or valid input.
pub fn diagnose(name: &str, args: &[Value]) -> Option<reify_core::Diagnostic> {
    match name {
        "affine_scale" => {
            if args.len() != 3 {
                return None;
            }
            // Check dimensioned factors first so a dimensioned-and-otherwise-fine factor
            // reports the dimensionless requirement rather than a spurious zero message.
            if args.iter().any(|a| !a.dimension().is_dimensionless()) {
                return Some(reify_core::Diagnostic::warning(
                    "affine_scale: scale factors must be dimensionless (Real); a dimensioned \
                     factor was dropped (the linear part of an affine map is dimensionless)",
                ));
            }
            if args.iter().any(|a| a.as_f64() == Some(0.0)) {
                return Some(reify_core::Diagnostic::warning(
                    "affine_scale dropped: factor=0 produces a degenerate (det=0) \
                     non-invertible map (every scale factor must be non-zero)",
                ));
            }
            None
        }
        // `affine_translate`'s diagnostic names the whole gesture `dx/dy/dz` in
        // ONE message rather than one position per rebuild. The two reconcile
        // exactly here: `decompose_xyz3` has ALREADY required all three
        // components to share one dimension, so when this gate fires all three
        // positions offend identically and naming them together is complete
        // information, not a shortcut (β's all-failures-at-once amendment,
        // esc-5743-4, expressed inside a hook whose signature is
        // `Option<Diagnostic>`).
        //
        // `length_group_rejection` returning None is this arm's no-mis-attribution
        // guard — the same discipline the RULING #6126 arms below apply. Wrong
        // arity, a non-numeric or non-finite component, and a MIXED-dimension
        // triple are all `decompose_xyz3` failures, not LENGTH ones, and stay
        // silent.
        "affine_translate" => {
            let rejection = length_group_rejection(args)?;
            Some(
                reify_core::Diagnostic::error(rejection.message("affine_translate", "dx/dy/dz"))
                    .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            )
        }
        // `affine_map` needs no such join: `translation` is literally the
        // builtin's own parameter name for the whole `Vector3`.
        //
        // GUARD ORDER MATTERS, and it is not restated here: this arm reads the
        // SAME `classify_affine_map_args` the eval arm builds from, so the order
        // — linear part first, translation second — is a property of that one
        // function. Everything except the units fault stays silent, which is this
        // arm's no-mis-attribution guard.
        "affine_map" => match classify_affine_map_args(args) {
            Err(AffineMapFault::TranslationNotLength(rejection)) => Some(
                reify_core::Diagnostic::error(rejection.message("affine_map", "translation"))
                    .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            ),
            Ok(_) | Err(AffineMapFault::Shape | AffineMapFault::LinearNotDimensionless) => None,
        },
        // ε's construction-datum arms. `offset` is the argument the author
        // actually wrote and the vocabulary `make_plane` itself uses
        // (`offset_index` / `offset_val` / `offset_f`), not the synthesized
        // origin coordinate the consumer-side gate names.
        //
        // `length_scalar_rejection` returning None is this arm's
        // no-mis-attribution guard: a non-numeric argument is a type failure and
        // a non-finite one a finiteness failure, both rejected by `make_plane` on
        // their own grounds, and an unresolved cell is not a wrong one (D10).
        "plane_xy" | "plane_xz" | "plane_yz" => {
            if args.len() != 1 {
                return None;
            }
            let rejection = length_scalar_rejection(&args[0])?;
            Some(
                reify_core::Diagnostic::error(rejection.message(name, "offset"))
                    .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            )
        }
        // The axis arm names the whole origin triple `ox/oy/oz` in ONE message
        // rather than one position per rebuild, for `affine_translate`'s reason
        // stated above: `decompose_xyz3` — which `make_axis` reads its verdict
        // through too — has ALREADY required all three components to share one
        // dimension, so when this gate fires all three positions offend
        // identically and naming them together is COMPLETE information, not a
        // shortcut (β's all-failures-at-once amendment, esc-5743-4, expressed
        // inside a hook whose signature is `Option<Diagnostic>`). `ox`/`oy`/`oz`
        // are also byte-identical to task δ's consumer-side naming for the same
        // coordinates.
        //
        // A non-`Point` argument is a SHAPE failure and stays silent here, as does
        // every cause `length_group_rejection` folds into None.
        "axis_x" | "axis_y" | "axis_z" => {
            if args.len() != 1 {
                return None;
            }
            let Value::Point(comps) = &args[0] else {
                return None;
            };
            let rejection = length_group_rejection(comps)?;
            Some(
                reify_core::Diagnostic::error(rejection.message(name, "ox/oy/oz"))
                    .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            )
        }
        "transform_log" => {
            if args.len() != 1 {
                return None;
            }
            // `decompose_transform` returning None covers every non-dimension cause:
            // a non-Transform argument, a non-Orientation or non-finite rotation, a
            // translation that is not a 3-Vector, mixed component dimensions, and
            // non-numeric or non-finite components. Staying silent there is the
            // no-mis-attribution guard.
            let (_, _, t_dim) = decompose_transform(&args[0])?;
            if t_dim == TWIST_LINEAR_DIM {
                return None;
            }
            Some(
                reify_core::Diagnostic::error(format!(
                    "transform_log: a Transform's translation must be Vector3<Length>; got {} \
                     (a twist's `linear` half carries Length — RULING #6126)",
                    dimension_label(t_dim)
                ))
                .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            )
        }
        "transform_exp" => {
            if args.len() != 1 {
                return None;
            }
            // Check the halves in EVAL'S OWN ORDER — angular, then linear — so the arm
            // blames the half eval actually rejected. Eval gates `angular` BEFORE
            // `linear`, so a twist wrong in both halves never reaches the linear gate,
            // and blaming `linear` there would be a mis-attribution.
            //
            // #6126 upheld that invariant by SILENCE, because it did not own the
            // angular gate and had nothing true to say about it — and documented the
            // resulting hazard: the user acts on a `linear` message, makes `linear` a
            // Length, is STILL Undef, and now gets NO diagnostic at all. #6080 owns the
            // angular gate, so that hazard is now CLOSED rather than merely avoided:
            // the arm names the angular half both times. This is exactly the
            // substitution #6126's comment anticipated ("#6080 … will add its own arm
            // to explain it").
            //
            // `TWIST_ANGULAR_DIM` is read from the const rather than re-spelled, so the
            // gate and this arm cannot drift; see that const's doc and the
            // `..._deferral_tracks_evals_angular_gate` pin.
            //
            // Shape failures on the angular half — a non-Map argument, a missing
            // `angular` key, a non-3-Vector — fold into `None` here for the same
            // no-mis-attribution reason they do on the linear half. It is the SAME
            // helper the eval arm now reads the angular shape through, so this arm
            // cannot go quiet while eval keeps rejecting.
            let (_, ang_dim) = decompose_twist_component(&args[0], "angular")?;
            if ang_dim != TWIST_ANGULAR_DIM {
                return Some(rotation_vector_dimension_error(
                    "transform_exp",
                    Some("angular"),
                    ang_dim,
                ));
            }
            // As on the transform_log arm, `decompose_twist_component` returning None
            // covers every non-dimension cause (non-Map arg, missing `linear` key — a
            // SHAPE failure, not a 3-Vector, mixed component dimensions, non-numeric or
            // non-finite components) — the no-mis-attribution guard. It is the SAME
            // helper the eval arm reads the shape through, so this arm cannot go quiet
            // while eval keeps rejecting.
            let (_, lin_dim) = decompose_twist_component(&args[0], "linear")?;
            if lin_dim == TWIST_LINEAR_DIM {
                return None;
            }
            Some(
                reify_core::Diagnostic::error(format!(
                    "transform_exp: a twist's `linear` must be Vector3<Length>; got {} \
                     (RULING #6126)",
                    dimension_label(lin_dim)
                ))
                .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
            )
        }
        "bbox" => {
            if args.len() != 2 {
                return None;
            }
            diagnose_bbox_corners(&args[0], &args[1])
        }
        // η's construction-datum arms (task 6591). Each reads the SAME
        // per-builtin classifier its eval gate is built from, so the gate and
        // this arm cannot drift asymmetrically — see [`DatumFault`] for why that
        // matters here in particular, and [`classify_midplane_args`] for the
        // guard ORDER those classifiers fix.
        //
        // Nothing below decides anything: the arm name selects a classifier, the
        // classifier names the offending argument, and `datum_fault_diagnostic`
        // renders it. Which operand is blamed when several offend, and which
        // causes stay silent, are properties of those classifiers alone.
        "midplane" => datum_fault_diagnostic(name, classify_midplane_args(args).err()),
        "axis_through" => datum_fault_diagnostic(name, classify_axis_through_args(args).err()),
        "plane_through" => datum_fault_diagnostic(name, classify_plane_through_args(args).err()),
        // The arity-3 `offset` is the γ RELATION and must never be given a
        // datum-constructor rejection. `classify_offset_plane_args` holds that
        // guard (`args.len() != 2` -> `Shape`, which is silent), so this arm does
        // not restate it.
        "offset" => datum_fault_diagnostic(name, classify_offset_plane_args(args).err()),
        "frame_at" => datum_fault_diagnostic(name, classify_frame_at_args(args).err()),
        _ => None,
    }
}

/// Render a construction-datum classifier's verdict as [`diagnose`] reports it.
///
/// The ONE place η's five arms turn a [`DatumFault`] into a `Diagnostic`, so
/// severity, code and wording are chosen once rather than five times.
///
/// `NotLength` is the only fault that speaks, and it speaks with the message
/// MINTED by the shared owner — Contract C1 invariant (i): the wording is
/// produced only by `ArgRejection::message`, so there is no hand-rolled
/// rejection string here to fork. `Severity::Error` plus the pre-existing
/// `DimensionedArgRejected` code is ruling A7: one rejection REASON gets one
/// code, which is how η joins the landed dimension arms rather than opening
/// another way to report the same fault class.
///
/// `Shape` and acceptance are both SILENT, and that silence is the
/// no-mis-attribution guard rather than a swallowed fault: a shape failure is
/// never blamed on a dimension, and an unresolved cell is not a wrong one
/// (decision D10). The gate still rejects every one of them on its own grounds.
fn datum_fault_diagnostic(name: &str, fault: Option<DatumFault>) -> Option<reify_core::Diagnostic> {
    match fault? {
        DatumFault::NotLength {
            arg_name,
            rejection,
        } => Some(
            reify_core::Diagnostic::error(rejection.message(name, arg_name))
                .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
        ),
        DatumFault::Shape => None,
    }
}

/// How a `bbox` corner argument decodes.
///
/// The SINGLE decoder shared by the `bbox` eval gate and
/// [`diagnose_bbox_corners`], so the classifier cannot drift from what eval
/// actually rejects — in the SHAPE dimension of that drift as well as the
/// quantity one. The RULING #6126 arms get that property by sharing
/// [`decompose_twist_component`] with their eval gate; before this the `bbox`
/// pair shared only the `DimensionVector::LENGTH` constant, and the shape halves
/// had duly diverged (the classifier reported a `Point3<…>` for a `Point2`
/// argument, naming a Point3 that did not exist).
///
/// The three cases fall on two sides of [`diagnose`]'s no-mis-attribution
/// invariant: a QUANTITY failure is the classifier's to explain, a SHAPE failure
/// is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BboxCorner {
    /// A `Point` of exactly 3 numeric components that all carry this dimension.
    /// The only shape eval admits — and then only at `LENGTH`.
    Uniform(DimensionVector),
    /// A `Point` of exactly 3 numeric components whose dimensions DISAGREE
    /// (`point3(1m, 2deg, 3m)`). A quantity failure: the classifier speaks.
    Mixed,
    /// Not a corner at all — a non-`Point`, a component count other than 3, or a
    /// non-numeric component. A shape failure: the classifier stays silent.
    NotACorner,
}

/// Decode one `bbox` corner argument. See [`BboxCorner`] for why the three cases
/// are distinguished rather than folded together.
fn classify_bbox_corner(v: &Value) -> BboxCorner {
    let comps = match v {
        Value::Point(comps) if comps.len() == 3 => comps,
        _ => return BboxCorner::NotACorner,
    };
    // Read the quantity through the same uniformity-checking extractor the two
    // accessors read a stored corner through, so the gate cannot admit a corner
    // `bbox_size`/`bbox_center` would go on to reject.
    if let Some((_, dim)) = tensor_components_f64(v) {
        return BboxCorner::Uniform(dim);
    }
    // `tensor_components_f64` folds MIXED component dimensions and NON-NUMERIC
    // components into one `None`; split them apart again, because they land on
    // opposite sides of the no-mis-attribution invariant. Non-numeric wins the
    // tie: it is the shape failure, and a shape failure is never blamed on a
    // dimension.
    if comps.iter().any(|c| c.as_f64().is_none()) {
        BboxCorner::NotACorner
    } else {
        BboxCorner::Mixed
    }
}

/// The `bbox` arm of [`diagnose`]: report the first corner that is not
/// `Point3<Length>`.
///
/// `min` is reported before `max` so a both-wrong call names `min`
/// deterministically. Every SHAPE failure returns `None` — a non-`Point`
/// argument, a component count other than 3, a non-numeric component — because a
/// type failure is not a dimension failure, and staying silent matches the
/// `affine_scale` convention of explaining only the user-correctable dimension
/// cause. That shape half is decided by [`classify_bbox_corner`], the SAME
/// decoder the eval gate reads, so this arm cannot speak for a corner eval
/// accepted nor stay quiet on one eval rejected on dimension grounds.
///
/// The dimension half of the message goes through [`dimension_label`] (added by
/// RULING #6126) rather than re-rolling a fourth rendering of the same thing, so an
/// unnamed dimension prints its actual exponents instead of the uninformative word
/// "dimensioned". DIMENSIONLESS is the one divergence and it is deliberate: the label
/// lands in a TYPE-ARGUMENT slot (`Point3<…>`), where the spelling is `Real`, not
/// `dimension_label`'s prose "dimensionless".
///
/// The message mirrors `ArgRejection::message`'s
/// `"{builtin}: {arg_name} argument expects {expected}, got {got}"` shape
/// (`crates/reify-ir/src/arg_acceptance.rs` — relocated there from reify-eval by
/// task 5791, PRD `docs/prds/v0_6/dimension-checked-readers.md` §3 Leg A).
///
/// It stays hand-mirrored, but the REASON changed with that relocation. The old
/// reason — "reify-stdlib cannot depend on reify-eval (reify-eval → reify-expr →
/// reify-stdlib would be a cycle)" — no longer applies: reify-stdlib already
/// deps reify-ir, and `arg_acceptance_is_reachable_from_reify_stdlib`
/// (helpers.rs's test module) pins that `reify_ir::arg_acceptance` IS importable
/// from here. What keeps this rendering separate is the divergence documented
/// directly above: the label lands in a TYPE-ARGUMENT slot (`Point3<…>`), where
/// DIMENSIONLESS spells `Real` rather than `dimension_label`'s prose
/// "dimensionless". Copying the wording across a boundary remains established
/// practice for the genuinely-unreachable case (see
/// `reify-compiler/src/conformance/mod.rs`, annotated "COPIED from
/// ArgRejection::message").
fn diagnose_bbox_corners(min: &Value, max: &Value) -> Option<reify_core::Diagnostic> {
    for (arg_name, corner) in [("min", min), ("max", max)] {
        let got = match classify_bbox_corner(corner) {
            // Shape failure — silent, per the invariant above.
            BboxCorner::NotACorner => continue,
            // The one accepted corner: nothing to explain.
            BboxCorner::Uniform(dim) if dim == DimensionVector::LENGTH => continue,
            BboxCorner::Uniform(dim) if dim.is_dimensionless() => "Point3<Real>".to_string(),
            BboxCorner::Uniform(dim) => format!("Point3<{}>", dimension_label(dim)),
            // No single quantity to name, so the slot describes the fault instead
            // of pretending to a `Point3<…>` spelling the argument does not have.
            BboxCorner::Mixed => "a Point3 with mixed component dimensions".to_string(),
        };
        return Some(
            reify_core::Diagnostic::error(format!(
                "bbox: {arg_name} argument expects Point3<Length>, got {got} \
                 (a bounding box is spatial by construction)"
            ))
            .with_code(reify_core::DiagnosticCode::DimensionedArgRejected),
        );
    }
    None
}

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

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — the axis-side twin of
    /// `plane_xy_real_zero_is_rejected_expects_length`.
    ///
    /// Its predecessor asserted that a DIMENSIONLESS `Point3` origin built an
    /// `Axis` carrying that origin verbatim. `make_axis` checked SHAPE only, so
    /// that recorded an accidental pre-doctrine gap rather than a sanctioned
    /// contract. `docs/prds/v0_6/units-length-gate-completion.md` task ε (R11,
    /// decision D4) shuts it: the origin is REQUIRED to be a `Point3<Length>`.
    ///
    /// Do NOT re-flip this back to an acceptance. Doing so re-opens R11 at the
    /// producer while task δ's consumer-side gate stays shut, so a bare origin
    /// silently places an axis 1000× out again.
    #[test]
    fn axis_x_dimensionless_point3_is_rejected_expects_length() {
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("axis_x", std::slice::from_ref(&origin)).is_undef());
    }

    /// The whole axis family rejects a non-LENGTH origin, in every `got` shape
    /// that can reach the gate — `decompose_xyz3` requires `as_f64` to succeed,
    /// so only `Real`, `Int` and `Scalar` arrive, and a LENGTH `Scalar` is
    /// accepted. The three names share ONE `make_axis`; enumerating them keeps
    /// that sharing honest rather than assumed.
    #[test]
    fn axis_family_rejects_non_length_origins() {
        let mass = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::MASS,
        };
        let triples = [
            Value::Point(vec![Value::Int(0), Value::Int(0), Value::Int(0)]),
            Value::Point(vec![mass.clone(), mass.clone(), mass]),
        ];
        for name in ["axis_x", "axis_y", "axis_z"] {
            for origin in &triples {
                assert!(
                    eval_builtin(name, std::slice::from_ref(origin)).is_undef(),
                    "{name}({origin:?}) must be Undef: the ox/oy/oz argument expects Length"
                );
            }
        }
    }

    /// A MIXED-dimension origin fails CLOSED, but for `decompose_xyz3`'s
    /// CONSISTENCY rule rather than the LENGTH rule — the distinction
    /// [`diagnose`]'s no-mis-attribution contract turns on, which is why the
    /// gate still says `Undef` here while the classifier stays silent.
    #[test]
    fn axis_x_mixed_dimension_origin_returns_undef() {
        let origin = Value::Point(vec![
            Value::length(1.0),
            Value::Real(0.0),
            Value::length(0.0),
        ]);
        assert!(eval_builtin("axis_x", std::slice::from_ref(&origin)).is_undef());
    }

    /// A NON-FINITE LENGTH origin is rejected too. That is a deliberate, stated
    /// TIGHTENING, not a units rejection: it makes `make_axis` symmetric with
    /// `make_plane`'s long-standing finiteness guard, which the shape-only gate
    /// this replaces never had.
    #[test]
    fn axis_x_non_finite_length_origin_returns_undef() {
        let origin = Value::Point(vec![
            Value::length(f64::NAN),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(eval_builtin("axis_x", std::slice::from_ref(&origin)).is_undef());
    }

    /// The anti-vacuity partner of the rows above: a `Point3<Length>` origin
    /// still builds an `Axis`, carrying that point VERBATIM (the byte-identical
    /// round-trip `decode_axis_producer_round_trip_*` depends on), with a
    /// DIMENSIONLESS direction — decision D3's scope lock, since a unit vector
    /// legitimately has bare components.
    #[test]
    fn axis_x_length_origin_round_trips_and_keeps_direction_bare() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_x", std::slice::from_ref(&origin));
        let Value::Axis {
            origin: o,
            direction,
        } = result
        else {
            panic!("expected Value::Axis, got {result:?}");
        };
        assert_eq!(*o, origin, "a LENGTH origin must round-trip verbatim");
        assert_eq!(
            *direction,
            Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
            "the synthesized direction is a unit vector and stays dimensionless (D3)"
        );
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

    // ── bbox is Length-valued by construction (task 6081) ────────────────────
    // A bounding box is spatial: both corners must be `Point3<Length>`. Agreeing
    // non-Length corners (two Angle points, two dimensionless points) used to
    // slip through the old `min_dim != max_dim` gate and construct a
    // quantity-polymorphic BoundingBox; they are now rejected outright.

    /// Build a `Point3<Angle>` — the polymorphism escape hatch the old gate
    /// admitted, since `point3` is dimension-polymorphic at eval.
    fn make_point3_angle(x: f64, y: f64, z: f64) -> Value {
        Value::Point(
            [x, y, z]
                .into_iter()
                .map(|si_value| Value::Scalar {
                    si_value,
                    dimension: DimensionVector::ANGLE,
                })
                .collect(),
        )
    }

    #[test]
    fn bbox_angle_corners_returns_undef() {
        let min = make_point3_angle(0.0, 0.0, 0.0);
        let max = make_point3_angle(1.0, 2.0, 3.0);
        assert!(
            eval_builtin("bbox", &[min, max]).is_undef(),
            "two agreeing Angle corners must be rejected: a BoundingBox is Length-valued"
        );
    }

    #[test]
    fn bbox_dimensionless_corners_returns_undef() {
        let min = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let max = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert!(
            eval_builtin("bbox", &[min, max]).is_undef(),
            "two agreeing dimensionless corners must be rejected: a BoundingBox is Length-valued"
        );
    }

    #[test]
    fn bbox_length_corners_still_constructs() {
        // Positive guard: the narrowing must not over-reject the valid case.
        let result = eval_builtin("bbox", &[make_point3_min(), make_point3_max()]);
        assert!(
            matches!(result, Value::BoundingBox { .. }),
            "metre-valued corners must still construct a BoundingBox, got {:?}",
            result
        );
    }

    /// A corner whose components DISAGREE is not `Point3<Length>` either, and
    /// must be rejected AT CONSTRUCTION.
    ///
    /// This is the case a first-component reading of the quantity let through.
    /// Letting it construct did not make it work: `bbox_size`/`bbox_center` read
    /// the stored corner through `tensor_components_f64`, which rejects mixed
    /// component dimensions, so the user got an Undef one call downstream — and,
    /// the `bbox` call itself having SUCCEEDED, with no diagnostic at all.
    #[test]
    fn bbox_mixed_dimension_corner_returns_undef() {
        let min = Value::Point(vec![
            Value::length(1.0),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::ANGLE,
            },
            Value::length(3.0),
        ]);
        assert!(
            eval_builtin("bbox", &[min, make_point3_max()]).is_undef(),
            "a corner mixing Length and Angle components is not Point3<Length>"
        );
    }

    /// The mixed-component rejection is per-corner, not min-only.
    #[test]
    fn bbox_mixed_dimension_max_corner_returns_undef() {
        let max = Value::Point(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::Scalar {
                si_value: 6.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        assert!(eval_builtin("bbox", &[make_point3_min(), max]).is_undef());
    }

    /// A non-numeric component is a SHAPE failure, and eval rejects it too — the
    /// gate admits only three numeric, uniformly-Length components.
    #[test]
    fn bbox_non_numeric_corner_component_returns_undef() {
        let min = Value::Point(vec![
            Value::length(1.0),
            Value::Bool(true),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("bbox", &[min, make_point3_max()]).is_undef());
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

    // ── accessors are Length-valued in their own right (task 6081) ───────────
    // These hand-built BoundingBox values deliberately BYPASS the `bbox`
    // constructor gate, which now admits only Length corners. They pin that
    // `bbox_size`/`bbox_center` emit Length components for EVERY
    // `Value::BoundingBox` — including one produced by the kernel
    // (`dispatch_bounding_box`, itself unconditionally Length) or hand-built —
    // which is what makes the static rows `Vector3<Length>` / `Point3<Length>`
    // sound. The old behaviour propagated the stored corner dimension; that
    // was incidental generic propagation, not a designed capability.

    fn make_dimensionless_bbox() -> Value {
        Value::BoundingBox {
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
        }
    }

    fn make_angle_bbox() -> Value {
        Value::BoundingBox {
            min: Box::new(make_point3_angle(0.0, 0.0, 0.0)),
            max: Box::new(make_point3_angle(2.0, 4.0, 6.0)),
        }
    }

    #[test]
    fn bbox_size_emits_length_components_for_dimensionless_bbox() {
        let result = eval_builtin("bbox_size", &[make_dimensionless_bbox()]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(2.0));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Vector of Lengths, got {:?}", other),
        }
    }

    #[test]
    fn bbox_center_emits_length_components_for_dimensionless_bbox() {
        let result = eval_builtin("bbox_center", &[make_dimensionless_bbox()]);
        match result {
            Value::Point(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(1.0));
                assert_eq!(comps[1], Value::length(2.0));
                assert_eq!(comps[2], Value::length(3.0));
            }
            other => panic!("expected Point of Lengths, got {:?}", other),
        }
    }

    #[test]
    fn bbox_size_emits_length_components_for_angle_bbox() {
        let result = eval_builtin("bbox_size", &[make_angle_bbox()]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(2.0));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Vector of Lengths, got {:?}", other),
        }
    }

    #[test]
    fn bbox_center_emits_length_components_for_angle_bbox() {
        let result = eval_builtin("bbox_center", &[make_angle_bbox()]);
        match result {
            Value::Point(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(1.0));
                assert_eq!(comps[1], Value::length(2.0));
                assert_eq!(comps[2], Value::length(3.0));
            }
            other => panic!("expected Point of Lengths, got {:?}", other),
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

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION.
    ///
    /// Its predecessor asserted that a BARE `Real` offset built a Plane whose
    /// origin was dimensionless, locking in an ACCIDENTAL pre-doctrine gap: git
    /// archaeology puts that assertion roughly four months before the units
    /// doctrine existed, so it recorded what the code happened to do, never a
    /// sanctioned contract. `docs/prds/v0_6/units-length-gate-completion.md` task
    /// ε (R11, decision D4) shuts that hole at the PRODUCER end — the offset is
    /// REQUIRED to be a Length.
    ///
    /// Do NOT re-flip this back to an acceptance. Doing so re-opens R11 at the
    /// producer while task δ's consumer-side gate stays shut, so the two ends of
    /// the same rule disagree and a bare offset silently mints a dimensionless
    /// plane origin again.
    #[test]
    fn plane_xy_real_zero_is_rejected_expects_length() {
        assert!(eval_builtin("plane_xy", &[Value::Real(0.0)]).is_undef());
    }

    /// The whole plane family rejects a non-LENGTH offset, in every `got` shape
    /// that can reach the gate (`as_f64` must succeed for it to fire, so only
    /// `Real`, `Int` and `Scalar` arrive). The three names share ONE `make_plane`;
    /// enumerating them keeps that sharing honest rather than assumed.
    #[test]
    fn plane_family_rejects_non_length_offsets() {
        let mass = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::MASS,
        };
        for name in ["plane_xy", "plane_xz", "plane_yz"] {
            for offender in [Value::Real(0.0), Value::Int(0), mass.clone()] {
                assert!(
                    eval_builtin(name, std::slice::from_ref(&offender)).is_undef(),
                    "{name}({offender:?}) must be Undef: the offset argument expects Length"
                );
            }
        }
    }

    /// The anti-vacuity partner of the two rows above: a LENGTH offset still
    /// builds a Plane, so the gate cannot pass by rejecting everything.
    ///
    /// It also VERIFIES, rather than re-deriving from the code, the two dimension
    /// claims `make_plane`'s doc makes: the offset's dimension is MIRRORED into
    /// the whole origin triple (a zero offset yields three LENGTH zeros, never
    /// bare `Real(0.0)`), while the synthesized normal stays DIMENSIONLESS —
    /// decision D3's scope lock, since a unit vector legitimately has bare
    /// components.
    #[test]
    fn plane_xy_length_zero_mirrors_length_into_origin_and_keeps_normal_bare() {
        let result = eval_builtin("plane_xy", &[Value::length(0.0)]);
        let Value::Plane { origin, normal } = result else {
            panic!("expected Value::Plane, got {result:?}");
        };
        assert_eq!(
            *origin,
            Value::Point(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0)
            ]),
            "a LENGTH offset must mirror LENGTH into all three origin components"
        );
        assert_eq!(
            *normal,
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
            "the synthesized normal is a unit vector and stays dimensionless (D3)"
        );
    }

    // ── η (task 4387) step-1: construction-datum constructor eval ────────────
    // RED until eval_geometry implements midplane / axis_through / plane_through /
    // offset(arity-2) / frame_at. Pure kernel-free value-algebra; bad args → Undef
    // (mirrors the point3 / plane_xy Undef convention). All premises are exact
    // closed-form: midplane(z=0, z=10mm)→z=5mm; offset(xy,5mm)→z=5mm;
    // axis_through((0,0,0),(0,0,10mm)).dir=(0,0,1); frame_at(o,x̂,ẑ)→identity basis.

    /// Extract the three f64 components (SI meters for dimensioned) of a Point/Vector.
    fn comps3(v: &Value) -> [f64; 3] {
        let comps = match v {
            Value::Point(c) | Value::Vector(c) => c,
            other => panic!("expected Point/Vector, got {:?}", other),
        };
        assert_eq!(comps.len(), 3, "expected 3 components, got {:?}", comps);
        [
            comps[0].as_f64().unwrap(),
            comps[1].as_f64().unwrap(),
            comps[2].as_f64().unwrap(),
        ]
    }

    /// Assert two 3-vectors agree component-wise within a tight tolerance.
    fn approx3(actual: [f64; 3], expected: [f64; 3]) {
        for i in 0..3 {
            assert!(
                (actual[i] - expected[i]).abs() < 1e-12,
                "component {i}: got {}, expected {}",
                actual[i],
                expected[i]
            );
        }
    }

    /// `plane_xy(z)` → a Plane at height `z` (meters) with normal (0,0,1).
    fn plane_at_z(z_meters: f64) -> Value {
        eval_builtin("plane_xy", &[Value::length(z_meters)])
    }

    /// A length-dimensioned Point3 (meters).
    fn point3_len(x: f64, y: f64, z: f64) -> Value {
        Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
    }

    #[test]
    fn midplane_of_two_parallel_planes_returns_midplane() {
        // midplane(z=0, z=10mm) → Plane origin z=5mm, normal (0,0,1)
        let a = plane_at_z(0.0);
        let b = plane_at_z(0.010);
        let result = eval_builtin("midplane", &[a, b]);
        match result {
            Value::Plane { origin, normal } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.005]);
                approx3(comps3(&normal), [0.0, 0.0, 1.0]);
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn axis_through_two_points_returns_axis() {
        // axis_through((0,0,0),(0,0,10mm)) → Axis origin (0,0,0), dir (0,0,1)
        let pa = point3_len(0.0, 0.0, 0.0);
        let pb = point3_len(0.0, 0.0, 0.010);
        let result = eval_builtin("axis_through", &[pa, pb]);
        match result {
            Value::Axis { origin, direction } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.0]);
                approx3(comps3(&direction), [0.0, 0.0, 1.0]);
            }
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn plane_through_three_points_returns_plane() {
        // plane_through((0,0,0),(10mm,0,0),(0,10mm,0)) → Plane origin (0,0,0), normal (0,0,1)
        let p1 = point3_len(0.0, 0.0, 0.0);
        let p2 = point3_len(0.010, 0.0, 0.0);
        let p3 = point3_len(0.0, 0.010, 0.0);
        let result = eval_builtin("plane_through", &[p1, p2, p3]);
        match result {
            Value::Plane { origin, normal } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.0]);
                approx3(comps3(&normal), [0.0, 0.0, 1.0]);
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn offset_plane_by_length_returns_shifted_plane() {
        // offset(z=0 plane normal (0,0,1), 5mm) → Plane origin z=5mm, normal (0,0,1)
        let plane = plane_at_z(0.0);
        let result = eval_builtin("offset", &[plane, Value::length(0.005)]);
        match result {
            Value::Plane { origin, normal } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.005]);
                approx3(comps3(&normal), [0.0, 0.0, 1.0]);
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn frame_at_with_x_and_z_returns_identity_frame() {
        // frame_at((0,0,0), dir(1,0,0), dir(0,0,1)) → Frame origin (0,0,0), identity basis
        let o = point3_len(0.0, 0.0, 0.0);
        let xdir = Value::Direction {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let zdir = Value::Direction {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        let result = eval_builtin("frame_at", &[o, xdir, zdir]);
        match result {
            Value::Frame { origin, basis } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.0]);
                match *basis {
                    Value::Orientation { w, x, y, z } => {
                        assert!((w - 1.0).abs() < 1e-9, "w should be 1, got {w}");
                        assert!(x.abs() < 1e-9, "x should be 0, got {x}");
                        assert!(y.abs() < 1e-9, "y should be 0, got {y}");
                        assert!(z.abs() < 1e-9, "z should be 0, got {z}");
                    }
                    other => panic!("basis should be Orientation, got {:?}", other),
                }
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    // --- construction-datum constructors: bad args → Undef ---

    #[test]
    fn midplane_wrong_arity_undef() {
        assert!(eval_builtin("midplane", &[plane_at_z(0.0)]).is_undef());
    }

    #[test]
    fn midplane_non_plane_args_undef() {
        assert!(eval_builtin("midplane", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
    }

    #[test]
    fn axis_through_wrong_arity_undef() {
        assert!(eval_builtin("axis_through", &[point3_len(0.0, 0.0, 0.0)]).is_undef());
    }

    #[test]
    fn axis_through_non_point_args_undef() {
        assert!(eval_builtin("axis_through", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
    }

    #[test]
    fn plane_through_wrong_arity_undef() {
        let p = point3_len(0.0, 0.0, 0.0);
        assert!(eval_builtin("plane_through", &[p.clone(), p]).is_undef());
    }

    #[test]
    fn offset_wrong_arity_undef() {
        // offset with 1 arg → Undef. The arity-3 form is the γ relation (compiled,
        // not eval'd here); eval only knows the arity-2 datum constructor.
        assert!(eval_builtin("offset", &[plane_at_z(0.0)]).is_undef());
    }

    #[test]
    fn offset_non_plane_first_arg_undef() {
        assert!(eval_builtin("offset", &[Value::Real(1.0), Value::length(0.005)]).is_undef());
    }

    #[test]
    fn frame_at_wrong_arity_undef() {
        assert!(eval_builtin("frame_at", &[point3_len(0.0, 0.0, 0.0)]).is_undef());
    }

    #[test]
    fn frame_at_non_direction_args_undef() {
        let o = point3_len(0.0, 0.0, 0.0);
        assert!(eval_builtin("frame_at", &[o, Value::Real(1.0), Value::Real(2.0)]).is_undef());
    }

    // ── η LENGTH gate (task 6591): the four POINT / PLANE-ORIGIN constructors ─
    //
    // `midplane` / `axis_through` / `plane_through` / `frame_at` REQUIRE LENGTH
    // of every POSITION operand. The dimension-AGREEMENT rule these rows replace
    // admitted any shared dimension, so a wholly bare call built a bare-origin
    // datum and exited 0 — the same R11 hole ε shut at `plane_xy` / `axis_x`, one
    // family over (units-length η, task 6591).
    //
    // DIRECTION / unit-vector operands are NOT gated: `frame_at`'s x/z arrive as
    // `Value::Direction` (three plain f64 fields — no dimension to gate) and plane
    // NORMALS keep their dimension-agnostic decode, because a unit vector
    // legitimately has bare components (decision D3).

    /// A `Value::Plane` whose ORIGIN carries `dim`, with the bare unit +z normal.
    ///
    /// The unit-level equivalent of `frame3(point3(…), orient_identity()).xy_plane`
    /// — which, once this gate lands, is the only `.ri` route left to a
    /// bare-origin Plane (`frame3` validates its origin's SHAPE and never its
    /// dimension; `Frame.xy_plane` clones it verbatim).
    fn plane_with_origin(xyz: [f64; 3], dim: DimensionVector) -> Value {
        Value::Plane {
            origin: Box::new(super::make_point3(xyz, dim)),
            normal: Box::new(super::make_real_vec3([0.0, 0.0, 1.0])),
        }
    }

    /// The +x / +z `Value::Direction` pair every `frame_at` row below passes.
    /// Well-formed on purpose: the only thing under test is the ORIGIN.
    fn unit_x_z_directions() -> (Value, Value) {
        (
            Value::Direction {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Value::Direction {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        )
    }

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — `midplane`'s half of η.
    ///
    /// Two bare-origin planes AGREE on their (dimensionless) origin dimension, so
    /// the rule this replaces accepted them and returned a bare-origin Plane at
    /// exit 0. The mixed rows pin that the gate is per-operand, not a re-spelling
    /// of agreement, and the MASS row pins it as "LENGTH and nothing else".
    #[test]
    fn midplane_requires_length_plane_origins() {
        let bare = plane_with_origin([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let heavy = plane_with_origin([1.0, 2.0, 3.0], DimensionVector::MASS);
        let heavier = plane_with_origin([4.0, 5.0, 6.0], DimensionVector::MASS);
        let long = plane_at_z(0.010);
        for (label, a, b) in [
            ("bare, bare", bare.clone(), bare.clone()),
            // The two AGREEING non-LENGTH rows are what pin the gate as "LENGTH
            // and nothing else" rather than merely "not bare": the rule this
            // replaces compared the two origins to each other, so both of these
            // built a datum at exit 0.
            ("MASS, MASS", heavy.clone(), heavier),
            ("bare, LENGTH", bare.clone(), long.clone()),
            ("LENGTH, bare", long.clone(), bare),
            ("MASS, LENGTH", heavy.clone(), long.clone()),
            ("LENGTH, MASS", long, heavy),
        ] {
            assert!(
                eval_builtin("midplane", &[a, b]).is_undef(),
                "midplane({label}) must be Undef: both plane origins expect Length"
            );
        }
    }

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — the task's HEADLINE case.
    ///
    /// `axis_through(point3(0,0,0), point3(0,0,1))` measured as
    /// `axis(point(0, 0, 0), vec(0, 0, 1))` at exit 0: a bare-origin Axis that
    /// task δ's consumer-side gate then rejects one layer later, 1000× out.
    #[test]
    fn axis_through_requires_length_points() {
        let bare = super::make_point3([0.0, 0.0, 1.0], DimensionVector::DIMENSIONLESS);
        let bare_origin = super::make_point3([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let angular = super::make_point3([1.0, 2.0, 3.0], DimensionVector::ANGLE);
        let angular_far = super::make_point3([1.0, 2.0, 7.0], DimensionVector::ANGLE);
        let long = point3_len(0.0, 0.0, 0.010);
        for (label, a, b) in [
            ("bare, bare", bare_origin, bare.clone()),
            // AGREEING but not LENGTH, and deliberately NON-coincident so the
            // degeneracy guard cannot be what rejects it. The rule this replaces
            // built an Axis here at exit 0.
            ("ANGLE, ANGLE", angular.clone(), angular_far),
            ("bare, LENGTH", bare.clone(), long.clone()),
            ("LENGTH, bare", long.clone(), bare),
            ("ANGLE, LENGTH", angular.clone(), long.clone()),
            ("LENGTH, ANGLE", long, angular),
        ] {
            assert!(
                eval_builtin("axis_through", &[a, b]).is_undef(),
                "axis_through({label}) must be Undef: both points expect Length"
            );
        }
    }

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — `plane_through`'s half.
    #[test]
    fn plane_through_requires_length_points() {
        let bare_o = super::make_point3([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let bare_x = super::make_point3([1.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let bare_y = super::make_point3([0.0, 1.0, 0.0], DimensionVector::DIMENSIONLESS);
        let heavy = super::make_point3([1.0, 2.0, 3.0], DimensionVector::MASS);
        let heavy_x = super::make_point3([5.0, 2.0, 3.0], DimensionVector::MASS);
        let heavy_y = super::make_point3([1.0, 6.0, 3.0], DimensionVector::MASS);
        let l1 = point3_len(0.0, 0.0, 0.0);
        let l2 = point3_len(0.010, 0.0, 0.0);
        let l3 = point3_len(0.0, 0.010, 0.0);
        for (label, a, b, c) in [
            ("bare, bare, bare", bare_o, bare_x.clone(), bare_y.clone()),
            // AGREEING but not LENGTH, and deliberately NON-collinear so the
            // degeneracy guard cannot be what rejects it.
            ("MASS, MASS, MASS", heavy.clone(), heavy_x, heavy_y),
            ("LENGTH, LENGTH, bare", l1.clone(), l2.clone(), bare_y),
            ("LENGTH, bare, LENGTH", l1.clone(), bare_x, l3.clone()),
            ("MASS, LENGTH, LENGTH", heavy, l2.clone(), l3.clone()),
        ] {
            assert!(
                eval_builtin("plane_through", &[a, b, c]).is_undef(),
                "plane_through({label}) must be Undef: all three points expect Length"
            );
        }
    }

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — `frame_at`'s half, which
    /// never checked its origin's dimension AT ALL (not even for agreement).
    ///
    /// Both axis arguments are well-formed `Value::Direction`s, so the ONLY thing
    /// these rows can be rejecting is the origin.
    #[test]
    fn frame_at_requires_a_length_origin() {
        let (xdir, zdir) = unit_x_z_directions();
        for (label, origin) in [
            (
                "bare",
                super::make_point3([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS),
            ),
            (
                "MASS",
                super::make_point3([1.0, 2.0, 3.0], DimensionVector::MASS),
            ),
        ] {
            assert!(
                eval_builtin("frame_at", &[origin, xdir.clone(), zdir.clone()]).is_undef(),
                "frame_at({label} origin) must be Undef: the origin expects Length"
            );
        }
    }

    // ── η LENGTH gate (task 6591): the arity-2 `offset` ──────────────────────
    //
    // BOTH operands are gated. The delta hole is the sharper of the two and is
    // R12-class rather than merely a missing check: `offset(<bare plane>, 2.0)`
    // MEASURED as `plane(point(0, 0, 2), vec(0, 0, 1))` at exit 0, so the
    // dimensionless `2` was silently REINTERPRETED as 2 METRES.
    //
    // `offset` is the one arity-OVERLOADED name in this family. Only the arity-2
    // form is a datum constructor; the arity-3 form is the γ RELATION, claimed by
    // an earlier compiler arm and never evaluated in this module.

    /// FLIPPING THIS ROW IS THE FIX, NOT A REGRESSION — `offset`'s half of η.
    ///
    /// The bare/bare row is the live hole. The LENGTH-plane/bare-delta row was
    /// ALREADY `Undef` via the agreement compare, and is kept as a regression row
    /// so the replacement cannot loosen it — silently, since it produced no
    /// diagnostic either (measured: `offset(plane_xy(5mm), 2.0)` printed `undef`
    /// and exited 0).
    #[test]
    fn offset_requires_a_length_plane_origin_and_a_length_delta() {
        let bare_plane = plane_with_origin([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let heavy_plane = plane_with_origin([1.0, 2.0, 3.0], DimensionVector::MASS);
        let long_plane = plane_at_z(0.010);
        let bare_delta = Value::Real(2.0);
        let angular_delta = Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::ANGLE,
        };
        let long_delta = Value::length(0.005);
        for (label, plane, delta) in [
            // The live hole: a dimensionless `2` silently becomes 2 METRES.
            (
                "bare plane, bare delta",
                bare_plane.clone(),
                bare_delta.clone(),
            ),
            // Already Undef via the agreement compare — a regression row.
            ("LENGTH plane, bare delta", long_plane.clone(), bare_delta),
            ("bare plane, LENGTH delta", bare_plane, long_delta.clone()),
            // AGREEING but not LENGTH: what pins the gate as "LENGTH and nothing
            // else" rather than merely "not bare".
            (
                "MASS plane, MASS delta",
                heavy_plane.clone(),
                Value::Scalar {
                    si_value: 2.0,
                    dimension: DimensionVector::MASS,
                },
            ),
            ("MASS plane, LENGTH delta", heavy_plane, long_delta),
            ("LENGTH plane, ANGLE delta", long_plane, angular_delta),
        ] {
            assert!(
                eval_builtin("offset", &[plane, delta]).is_undef(),
                "offset({label}) must be Undef: both the plane origin and delta expect Length"
            );
        }
    }

    /// The INSEPARABLE control: a LENGTH plane and a DISTINCT NON-ZERO LENGTH
    /// delta still shift the origin along the unit normal, leaving the normal
    /// unchanged and the origin at LENGTH.
    #[test]
    fn offset_of_a_length_plane_by_a_length_delta_still_shifts_the_origin() {
        let result = eval_builtin("offset", &[plane_at_z(0.005), Value::length(0.003)]);
        let Value::Plane { origin, normal } = result else {
            panic!("expected Value::Plane, got {result:?}");
        };
        approx3(comps3(&origin), [0.0, 0.0, 0.008]);
        assert_eq!(
            super::decompose_point3(&origin).map(|(_, dim)| dim),
            Some(DimensionVector::LENGTH),
            "the shifted origin must stay LENGTH"
        );
        approx3(comps3(&normal), [0.0, 0.0, 1.0]);
    }

    /// A plane whose NORMAL is a `Value::Direction` rather than a `Value::Vector`
    /// is still ACCEPTED.
    ///
    /// This is the regression guard for `examples/geometric_relations/`
    /// `construction_datum.ri`, the only real `.ri` call site of this builtin:
    /// its `top_plane` is kernel-realized and MEASURES as
    /// `plane(point(… m), direction(0, 0, 1))`. Narrowing `decode_plane`'s dual
    /// normal acceptance while adding the LENGTH gate would break the shipped
    /// example and its OCCT-gated e2e test while the rest of this suite stayed
    /// green.
    #[test]
    fn offset_accepts_a_plane_whose_normal_is_a_direction() {
        let realized = Value::Plane {
            origin: Box::new(point3_len(0.0, 0.0, 0.005)),
            normal: Box::new(Value::Direction {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }),
        };
        let result = eval_builtin("offset", &[realized, Value::length(0.003)]);
        let Value::Plane { origin, .. } = result else {
            panic!("expected Value::Plane for a Direction-normal plane, got {result:?}");
        };
        approx3(comps3(&origin), [0.0, 0.0, 0.008]);
    }

    /// The arity-3 `offset` is the γ RELATION and is UNTOUCHED by this gate: it
    /// is claimed by an earlier compiler arm and never evaluated in this module,
    /// so eval keeps returning `Undef` for it on ARITY grounds — not on units
    /// grounds. Pinned as behaviour here, and as silence at the classifier in
    /// the step-5 rows.
    #[test]
    fn offset_arity_three_stays_an_arity_rejection() {
        let args = [plane_at_z(0.0), plane_at_z(0.010), Value::length(0.005)];
        assert!(
            eval_builtin("offset", &args).is_undef(),
            "the arity-3 offset is the γ relation and is not evaluated here"
        );
    }

    // ── η LENGTH gate: the INSEPARABLE positive controls ─────────────────────
    //
    // A gate that rejected EVERYTHING would satisfy every rejection row above
    // perfectly. Each control drives the same builtin with LENGTH positions and
    // DISTINCT NON-ZERO coordinates, so it can tell a correct implementation from
    // a hardcoded one — the lesson task 5746's review round 1 recorded.

    /// A LENGTH `midplane` still bisects. The origin is REBUILT at the shared
    /// dimension (`make_point3(mid, …)`), so this pins the geometry and the
    /// dimension rather than a byte-identical round-trip.
    #[test]
    fn midplane_of_length_planes_still_bisects() {
        let result = eval_builtin("midplane", &[plane_at_z(0.002), plane_at_z(0.008)]);
        let Value::Plane { origin, normal } = result else {
            panic!("expected Value::Plane, got {result:?}");
        };
        approx3(comps3(&origin), [0.0, 0.0, 0.005]);
        assert_eq!(
            super::decompose_point3(&origin).map(|(_, dim)| dim),
            Some(DimensionVector::LENGTH),
            "the bisecting plane's origin must stay LENGTH"
        );
        approx3(comps3(&normal), [0.0, 0.0, 1.0]);
    }

    /// A LENGTH `axis_through` still builds its Axis, carrying the first point
    /// VERBATIM (the impl clones it) beside a DIMENSIONLESS direction — D3's
    /// scope lock.
    #[test]
    fn axis_through_length_points_round_trip_origin_and_keep_direction_bare() {
        let pa = point3_len(0.001, 0.002, 0.003);
        let pb = point3_len(0.001, 0.002, 0.007);
        let result = eval_builtin("axis_through", &[pa.clone(), pb]);
        let Value::Axis { origin, direction } = result else {
            panic!("expected Value::Axis, got {result:?}");
        };
        assert_eq!(*origin, pa, "a LENGTH origin must round-trip verbatim");
        assert_eq!(
            *direction,
            super::make_real_vec3([0.0, 0.0, 1.0]),
            "the synthesized direction stays dimensionless (decision D3)"
        );
    }

    /// A LENGTH `plane_through` still builds its Plane, carrying `a` VERBATIM
    /// beside a DIMENSIONLESS normal.
    #[test]
    fn plane_through_length_points_round_trip_origin_and_keep_normal_bare() {
        let a = point3_len(0.001, 0.002, 0.003);
        let b = point3_len(0.005, 0.002, 0.003);
        let c = point3_len(0.001, 0.006, 0.003);
        let result = eval_builtin("plane_through", &[a.clone(), b, c]);
        let Value::Plane { origin, normal } = result else {
            panic!("expected Value::Plane, got {result:?}");
        };
        assert_eq!(*origin, a, "a LENGTH origin must round-trip verbatim");
        assert_eq!(
            *normal,
            super::make_real_vec3([0.0, 0.0, 1.0]),
            "the synthesized normal stays dimensionless (decision D3)"
        );
    }

    /// A LENGTH `frame_at` still builds its Frame, carrying the origin VERBATIM,
    /// from x/z arguments that stay BARE `Value::Direction`s — D3's scope lock,
    /// stated as behaviour: nothing here widens the gate to a unit vector.
    #[test]
    fn frame_at_length_origin_round_trips_from_bare_direction_axes() {
        let (xdir, zdir) = unit_x_z_directions();
        let origin = point3_len(0.004, 0.005, 0.006);
        let result = eval_builtin("frame_at", &[origin.clone(), xdir, zdir]);
        let Value::Frame { origin: o, basis } = result else {
            panic!("expected Value::Frame, got {result:?}");
        };
        assert_eq!(*o, origin, "a LENGTH origin must round-trip verbatim");
        assert!(
            matches!(*basis, Value::Orientation { .. }),
            "expected an Orientation basis, got {basis:?}"
        );
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

    /// transform_log(identity) == Map { angular=[0,0,0] ANGLE, linear=[0,0,0] LENGTH }.
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
        assert_eq!(map_vec3_dim(&result, "angular"), DimensionVector::ANGLE);
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
        assert_eq!(map_vec3_dim(&result, "angular"), DimensionVector::ANGLE);
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
    /// The gate is `t_dim != LENGTH` per RULING #6126 — LENGTH is the single admitted
    /// dimension. MASS flows through the same rejection branch as ANGLE, so MASS and
    /// ANGLE remain covered as two distinct non-admitted dimensions and no future
    /// re-widening of the gate (e.g. re-admitting one dimension as a special case) can
    /// silently pass.
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

    /// transform_log with DIMENSIONLESS translation → Undef.
    ///
    /// RULING #6126: a twist's `linear` half is `Vector3<Length>`, and mirrored, a
    /// Transform's translation on the log↔exp seam carries LENGTH and only LENGTH.
    /// DIMENSIONLESS is therefore no longer admitted — after the `Real` →
    /// `Scalar{DIMENSIONLESS}` unification, "admits DIMENSIONLESS" means "admits bare
    /// numbers", which is the affordance this ruling removes.
    #[test]
    fn transform_log_dimensionless_translation_returns_undef() {
        let t = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
        };
        assert!(
            eval_builtin("transform_log", &[t]).is_undef(),
            "RULING #6126: a Transform's translation must be Vector3<Length>; \
             DIMENSIONLESS is no longer admitted"
        );
    }

    // ── transform_exp tests (step-21) ────────────────────────────────────────

    /// Helper: build a twist Map with given angular & linear vectors, each half
    /// carrying an explicit dimension. `DIMENSIONLESS` components are built as
    /// `Value::Real` (not `Scalar{DIMENSIONLESS}`), matching how `.ri` bare numbers
    /// actually reach eval.
    ///
    /// The angular dimension is a parameter because both eval and the classifier gate on
    /// it (see `diagnose`'s transform_exp arm), which is only testable by building a
    /// NON-default angular half.
    fn make_twist_with_dims(
        angular: [f64; 3],
        angular_dim: DimensionVector,
        linear: [f64; 3],
        linear_dim: DimensionVector,
    ) -> Value {
        let component = |v: f64, dim: DimensionVector| -> Value {
            if dim.is_dimensionless() {
                Value::Real(v)
            } else {
                Value::Scalar {
                    si_value: v,
                    dimension: dim,
                }
            }
        };
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(angular.iter().map(|v| component(*v, angular_dim)).collect()),
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(linear.iter().map(|v| component(*v, linear_dim)).collect()),
        );
        Value::Map(m)
    }

    /// Helper: build a twist Map with an ANGLE angular half (the only one eval admits,
    /// RULING #6080) and a given linear dimension.
    fn make_twist(angular: [f64; 3], linear: [f64; 3], linear_dim: DimensionVector) -> Value {
        make_twist_with_dims(angular, DimensionVector::ANGLE, linear, linear_dim)
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
    ///
    /// The angular half must be a VALID ANGLE vector (#6080), or this test would
    /// return Undef at the angular gate and stop covering the missing-key path it
    /// is named for.
    #[test]
    fn transform_exp_missing_linear_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with a non-ANGLE angular dimension → Undef (#6080).
    #[test]
    fn transform_exp_angular_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]), // LENGTH instead of ANGLE
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with non-LENGTH linear dimension → Undef.
    ///
    /// The angular half must be a VALID ANGLE vector (#6080), or this test would
    /// return Undef at the angular gate and stop covering the linear one (#6126's)
    /// it is named for.
    #[test]
    fn transform_exp_linear_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]),
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]), // ANGLE instead of LENGTH
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with DIMENSIONLESS linear dimension → Undef.
    ///
    /// RULING #6126 closes the LAST `LENGTH | DIMENSIONLESS` admission in the transform
    /// family: a twist's `linear` half is `Vector3<Length>`, so a dimensionless linear
    /// is rejected exactly like ANGLE or MASS.
    ///
    /// This positive-acceptance path was previously UNTESTED — every existing
    /// `transform_exp` test builds a LENGTH twist, and the comment formerly sitting on
    /// `transform_log_dimensionless_translation_returns_non_undef` mis-cited
    /// `transform_exp_zero_twist_is_identity` as covering the dimensionless case.
    #[test]
    fn transform_exp_dimensionless_linear_returns_undef() {
        let twist = make_twist(
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
        );
        assert!(
            eval_builtin("transform_exp", &[twist]).is_undef(),
            "RULING #6126: a twist's `linear` must be Vector3<Length>; \
             DIMENSIONLESS is no longer admitted"
        );
    }

    // ── Twist.angular DIMENSION tests (#6080) ────────────────────────────────
    //
    // A twist's angular half is a rotation vector — axis * angle — so it carries
    // ANGLE, exactly like `orient_log`'s output. Both ends of the transform_log /
    // transform_exp pair are pinned here, because the pair is an identity and only
    // moves correctly if the emitter and the gate agree.
    //
    // SCOPE: the `linear` half is NOT touched by any of these. Its `Vector3<Length>`
    // gate is RULING #6126's, and `transform_log_then_exp_round_trip_angular_is_angle`
    // asserts positively that this task leaves it alone.

    /// The transform_log tests above read `angular` through `map_vec3_components`
    /// (i.e. `as_f64()`), which is dimension-blind, and `map_vec3_dim`, which reads
    /// only component 0; this asserts every component's dimension directly.
    #[test]
    fn transform_log_emits_angle_dimensioned_angular() {
        let t = make_transform(make_rot90z(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_log", &[t]);
        let map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Map, got {other:?}"),
        };
        let ang = match map.get(&Value::String("angular".to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => items,
            other => panic!("expected Vector3 angular, got {other:?}"),
        };
        assert_scalar_approx!(ang[0].clone(), 0.0, DimensionVector::ANGLE);
        assert_scalar_approx!(ang[1].clone(), 0.0, DimensionVector::ANGLE);
        assert_scalar_approx!(
            ang[2].clone(),
            std::f64::consts::FRAC_PI_2,
            DimensionVector::ANGLE
        );
    }

    /// The zero / near-identity rotation path is a SEPARATE return path in
    /// `transform_log`, so it would otherwise stay dimensionless unnoticed.
    #[test]
    fn transform_log_identity_emits_angle_dimensioned_zero_angular() {
        let id = eval_builtin("transform3_identity", &[]);
        let result = eval_builtin("transform_log", &[id]);
        let map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Map, got {other:?}"),
        };
        let ang = match map.get(&Value::String("angular".to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => items,
            other => panic!("expected Vector3 angular, got {other:?}"),
        };
        for comp in ang.iter() {
            assert_scalar_approx!(comp.clone(), 0.0, DimensionVector::ANGLE);
        }
    }

    /// The widening: an ANGLE angular half is accepted.
    #[test]
    fn transform_exp_accepts_angle_angular() {
        let twist = make_twist_with_dims(
            [0.0, 0.0, std::f64::consts::FRAC_PI_2],
            DimensionVector::ANGLE,
            [1.0, 2.0, 3.0],
            DimensionVector::LENGTH,
        );
        let result = eval_builtin("transform_exp", &[twist]);
        match result {
            Value::Transform { rotation, .. } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
            }
            other => panic!("expected Transform, got {other:?}"),
        }
    }

    /// The narrowing: a DIMENSIONLESS angular half is rejected. DIMENSIONLESS is a
    /// SPECIFIC dimension (the zero exponent vector), not a wildcard, so admitting it
    /// would re-open the hole PRD #5747 decision D11 closed for this family.
    #[test]
    fn transform_exp_rejects_dimensionless_angular() {
        let twist = make_twist_with_dims(
            [0.0, 0.0, std::f64::consts::FRAC_PI_2],
            DimensionVector::DIMENSIONLESS,
            [1.0, 2.0, 3.0],
            DimensionVector::LENGTH,
        );
        assert!(
            eval_builtin("transform_exp", &[twist]).is_undef(),
            "#6080: a twist's `angular` must be Vector3<Angle>; DIMENSIONLESS is a \
             specific dimension, not a wildcard"
        );
    }

    /// Regression pin: widening to ANGLE must not have made the gate merely permissive.
    ///
    /// Distinct from `transform_exp_angular_wrong_dimension_returns_undef` above, which
    /// uses a ZERO-magnitude LENGTH vector: this one is non-zero, so the rejection
    /// cannot be confused with the `theta < EPS` identity-rotation path.
    #[test]
    fn transform_exp_rejects_length_angular() {
        let twist = make_twist_with_dims(
            [0.0, 0.0, 0.001],
            DimensionVector::LENGTH,
            [1.0, 2.0, 3.0],
            DimensionVector::LENGTH,
        );
        assert!(eval_builtin("transform_exp", &[twist]).is_undef());
    }

    /// Coupled-change guard: `transform_exp(transform_log(T)) ≈ T` survives only if the
    /// emission and the gate agree on ANGLE. Also asserts the intermediate twist's
    /// angular half is ANGLE — and, positively, that its linear half is still LENGTH,
    /// i.e. that #6126's half is untouched by this task.
    #[test]
    fn transform_log_then_exp_round_trip_angular_is_angle() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q, 1.5, -2.5, 3.0);
        let twist = eval_builtin("transform_log", std::slice::from_ref(&t));
        assert_eq!(map_vec3_dim(&twist, "angular"), DimensionVector::ANGLE);
        assert_eq!(map_vec3_dim(&twist, "linear"), DimensionVector::LENGTH);
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
                    other => panic!("expected Vector3, got {other:?}"),
                }
            }
            other => panic!("expected Transform, got {other:?}"),
        }
    }

    /// transform_exp with LENGTH linear dimension → accepted, translation stays LENGTH.
    ///
    /// The one-sidedness control for `transform_exp_dimensionless_linear_returns_undef`:
    /// the narrowing must reject non-LENGTH without being "fixed" by rejecting
    /// everything. Asserts both the accepted shape and that the emitted translation
    /// components still carry `LENGTH`.
    #[test]
    fn transform_exp_length_linear_is_accepted() {
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], DimensionVector::LENGTH);
        let result = eval_builtin("transform_exp", &[twist]);
        let translation = match &result {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("transform_exp on a LENGTH twist must yield a Transform; got {other:?}"),
        };
        let comps = match translation {
            Value::Vector(items) => items,
            other => panic!("translation must be a Vector; got {other:?}"),
        };
        assert_eq!(comps.len(), 3, "translation must have 3 components");
        for (i, c) in comps.iter().enumerate() {
            assert_eq!(
                c.dimension(),
                DimensionVector::LENGTH,
                "translation component {i} must carry LENGTH; got {c:?}"
            );
        }
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
        // Non-finite component. All three are LENGTH so that non-finiteness is
        // the ONLY reason this is Undef: a bare `Real` triple would also be
        // rejected by the R12 dimension gate, and the row would stop witnessing
        // the `is_finite()` check it is named for.
        let nan = [
            Value::length(f64::NAN),
            Value::length(0.0),
            Value::length(0.0),
        ];
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
        assert!(
            eval_builtin("affine_map", std::slice::from_ref(&m)).is_undef(),
            "1 arg"
        );
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

    // ── units-length ζ (task 5747): the R12 affine_map TRANSLATION ────────────

    #[test]
    fn affine_map_mass_translation_returns_undef() {
        let m = matrix3x3(IDENTITY_3X3);
        let mass = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MASS,
        };
        let t = Value::Vector(vec![mass(5.0), mass(0.0), mass(0.0)]);
        assert!(
            eval_builtin("affine_map", &[m, t]).is_undef(),
            "a MASS translation Vector must be Undef (ζ/R12)"
        );
    }

    #[test]
    fn affine_map_bare_real_translation_returns_undef() {
        let m = matrix3x3(IDENTITY_3X3);
        let t = Value::Vector(vec![
            Value::Real(5.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("affine_map", &[m, t]).is_undef(),
            "a bare Real translation Vector must be Undef (ζ/R12)"
        );
    }

    #[test]
    fn diagnose_affine_map_mass_translation_is_a_coded_error() {
        let m = matrix3x3(IDENTITY_3X3);
        let mass = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MASS,
        };
        let t = Value::Vector(vec![mass(5.0), mass(0.0), mass(0.0)]);
        let diag = super::diagnose("affine_map", &[m, t])
            .expect("a MASS translation must be diagnosed");
        assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "{diag:?}"
        );
        // FULL EQUALITY — the message IS the user-facing contract. `translation`
        // is the builtin's own parameter name, unlike `affine_translate`'s three
        // positional scalars.
        assert_eq!(
            diag.message,
            "affine_map: translation argument expects Length, got Mass Scalar; \
             pass a dimensioned length such as `5mm`"
        );
    }

    #[test]
    fn diagnose_affine_map_bare_real_translation_is_a_coded_error() {
        let m = matrix3x3(IDENTITY_3X3);
        let t = Value::Vector(vec![
            Value::Real(5.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let diag = super::diagnose("affine_map", &[m, t])
            .expect("a bare Real translation must be diagnosed");
        assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "{diag:?}"
        );
        assert_eq!(
            diag.message,
            "affine_map: translation argument expects Length, got Real; \
             pass a dimensioned length such as `5mm`"
        );
    }

    #[test]
    fn diagnose_affine_map_silent_cases_return_none() {
        let m = matrix3x3(IDENTITY_3X3);
        let length_t = Value::Vector(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            super::diagnose("affine_map", &[m.clone(), length_t.clone()]).is_none(),
            "an accepted LENGTH translation must not be diagnosed"
        );
        // Non-3 Vector, non-Vector, and wrong arity are SHAPE failures, not units
        // ones — silent, matching the `transform3` convention.
        assert!(
            super::diagnose(
                "affine_map",
                &[
                    m.clone(),
                    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0)])
                ]
            )
            .is_none(),
            "a non-3 Vector translation must stay silent"
        );
        assert!(
            super::diagnose("affine_map", &[m.clone(), Value::Real(0.0)]).is_none(),
            "a non-Vector translation must stay silent"
        );
        assert!(
            super::diagnose("affine_map", &[]).is_none(),
            "0 args must stay silent"
        );
        assert!(
            super::diagnose("affine_map", std::slice::from_ref(&m)).is_none(),
            "1 arg must stay silent"
        );
        assert!(
            super::diagnose("affine_map", &[m.clone(), length_t, Value::Real(0.0)]).is_none(),
            "3 args must stay silent"
        );

        // NO-MIS-ATTRIBUTION GUARD, mirroring `affine_translate`'s
        // `diagnose_affine_translate_mixed_dimensions_stays_silent` and its
        // non-finite row. Both causes below reach the identical silent path —
        // `classify_affine_map_args` reading `AffineMapFault::Shape` out of
        // `decompose_xyz3` — and both are deliberate NON-rejections. Without
        // these rows a later change that "completes" the arm (say, falling back
        // to `accept_arg(&items[0], ..)` when `decompose_xyz3` returns `None`)
        // would start reporting `affine_map: translation argument expects Length,
        // got ...` for a MIXED-dimension triple, misnaming a consistency fault as
        // a units fault, with the suite staying green. Each row pins the eval and
        // diagnose halves TOGETHER, so the pair cannot drift.
        for (label, translation) in [
            (
                "mixed dimensions",
                Value::Vector(vec![
                    Value::length(0.005),
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::MASS,
                    },
                    Value::length(0.0),
                ]),
            ),
            (
                "non-finite LENGTH",
                Value::Vector(vec![
                    Value::length(f64::NAN),
                    Value::length(0.0),
                    Value::length(0.0),
                ]),
            ),
            (
                "non-numeric",
                Value::Vector(vec![
                    Value::String("x".to_string()),
                    Value::length(0.0),
                    Value::length(0.0),
                ]),
            ),
        ] {
            let args = [m.clone(), translation];
            assert!(
                eval_builtin("affine_map", &args).is_undef(),
                "{label}: was Undef before ζ and stays Undef"
            );
            assert!(
                super::diagnose("affine_map", &args).is_none(),
                "{label}: a consistency/shape failure is NOT a ζ units rejection and must \
                 stay silent; got: {:?}",
                super::diagnose("affine_map", &args)
            );
        }
    }

    /// ANGLE is a third non-admitted dimension alongside MASS and dimensionless,
    /// and it must be NAMED rather than hardcoded — the sibling coverage
    /// `affine_translate_shared_angle_returns_undef` /
    /// `diagnose_transform_log_angle_translation_names_angle` already carries for
    /// their arms. Pins the eval and diagnose halves together.
    #[test]
    fn affine_map_shared_angle_translation_is_undef_and_names_angle() {
        let m = matrix3x3(IDENTITY_3X3);
        let t = Value::Vector(vec![Value::angle(1.0), Value::angle(0.0), Value::angle(0.0)]);
        assert!(
            eval_builtin("affine_map", &[m.clone(), t.clone()]).is_undef(),
            "a shared-ANGLE translation Vector must be Undef (ζ/R12)"
        );
        let diag = super::diagnose("affine_map", &[m, t])
            .expect("an ANGLE translation must be diagnosed");
        assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "{diag:?}"
        );
        assert!(
            diag.message.contains("Angle"),
            "the message must NAME the offending dimension rather than hardcode one \
             string, got: {}",
            diag.message
        );
    }

    /// LINEAR-PART SCOPE LOCK — the row that stops ζ drifting into D11's other
    /// half.
    ///
    /// `affine_map` with a LENGTH-dimensioned 3×3 LINEAR part and a LENGTH
    /// translation must STILL return `Undef` via the pre-existing dimensionless
    /// check, and `diagnose` must return `None` for it: the linear part's
    /// rejection is NOT a ζ units diagnostic and must not acquire one, or the
    /// message would misname the offending argument as `translation`.
    #[test]
    fn diagnose_affine_map_dimensioned_linear_stays_silent() {
        let m = Value::Matrix(vec![
            vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(1.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(0.0), Value::length(1.0)],
        ]);
        let length_t = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("affine_map", &[m.clone(), length_t.clone()]).is_undef(),
            "a dimensioned linear part must stay Undef (D11, pre-existing)"
        );
        assert!(
            super::diagnose("affine_map", &[m, length_t]).is_none(),
            "the linear part's rejection must NOT acquire a ζ units diagnostic — it \
             would misname the offending argument"
        );
    }

    /// GUARD ORDER, proven rather than argued in a comment.
    ///
    /// `classify_affine_map_args` classifies the LINEAR part FIRST, so every
    /// linear-part failure must stay silent EVEN WHEN the translation is ALSO
    /// bad. Without the second column below, `affine_map(<2x2>, <MASS triple>)`
    /// could start reporting "translation argument expects Length" for a call
    /// whose real fault is the matrix — sending the author to the wrong argument
    /// — and nothing would fail.
    ///
    /// The sibling `diagnose_affine_map_dimensioned_linear_stays_silent` is the
    /// scope lock on the dimensioned-3×3 arm; this covers all three arms the
    /// classifier can reject a linear part through, which were otherwise proven
    /// for only one of the three.
    #[test]
    fn diagnose_affine_map_linear_failures_stay_silent_even_with_a_bad_translation() {
        let mass = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MASS,
        };
        let length_t = Value::Vector(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let mass_t = Value::Vector(vec![mass(5.0), mass(0.0), mass(0.0)]);

        let two_by_two = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        let three_by_four = Value::Matrix(vec![
            vec![
                Value::Real(1.0),
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ],
            vec![
                Value::Real(0.0),
                Value::Real(1.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ],
            vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(1.0),
                Value::Real(0.0),
            ],
        ]);
        let dimensioned_3x3 = Value::Matrix(vec![
            vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(1.0), Value::length(0.0)],
            vec![Value::length(0.0), Value::length(0.0), Value::length(1.0)],
        ]);

        for (label, linear) in [
            ("2x2 matrix", two_by_two),
            ("3x4 matrix", three_by_four),
            ("non-matrix args[0]", Value::Real(1.0)),
            ("dimensioned 3x3", dimensioned_3x3),
        ] {
            for (t_label, translation) in [
                ("good LENGTH translation", &length_t),
                ("bad MASS translation", &mass_t),
            ] {
                let args = [linear.clone(), translation.clone()];
                assert!(
                    eval_builtin("affine_map", &args).is_undef(),
                    "{label} / {t_label}: a linear-part failure must still be Undef"
                );
                assert!(
                    super::diagnose("affine_map", &args).is_none(),
                    "{label} / {t_label}: the LINEAR part is classified FIRST, so this must \
                     stay silent rather than misname `translation`; got: {:?}",
                    super::diagnose("affine_map", &args)
                );
            }
        }
    }

    // ── affine_from_transform tests (step-9) ──────────────────────────────────

    /// Assert two 3×3 matrices are elementwise equal within `tol`.
    fn assert_matrix_approx(actual: [[f64; 3]; 3], expected: [[f64; 3]; 3], tol: f64) {
        for (r, (arow, erow)) in actual.iter().zip(expected.iter()).enumerate() {
            for (c, (a, e)) in arow.iter().zip(erow.iter()).enumerate() {
                assert!(
                    (a - e).abs() < tol,
                    "linear[{r}][{c}]: expected {e}, got {a} (tol {tol})",
                );
            }
        }
    }

    #[test]
    fn affine_from_transform_identity_yields_identity_map() {
        let t = eval_builtin("transform3_identity", &[]);
        let (linear, translation) = expect_affine(eval_builtin("affine_from_transform", &[t]));
        assert_eq!(
            linear, IDENTITY_3X3,
            "identity transform must widen to identity linear EXACTLY"
        );
        assert_eq!(
            translation,
            [0.0, 0.0, 0.0],
            "identity transform translation must be 0"
        );
    }

    #[test]
    fn affine_from_transform_z90_yields_rotation_matrix() {
        // 90°-Z quaternion (√2/2, 0, 0, √2/2), translation (5mm, 0, 0).
        let q = Value::Orientation {
            w: std::f64::consts::FRAC_1_SQRT_2,
            x: 0.0,
            y: 0.0,
            z: std::f64::consts::FRAC_1_SQRT_2,
        };
        let trans = Value::Vector(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let t = eval_builtin("transform3", &[q, trans]);
        let (linear, translation) = expect_affine(eval_builtin("affine_from_transform", &[t]));
        assert_matrix_approx(
            linear,
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            1e-12,
        );
        assert!((translation[0] - 0.005).abs() < 1e-12, "tx");
        assert!(translation[1].abs() < 1e-12, "ty");
        assert!(translation[2].abs() < 1e-12, "tz");
    }

    #[test]
    fn affine_from_transform_non_transform_returns_undef() {
        assert!(
            eval_builtin("affine_from_transform", &[Value::Real(1.0)]).is_undef(),
            "non-Transform arg must be Undef"
        );
    }

    #[test]
    fn affine_from_transform_wrong_arity_returns_undef() {
        assert!(
            eval_builtin("affine_from_transform", &[]).is_undef(),
            "0 args"
        );
        let t = eval_builtin("transform3_identity", &[]);
        assert!(
            eval_builtin("affine_from_transform", &[t.clone(), t]).is_undef(),
            "2 args"
        );
    }

    // ── diagnose classifier tests (step-15) ───────────────────────────────────
    // The post-Undef diagnose hook (mirrors stackup_diagnose/fea_diagnose) lets a
    // pure value constructor surface a CLI warning for the two distinguishable
    // affine_scale failure causes: a zero (degenerate) factor and a dimensioned
    // factor. Arity errors stay silent (None); only these two cases warn.

    #[test]
    fn diagnose_affine_scale_zero_factor_warns_degenerate() {
        let diag = super::diagnose(
            "affine_scale",
            &[Value::Real(0.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("zero scale factor must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "zero-factor diagnostic must be a warning"
        );
        assert!(
            diag.message.contains("degenerate"),
            "zero-factor message must mention the degenerate (det=0) cause, got: {}",
            diag.message
        );
    }

    #[test]
    fn diagnose_affine_scale_dimensioned_factor_warns_dimensionless() {
        let diag = super::diagnose(
            "affine_scale",
            &[Value::length(2.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("dimensioned scale factor must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Warning,
            "dimensioned-factor diagnostic must be a warning"
        );
        assert!(
            diag.message.contains("dimensionless"),
            "dimensioned-factor message must mention the dimensionless requirement, got: {}",
            diag.message
        );
    }

    #[test]
    fn diagnose_affine_scale_valid_factors_returns_none() {
        assert!(
            super::diagnose(
                "affine_scale",
                &[Value::Real(2.0), Value::Real(1.0), Value::Real(0.5)],
            )
            .is_none(),
            "valid scale factors must not produce a diagnostic"
        );
    }

    // ── units-length ζ (task 5747): the R12 affine_translate translation ──────
    //
    // TWO HALVES, because the VALUE and the DIAGNOSTIC are produced by different
    // functions: `eval_builtin` decides whether the map is built at all, while
    // `diagnose` — the post-`Undef` hook reify-expr already calls — is what
    // supplies the `Severity::Error` that makes `reify eval` exit 1. Returning
    // `Value::Undef` alone does NOT flip the exit code (measured: a mixed-dimension
    // `affine_translate(5mm, 0kg, 0mm)` prints `undef` + a `note:` at exit 0).

    #[test]
    fn affine_translate_shared_mass_returns_undef() {
        let mass = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MASS,
        };
        assert!(
            eval_builtin("affine_translate", &[mass(5.0), mass(0.0), mass(0.0)]).is_undef(),
            "a shared-MASS translation triple must be Undef (ζ/R12)"
        );
    }

    #[test]
    fn affine_translate_bare_reals_return_undef() {
        // D1 / ratified decision 2: bare is DIMENSIONLESS and strict equality
        // rejects it. Zero is NOT special-cased.
        assert!(
            eval_builtin(
                "affine_translate",
                &[Value::Real(5.0), Value::Real(0.0), Value::Real(0.0)]
            )
            .is_undef(),
            "a bare Real translation triple must be Undef (ζ/R12)"
        );
        assert!(
            eval_builtin(
                "affine_translate",
                &[Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]
            )
            .is_undef(),
            "bare ZERO is not special-cased"
        );
    }

    #[test]
    fn affine_translate_shared_angle_returns_undef() {
        assert!(
            eval_builtin(
                "affine_translate",
                &[Value::angle(1.0), Value::angle(0.0), Value::angle(0.0)]
            )
            .is_undef(),
            "a shared-ANGLE translation triple must be Undef (ζ/R12)"
        );
    }

    #[test]
    fn diagnose_affine_translate_mass_is_a_coded_error() {
        let mass = |v: f64| Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MASS,
        };
        let diag = super::diagnose("affine_translate", &[mass(5.0), mass(0.0), mass(0.0)])
            .expect("a MASS translation triple must be diagnosed");
        assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "{diag:?}"
        );
        // FULL EQUALITY, not `contains`: this message IS the user-facing contract.
        assert_eq!(
            diag.message,
            "affine_translate: dx/dy/dz argument expects Length, got Mass Scalar; \
             pass a dimensioned length such as `5mm`"
        );
    }

    #[test]
    fn diagnose_affine_translate_bare_real_is_a_coded_error() {
        let diag = super::diagnose(
            "affine_translate",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(0.0)],
        )
        .expect("a bare Real translation triple must be diagnosed");
        assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "{diag:?}"
        );
        assert_eq!(
            diag.message,
            "affine_translate: dx/dy/dz argument expects Length, got Real; \
             pass a dimensioned length such as `5mm`"
        );
    }

    #[test]
    fn diagnose_affine_translate_length_returns_none() {
        assert!(
            super::diagnose(
                "affine_translate",
                &[Value::length(0.005), Value::length(0.0), Value::length(0.0)]
            )
            .is_none(),
            "an accepted LENGTH triple must not be diagnosed"
        );
    }

    /// ζ deliberately changes only what its OWN gate newly rejects.
    ///
    /// A MIXED-dimension triple fails `decompose_xyz3`'s CONSISTENCY rule, not
    /// the LENGTH rule — and it ALREADY returned `Undef` at exit 0 before ζ.
    /// Pinning the non-change stops a later reader "completing" it by accident
    /// and thereby misnaming the offending argument.
    #[test]
    fn diagnose_affine_translate_mixed_dimensions_stays_silent() {
        let args = [
            Value::length(0.005),
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::MASS,
            },
            Value::length(0.0),
        ];
        assert!(
            eval_builtin("affine_translate", &args).is_undef(),
            "a mixed-dimension triple was Undef before ζ and stays Undef"
        );
        assert!(
            super::diagnose("affine_translate", &args).is_none(),
            "a consistency failure is NOT a ζ units rejection and must stay silent"
        );
    }

    /// Wrong arity and non-numeric / non-finite components stay silent, matching
    /// the `transform3` convention the existing `diagnose` doc states.
    #[test]
    fn diagnose_affine_translate_arity_and_non_numeric_stay_silent() {
        for (label, args) in [
            ("0 args", vec![]),
            ("1 arg", vec![Value::Real(1.0)]),
            ("2 args", vec![Value::Real(1.0), Value::Real(2.0)]),
            (
                "4 args",
                vec![
                    Value::length(1.0),
                    Value::length(2.0),
                    Value::length(3.0),
                    Value::length(4.0),
                ],
            ),
            (
                "non-numeric",
                vec![
                    Value::String("x".to_string()),
                    Value::length(0.0),
                    Value::length(0.0),
                ],
            ),
            (
                "non-finite",
                vec![Value::Real(f64::NAN), Value::Real(0.0), Value::Real(0.0)],
            ),
        ] {
            assert!(
                super::diagnose("affine_translate", &args).is_none(),
                "{label}: must stay silent; got: {:?}",
                super::diagnose("affine_translate", &args)
            );
        }
    }

    // ── diagnose: ε's construction-datum LENGTH arms (task 5746, R11) ─────────
    // The gate is user-observable ONLY through this hook. A bare datum call
    // returns `Value::Undef`, which on its own prints `undef` and exits 0
    // (measured against `target/debug/reify` before the arms landed:
    // "DatumUnitsPlaneBare.p = undef", exit 0) — `push_op_contract_failure`
    // writes `undef_causes`, not the diagnostics sink, and the CLI exit gate is a
    // pure `Severity::Error` fold. So the Error these arms emit IS the exit code.

    /// The `got` shapes that can reach ε's gate. `as_f64` must succeed for it to
    /// fire, so only `Real`, `Int` and `Scalar` ever arrive, and a LENGTH
    /// `Scalar` is accepted rather than rejected — leaving the dimensionless and
    /// dimensioned `Scalar` forms. Exhaustive by that argument, which is what
    /// makes the enumeration executable rather than aspirational.
    fn non_length_offenders() -> [Value; 4] {
        [
            Value::Real(5.0),
            Value::Int(5),
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::MASS,
            },
        ]
    }

    /// Every offending shape, on every name in the plane family, is reported as
    /// the SHARED `ArgRejection` sentence at `Severity::Error` carrying the
    /// pre-existing `DimensionedArgRejected` code — ruling A7, one rejection
    /// REASON gets one code, which is how ε joins the six landed dimension arms
    /// rather than opening a seventh way to report the same fault class.
    ///
    /// The expected wording is obtained LIVE from the owner on every row instead
    /// of being spelled here, so a reword on `ArgRejection::message`'s side fails
    /// this test rather than silently forking this crate's copy (Contract C1
    /// invariant (i)).
    #[test]
    fn diagnose_plane_family_non_length_offset_is_the_shared_coded_error() {
        use reify_ir::arg_acceptance::{Acceptance, accept_arg, length_spec};

        for offender in non_length_offenders() {
            let Acceptance::Rejected(rejection) = accept_arg(&offender, &length_spec()) else {
                panic!("{offender:?}: the owner must REJECT this at a length_spec position");
            };
            for name in ["plane_xy", "plane_xz", "plane_yz"] {
                let diag = super::diagnose(name, std::slice::from_ref(&offender))
                    .unwrap_or_else(|| panic!("{name}({offender:?}) must be diagnosed"));
                assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
                assert_eq!(
                    diag.code,
                    Some(reify_core::DiagnosticCode::DimensionedArgRejected),
                    "{diag:?}"
                );
                assert_eq!(diag.message, rejection.message(name, "offset"), "{diag:?}");
            }
        }
    }

    /// The axis half of the row above. ONE message names the whole `ox/oy/oz`
    /// triple: the decoder both the gate and the classifier read has ALREADY
    /// required the three components to share one dimension, so when this fires
    /// all three positions offend identically and naming them together is
    /// COMPLETE information, not a shortcut.
    #[test]
    fn diagnose_axis_family_non_length_origin_is_the_shared_coded_error() {
        use reify_ir::arg_acceptance::{Acceptance, accept_arg, length_spec};

        for offender in non_length_offenders() {
            let Acceptance::Rejected(rejection) = accept_arg(&offender, &length_spec()) else {
                panic!("{offender:?}: the owner must REJECT this at a length_spec position");
            };
            let origin = Value::Point(vec![offender.clone(), offender.clone(), offender.clone()]);
            for name in ["axis_x", "axis_y", "axis_z"] {
                let diag = super::diagnose(name, std::slice::from_ref(&origin))
                    .unwrap_or_else(|| panic!("{name}({origin:?}) must be diagnosed"));
                assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
                assert_eq!(
                    diag.code,
                    Some(reify_core::DiagnosticCode::DimensionedArgRejected),
                    "{diag:?}"
                );
                assert_eq!(
                    diag.message,
                    rejection.message(name, "ox/oy/oz"),
                    "{diag:?}"
                );
            }
        }
    }

    /// ε's two arms speak ONLY for a well-formed, dimension-consistent,
    /// non-LENGTH argument. Every OTHER cause of the same `Value::Undef` stays
    /// silent, so an unrelated failure is never mis-attributed to a dimension
    /// problem — the no-mis-attribution contract the four landed sibling arms
    /// already hold.
    ///
    /// Each row is a cause the gate rejects for its OWN reason: wrong arity; an
    /// unresolved cell (D10 — an unresolved cell is not a wrong one); a
    /// non-numeric argument or component; a NON-FINITE one (a finiteness
    /// failure, which is exactly why `plane_xy_nan_returns_undef` /
    /// `plane_xy_inf_returns_undef` stay green for their own reason); an
    /// accepted LENGTH argument; and — on the axis side — a non-`Point`, a
    /// wrong-length `Point`, and a MIXED-dimension `Point`, which is a
    /// `decompose_xyz3` CONSISTENCY failure rather than a LENGTH one.
    ///
    /// The MIXED-dimension and non-finite axis rows are silent over inputs NO
    /// `.ri` source can produce: `point3` refuses a mixed triple and `1mm / 0.0`
    /// is already `undef`, both measured, so such a `Value::Point` only ever
    /// exists hand-built in a test. [`super::make_axis`]'s doc carries the
    /// measurement. They are pinned here so the silence reads as the deliberate
    /// contract it is rather than as an unnoticed gap.
    #[test]
    fn diagnose_datum_non_dimension_causes_stay_silent() {
        let triple = |c: Value| Value::Point(vec![c.clone(), c.clone(), c]);
        let mass = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::MASS,
        };
        for (label, name, args) in [
            ("plane: wrong arity (zero)", "plane_xy", vec![]),
            (
                "plane: wrong arity (two)",
                "plane_xy",
                vec![Value::Real(0.0), Value::Real(0.0)],
            ),
            (
                "plane: Undef argument (D10)",
                "plane_xy",
                vec![Value::Undef],
            ),
            (
                "plane: non-numeric argument",
                "plane_xy",
                vec![Value::Bool(true)],
            ),
            (
                "plane: NaN argument",
                "plane_xy",
                vec![Value::Real(f64::NAN)],
            ),
            (
                "plane: infinite argument",
                "plane_xz",
                vec![Value::Real(f64::INFINITY)],
            ),
            (
                "plane: accepted LENGTH",
                "plane_yz",
                vec![Value::length(0.005)],
            ),
            ("axis: wrong arity (zero)", "axis_x", vec![]),
            (
                "axis: wrong arity (two)",
                "axis_x",
                vec![triple(Value::length(0.0)), triple(Value::length(0.0))],
            ),
            ("axis: Undef argument (D10)", "axis_x", vec![Value::Undef]),
            (
                "axis: Undef component (D10)",
                "axis_x",
                vec![triple(Value::Undef)],
            ),
            ("axis: non-Point argument", "axis_y", vec![Value::Real(0.0)]),
            (
                "axis: Point of length 2",
                "axis_y",
                vec![Value::Point(vec![Value::Real(0.0), Value::Real(0.0)])],
            ),
            (
                "axis: non-numeric component",
                "axis_z",
                vec![triple(Value::Bool(true))],
            ),
            (
                "axis: NaN component",
                "axis_z",
                vec![triple(Value::Real(f64::NAN))],
            ),
            (
                "axis: infinite component",
                "axis_z",
                vec![triple(Value::Real(f64::INFINITY))],
            ),
            (
                "axis: MIXED dimensions",
                "axis_x",
                vec![Value::Point(vec![
                    Value::length(1.0),
                    mass.clone(),
                    Value::length(0.0),
                ])],
            ),
            (
                "axis: accepted LENGTH",
                "axis_x",
                vec![triple(Value::length(0.005))],
            ),
        ] {
            assert!(
                super::diagnose(name, &args).is_none(),
                "{label}: must stay silent; got: {:?}",
                super::diagnose(name, &args)
            );
        }
    }

    /// R12's AND R11's rejection wording must be the SHARED
    /// `reify_ir::arg_acceptance::ArgRejection::message` template, not a fork of it
    /// — PRD C1 invariant (i): "wording is produced only by `ArgRejection::message`
    /// — no hand-rolled rejection strings".
    ///
    /// ε (task 5746, R11) joins this test rather than forking a twin of it. Its two
    /// construction-datum arms answer to the same invariant from the same owner, and
    /// the `got`-shape loop below is already exhaustive for their argument positions
    /// too, so ONE table covering four builtins is one place to reword rather than
    /// two to keep in step.
    ///
    /// The reference string is built from the OWNER on every row: `accept_arg` is
    /// asked for the rejection that the SAME component value produces at a
    /// `length_spec()` position, and `ArgRejection::message` renders it. Nothing
    /// here restates the sentence shape, the type name or the migration hint, so a
    /// reword on the owner's side fails this test rather than silently forking the
    /// two crates.
    ///
    /// This test is reachable at all only because task 5791 relocated
    /// `arg_acceptance` from `reify-eval` (where it was `pub(crate)`, and
    /// unreachable from this crate — the edge runs reify-eval → reify-stdlib) into
    /// `reify-ir`, which this crate already depends on. `helpers.rs`'s
    /// `arg_acceptance_is_reachable_from_reify_stdlib` pins that import path and
    /// records that it adds no new crate edge; this is the same path, used for the
    /// wording rather than for the verdict.
    ///
    /// The rows below are EXHAUSTIVE over the `got` shapes that can reach an R11 or
    /// R12 rejection, which is what makes the enumeration executable rather than
    /// aspirational: `decompose_xyz3` requires `Value::as_f64` to succeed, so only
    /// `Real`, `Int` and `Scalar` ever arrive; a LENGTH `Scalar` is accepted rather
    /// than rejected, leaving the dimensionless and dimensioned `Scalar` forms. R11's
    /// plane offset is gated by `as_f64` directly, so the same four shapes exhaust it
    /// too.
    #[test]
    fn length_rejection_wording_is_the_shared_arg_rejection_template() {
        use reify_ir::arg_acceptance::{Acceptance, accept_arg, length_spec};

        let scalar = |v: f64, dimension| Value::Scalar {
            si_value: v,
            dimension,
        };

        // `decompose_xyz3` requires all three components to share ONE dimension
        // before this gate can fire, so each row carries its own matching zero.
        for (shape, offender, zero) in [
            ("bare Real", Value::Real(5.0), Value::Real(0.0)),
            // `Int` is a DISTINCT rendering, not a spelling of `Real`:
            // `value_short_label` prints "Int", so `affine_translate(5, 0, 0)`
            // written with integer literals produces a materially different
            // sentence — and an integer triple is at least as likely an authoring
            // mistake as a float one.
            ("bare Int", Value::Int(5), Value::Int(0)),
            (
                "dimensionless Scalar",
                scalar(5.0, DimensionVector::DIMENSIONLESS),
                scalar(0.0, DimensionVector::DIMENSIONLESS),
            ),
            (
                "MASS Scalar",
                scalar(5.0, DimensionVector::MASS),
                scalar(0.0, DimensionVector::MASS),
            ),
        ] {
            let Acceptance::Rejected(rejection) = accept_arg(&offender, &length_spec()) else {
                panic!("{shape}: the owner must REJECT this component at a length_spec position");
            };

            let triple = vec![offender.clone(), zero.clone(), zero];
            for (builtin, arg_name, args) in [
                ("affine_translate", "dx/dy/dz", triple.clone()),
                (
                    "affine_map",
                    "translation",
                    vec![matrix3x3(IDENTITY_3X3), Value::Vector(triple.clone())],
                ),
                ("plane_yz", "offset", vec![offender]),
                ("axis_x", "ox/oy/oz", vec![Value::Point(triple.clone())]),
            ] {
                let diag = super::diagnose(builtin, &args)
                    .unwrap_or_else(|| panic!("{builtin} / {shape}: must be diagnosed"));
                assert_eq!(
                    diag.message,
                    rejection.message(builtin, arg_name),
                    "{builtin} / {shape}: this wording has forked from the shared \
                     ArgRejection template"
                );
            }
        }
    }

    #[test]
    fn diagnose_non_affine_name_returns_none() {
        assert!(
            super::diagnose("box", &[Value::Real(0.0), Value::Real(1.0), Value::Real(1.0)])
                .is_none(),
            "a non-affine_scale name must not produce a diagnostic"
        );
    }

    // ── diagnose: transform_log dimension arm (RULING #6126) ──────────────────
    // The narrowed gate must not degrade to a SILENT Undef: when a Transform's
    // translation is not Vector3<Length>, the classifier names the offending
    // dimension. It stays silent (None) for every NON-dimension Undef cause, so an
    // unrelated failure is never mis-attributed to a dimension problem.

    /// Helper: a Transform with the given translation components.
    fn make_transform_with_translation(components: [Value; 3]) -> Value {
        Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(components.to_vec())),
        }
    }

    #[test]
    fn diagnose_transform_log_dimensionless_translation_names_dimension() {
        let t = make_transform_with_translation([
            Value::Real(1.0),
            Value::Real(2.0),
            Value::Real(3.0),
        ]);
        let diag = super::diagnose("transform_log", &[t])
            .expect("a dimensionless translation must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "a wrong dimension is a design-correctness fault, so RULING #6126 reports it as \
             an Error — NEVER a Warning — and both halves of the transform_log/transform_exp \
             family must report this fault class with the SAME exit code (#6080 plans \
             Error/exit-1 for the angular half). Leo's severity amendment, 2026-08-19, via \
             esc-6080-6."
        );
        for needle in ["transform_log", "Length", "dimensionless"] {
            assert!(
                diag.message.contains(needle),
                "message must contain {needle:?}, got: {}",
                diag.message
            );
        }
    }

    #[test]
    fn diagnose_transform_log_angle_translation_names_angle() {
        let t = make_transform_with_translation([
            Value::angle(1.0),
            Value::angle(2.0),
            Value::angle(3.0),
        ]);
        let diag = super::diagnose("transform_log", &[t])
            .expect("an ANGLE translation must produce a diagnostic");
        assert!(
            diag.message.contains("Angle"),
            "the message must NAME the offending dimension rather than hardcode one \
             string, got: {}",
            diag.message
        );
    }

    #[test]
    fn diagnose_transform_log_length_translation_returns_none() {
        let t = make_transform_with_translation([
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(
            super::diagnose("transform_log", &[t]).is_none(),
            "a valid Vector3<Length> translation must not produce a diagnostic"
        );
    }

    #[test]
    fn diagnose_transform_log_degenerate_quaternion_returns_none() {
        // LENGTH translation (so the dimension is fine) but a quaternion whose squared
        // norm is below the 1e-24 gate — eval returns Undef for a NON-dimension reason,
        // and the classifier must not mis-attribute it.
        let t = Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1e-13,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![Value::length(1.0); 3])),
        };
        assert!(
            eval_builtin("transform_log", std::slice::from_ref(&t)).is_undef(),
            "precondition: the degenerate quaternion must make eval return Undef"
        );
        assert!(
            super::diagnose("transform_log", &[t]).is_none(),
            "a degenerate-quaternion Undef must not be reported as a dimension problem"
        );
    }

    #[test]
    fn diagnose_transform_log_non_transform_arg_returns_none() {
        assert!(
            super::diagnose("transform_log", &[Value::Real(1.0)]).is_none(),
            "a non-Transform argument is a shape failure, not a dimension failure"
        );
    }

    // ── diagnose: transform_exp dimension arm (RULING #6126) ──────────────────
    // The mirror of the transform_log arm, on the twist's `linear` half.

    #[test]
    fn diagnose_transform_exp_dimensionless_linear_names_dimension() {
        let twist = make_twist(
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
        );
        let diag = super::diagnose("transform_exp", &[twist])
            .expect("a dimensionless linear must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "a wrong dimension is a design-correctness fault, so RULING #6126 reports it as \
             an Error — NEVER a Warning — and both halves of the transform_log/transform_exp \
             family must report this fault class with the SAME exit code (#6080 plans \
             Error/exit-1 for the angular half). Leo's severity amendment, 2026-08-19, via \
             esc-6080-6."
        );
        for needle in ["transform_exp", "linear", "Length", "dimensionless"] {
            assert!(
                diag.message.contains(needle),
                "message must contain {needle:?}, got: {}",
                diag.message
            );
        }
    }

    #[test]
    fn diagnose_transform_exp_mass_linear_names_mass() {
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], DimensionVector::MASS);
        let diag = super::diagnose("transform_exp", &[twist])
            .expect("a MASS linear must produce a diagnostic");
        assert!(
            diag.message.contains("Mass"),
            "a second distinct dimension proves the label is DERIVED, not hardcoded, \
             got: {}",
            diag.message
        );
    }

    /// An UNNAMED composite dimension must still print its actual exponents, not a
    /// generic label — the `dimension_label` fallback branch, which every other test
    /// here bypasses by using a NAMED dimension (Length/Angle/Mass) or DIMENSIONLESS.
    ///
    /// That fallback is the documented divergence from the two `reify-eval` siblings
    /// (`value_short_label` / `scalar_got_label`), which collapse this case to the
    /// uninformative word "dimensioned". `MONEY / MASS` is the codebase's canonical
    /// no-canonical-name example (see `canonical_name_composite_returns_none` in
    /// reify-core), and `Display` renders it "USD·kg^-1" — so a regression that
    /// re-collapsed the fallback to a generic string turns this red.
    #[test]
    fn diagnose_transform_exp_unnamed_composite_linear_prints_exponents() {
        let cost_per_mass = DimensionVector::MONEY.div(&DimensionVector::MASS);
        assert!(
            cost_per_mass.canonical_name().is_none(),
            "premise: MONEY/MASS has no canonical name, so the message must come from \
             the Display fallback"
        );
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], cost_per_mass);
        let diag = super::diagnose("transform_exp", &[twist])
            .expect("an unnamed composite linear dimension must still produce a diagnostic");
        for needle in ["USD", "kg^-1"] {
            assert!(
                diag.message.contains(needle),
                "an unnamed dimension must print its EXPONENTS (expected {needle:?}) \
                 rather than a generic label, got: {}",
                diag.message
            );
        }
    }

    #[test]
    fn diagnose_transform_exp_length_linear_returns_none() {
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], DimensionVector::LENGTH);
        assert!(
            super::diagnose("transform_exp", &[twist]).is_none(),
            "a valid Vector3<Length> linear must not produce a diagnostic"
        );
    }

    /// A twist wrong in BOTH halves must blame `angular`, never `linear`.
    ///
    /// The no-mis-attribution invariant is unchanged and still worth asserting; only
    /// the way it is upheld changed. Eval gates `angular` before `linear`, so this
    /// twist is rejected by the angular gate and the linear gate is never reached.
    /// #6126 upheld that by SILENCE, because it did not own the angular gate; #6080
    /// does, so the arm now upholds it by ORDER — it names the half eval actually
    /// rejected. Blaming `linear` would still send the user to fix a half that is not
    /// what stopped them.
    #[test]
    fn diagnose_transform_exp_bad_angular_and_bad_linear_blames_angular() {
        let twist = make_twist_with_dims(
            [1.0, 0.0, 0.0],
            DimensionVector::LENGTH,
            [1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
        );
        assert!(
            eval_builtin("transform_exp", std::slice::from_ref(&twist)).is_undef(),
            "premise: eval rejects this twist (at the ANGULAR gate, before linear)"
        );
        let diag = super::diagnose("transform_exp", &[twist]).expect(
            "eval rejected this twist at the ANGULAR gate, and #6080 owns that gate, so \
             the arm must explain it",
        );
        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert!(
            diag.message.contains("angular"),
            "the message must blame the half eval actually rejected; got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("linear"),
            "the linear gate was never reached, so naming it would mis-attribute the \
             failure; got: {}",
            diag.message
        );
    }

    /// The second act of the old mis-attribution, now closed rather than merely
    /// avoided: under #6126 the user acted on a `linear` message, made `linear` a
    /// Length, was STILL Undef on the angular gate — and got NO diagnostic at all,
    /// because the arm went silent. It now speaks about the angular half both times.
    #[test]
    fn diagnose_transform_exp_bad_angular_with_length_linear_blames_angular() {
        let twist = make_twist_with_dims(
            [1.0, 0.0, 0.0],
            DimensionVector::LENGTH,
            [1.0, 2.0, 3.0],
            DimensionVector::LENGTH,
        );
        assert!(
            eval_builtin("transform_exp", std::slice::from_ref(&twist)).is_undef(),
            "premise: a non-ANGLE angular half is Undef even with a Length linear"
        );
        let diag = super::diagnose("transform_exp", &[twist])
            .expect("the angular half is the only thing wrong, so the arm must say so");
        assert!(
            diag.message.contains("angular"),
            "the message must name the angular half; got: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("linear"),
            "the linear half is VALID here, so naming it would be actively wrong; \
             got: {}",
            diag.message
        );
    }

    /// The deferral must track eval's angular gate WHEREVER #6080 moves it.
    ///
    /// Every other test here builds a DIMENSIONLESS angular half by literal, so all of
    /// them would keep passing if the gate widened (say, to ANGLE) while this
    /// classifier kept requiring DIMENSIONLESS — and the regression is silent: the arm
    /// would simply stop emitting the #6126 linear Warning for every twist whose
    /// angular half is newly-valid. This test builds its angular half FROM
    /// `TWIST_ANGULAR_DIM`, the const both sites now read, so it follows the gate and
    /// goes red the moment only one of the two sites moves.
    #[test]
    fn diagnose_transform_exp_deferral_tracks_evals_angular_gate() {
        let twist = make_twist_with_dims(
            [0.0, 0.0, 0.0],
            super::TWIST_ANGULAR_DIM,
            [1.0, 2.0, 3.0],
            DimensionVector::MASS,
        );
        assert!(
            eval_builtin("transform_exp", std::slice::from_ref(&twist)).is_undef(),
            "premise: an angular half eval ADMITS plus a non-Length linear is rejected \
             by the LINEAR gate — the one this arm speaks for"
        );
        let diag = super::diagnose("transform_exp", &[twist]).expect(
            "the linear gate owns this failure, so the arm must speak; if this panics, \
             the classifier's angular deferral has drifted from eval's angular gate",
        );
        assert!(
            diag.message.contains("linear"),
            "the surviving message must still blame `linear`, got: {}",
            diag.message
        );
    }

    // ── diagnose: transform_exp ANGULAR arm (#6080) ──────────────────────────
    //
    // `Twist.angular` is a rotation vector, so it carries ANGLE. Narrowing that gate is
    // a breaking change to a published stdlib signature, so the wrong-dimension path
    // must surface a `Severity::Error` naming `angular` and the offending dimension
    // instead of a silent `Undef`. The message is built by the SAME constructor
    // `orientation::diagnose`'s `orient_exp` arm uses, so one fault class reports one
    // way across the whole builtin family.
    //
    // SCOPE: these assert the ANGULAR half only. The `linear` arm above is #6126's and
    // is unchanged; `diagnose_transform_exp_angle_angular_with_mass_linear_blames_linear`
    // is the guard that this arm did not swallow it.

    /// Build a twist `Value::Map` from raw `angular` and `linear` values.
    ///
    /// Distinct from `make_twist_with_dims`, which builds well-shaped 3-vectors from
    /// f64 arrays: the shape-error tests below need to hand in a 2-component vector or
    /// a non-Vector, which that helper cannot express.
    fn twist_map(angular: Value, linear: Value) -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(Value::String("angular".to_string()), angular);
        m.insert(Value::String("linear".to_string()), linear);
        Value::Map(m)
    }

    /// The newly-rejected spelling: a bare (DIMENSIONLESS) angular half.
    #[test]
    fn diagnose_transform_exp_dimensionless_angular_errors() {
        let tw = twist_map(
            Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(std::f64::consts::FRAC_PI_2),
            ]),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        let diag = super::diagnose("transform_exp", &[tw])
            .expect("a dimensionless angular half must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "a wrong dimension is a design-correctness fault, so `reify eval` must EXIT \
             1 — and the sibling `linear` half of the same builtin reports this fault \
             class at the same severity (Leo's amendment, 2026-08-19, via esc-6080-6)"
        );
        assert!(
            diag.message.contains("angular"),
            "message must name the offending Twist field; got: {}",
            diag.message
        );
        assert!(
            diag.message
                .contains(&DimensionVector::DIMENSIONLESS.to_string()),
            "message must render the offending dimension ({}); got: {}",
            DimensionVector::DIMENSIONLESS,
            diag.message
        );
    }

    /// A LENGTH angular half was never accepted, but used to fail silently. A second
    /// distinct dimension also proves the label is DERIVED rather than hardcoded.
    #[test]
    fn diagnose_transform_exp_length_angular_errors() {
        let tw = twist_map(
            Value::Vector(vec![Value::length(0.001); 3]),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        let diag = super::diagnose("transform_exp", &[tw])
            .expect("a LENGTH angular half must produce a diagnostic");
        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert!(
            diag.message.contains(&DimensionVector::LENGTH.to_string()),
            "message must render the offending dimension ({}); got: {}",
            DimensionVector::LENGTH,
            diag.message
        );
    }

    /// A valid ANGLE angular half is not diagnosed by this arm, even when asked
    /// directly.
    #[test]
    fn diagnose_transform_exp_angle_angular_returns_none() {
        let tw = twist_map(
            Value::Vector(vec![
                Value::angle(0.0),
                Value::angle(0.0),
                Value::angle(std::f64::consts::FRAC_PI_2),
            ]),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(
            super::diagnose("transform_exp", &[tw]).is_none(),
            "a valid ANGLE angular half must not produce a diagnostic"
        );
    }

    /// Scope guard: a VALID angular half with a wrong `linear` must still yield #6126's
    /// LINEAR message. The angular arm runs first, so it is the arm that could swallow
    /// the linear one; this pins that it does not.
    #[test]
    fn diagnose_transform_exp_angle_angular_with_mass_linear_blames_linear() {
        let twist = make_twist_with_dims(
            [0.0, 0.0, 0.0],
            DimensionVector::ANGLE,
            [1.0, 2.0, 3.0],
            DimensionVector::MASS,
        );
        let diag = super::diagnose("transform_exp", &[twist])
            .expect("the linear gate owns this failure, so the arm must still speak");
        assert!(
            diag.message.contains("linear"),
            "#6126's linear message must survive the angular arm being added ahead of \
             it; got: {}",
            diag.message
        );
    }

    /// Shape errors stay silent, so a SHAPE failure is never mis-attributed as a
    /// dimension failure — the restraint the whole `diagnose` family documents.
    #[test]
    fn diagnose_transform_exp_shape_errors_return_none() {
        assert!(
            super::diagnose("transform_exp", &[]).is_none(),
            "wrong arity must stay silent"
        );
        assert!(
            super::diagnose("transform_exp", &[Value::Real(1.0)]).is_none(),
            "a non-Map argument must stay silent"
        );
        let mut only_linear = std::collections::BTreeMap::new();
        only_linear.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(
            super::diagnose("transform_exp", &[Value::Map(only_linear)]).is_none(),
            "a missing `angular` key must stay silent"
        );
        let tw = twist_map(
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0)]),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(
            super::diagnose("transform_exp", &[tw]).is_none(),
            "a non-3d angular half must stay silent"
        );
    }

    /// The name families stay disjoint: this arm declines the emitting sibling
    /// `transform_log` and the orientation family's `orient_exp`.
    #[test]
    fn diagnose_non_transform_exp_name_returns_none() {
        let tw = twist_map(
            Value::Vector(vec![Value::Real(0.0); 3]),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(
            super::diagnose("transform_log", std::slice::from_ref(&tw)).is_none(),
            "transform_log is an emitter, not a rotation-vector gate"
        );
        assert!(
            super::diagnose("orient_exp", &[Value::Vector(vec![Value::Real(0.0); 3])]).is_none(),
            "orient_exp belongs to orientation::diagnose, not geometry::diagnose"
        );
    }

    /// The angular half must be a VALID ANGLE vector (#6080), or this test would take
    /// the angular arm and stop covering the missing-key SHAPE path it is named for.
    #[test]
    fn diagnose_transform_exp_missing_linear_key_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]),
        );
        assert!(
            super::diagnose("transform_exp", &[Value::Map(m)]).is_none(),
            "a missing `linear` key is a SHAPE failure, not a dimension failure"
        );
    }

    /// The angular half is now read first (to defer to eval's angular gate), so its
    /// shape failures must stay silent for the same reason the linear half's do.
    #[test]
    fn diagnose_transform_exp_missing_angular_key_returns_none() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::Real(1.0); 3]),
        );
        assert!(
            super::diagnose("transform_exp", &[Value::Map(m)]).is_none(),
            "a missing `angular` key is a SHAPE failure, not a dimension failure — even \
             though the `linear` half present here IS non-Length"
        );
    }

    #[test]
    fn diagnose_transform_exp_non_map_arg_returns_none() {
        assert!(
            super::diagnose("transform_exp", &[Value::Real(1.0)]).is_none(),
            "a non-Map argument is a shape failure, not a dimension failure"
        );
    }

    #[test]
    fn diagnose_transform_exp_wrong_arity_returns_none() {
        let twist = make_twist(
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
        );
        assert!(
            super::diagnose("transform_exp", &[twist.clone(), twist]).is_none(),
            "wrong arity stays silent, like the affine_scale / transform3 convention"
        );
        assert!(
            super::diagnose("transform_exp", &[]).is_none(),
            "zero args stays silent"
        );
    }

    #[test]
    fn diagnose_transform_log_wrong_arity_returns_none() {
        let t = make_transform_with_translation([
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
        ]);
        assert!(
            super::diagnose("transform_log", &[t.clone(), t]).is_none(),
            "wrong arity stays silent, like the affine_scale / transform3 convention"
        );
        assert!(
            super::diagnose("transform_log", &[]).is_none(),
            "zero args stays silent"
        );
    }

    // ── bbox dimension-rejection diagnostics (task 6081) ──────────────────────
    // `bbox` with a non-Length corner returns a bare Value::Undef; this
    // classifier is what turns that silence into a Severity::Error naming the
    // builtin, the offending corner and the offending dimension.

    fn make_point3_dim(dimension: DimensionVector) -> Value {
        Value::Point(
            [0.0, 1.0, 2.0]
                .into_iter()
                .map(|si_value| Value::Scalar {
                    si_value,
                    dimension,
                })
                .collect(),
        )
    }

    #[test]
    fn diagnose_bbox_angle_corners_errors_naming_angle() {
        let min = make_point3_angle(0.0, 0.0, 0.0);
        let max = make_point3_angle(1.0, 2.0, 3.0);
        let diag = super::diagnose("bbox", &[min, max])
            .expect("Angle-cornered bbox must produce a diagnostic");
        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "a bbox dimension rejection is a construction failure, not a drop-and-continue"
        );
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "must carry the canonical runtime dimension-rejection code"
        );
        assert!(
            diag.message.contains("bbox"),
            "message must name the builtin, got: {}",
            diag.message
        );
        let angle_name = DimensionVector::ANGLE
            .canonical_name()
            .expect("ANGLE is a named dimension");
        assert!(
            diag.message.contains(angle_name),
            "message must name the offending dimension {angle_name:?}, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("Length"),
            "message must name the expected Length quantity, got: {}",
            diag.message
        );
    }

    #[test]
    fn diagnose_bbox_mass_corner_errors_naming_mass() {
        // Length min + Mass max: the max corner is the offender.
        let min = make_point3_min();
        let max = make_point3_dim(DimensionVector::MASS);
        let diag = super::diagnose("bbox", &[min, max])
            .expect("Mass-cornered bbox must produce a diagnostic");
        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected)
        );
        let mass_name = DimensionVector::MASS
            .canonical_name()
            .expect("MASS is a named dimension");
        assert!(
            diag.message.contains(mass_name),
            "message must name the offending dimension {mass_name:?}, got: {}",
            diag.message
        );
    }

    #[test]
    fn diagnose_bbox_length_corners_returns_none() {
        assert!(
            super::diagnose("bbox", &[make_point3_min(), make_point3_max()]).is_none(),
            "a valid metre-valued bbox must not produce a diagnostic"
        );
    }

    #[test]
    fn diagnose_bbox_wrong_arity_returns_none() {
        // Arity failures stay silent, matching the affine_scale/transform3
        // convention: only the user-correctable dimension cause is explained.
        assert!(super::diagnose("bbox", &[]).is_none());
        assert!(super::diagnose("bbox", &[make_point3_angle(0.0, 0.0, 0.0)]).is_none());
        assert!(
            super::diagnose(
                "bbox",
                &[
                    make_point3_angle(0.0, 0.0, 0.0),
                    make_point3_angle(1.0, 2.0, 3.0),
                    make_point3_angle(4.0, 5.0, 6.0),
                ]
            )
            .is_none()
        );
    }

    #[test]
    fn diagnose_bbox_non_point_args_return_none() {
        // Type failures stay silent too — only the dimension cause is explained.
        assert!(super::diagnose("bbox", &[Value::Real(1.0), Value::Real(2.0)]).is_none());
    }

    /// A mixed-component corner is a QUANTITY failure, so it is explained rather
    /// than left as a silent Undef — the same fault class as a uniformly-Angle
    /// corner, just without one dimension to name.
    #[test]
    fn diagnose_bbox_mixed_dimension_corner_errors_naming_the_corner() {
        let min = Value::Point(vec![
            Value::length(1.0),
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::ANGLE,
            },
            Value::length(3.0),
        ]);
        let diag = super::diagnose("bbox", &[min, make_point3_max()])
            .expect("a mixed-dimension corner must produce a diagnostic, not a silent Undef");
        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected)
        );
        assert!(
            diag.message.contains("min argument expects Point3<Length>"),
            "message must name the offending corner and the expected quantity, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("mixed component dimensions"),
            "message must say WHY, without inventing a single quantity to blame, got: {}",
            diag.message
        );
    }

    /// A wrong-SHAPE corner must stay silent rather than be mislabelled as a
    /// dimension failure.
    ///
    /// Before the shared decoder this reported `bbox: min argument expects
    /// Point3<Length>, got Point3<Angle>` for a `Point2` argument — naming a
    /// Point3 that does not exist, and blaming the dimension for what is really
    /// an arity-of-components fault.
    #[test]
    fn diagnose_bbox_point2_corner_returns_none() {
        let min = Value::Point(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::ANGLE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::ANGLE,
            },
        ]);
        assert!(
            super::diagnose("bbox", &[min, make_point3_min()]).is_none(),
            "a 2-component corner is a shape failure; a shape failure is not a dimension failure"
        );
    }

    /// A non-numeric component is a shape failure too, and stays silent — it wins
    /// the tie against the mixed-dimension reading it would otherwise produce.
    #[test]
    fn diagnose_bbox_non_numeric_corner_component_returns_none() {
        let min = Value::Point(vec![
            Value::length(1.0),
            Value::Bool(true),
            Value::length(3.0),
        ]);
        assert!(super::diagnose("bbox", &[min, make_point3_max()]).is_none());
    }

    #[test]
    fn diagnose_affine_scale_behaviour_survives_the_bbox_arm() {
        // Guards the restructure of `diagnose`'s early-return guard into a
        // `match name`: the affine_scale arm must be preserved verbatim.
        let dimensioned = super::diagnose(
            "affine_scale",
            &[Value::length(2.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("dimensioned scale factor must still produce a diagnostic");
        assert_eq!(dimensioned.severity, reify_core::Severity::Warning);
        assert!(dimensioned.message.contains("dimensionless"));

        let zero = super::diagnose(
            "affine_scale",
            &[Value::Real(0.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("zero scale factor must still produce a diagnostic");
        assert_eq!(zero.severity, reify_core::Severity::Warning);
        assert!(zero.message.contains("degenerate"));

        assert!(
            super::diagnose(
                "affine_scale",
                &[Value::Real(2.0), Value::Real(1.0), Value::Real(0.5)],
            )
            .is_none()
        );
        // Wrong arity for affine_scale still stays silent.
        assert!(super::diagnose("affine_scale", &[Value::Real(2.0)]).is_none());
    }

    // ── affine_compose tests (step-3 RED / step-4 GREEN) ──────────────────────

    /// Build a `Value::AffineMap` directly for test purposes.
    fn make_test_affine(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Value {
        Value::AffineMap {
            linear,
            translation,
        }
    }

    #[test]
    fn affine_compose_right_identity() {
        // compose(a, identity) == a
        let a = eval_builtin("affine_scale", &[Value::Real(2.0), Value::Real(3.0), Value::Real(4.0)]);
        let id = eval_builtin("affine_identity", &[]);
        let result = eval_builtin("affine_compose", &[a.clone(), id]);
        let (a_linear, a_trans) = expect_affine(a);
        let (r_linear, r_trans) = expect_affine(result);
        for i in 0..3 {
            for j in 0..3 {
                assert!((r_linear[i][j] - a_linear[i][j]).abs() < 1e-12,
                    "compose(a, id) linear[{i}][{j}]: expected {}, got {}", a_linear[i][j], r_linear[i][j]);
            }
        }
        for k in 0..3 {
            assert!((r_trans[k] - a_trans[k]).abs() < 1e-12,
                "compose(a, id) translation[{k}]: expected {}, got {}", a_trans[k], r_trans[k]);
        }
    }

    #[test]
    fn affine_compose_left_identity() {
        // compose(identity, a) == a
        let a = eval_builtin("affine_scale", &[Value::Real(2.0), Value::Real(3.0), Value::Real(4.0)]);
        let id = eval_builtin("affine_identity", &[]);
        let result = eval_builtin("affine_compose", &[id, a.clone()]);
        let (a_linear, a_trans) = expect_affine(a);
        let (r_linear, r_trans) = expect_affine(result);
        for i in 0..3 {
            for j in 0..3 {
                assert!((r_linear[i][j] - a_linear[i][j]).abs() < 1e-12,
                    "compose(id, a) linear[{i}][{j}]");
            }
        }
        for k in 0..3 {
            assert!((r_trans[k] - a_trans[k]).abs() < 1e-12);
        }
    }

    #[test]
    fn affine_compose_scale_then_shear() {
        // compose(scale(2,1,1), shear_xy(1)) → linear [[2,2,0],[0,1,0],[0,0,1]]
        // a.linear = diag(2,1,1), b.linear = I + shear[0][1]=1
        // result.linear[0] = a.linear[0] · b.linear = [2*1, 2*1, 2*0] = [2, 2, 0]
        let scale = eval_builtin("affine_scale", &[Value::Real(2.0), Value::Real(1.0), Value::Real(1.0)]);
        let shear = eval_builtin("affine_shear_xy", &[Value::Real(1.0)]);
        let (r_linear, r_trans) = expect_affine(eval_builtin("affine_compose", &[scale, shear]));
        let expected_linear = [[2.0, 2.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for i in 0..3 {
            for j in 0..3 {
                assert!((r_linear[i][j] - expected_linear[i][j]).abs() < 1e-12,
                    "linear[{i}][{j}]: expected {}, got {}", expected_linear[i][j], r_linear[i][j]);
            }
        }
        assert_eq!(r_trans, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn affine_compose_translation_formula() {
        // compose(scale(2,2,2), translate(1m, 0, 0)):
        //   a = scale(2,2,2): linear=diag(2,2,2), trans=[0,0,0]
        //   b = translate(1m,0,0): linear=I, trans=[1,0,0]
        //   result.linear = a.linear · b.linear = diag(2,2,2)
        //   result.trans = a.linear · b.trans + a.trans = [2*1,2*0,2*0] + [0,0,0] = [2,0,0]
        let scale = eval_builtin("affine_scale", &[Value::Real(2.0), Value::Real(2.0), Value::Real(2.0)]);
        let translate = eval_builtin("affine_translate", &[Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH }, Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH }, Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH }]);
        let (r_linear, r_trans) = expect_affine(eval_builtin("affine_compose", &[scale, translate]));
        // linear should be diag(2,2,2)
        let expected_linear = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        for i in 0..3 {
            for j in 0..3 {
                assert!((r_linear[i][j] - expected_linear[i][j]).abs() < 1e-12,
                    "linear[{i}][{j}]");
            }
        }
        // translation = a.linear · b.trans + a.trans = [2,0,0] + [0,0,0] = [2,0,0]
        assert!((r_trans[0] - 2.0).abs() < 1e-12, "trans[0]: expected 2.0, got {}", r_trans[0]);
        assert!(r_trans[1].abs() < 1e-12);
        assert!(r_trans[2].abs() < 1e-12);
    }

    #[test]
    fn affine_compose_wrong_arity_returns_undef() {
        let a = eval_builtin("affine_identity", &[]);
        assert!(eval_builtin("affine_compose", &[]).is_undef(), "0 args");
        assert!(eval_builtin("affine_compose", std::slice::from_ref(&a)).is_undef(), "1 arg");
        assert!(eval_builtin("affine_compose", &[a.clone(), a.clone(), a.clone()]).is_undef(), "3 args");
    }

    #[test]
    fn affine_compose_non_affine_args_return_undef() {
        let a = eval_builtin("affine_identity", &[]);
        // first arg is not AffineMap
        assert!(eval_builtin("affine_compose", &[Value::Real(1.0), a.clone()]).is_undef());
        // second arg is not AffineMap
        assert!(eval_builtin("affine_compose", &[a, Value::Real(1.0)]).is_undef());
    }

    // ── affine_inverse tests (step-5 RED / step-6 GREEN) ──────────────────────

    #[test]
    fn affine_inverse_invertible_map_returns_option_some() {
        // Well-conditioned map: linear=[[2,0,0],[0,3,0],[1,0,4]], det=24, + translation.
        let a = make_test_affine(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [1.0, 0.0, 4.0]],
            [0.1, 0.2, 0.3],
        );
        let result = eval_builtin("affine_inverse", &[a]);
        match result {
            Value::Option(Some(inner)) => {
                // Just verify it is AffineMap — detailed numeric check below.
                assert!(
                    matches!(*inner, Value::AffineMap { .. }),
                    "affine_inverse should return Some(AffineMap), got {:?}", inner
                );
            }
            other => panic!("expected Value::Option(Some(AffineMap)), got {:?}", other),
        }
    }

    #[test]
    fn affine_inverse_round_trip_approx_identity() {
        // compose(a, inverse(a)) ≈ affine_identity() within 1e-12.
        let a = make_test_affine(
            [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [1.0, 0.0, 4.0]],
            [0.1, 0.2, 0.3],
        );
        let inv_result = eval_builtin("affine_inverse", std::slice::from_ref(&a));
        let inv = match inv_result {
            Value::Option(Some(inner)) => *inner,
            other => panic!("expected Option(Some(AffineMap)), got {:?}", other),
        };
        let composed = eval_builtin("affine_compose", &[a, inv]);
        let (composed_linear, composed_trans) = expect_affine(composed);
        for (i, row) in composed_linear.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (val - expected).abs() < 1e-12,
                    "round-trip linear[{i}][{j}]: expected {expected}, got {val}",
                );
            }
        }
        for (k, &val) in composed_trans.iter().enumerate() {
            assert!(
                val.abs() < 1e-12,
                "round-trip translation[{k}]: expected 0, got {val}",
            );
        }
    }

    #[test]
    fn affine_inverse_singular_returns_option_none() {
        // A zero row ⇒ det=0 ⇒ affine_inverse returns Value::Option(None).
        let singular = make_test_affine(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0],
        );
        let result = eval_builtin("affine_inverse", &[singular]);
        assert!(
            matches!(result, Value::Option(None)),
            "singular affine_inverse must return Value::Option(None), got {:?}",
            result
        );
    }

    #[test]
    fn affine_inverse_wrong_arity_returns_undef() {
        assert!(eval_builtin("affine_inverse", &[]).is_undef(), "0 args");
        let a = eval_builtin("affine_identity", &[]);
        let b = eval_builtin("affine_identity", &[]);
        assert!(eval_builtin("affine_inverse", &[a, b]).is_undef(), "2 args");
    }

    #[test]
    fn affine_inverse_non_affine_arg_returns_undef() {
        assert!(eval_builtin("affine_inverse", &[Value::Real(1.0)]).is_undef());
    }

    // ── δ step-3: inverse round-trip on non-diagonal composed map ─────────────
    // Richer than γ's diagonal example (4973): uses a = compose(scale(2,1,1),
    // shear_xy(1)) with linear [[2,2,0],[0,1,0],[0,0,1]], det=2 (invertible).
    // Checks both orders of round-trip AND a point-level round-trip.

    #[test]
    fn affine_inverse_round_trip_composed_nondiagoal_both_orders() {
        // a = compose(scale(2,1,1), shear_xy(1)): det=2, non-diagonal, invertible.
        let scale = eval_builtin(
            "affine_scale",
            &[Value::Real(2.0), Value::Real(1.0), Value::Real(1.0)],
        );
        let shear = eval_builtin("affine_shear_xy", &[Value::Real(1.0)]);
        let a = eval_builtin("affine_compose", &[scale, shear]);

        let inv_result = eval_builtin("affine_inverse", std::slice::from_ref(&a));
        let inv = match inv_result {
            Value::Option(Some(inner)) => *inner,
            other => panic!("expected Option(Some(AffineMap)), got {:?}", other),
        };

        // (1) compose(a, inv) ≈ identity
        let (fwd_linear, fwd_trans) = expect_affine(eval_builtin(
            "affine_compose",
            &[a.clone(), inv.clone()],
        ));
        assert_matrix_approx(fwd_linear, IDENTITY_3X3, 1e-12);
        for (k, &v) in fwd_trans.iter().enumerate() {
            assert!(
                v.abs() < 1e-12,
                "compose(a,inv) translation[{k}] = {v}, expected 0"
            );
        }

        // (2) compose(inv, a) ≈ identity  [two-sided inverse]
        let (bwd_linear, bwd_trans) = expect_affine(eval_builtin(
            "affine_compose",
            &[inv.clone(), a.clone()],
        ));
        assert_matrix_approx(bwd_linear, IDENTITY_3X3, 1e-12);
        for (k, &v) in bwd_trans.iter().enumerate() {
            assert!(
                v.abs() < 1e-12,
                "compose(inv,a) translation[{k}] = {v}, expected 0"
            );
        }

        // (3) point-level round-trip: apply_affine_to_point(compose(a,inv), p) ≈ p
        let p = [1.0, 1.0, 0.0];
        let round_trip = apply_affine_to_point(fwd_linear, fwd_trans, p);
        for (i, (&got, &expected)) in round_trip.iter().zip(p.iter()).enumerate() {
            assert!(
                (got - expected).abs() < 1e-12,
                "point round-trip[{i}]: expected {expected}, got {got}"
            );
        }
    }

    // ── δ step-2: transform-widening structural-invariant pin ─────────────────
    // Goes beyond γ's bare matrix-equality by asserting det=+1 (orientation-
    // preserving) and orthonormality (linear·linearᵀ=I) on the widened rotation.

    #[test]
    fn affine_from_transform_z90_det_is_one() {
        // 90°-Z rotation widened to AffineMap must have det(linear) = +1.
        let q = Value::Orientation {
            w: std::f64::consts::FRAC_1_SQRT_2,
            x: 0.0,
            y: 0.0,
            z: std::f64::consts::FRAC_1_SQRT_2,
        };
        let trans = Value::Vector(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let t = eval_builtin("transform3", &[q, trans]);
        let w = eval_builtin("affine_from_transform", &[t]);
        // Verify matrix and translation as in γ (reuse assert_matrix_approx)
        let (linear, translation) = expect_affine(w.clone());
        assert_matrix_approx(
            linear,
            [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            1e-12,
        );
        assert!((translation[0] - 0.005).abs() < 1e-12, "tx");
        assert!(translation[1].abs() < 1e-12, "ty");
        assert!(translation[2].abs() < 1e-12, "tz");
        // Now pin det=+1 via the determinant builtin's AffineMap arm.
        let det_val = eval_builtin("determinant", std::slice::from_ref(&w));
        match det_val {
            Value::Real(d) => {
                assert!(
                    (d - 1.0).abs() < 1e-12,
                    "widened rotation det must be +1, got {}",
                    d
                );
            }
            other => panic!("determinant must return Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn affine_from_transform_z90_is_orthonormal() {
        // linear·linearᵀ must equal I₃ within 1e-12 (pins "proper rotation").
        let q = Value::Orientation {
            w: std::f64::consts::FRAC_1_SQRT_2,
            x: 0.0,
            y: 0.0,
            z: std::f64::consts::FRAC_1_SQRT_2,
        };
        let trans = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let t = eval_builtin("transform3", &[q, trans]);
        let (linear, _) = expect_affine(eval_builtin("affine_from_transform", &[t]));
        // Compute linear · linearᵀ in the test.
        let mut product = [[0.0f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                product[i][j] = linear[i]
                    .iter()
                    .zip(linear[j].iter())
                    .map(|(&a, &b)| a * b)
                    .sum::<f64>(); // linearᵀ[k][j] = linear[j][k]
            }
        }
        assert_matrix_approx(product, IDENTITY_3X3, 1e-12);
    }

    // ── δ step-1: composition-order point-application pin ─────────────────────
    // Pins the left-applied a∘b convention: (a∘b)(p) = a(b(p)).
    // Uses linear·p + translation computed in-test (affine_apply is owned by
    // downstream ζ/3963 and absent on main).

    /// Apply an AffineMap to a point: result[i] = sum_j(linear[i][j]*p[j]) + translation[i].
    fn apply_affine_to_point(
        linear: [[f64; 3]; 3],
        translation: [f64; 3],
        p: [f64; 3],
    ) -> [f64; 3] {
        let mut result = [0.0f64; 3];
        for i in 0..3 {
            result[i] = linear[i][0] * p[0]
                + linear[i][1] * p[1]
                + linear[i][2] * p[2]
                + translation[i];
        }
        result
    }

    #[test]
    fn affine_compose_order_ab_point_application() {
        // a = scale(2,1,1): linear=diag(2,1,1)
        // b = shear_xy(1):  linear=[[1,1,0],[0,1,0],[0,0,1]]
        // ab = compose(a, b): a.linear · b.linear = [[2,2,0],[0,1,0],[0,0,1]]
        // ab applied to (1,1,0) = (2*1+2*1, 1, 0) = (4, 1, 0)  [pins left-applied a∘b]
        let scale = eval_builtin(
            "affine_scale",
            &[Value::Real(2.0), Value::Real(1.0), Value::Real(1.0)],
        );
        let shear = eval_builtin("affine_shear_xy", &[Value::Real(1.0)]);
        let ab = eval_builtin("affine_compose", &[scale, shear]);
        let (ab_linear, ab_trans) = expect_affine(ab);

        let p = [1.0, 1.0, 0.0];
        let result = apply_affine_to_point(ab_linear, ab_trans, p);

        assert!(
            (result[0] - 4.0).abs() < 1e-12,
            "ab(p).x: expected 4.0, got {}",
            result[0]
        );
        assert!(
            (result[1] - 1.0).abs() < 1e-12,
            "ab(p).y: expected 1.0, got {}",
            result[1]
        );
        assert!(
            result[2].abs() < 1e-12,
            "ab(p).z: expected 0.0, got {}",
            result[2]
        );
    }

    #[test]
    fn affine_compose_order_decomposition_a_of_b_of_p() {
        // Verify the decomposition: (a∘b)(p) = a(b(p)).
        // b(p=(1,1,0)) with shear_xy(1): x+y=2, y=1, z=0 → (2,1,0)
        // a((2,1,0)) with scale(2,1,1): 2*2=4, 1, 0 → (4,1,0)
        let scale = eval_builtin(
            "affine_scale",
            &[Value::Real(2.0), Value::Real(1.0), Value::Real(1.0)],
        );
        let shear = eval_builtin("affine_shear_xy", &[Value::Real(1.0)]);
        let (scale_linear, scale_trans) = expect_affine(scale.clone());
        let (shear_linear, shear_trans) = expect_affine(shear.clone());

        let p = [1.0, 1.0, 0.0];
        let bp = apply_affine_to_point(shear_linear, shear_trans, p);
        assert!(
            (bp[0] - 2.0).abs() < 1e-12,
            "b(p).x: expected 2.0, got {}",
            bp[0]
        );
        assert!(
            (bp[1] - 1.0).abs() < 1e-12,
            "b(p).y: expected 1.0, got {}",
            bp[1]
        );

        let abp = apply_affine_to_point(scale_linear, scale_trans, bp);
        assert!(
            (abp[0] - 4.0).abs() < 1e-12,
            "a(b(p)).x: expected 4.0, got {}",
            abp[0]
        );
        assert!(
            (abp[1] - 1.0).abs() < 1e-12,
            "a(b(p)).y: expected 1.0, got {}",
            abp[1]
        );
        assert!(
            abp[2].abs() < 1e-12,
            "a(b(p)).z: expected 0.0, got {}",
            abp[2]
        );
    }

    #[test]
    fn affine_compose_order_is_load_bearing() {
        // The OPPOSITE order ba = compose(b, a) must give a DIFFERENT result at (1,1,0).
        // ba.linear = b.linear · a.linear = [[2,1,0],[0,1,0],[0,0,1]]
        // ba(1,1,0) = (2*1+1*1, 1, 0) = (3, 1, 0) ≠ (4, 1, 0)
        // This proves the left-applied a∘b convention is truly load-bearing.
        let scale = eval_builtin(
            "affine_scale",
            &[Value::Real(2.0), Value::Real(1.0), Value::Real(1.0)],
        );
        let shear = eval_builtin("affine_shear_xy", &[Value::Real(1.0)]);
        let ba = eval_builtin("affine_compose", &[shear, scale]);
        let (ba_linear, ba_trans) = expect_affine(ba);

        let p = [1.0, 1.0, 0.0];
        let result = apply_affine_to_point(ba_linear, ba_trans, p);

        assert!(
            (result[0] - 3.0).abs() < 1e-12,
            "ba(p).x: expected 3.0, got {}",
            result[0]
        );
        assert!(
            (result[1] - 1.0).abs() < 1e-12,
            "ba(p).y: expected 1.0, got {}",
            result[1]
        );
        // Prove the two orders diverge: ba(p).x=3 ≠ ab(p).x=4
        assert!(
            (result[0] - 4.0).abs() > 0.5,
            "compose(b,a)(p).x must differ from compose(a,b)(p).x=4.0; got {}",
            result[0]
        );
    }

    // ── BINDING ruling A7: the three DIMENSION arms converge on ──────────────
    // ── DiagnosticCode::DimensionedArgRejected (task 5791) ───────────────────

    /// The two RULING #6126 arms (`transform_log`, `transform_exp`) carry the
    /// canonical runtime dimension-rejection code.
    ///
    /// Under BINDING ruling A7 (Leo, 2026-08-30, esc-5791-3) there is nothing
    /// to mint: `DiagnosticCode::ArgDimensionMismatch` — which the deferral
    /// comment on `diagnose` used to promise — is deliberately NOT created, and
    /// the shipped `DimensionedArgRejected` is reused instead, one code per
    /// rejection REASON. See PRD
    /// `docs/prds/v0_6/dimension-checked-readers.md` §6 decision 1's
    /// RECONCILIATION block (landed b3ba3228f5).
    ///
    /// Input shapes are reused verbatim from the neighbouring shipped tests
    /// (`diagnose_transform_log_dimensionless_translation_names_dimension` and
    /// `diagnose_transform_exp_dimensionless_linear_names_dimension`) so this
    /// test adds a code assertion and nothing else.
    #[test]
    fn ruling_6126_dimension_arms_carry_dimensioned_arg_rejected() {
        let t = make_transform_with_translation([
            Value::Real(1.0),
            Value::Real(2.0),
            Value::Real(3.0),
        ]);
        let log_diag = super::diagnose("transform_log", &[t])
            .expect("a dimensionless translation must produce a diagnostic");
        assert_eq!(
            log_diag.severity,
            reify_core::Severity::Error,
            "the transform_log dimension arm stays Error (RULING #6126, Leo's \
             severity amendment 2026-08-19 via esc-6080-6)"
        );
        assert_eq!(
            log_diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "the transform_log dimension arm must carry the canonical runtime \
             dimension-rejection code (ruling A7)"
        );

        let twist = make_twist(
            [0.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            DimensionVector::DIMENSIONLESS,
        );
        let exp_diag = super::diagnose("transform_exp", &[twist])
            .expect("a dimensionless linear must produce a diagnostic");
        assert_eq!(
            exp_diag.severity,
            reify_core::Severity::Error,
            "the transform_exp dimension arm stays Error"
        );
        assert_eq!(
            exp_diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "the transform_exp dimension arm must carry the same code as its \
             transform_log mirror"
        );
    }

    /// Re-pins the already-correct THIRD arm so the convergence is asserted as
    /// a SET of three arms sharing one code, not as two independent edits.
    /// `bbox` has carried `DimensionedArgRejected` since task 6081
    /// (2026-08-27); it is the shipped counter-example proving the reuse was
    /// always the right shape.
    #[test]
    fn bbox_arm_still_carries_dimensioned_arg_rejected() {
        let min = make_point3_angle(0.0, 0.0, 0.0);
        let max = make_point3_angle(1.0, 2.0, 3.0);
        let diag = super::diagnose("bbox", &[min, max])
            .expect("Angle-cornered bbox must produce a diagnostic");
        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::DimensionedArgRejected),
            "the bbox arm is the third member of the converged set"
        );
    }

    /// The `affine_scale` arms are a DIFFERENT FAULT CLASS — a dropped factor,
    /// drop-and-continue — and ruling A7 converges the DIMENSION reason, not
    /// every arm in `diagnose`. They must stay `Severity::Warning` and stay
    /// CODE-LESS.
    #[test]
    fn affine_scale_arms_stay_warning_and_code_less() {
        let dimensioned = super::diagnose(
            "affine_scale",
            &[Value::length(2.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("a dimensioned scale factor must still produce a diagnostic");
        assert_eq!(
            dimensioned.severity,
            reify_core::Severity::Warning,
            "affine_scale is drop-and-continue, not a construction failure"
        );
        assert_eq!(
            dimensioned.code, None,
            "affine_scale must stay code-less — it is not the dimension \
             rejection reason ruling A7 converges"
        );

        let zero = super::diagnose(
            "affine_scale",
            &[Value::Real(0.0), Value::Real(1.0), Value::Real(1.0)],
        )
        .expect("a zero scale factor must still produce a diagnostic");
        assert_eq!(zero.severity, reify_core::Severity::Warning);
        assert_eq!(zero.code, None);
    }

    // ── η classifier contract (task 6591): `diagnose` for the five ───────────
    //
    // The gate alone is INVISIBLE to the author: `Value::Undef` prints `undef`
    // and exits 0 (measured for all five before these arms landed). The
    // classifier is what turns each silent acceptance-turned-`undef` into a
    // `Severity::Error` and a nonzero `reify eval` exit code.
    //
    // Argument NAMES come from the datum-constructor signature block in
    // `reify-compiler/src/units.rs` — `midplane(a, b)`, `axis_through(a, b)`,
    // `plane_through(a, b, c)`, `offset(p, delta)`, `frame_at(o, x, z)` — one
    // scheme across all five, so the message names the argument the author
    // actually wrote. A POINT/ORIGIN operand is named as the WHOLE parameter in
    // ONE message rather than per coordinate, for the `ox/oy/oz` reason: the
    // decoder both the gate and the classifier read has already required the
    // three components to share one dimension, so all three offend identically
    // and one edit fixes the one line the author wrote.

    /// Every offending shape, at every gated POSITION of all five construction
    /// datum constructors, is reported as the SHARED `ArgRejection` sentence at
    /// `Severity::Error` carrying the pre-existing `DimensionedArgRejected` code
    /// — ruling A7, one rejection REASON gets one code.
    ///
    /// The expected wording is obtained LIVE from the owner on every row rather
    /// than spelled here, so a reword on `ArgRejection::message`'s side fails
    /// this test instead of silently forking this crate's copy (Contract C1
    /// invariant (i)). This is the η twin of
    /// `diagnose_plane_family_non_length_offset_is_the_shared_coded_error`.
    #[test]
    fn diagnose_datum_family_non_length_position_is_the_shared_coded_error() {
        use reify_ir::arg_acceptance::{Acceptance, accept_arg, length_spec};

        for offender in non_length_offenders() {
            let Acceptance::Rejected(rejection) = accept_arg(&offender, &length_spec()) else {
                panic!("{offender:?}: the owner must REJECT this at a length_spec position");
            };
            let point = Value::Point(vec![offender.clone(), offender.clone(), offender.clone()]);
            let plane = Value::Plane {
                origin: Box::new(point.clone()),
                normal: Box::new(super::make_real_vec3([0.0, 0.0, 1.0])),
            };
            let good_point = point3_len(0.001, 0.002, 0.003);
            let good_plane = plane_at_z(0.010);
            let (xdir, zdir) = unit_x_z_directions();

            for (name, arg_name, args) in [
                ("midplane", "a", vec![plane.clone(), good_plane.clone()]),
                ("midplane", "b", vec![good_plane.clone(), plane.clone()]),
                ("axis_through", "a", vec![point.clone(), good_point.clone()]),
                ("axis_through", "b", vec![good_point.clone(), point.clone()]),
                (
                    "plane_through",
                    "a",
                    vec![point.clone(), good_point.clone(), good_point.clone()],
                ),
                (
                    "plane_through",
                    "c",
                    vec![good_point.clone(), good_point.clone(), point.clone()],
                ),
                ("offset", "p", vec![plane.clone(), Value::length(0.005)]),
                (
                    "offset",
                    "delta",
                    vec![good_plane.clone(), offender.clone()],
                ),
                (
                    "frame_at",
                    "o",
                    vec![point.clone(), xdir.clone(), zdir.clone()],
                ),
            ] {
                let diag = super::diagnose(name, &args).unwrap_or_else(|| {
                    panic!("{name} / {arg_name} / {offender:?}: must be diagnosed")
                });
                assert_eq!(diag.severity, reify_core::Severity::Error, "{diag:?}");
                assert_eq!(
                    diag.code,
                    Some(reify_core::DiagnosticCode::DimensionedArgRejected),
                    "{diag:?}"
                );
                assert_eq!(
                    diag.message,
                    rejection.message(name, arg_name),
                    "{name} / {arg_name}: this wording has forked from the shared \
                     ArgRejection template"
                );
            }
        }
    }

    /// When SEVERAL operands offend, the FIRST in ARGUMENT ORDER is named —
    /// deterministically, because the order is a property of the ONE per-builtin
    /// classifier both the gate and this arm read, not a convention restated at
    /// each (the `diagnose_bbox_corners` `min`-before-`max` precedent).
    #[test]
    fn diagnose_datum_family_names_the_first_offending_argument() {
        let bare_point = super::make_point3([0.0, 0.0, 1.0], DimensionVector::DIMENSIONLESS);
        let bare_point2 = super::make_point3([1.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let bare_point3 = super::make_point3([0.0, 1.0, 0.0], DimensionVector::DIMENSIONLESS);
        let bare_plane = plane_with_origin([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        let bare_plane2 = plane_with_origin([0.0, 0.0, 1.0], DimensionVector::DIMENSIONLESS);
        let good_point = point3_len(0.001, 0.002, 0.003);

        for (name, expected_arg, args) in [
            // Both wrong → the first is named.
            ("midplane", "a", vec![bare_plane.clone(), bare_plane2]),
            (
                "axis_through",
                "a",
                vec![bare_point.clone(), bare_point2.clone()],
            ),
            (
                "plane_through",
                "a",
                vec![bare_point.clone(), bare_point2.clone(), bare_point3.clone()],
            ),
            // First good, the other two wrong → the SECOND is named, which is
            // what makes this an order pin rather than an "always names `a`" one.
            (
                "plane_through",
                "b",
                vec![good_point, bare_point2, bare_point3],
            ),
            ("offset", "p", vec![bare_plane, Value::Real(2.0)]),
        ] {
            let diag = super::diagnose(name, &args)
                .unwrap_or_else(|| panic!("{name}: a both-wrong call must be diagnosed"));
            assert!(
                diag.message
                    .starts_with(&format!("{name}: {expected_arg} argument")),
                "{name}: expected the message to name `{expected_arg}` first; got {:?}",
                diag.message
            );
        }
    }

    /// The SILENCE contract: `diagnose` says nothing for every cause that is not
    /// a units fault, because a shape failure is never blamed on a dimension and
    /// an unresolved cell is not a wrong one (decision D10).
    ///
    /// Each row is a cause the GATE still rejects — these are not acceptances.
    /// They are the half of the no-mis-attribution invariant that a classifier
    /// re-spelling the gate's predicate would get wrong.
    #[test]
    fn diagnose_datum_family_stays_silent_on_every_shape_fault() {
        let good_point = point3_len(0.001, 0.002, 0.003);
        let good_plane = plane_at_z(0.010);
        let (xdir, zdir) = unit_x_z_directions();
        let mixed = Value::Point(vec![
            Value::length(1.0),
            Value::Real(0.0),
            Value::length(0.0),
        ]);
        let non_finite = Value::Point(vec![
            Value::length(f64::NAN),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let mixed_plane = Value::Plane {
            origin: Box::new(mixed.clone()),
            normal: Box::new(super::make_real_vec3([0.0, 0.0, 1.0])),
        };
        // A NORMAL that cannot be decoded at all, and one that decodes but is
        // degenerate. Neither is a units fault, and neither may be blamed on the
        // origin's dimension — the hazard a classifier reading only the origin
        // would walk straight into.
        let bad_normal_plane = Value::Plane {
            origin: Box::new(good_point.clone()),
            normal: Box::new(Value::Real(1.0)),
        };
        let degenerate_normal_plane = Value::Plane {
            origin: Box::new(good_point.clone()),
            normal: Box::new(super::make_real_vec3([0.0, 0.0, 0.0])),
        };

        for (label, name, args) in [
            // Wrong variant in a POSITION slot.
            (
                "midplane non-Plane",
                "midplane",
                vec![Value::Real(1.0), good_plane.clone()],
            ),
            (
                "axis_through non-Point",
                "axis_through",
                vec![Value::Real(1.0), good_point.clone()],
            ),
            (
                "plane_through non-Point",
                "plane_through",
                vec![Value::Real(1.0), good_point.clone(), good_point.clone()],
            ),
            (
                "offset non-Plane",
                "offset",
                vec![Value::Real(1.0), Value::length(0.005)],
            ),
            (
                "frame_at non-Point origin",
                "frame_at",
                vec![Value::Real(1.0), xdir.clone(), zdir.clone()],
            ),
            // MIXED component dimensions and a NON-FINITE component: both
            // `decompose_xyz3` CONSISTENCY failures, not LENGTH ones.
            (
                "axis_through mixed-dimension point",
                "axis_through",
                vec![mixed.clone(), good_point.clone()],
            ),
            (
                "axis_through non-finite point",
                "axis_through",
                vec![non_finite, good_point.clone()],
            ),
            (
                "midplane mixed-dimension origin",
                "midplane",
                vec![mixed_plane, good_plane.clone()],
            ),
            // A malformed or degenerate plane NORMAL.
            (
                "midplane undecodable normal",
                "midplane",
                vec![bad_normal_plane.clone(), good_plane.clone()],
            ),
            (
                "offset undecodable normal",
                "offset",
                vec![bad_normal_plane, Value::length(0.005)],
            ),
            (
                "offset degenerate normal",
                "offset",
                vec![degenerate_normal_plane, Value::length(0.005)],
            ),
            // `frame_at`'s x/z slots are NOT gated (decision D3), so a fault
            // confined to them is a shape fault and stays silent.
            (
                "frame_at non-Direction axes",
                "frame_at",
                vec![good_point.clone(), Value::Real(1.0), Value::Real(2.0)],
            ),
            (
                "frame_at parallel axes",
                "frame_at",
                vec![good_point.clone(), zdir.clone(), zdir.clone()],
            ),
            // Wrong arity in every family.
            ("midplane arity 1", "midplane", vec![good_plane.clone()]),
            (
                "axis_through arity 1",
                "axis_through",
                vec![good_point.clone()],
            ),
            (
                "plane_through arity 2",
                "plane_through",
                vec![good_point.clone(), good_point.clone()],
            ),
            (
                "frame_at arity 2",
                "frame_at",
                vec![good_point.clone(), xdir.clone()],
            ),
            // A wholly VALID call: nothing to say.
            (
                "midplane valid",
                "midplane",
                vec![plane_at_z(0.002), plane_at_z(0.008)],
            ),
            (
                "axis_through valid",
                "axis_through",
                vec![good_point.clone(), point3_len(0.001, 0.002, 0.007)],
            ),
            (
                "offset valid",
                "offset",
                vec![good_plane, Value::length(0.003)],
            ),
            ("frame_at valid", "frame_at", vec![good_point, xdir, zdir]),
        ] {
            assert!(
                super::diagnose(name, &args).is_none(),
                "{label}: a shape failure is never blamed on a dimension, so \
                 diagnose must stay silent; got {:?}",
                super::diagnose(name, &args)
            );
        }
    }

    /// The ARITY-3 `offset` is the γ RELATION and must NEVER be given a
    /// datum-constructor rejection, whatever its arguments look like.
    ///
    /// `eval_geometry`'s dispatch is arity-blind (`"offset" => eval_offset_plane`),
    /// so without the `args.len() == 2` guard inside `classify_offset_plane_args`
    /// this arm would read `args[1]` on a three-argument call and attribute a
    /// units fault to a builtin this gate has no authority over.
    #[test]
    fn diagnose_offset_stays_silent_at_arity_three() {
        let bare_plane = plane_with_origin([0.0, 0.0, 0.0], DimensionVector::DIMENSIONLESS);
        for (label, args) in [
            (
                "all-bare γ relation",
                vec![bare_plane.clone(), bare_plane, Value::Real(2.0)],
            ),
            (
                "LENGTH γ relation",
                vec![plane_at_z(0.0), plane_at_z(0.010), Value::length(0.005)],
            ),
            (
                "bare delta in the γ slot",
                vec![plane_at_z(0.0), plane_at_z(0.010), Value::Real(2.0)],
            ),
        ] {
            assert!(
                super::diagnose("offset", &args).is_none(),
                "{label}: the arity-3 offset is the γ relation, not this gate's \
                 to reject; got {:?}",
                super::diagnose("offset", &args)
            );
        }
    }
}
