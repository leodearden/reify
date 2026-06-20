//! Stress analysis builtins: von_mises, principal_stresses, max_shear, safety_factor,
//! stress_invariants.

use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Sentinel `StructureTypeId` for engine-assembled (registry-free) instances.
///
/// The `eval_builtin` path has no `StructureRegistry`, so result instances
/// are minted with the nominal `type_name` as the authoritative source of
/// truth for downstream consumers.  This mirrors the identical constant in
/// `crates/reify-eval/src/dynamics_ops.rs:41` and
/// `crates/reify-stdlib/src/dynamics/eval.rs:51`.
///
/// **Single-source-of-truth note**: all four occurrences of this sentinel
/// across the codebase use `StructureTypeId(u32::MAX)` directly.  A full
/// cross-crate dedup (hoisting to `reify-ir` or a shared `reify-stdlib` helper)
/// is deferred beyond task 2884's scope.
const REGISTRY_FREE_TYPE_ID: StructureTypeId = StructureTypeId(u32::MAX);

use crate::helpers::{binary, sanitize_value, unary};
use crate::matrix::matrix_components_f64;

/// Evaluate a stress-analysis builtin by name.
///
/// Returns `Some(value)` if the name is a recognised analysis function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_analysis(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "von_mises" => von_mises(args),
        "principal_stresses" => principal_stresses(args),
        "max_shear" => max_shear(args),
        "safety_factor" => safety_factor(args),
        "stress_invariants" => stress_invariants(args),
        _ => return None,
    })
}

/// Compute von Mises equivalent stress for a 3×3 row-major stress window.
///
/// Formula: σ_vm = √(0.5·((σ_xx−σ_yy)²+(σ_yy−σ_zz)²+(σ_zz−σ_xx)²+6·(σ_xy²+σ_yz²+σ_xz²)))
///
/// Uses the direct component formula (avoids eigenvalue computation).
/// Row-major flat layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
///                         d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
///                         d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
///
/// `pub` (widened from `pub(crate)`) so the formula has a single home — the
/// `Value::Tensor`-shaped `von_mises` builtin (below), the SampledField
/// hot-path projection in `crates/reify-stdlib/src/fea.rs`, AND the
/// cross-crate VonMises field reduction in
/// `crates/reify-expr/src/field_reductions.rs` all route through this kernel.
/// Re-exported via `pub use analysis::compute_von_mises_3x3` in `lib.rs` so
/// the call site in reify-expr is `reify_stdlib::compute_von_mises_3x3`.
/// Mirrors the `pub(crate)` promotion of `compute_eigenvalues_3x3` for the
/// same cross-module reuse pattern.
///
/// Window must be at least 9 floats long; only `d[0..9]` is read.
pub fn compute_von_mises_3x3(d: &[f64]) -> f64 {
    debug_assert!(
        d.len() >= 9,
        "compute_von_mises_3x3 requires at least 9 elements, got {}",
        d.len()
    );
    debug_assert!(
        {
            // NaN inputs (out-of-solid sentinel windows from the FEA elaborator)
            // are trivially "symmetric" — the assertion is only meaningful for
            // non-NaN programmer-error catches.
            let tol = |a: f64, b: f64| {
                (a.is_nan() && b.is_nan())
                    || (a - b).abs() <= 1e-10 * (1.0 + a.abs().max(b.abs()))
            };
            tol(d[1], d[3]) && tol(d[2], d[6]) && tol(d[5], d[7])
        },
        "compute_von_mises_3x3: input matrix is not symmetric"
    );

    let sxx = d[0];
    let syy = d[4];
    let szz = d[8];
    let sxy = d[1];
    let syz = d[5];
    let sxz = d[2];

    (0.5 * ((sxx - syy).powi(2)
        + (syy - szz).powi(2)
        + (szz - sxx).powi(2)
        + 6.0 * (sxy.powi(2) + syz.powi(2) + sxz.powi(2))))
    .sqrt()
}

/// Compute von Mises equivalent stress from a 3×3 stress tensor `Value::Tensor`.
///
/// Wraps `compute_von_mises_3x3` for the dynamic-Value entry point used by
/// the `eval_builtin("von_mises", ...)` dispatch path.
fn von_mises(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols); // used only for the 3×3 guard above

        let vm = compute_von_mises_3x3(&d);

        sanitize_value(Value::from_real_scalar(vm, dim))
    })
}

/// Compute the maximum shear stress from a 3×3 row-major stress window.
///
/// Formula: τ_max = (σ₁ − σ₃) / 2, where σ₁ and σ₃ are the maximum and
/// minimum principal stresses (ascending eigenvalues of the symmetric stress
/// tensor: `eigs[2]` and `eigs[0]`).
///
/// Returns `f64::NAN` if eigenvalue decomposition fails (e.g. all-NaN window).
///
/// `pub` so the formula has a single home — the `max_shear` builtin (below),
/// AND the cross-crate MaxShear field reduction in
/// `crates/reify-expr/src/field_reductions.rs`. Mirrors the `pub` promotion
/// of `compute_von_mises_3x3`. Re-exported via
/// `pub use analysis::compute_max_shear_3x3` in `lib.rs`.
///
/// Window must be at least 9 floats long; only `d[0..9]` is read.
pub fn compute_max_shear_3x3(d: &[f64]) -> f64 {
    match compute_eigenvalues_3x3(d) {
        Some(eigs) => (eigs[2] - eigs[0]) / 2.0,
        None => f64::NAN,
    }
}

/// Compute eigenvalues of a symmetric 3×3 matrix.
///
/// Uses the closed-form formula optimized for symmetric matrices: two eigenvalues
/// are computed trigonometrically and the third is recovered from the trace
/// constraint (trace = λ₁ + λ₂ + λ₃), which avoids precision loss at repeated roots.
///
/// Returns `Some([λ₁, λ₂, λ₃])` sorted ascending.
///
/// `pub` for cross-crate reuse from:
/// - `crates/reify-stdlib/src/fea.rs::envelope_max_principal` — per-grid-point
///   projection inlines this call on each 9-float row-major stress window and
///   selects `eigs[2]` (the largest principal stress).
/// - `crates/reify-expr/src/field_reductions.rs::project_principal_stresses_sampled`
///   — selects `eigs[2]` (max) or `eigs[0]` (min) per window during a
///   `max|min|argmax|argmin(principal_stresses_field)` reduction (task 4562).
pub fn compute_eigenvalues_3x3(d: &[f64]) -> Option<[f64; 3]> {
    debug_assert!(
        d.len() >= 9,
        "compute_eigenvalues_3x3 requires at least 9 elements, got {}",
        d.len()
    );
    // Row-major: d[0]=a00, d[1]=a01, d[2]=a02, d[4]=a11, d[5]=a12, d[8]=a22
    // This function assumes a symmetric matrix — only upper-triangle entries
    // (d[1], d[2], d[5]) are read. The lower-triangle (d[3], d[6], d[7]) is
    // ignored. For non-symmetric inputs the result will be silently wrong.
    debug_assert!(
        {
            // NaN inputs (out-of-solid sentinel windows from the FEA elaborator)
            // are trivially "symmetric" — the assertion is only meaningful for
            // non-NaN programmer-error catches.  Matches the NaN short-circuit
            // already present in `compute_von_mises_3x3`.
            let tol = |a: f64, b: f64| {
                (a.is_nan() && b.is_nan())
                    || (a - b).abs() <= 1e-10 * (1.0 + a.abs().max(b.abs()))
            };
            tol(d[1], d[3]) && tol(d[2], d[6]) && tol(d[5], d[7])
        },
        "compute_eigenvalues_3x3 assumes a symmetric matrix but got non-symmetric entries: \
         a01={} vs a10={}, a02={} vs a20={}, a12={} vs a21={}",
        d[1],
        d[3],
        d[2],
        d[6],
        d[5],
        d[7]
    );

    let a00 = d[0];
    let a11 = d[4];
    let a22 = d[8];
    let a01 = d[1];
    let a02 = d[2];
    let a12 = d[5];

    let q = (a00 + a11 + a22) / 3.0; // trace / 3

    // Sum of squared off-diagonal elements (symmetric, so count each pair once)
    let p1 = a01 * a01 + a02 * a02 + a12 * a12;

    if p1 <= 1e-30 {
        // Matrix is (effectively) diagonal — eigenvalues are the diagonal entries
        let mut eigs = [a00, a11, a22];
        eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        return Some(eigs);
    }

    let p2 = (a00 - q).powi(2) + (a11 - q).powi(2) + (a22 - q).powi(2) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();

    // B = (A - q·I) / p
    let b00 = (a00 - q) / p;
    let b11 = (a11 - q) / p;
    let b22 = (a22 - q) / p;
    let b01 = a01 / p;
    let b02 = a02 / p;
    let b12 = a12 / p;

    // det(B) / 2 — B is symmetric so b10=b01, b20=b02, b21=b12
    let det_b = b00 * (b11 * b22 - b12 * b12) - b01 * (b01 * b22 - b12 * b02)
        + b02 * (b01 * b12 - b11 * b02);
    let r = (det_b / 2.0).clamp(-1.0, 1.0);

    let phi = r.acos() / 3.0;

    // Two eigenvalues via trigonometry, third from trace constraint
    let eig1 = q + 2.0 * p * phi.cos();
    let eig3 = q + 2.0 * p * (phi + 4.0 * std::f64::consts::FRAC_PI_3).cos();
    let eig2 = 3.0 * q - eig1 - eig3;

    let mut eigs = [eig1, eig2, eig3];
    eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(eigs)
}

/// Compute principal stresses (eigenvalues) of a 3×3 stress tensor.
///
/// Returns a sorted `Value::List` of 3 scalars (ascending order).
fn principal_stresses(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols);

        let eigs = match compute_eigenvalues_3x3(&d) {
            Some(e) => e,
            None => return Value::Undef,
        };

        let make_val = |x: f64| sanitize_value(Value::from_real_scalar(x, dim));
        Value::List(eigs.iter().map(|&e| make_val(e)).collect())
    })
}

/// Compute maximum shear stress from a 3×3 stress tensor.
///
/// max_shear = (σ₁ − σ₃) / 2 where σ₁ and σ₃ are the max and min principal stresses.
///
/// Delegates to [`compute_max_shear_3x3`] so the formula has a single home
/// shared with the cross-crate MaxShear field reduction in
/// `crates/reify-expr/src/field_reductions.rs`.
fn max_shear(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols);

        let tau_max = compute_max_shear_3x3(&d);
        sanitize_value(Value::from_real_scalar(tau_max, dim))
    })
}

/// Compute safety factor: yield_strength / von_mises(tensor).
///
/// Returns a dimensionless Real. If von_mises is zero (e.g. hydrostatic stress),
/// the division produces infinity which sanitize_value converts to Undef.
fn safety_factor(args: &[Value]) -> Value {
    binary(args, |tensor, yield_val| {
        let yield_f64 = match yield_val.as_f64() {
            Some(v) => v,
            None => return Value::Undef,
        };

        // Compute von Mises of the tensor via the same logic as the von_mises builtin
        let vm = von_mises(std::slice::from_ref(tensor));
        let vm_f64 = match vm.as_f64() {
            Some(v) => v,
            None => return Value::Undef,
        };

        sanitize_value(Value::Real(yield_f64 / vm_f64))
    })
}

/// Compute the three stress invariants of a 3×3 row-major symmetric stress tensor.
///
/// Returns `[I1, I2, I3]` where:
///   I1 = trace = σ_xx + σ_yy + σ_zz
///   I2 = (σ_xx·σ_yy + σ_yy·σ_zz + σ_zz·σ_xx) − (σ_xy² + σ_yz² + σ_xz²)
///   I3 = determinant of the stress tensor
///
/// Row-major flat layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
///                         d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
///                         d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
///
/// ## Symmetry convention
///
/// Only upper-triangle entries (`d[1]`, `d[2]`, `d[5]`) are read; the
/// lower-triangle (`d[3]`, `d[6]`, `d[7]`) is ignored.  In debug/test builds,
/// a `debug_assert!` checks near-symmetry (same tolerance as the sibling
/// kernels `compute_von_mises_3x3` and `compute_eigenvalues_3x3`) so
/// programmer errors and asymmetric inputs are caught early.
///
/// A user passing a genuinely asymmetric 3×3 `.ri` matrix will panic in debug
/// builds (same as the sibling kernels) rather than producing silently wrong
/// results.  This matches the established convention for all 3×3 stress kernels
/// in this module.
pub(crate) fn compute_stress_invariants_3x3(d: &[f64]) -> [f64; 3] {
    debug_assert!(
        d.len() >= 9,
        "compute_stress_invariants_3x3 requires at least 9 elements, got {}",
        d.len()
    );
    debug_assert!(
        {
            let tol = |a: f64, b: f64| {
                (a.is_nan() && b.is_nan())
                    || (a - b).abs() <= 1e-10 * (1.0 + a.abs().max(b.abs()))
            };
            tol(d[1], d[3]) && tol(d[2], d[6]) && tol(d[5], d[7])
        },
        "compute_stress_invariants_3x3: input matrix is not symmetric"
    );

    let sxx = d[0];
    let syy = d[4];
    let szz = d[8];
    let sxy = d[1]; // upper-triangle; lower triangle ignored for symmetric inputs
    let syz = d[5];
    let sxz = d[2];

    let i1 = sxx + syy + szz;
    let i2 = sxx * syy + syy * szz + szz * sxx - (sxy * sxy + syz * syz + sxz * sxz);
    // I3 = determinant (using upper-triangle symmetry: syx=sxy, szx=sxz, szy=syz)
    let i3 = sxx * (syy * szz - syz * syz) - sxy * (sxy * szz - syz * sxz)
        + sxz * (sxy * syz - syy * sxz);

    [i1, i2, i3]
}

/// Rotate a 3×3 row-major stress tensor into a new frame: `σ' = R·σ·Rᵀ`.
///
/// `r` is a row-major 3×3 rotation matrix mapping the *local* frame to the
/// *global* frame (`F = local→global`). This matches the rotation convention
/// of `reify-solver-elastic`'s `flatten_shell_channels`
/// (`crates/reify-solver-elastic/src/shell_result.rs`), which carries each
/// shell element's `ShellFrame::local_to_global()` matrix so downstream
/// consumers can map local-frame Cauchy stress into global coordinates via
/// `σ_global = F·σ_local·Fᵀ`.
///
/// Both inputs are flat row-major 3×3 windows: index `i*3 + j` is row `i`,
/// column `j`. Only the first 9 elements of each slice are read.
///
/// `pub(crate)` so the closed-form rotation has a single home alongside the
/// other 3×3 stress kernels (`compute_von_mises_3x3`,
/// `compute_eigenvalues_3x3`, `compute_stress_invariants_3x3`) and is reused
/// by the `to_global` builtin in `crates/reify-stdlib/src/fea.rs` (and any
/// future ShellStress-container or GUI populator) without re-deriving the
/// rotation order.
///
/// # Identity invariant
///
/// When `r` is the identity matrix the result is bit-identical to `sigma`
/// (each output element is `0.0 + … + 1.0·σ_ij`, exact for finite inputs).
/// No symmetry `debug_assert!` is imposed: a rotation maps a symmetric tensor
/// to a symmetric tensor, but this kernel multiplies the full matrix and does
/// not assume symmetry, so it is correct for the general (and the symmetric)
/// case alike.
pub(crate) fn rotate_stress_3x3(sigma: &[f64], r: &[f64]) -> [f64; 9] {
    debug_assert!(
        sigma.len() >= 9,
        "rotate_stress_3x3 requires sigma of at least 9 elements, got {}",
        sigma.len()
    );
    debug_assert!(
        r.len() >= 9,
        "rotate_stress_3x3 requires r of at least 9 elements, got {}",
        r.len()
    );

    // M = R · σ  (row-major 3×3): M[i][j] = Σ_k R[i][k]·σ[k][j].
    let mut m = [0.0_f64; 9];
    for i in 0..3 {
        for j in 0..3 {
            let mut acc = 0.0;
            for k in 0..3 {
                acc += r[i * 3 + k] * sigma[k * 3 + j];
            }
            m[i * 3 + j] = acc;
        }
    }

    // out = M · Rᵀ: out[i][j] = Σ_k M[i][k]·Rᵀ[k][j] = Σ_k M[i][k]·R[j][k].
    let mut out = [0.0_f64; 9];
    for i in 0..3 {
        for j in 0..3 {
            let mut acc = 0.0;
            for k in 0..3 {
                acc += m[i * 3 + k] * r[j * 3 + k];
            }
            out[i * 3 + j] = acc;
        }
    }

    out
}

/// Compute the three stress invariants of a 3×3 stress tensor `Value::Tensor`.
///
/// Returns a `Value::StructureInstance` with `type_name = "StressInvariants"` and
/// fields `i1` (PRESSURE), `i2` (PRESSURE²), `i3` (PRESSURE³) — or `Value::Real`
/// for dimensionless inputs.
fn stress_invariants(args: &[Value]) -> Value {
    unary(args, |tensor| {
        let (nrows, ncols, d, dim) = match matrix_components_f64(tensor) {
            Some(v) if v.0 == 3 && v.1 == 3 => v,
            _ => return Value::Undef,
        };
        let _ = (nrows, ncols);

        let [i1, i2, i3] = compute_stress_invariants_3x3(&d);

        // Build correctly-dimensioned scalars: I1 ∝ dim, I2 ∝ dim², I3 ∝ dim³.
        // Value::from_real_scalar(v, DIMENSIONLESS) → Value::Real(v), so the
        // dimensionless case produces Real fields automatically.
        let dim2 = dim.mul(&dim);
        let dim3 = dim2.mul(&dim);

        let fields: PersistentMap<String, Value> = [
            (
                "i1".to_string(),
                sanitize_value(Value::from_real_scalar(i1, dim)),
            ),
            (
                "i2".to_string(),
                sanitize_value(Value::from_real_scalar(i2, dim2)),
            ),
            (
                "i3".to_string(),
                sanitize_value(Value::from_real_scalar(i3, dim3)),
            ),
        ]
        .into_iter()
        .collect();

        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: REGISTRY_FREE_TYPE_ID,
            type_name: "StressInvariants".to_string(),
            version: 1,
            fields,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_analysis("foo", &[]).is_none());
    }

    #[test]
    fn known_function_returns_some() {
        assert!(eval_analysis("von_mises", &[]).is_some());
        assert!(eval_analysis("principal_stresses", &[]).is_some());
        assert!(eval_analysis("max_shear", &[]).is_some());
        assert!(eval_analysis("safety_factor", &[]).is_some());
    }

    // ── test helpers ────────────────────────────────────────────────────────

    /// Build a dimensionless 3x3 matrix from rows.
    fn make_matrix(rows: &[&[f64]]) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| Value::Tensor(row.iter().map(|&v| Value::Real(v)).collect()))
                .collect(),
        )
    }

    /// Build a dimensioned 3x3 matrix with all elements sharing one dimension.
    fn make_dimensioned_matrix(rows: &[&[f64]], dim: DimensionVector) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| {
                    Value::Tensor(
                        row.iter()
                            .map(|&v| Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    // ── von_mises tests ─────────────────────────────────────────────────────

    #[test]
    fn von_mises_uniaxial_dimensionless() {
        // Uniaxial stress [[σ,0,0],[0,0,0],[0,0,0]] → von Mises = σ
        let sigma = 100.0;
        let tensor = make_matrix(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_real_approx!(result, sigma);
    }

    #[test]
    fn von_mises_uniaxial_pressure() {
        // Uniaxial stress with PRESSURE dimension → von Mises = σ (same dim)
        let sigma = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_hydrostatic_returns_zero() {
        // Hydrostatic stress [[p,0,0],[0,p,0],[0,0,p]] → von Mises = 0
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        assert_scalar_approx!(result, 0.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_pure_shear() {
        // Pure shear [[0,τ,0],[τ,0,0],[0,0,0]] → von Mises = τ·√3
        let tau = 50e6;
        let tensor = make_dimensioned_matrix(
            &[&[0.0, tau, 0.0], &[tau, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("von_mises", &[tensor]).unwrap();
        let expected = tau * 3.0_f64.sqrt();
        assert_scalar_approx!(result, expected, DimensionVector::PRESSURE);
    }

    #[test]
    fn von_mises_wrong_arg_count_returns_undef() {
        assert!(eval_analysis("von_mises", &[]).unwrap().is_undef());
        let t = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        assert!(
            eval_analysis("von_mises", &[t.clone(), t])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn von_mises_non_matrix_returns_undef() {
        assert!(
            eval_analysis("von_mises", &[Value::Real(42.0)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn von_mises_non_3x3_returns_undef() {
        // 2x2 matrix
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(eval_analysis("von_mises", &[m2x2]).unwrap().is_undef());
    }

    // ── principal_stresses tests ────────────────────────────────────────────

    #[test]
    fn principal_stresses_diagonal_dimensionless() {
        // Diagonal tensor [[100,0,0],[0,50,0],[0,0,25]] → sorted [25, 50, 100]
        let tensor = make_matrix(&[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]]);
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_real_approx!(items[0].clone(), 25.0);
                assert_real_approx!(items[1].clone(), 50.0);
                assert_real_approx!(items[2].clone(), 100.0);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_diagonal_pressure() {
        // Diagonal tensor with PRESSURE dimension → sorted List of Scalar
        let tensor = make_dimensioned_matrix(
            &[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 25.0, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[1].clone(), 50.0, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[2].clone(), 100.0, DimensionVector::PRESSURE);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_symmetric_tensor() {
        // Symmetric tensor [[2,1,0],[1,3,1],[0,1,2]] → eigenvalues [1, 2, 4]
        let tensor = make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_real_approx!(items[0].clone(), 1.0);
                assert_real_approx!(items[1].clone(), 2.0);
                assert_real_approx!(items[2].clone(), 4.0);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_hydrostatic() {
        // Hydrostatic [[p,0,0],[0,p,0],[0,0,p]] → [p, p, p]
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("principal_stresses", &[tensor]).unwrap();
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), p, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[1].clone(), p, DimensionVector::PRESSURE);
                assert_scalar_approx!(items[2].clone(), p, DimensionVector::PRESSURE);
            }
            other => panic!("expected List, got {:?}", other),
        }
    }

    #[test]
    fn principal_stresses_wrong_args_return_undef() {
        assert!(eval_analysis("principal_stresses", &[]).unwrap().is_undef());
        assert!(
            eval_analysis("principal_stresses", &[Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(
            eval_analysis("principal_stresses", &[m2x2])
                .unwrap()
                .is_undef()
        );
    }

    // ── max_shear tests ─────────────────────────────────────────────────────

    #[test]
    fn max_shear_hydrostatic_returns_zero() {
        // Hydrostatic: all principal stresses equal → (σ₁−σ₃)/2 = 0
        let p = 100e6;
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, 0.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn max_shear_uniaxial() {
        // Uniaxial [[σ,0,0],[0,0,0],[0,0,0]] → max_shear = σ/2
        // Use small values to keep within absolute tolerance of assert macros
        let sigma = 200.0;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma / 2.0, DimensionVector::PRESSURE);
    }

    #[test]
    fn max_shear_biaxial() {
        // Biaxial [[σ,0,0],[0,-σ,0],[0,0,0]] → max_shear = σ
        let sigma = 100.0;
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, -sigma, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[tensor]).unwrap();
        assert_scalar_approx!(result, sigma, DimensionVector::PRESSURE);
    }

    #[test]
    fn max_shear_wrong_args_return_undef() {
        assert!(eval_analysis("max_shear", &[]).unwrap().is_undef());
        assert!(
            eval_analysis("max_shear", &[Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
    }

    // ── compute_max_shear_3x3 kernel tests ──────────────────────────────────

    /// `compute_max_shear_3x3` on a uniaxial window [σ,0,...,0] returns σ/2
    /// (eigenvalues = [0,0,σ] → (σ−0)/2 = σ/2).
    ///
    /// Also verifies a hydrostatic window [p,0,0, 0,p,0, 0,0,p] → 0.0
    /// (all eigenvalues equal → (p−p)/2 = 0).
    ///
    /// RED before step-2: `compute_max_shear_3x3` does not exist (compile error).
    #[test]
    fn compute_max_shear_3x3_uniaxial_and_hydrostatic() {
        // Uniaxial [σ,0,0,0,0,0,0,0,0] → τ_max = σ/2
        let sigma = 200e6_f64;
        let uniaxial = [sigma, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let tau = compute_max_shear_3x3(&uniaxial);
        assert!(
            (tau - sigma / 2.0).abs() < 1e-6,
            "uniaxial: expected τ_max={}, got {}",
            sigma / 2.0,
            tau
        );

        // Hydrostatic [p,0,0, 0,p,0, 0,0,p] → τ_max = 0
        let p = 100e6_f64;
        let hydrostatic = [p, 0.0, 0.0, 0.0, p, 0.0, 0.0, 0.0, p];
        let tau_h = compute_max_shear_3x3(&hydrostatic);
        assert!(
            tau_h.abs() < 1e-6,
            "hydrostatic: expected τ_max=0.0, got {}",
            tau_h
        );
    }

    /// All-NaN window routed through the `max_shear` builtin must NOT panic
    /// on the symmetry `debug_assert` in `compute_eigenvalues_3x3`, and must
    /// return `Value::Undef` (NaN projected to NaN → `sanitize_value` → Undef).
    ///
    /// RED before step-2: the NaN short-circuit in `compute_eigenvalues_3x3`
    /// is missing, so `(NaN-NaN).abs() <= ...` = false and the assert fires.
    #[test]
    fn max_shear_builtin_all_nan_window_returns_undef_without_panic() {
        let nan_tensor = make_dimensioned_matrix(
            &[
                &[f64::NAN, f64::NAN, f64::NAN],
                &[f64::NAN, f64::NAN, f64::NAN],
                &[f64::NAN, f64::NAN, f64::NAN],
            ],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("max_shear", &[nan_tensor]).unwrap();
        assert!(
            result.is_undef(),
            "max_shear(all-NaN window) must return Undef without panicking, got {:?}",
            result
        );
    }

    // ── rotate_stress_3x3 kernel tests (step-1) ─────────────────────────────

    /// Identity rotation is a no-op: `R = I` ⟹ `R·σ·Rᵀ = σ`, bit-for-bit.
    ///
    /// The output must bit-equal the input stride-9 row-major tensor (no
    /// floating-point drift introduced by multiplying through identity).
    ///
    /// RED before step-2: `rotate_stress_3x3` does not exist (compile error).
    #[test]
    fn rotate_stress_3x3_identity_is_noop() {
        // Known symmetric tensor (row-major): σxx=1, σyy=2, σzz=3,
        // σxy=4, σxz=5, σyz=6.
        let sigma = [1.0, 4.0, 5.0, 4.0, 2.0, 6.0, 5.0, 6.0, 3.0];
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let out = rotate_stress_3x3(&sigma, &identity);
        for (i, (&o, &s)) in out.iter().zip(sigma.iter()).enumerate() {
            assert_eq!(
                o.to_bits(),
                s.to_bits(),
                "identity rotation must be bit-identical at index {i}: got {o}, expected {s}"
            );
        }
    }

    /// A +90° rotation about the z-axis applied to a known symmetric stress
    /// tensor yields the hand-computed `σ_global = R·σ·Rᵀ`.
    ///
    /// F = local→global rotation (matching `flatten_shell_channels`'s
    /// `σ_global = F·σ_local·Fᵀ` convention, row-major). For +90° about z:
    ///   R = [[0,-1,0],[1,0,0],[0,0,1]]   (cos90=0, sin90=1)
    /// σ_local (row-major) = [1,4,5, 4,2,6, 5,6,3]
    ///
    /// Hand-computed (M = R·σ, then σ_global = M·Rᵀ):
    ///   σ_global = [[2,-4,-6],[-4,1,5],[-6,5,3]]
    /// i.e. row-major [2,-4,-6, -4,1,5, -6,5,3].
    /// Physical check: a 90° rotation about z swaps the x/y axes, so the new
    /// σxx = old σyy = 2, new σyy = old σxx = 1, σzz unchanged at 3, and the
    /// trace (=6) is preserved. Result stays symmetric.
    ///
    /// RED before step-2: `rotate_stress_3x3` does not exist (compile error).
    #[test]
    fn rotate_stress_3x3_z90_matches_hand_computed() {
        let sigma = [1.0, 4.0, 5.0, 4.0, 2.0, 6.0, 5.0, 6.0, 3.0];
        // +90° about z, local→global, row-major.
        let r = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let expected = [2.0, -4.0, -6.0, -4.0, 1.0, 5.0, -6.0, 5.0, 3.0];
        let out = rotate_stress_3x3(&sigma, &r);
        for (i, (&o, &e)) in out.iter().zip(expected.iter()).enumerate() {
            assert!(
                (o - e).abs() < 1e-12,
                "z90 rotation mismatch at index {i}: got {o}, expected {e}"
            );
        }
    }

    // ── safety_factor tests ─────────────────────────────────────────────────

    #[test]
    fn safety_factor_safe_dimensionless() {
        // yield=250, uniaxial stress=100 → von_mises=100 → SF=250/100=2.5
        let sigma = 100.0;
        let yield_strength = Value::Real(250.0);
        let tensor = make_matrix(&[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert_real_approx!(result, 2.5);
    }

    #[test]
    fn safety_factor_safe_pressure() {
        // yield=250e6 Pa, uniaxial stress=100e6 Pa → SF=2.5 (dimensionless)
        let sigma = 100e6;
        let yield_strength = Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        // Result is dimensionless (pressure / pressure)
        assert_real_approx!(result, 2.5);
    }

    #[test]
    fn safety_factor_unsafe() {
        // yield=100e6, uniaxial stress=200e6 → SF=100/200=0.5 (<1, unsafe)
        let sigma = 200e6;
        let yield_strength = Value::Scalar {
            si_value: 100e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[sigma, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert_real_approx!(result, 0.5);
    }

    #[test]
    fn safety_factor_hydrostatic_returns_undef() {
        // Hydrostatic: von_mises = 0 → division by zero → Undef
        let p = 100e6;
        let yield_strength = Value::Scalar {
            si_value: 250e6,
            dimension: DimensionVector::PRESSURE,
        };
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("safety_factor", &[tensor, yield_strength]).unwrap();
        assert!(result.is_undef());
    }

    #[test]
    fn safety_factor_wrong_arg_count_returns_undef() {
        assert!(eval_analysis("safety_factor", &[]).unwrap().is_undef());
        let t = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 0.0, 0.0], &[0.0, 0.0, 0.0]]);
        assert!(eval_analysis("safety_factor", &[t]).unwrap().is_undef());
    }

    // ── stress_invariants tests ─────────────────────────────────────────────

    /// Helper: get a field value from a StructureInstance by name.
    fn si_field(v: &Value, field_name: &str) -> Value {
        match v {
            Value::StructureInstance(data) => data
                .fields
                .get(&field_name.to_string())
                .cloned()
                .unwrap_or_else(|| panic!("field '{}' missing from StructureInstance", field_name)),
            other => panic!("expected StructureInstance, got {:?}", other),
        }
    }

    /// `stress_invariants` is a recognised name: `eval_analysis` must return `Some`
    /// (even with no args — the function is known, dispatch returns `Some(Undef)`).
    #[test]
    fn stress_invariants_name_is_recognised() {
        assert!(
            eval_analysis("stress_invariants", &[]).is_some(),
            "stress_invariants must be a recognised analysis name (returns Some)"
        );
        assert!(
            eval_analysis("stress_invariants", &[]).unwrap().is_undef(),
            "stress_invariants([]) must return Some(Undef) (wrong arity)"
        );
    }

    /// Diagonal dimensionless tensor [[100,0,0],[0,50,0],[0,0,25]]:
    ///   I1 = trace = 175
    ///   I2 = 100·50 + 50·25 + 25·100 − 0 = 8750
    ///   I3 = det  = 100·50·25 = 125000
    /// All invariants should be `Value::Real` (dimensionless tensor).
    #[test]
    fn stress_invariants_diagonal_dimensionless() {
        let tensor =
            make_matrix(&[&[100.0, 0.0, 0.0], &[0.0, 50.0, 0.0], &[0.0, 0.0, 25.0]]);
        let result = eval_analysis("stress_invariants", &[tensor]).unwrap();
        match &result {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "StressInvariants",
                    "type_name must be 'StressInvariants', got {:?}",
                    data.type_name
                );
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
        assert_real_approx!(si_field(&result, "i1"), 175.0);
        assert_real_approx!(si_field(&result, "i2"), 8750.0);
        assert_real_approx!(si_field(&result, "i3"), 125000.0);
    }

    /// Hydrostatic dimensioned tensor [[p,0,0],[0,p,0],[0,0,p]] (PRESSURE):
    ///   I1 = 3p  (PRESSURE)
    ///   I2 = 3p² (PRESSURE²)
    ///   I3 = p³  (PRESSURE³)
    #[test]
    fn stress_invariants_hydrostatic_pressure() {
        let p = 100e6_f64; // 100 MPa
        let tensor = make_dimensioned_matrix(
            &[&[p, 0.0, 0.0], &[0.0, p, 0.0], &[0.0, 0.0, p]],
            DimensionVector::PRESSURE,
        );
        let result = eval_analysis("stress_invariants", &[tensor]).unwrap();
        match &result {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "StressInvariants");
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
        let dim2 = DimensionVector::PRESSURE.mul(&DimensionVector::PRESSURE);
        let dim3 = dim2.mul(&DimensionVector::PRESSURE);
        // I1 = 3p (PRESSURE)
        assert_scalar_approx!(si_field(&result, "i1"), 3.0 * p, DimensionVector::PRESSURE);
        // I2 = 3p² (PRESSURE²)
        assert_scalar_approx!(si_field(&result, "i2"), 3.0 * p * p, dim2);
        // I3 = p³ (PRESSURE³)
        assert_scalar_approx!(si_field(&result, "i3"), p * p * p, dim3);
    }

    /// General symmetric tensor [[2,1,0],[1,3,1],[0,1,2]] (dimensionless):
    ///   I1 = 2+3+2 = 7
    ///   I2 = (2·3+3·2+2·2) − (1²+1²+0²) = 16 − 2 = 14
    ///   I3 = det = 2·(3·2−1·1) − 1·(1·2−1·0) + 0 = 2·5−2 = 8
    #[test]
    fn stress_invariants_general_symmetric_dimensionless() {
        let tensor =
            make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_analysis("stress_invariants", &[tensor]).unwrap();
        match &result {
            Value::StructureInstance(data) => {
                assert_eq!(data.type_name, "StressInvariants");
            }
            other => panic!("expected StructureInstance, got {:?}", other),
        }
        assert_real_approx!(si_field(&result, "i1"), 7.0);
        assert_real_approx!(si_field(&result, "i2"), 14.0);
        assert_real_approx!(si_field(&result, "i3"), 8.0);
    }

    /// Wrong arity / non-matrix / non-3×3 → `Some(Value::Undef)`.
    #[test]
    fn stress_invariants_bad_args_return_undef() {
        // Too many args
        let t = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert!(
            eval_analysis("stress_invariants", &[t.clone(), t.clone()])
                .unwrap()
                .is_undef(),
            "two args must return Undef"
        );
        // Non-matrix arg
        assert!(
            eval_analysis("stress_invariants", &[Value::Real(42.0)])
                .unwrap()
                .is_undef(),
            "scalar arg must return Undef"
        );
        // 2×2 matrix
        let m2x2 = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert!(
            eval_analysis("stress_invariants", &[m2x2])
                .unwrap()
                .is_undef(),
            "2×2 matrix must return Undef"
        );
    }
}
