// shell_result.rs — Rust runtime container for the structured shell stress
// result (PRD task T16, `docs/prds/v0_4/structural-analysis-shells.md` §
// "Stress through thickness").
//
// Sibling to the stdlib-level `ShellStress` structure_def declared at
// `crates/reify-compiler/stdlib/solver_elastic.ri:366` (std/solver/elastic).
// Both definitions must stay shape-aligned (top/mid/bottom); if a future task
// adds a fourth layer, update both sides together. Engine-integration tasks
// T18-T20 will add a cross-assertion once they consume both sides. This
// file ships the data-only contract (define the shape + tet constructor);
// engine-integration tasks T18-T20 are responsible for actually populating
// these fields from the MITC3+ kernel and wiring the `to_global(stress,
// frame)` dispatch helper.

use crate::shell_assembly::build_shell_frame;
use reify_types::Value;

/// Returns the local-to-global rotation matrix for a three-node MITC3+ shell element.
///
/// # Convention
///
/// The returned 3×3 matrix is the *local-to-global* rotation:
/// - `result[i][j]` is the j-th global component of the i-th local basis vector.
/// - A local-frame displacement vector `v_local` maps to global via `v_global = frame · v_local`.
/// - A local-frame rank-2 stress tensor maps to global via `σ_global = frame · σ_local · frameᵀ`.
///
/// This is the **transpose** of [`crate::shell_assembly::build_shell_frame`]`.r`, which stores
/// the *global-to-local* rotation (rows = local basis vectors in global coordinates,
/// so `R · v_global = v_local`).  Transposing gives the local-to-global direction:
/// `result[i][j] = frame.r[j][i]`.
///
/// # Relation to `ElasticResult.frame`
///
/// Matches the `ElasticResult.frame` local-to-global convention documented in
/// `crates/reify-compiler/stdlib/solver_elastic.ri:276–294`.  The future
/// `to_global(stress, frame)` helper (T18-T20) can use this directly as
/// `σ_global = frame · σ_local · frameᵀ` without any transpose step at the call site.
pub fn shell_element_frame(nodes: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let r = build_shell_frame(nodes).r;
    // Transpose: result[i][j] = r[j][i].
    let mut result = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            result[i][j] = r[j][i];
        }
    }
    result
}

/// Structured shell stress result carrying per-integration-layer stress
/// channels.
///
/// # Channels
///
/// - `top`    — top-surface stress (outer fibre in the element's local-z).
/// - `mid`    — mid-surface (neutral-plane) stress. For tet results all three
///   channels are equal (no through-thickness gradient).
/// - `bottom` — bottom-surface stress (inner fibre opposite to `top`).
///
/// The per-element local-to-global rotation frame lives on `ElasticResult`
/// (as `frame : Real` placeholder for `Field<Point3<Length>, Matrix<3,3,Real>>`),
/// not on `ShellStress`. All three channels share the same per-element
/// rotation, so keeping `frame` at the `ElasticResult` level avoids
/// duplicating it across channels.
///
/// # Note on `Eq`
///
/// `PartialEq` is derived; `Eq` cannot be derived because `Value` contains
/// `f64`, which does not implement `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellStress {
    pub top: Value,
    pub mid: Value,
    pub bottom: Value,
}

impl ShellStress {
    /// Canonical tet-result constructor. Sets `top == mid == bottom == field`
    /// (no through-thickness stress variation for solid elements).
    ///
    /// Engine-integration tasks T18-T20 call this for every tet-element result
    /// when packaging the solver output. For shell elements they use direct
    /// struct initialisation with distinct per-layer fields.
    pub fn homogeneous(field: Value) -> Self {
        Self {
            top: field.clone(),
            mid: field.clone(),
            bottom: field,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::Value;

    /// `shell_element_frame(nodes)` must return the transpose of `build_shell_frame(nodes).r`.
    ///
    /// `build_shell_frame.r` has rows = local basis vectors in global coordinates, so it maps
    /// global → local.  The frame field convention (see `ElasticResult.frame` in solver_elastic.ri)
    /// is local-to-global.  Therefore `shell_element_frame` must return the transpose of `r`.
    ///
    /// Also verified: each row of the returned matrix has unit norm (orthonormal).
    #[test]
    fn shell_element_frame_is_transpose_of_shell_frame_rotation() {
        let nodes: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];
        let frame_r = build_shell_frame(&nodes).r;
        let result = shell_element_frame(&nodes);

        // result[i][j] must equal frame_r[j][i] (transpose)
        for i in 0..3 {
            for j in 0..3 {
                let expected = frame_r[j][i];
                let got = result[i][j];
                assert!(
                    (got - expected).abs() < 1e-12,
                    "result[{i}][{j}] = {got}, expected frame.r[{j}][{i}] = {expected}",
                );
            }
        }

        // Each column of result (= each row of frame_r) has unit norm.
        for i in 0..3 {
            let norm_sq = result[i][0] * result[i][0]
                + result[i][1] * result[i][1]
                + result[i][2] * result[i][2];
            assert!(
                (norm_sq - 1.0).abs() < 1e-12,
                "result row {i} norm² = {norm_sq}, expected 1.0",
            );
        }
    }

    /// `ShellStress::homogeneous(field)` is the canonical tet-result constructor.
    /// It must set all three stress channels to the same field value.
    ///
    /// This test pins the tet-result population contract:
    ///   result.top    == input field
    ///   result.mid    == input field
    ///   result.bottom == input field
    #[test]
    fn shell_stress_homogeneous_replicates_field_across_channels() {
        let field = Value::Real(42.0);
        let result = ShellStress::homogeneous(field.clone());

        assert_eq!(
            result.top, field,
            "homogeneous: top should equal the input field"
        );
        assert_eq!(
            result.mid, field,
            "homogeneous: mid should equal the input field"
        );
        assert_eq!(
            result.bottom, field,
            "homogeneous: bottom should equal the input field"
        );
    }

    /// Explicit construction must preserve distinct per-channel values, proving
    /// that `ShellStress` can represent the fully differentiated per-layer
    /// stress distribution produced by the MITC3+ shell kernel.
    ///
    /// This test pins the explicit/per-channel shape needed for shell results:
    /// each of top/mid/bottom round-trips through the struct unchanged.
    #[test]
    fn shell_stress_explicit_construction_preserves_per_channel_values() {
        let top = Value::Real(1.0);
        let mid = Value::Real(2.0);
        let bottom = Value::Real(3.0);

        let result = ShellStress {
            top: top.clone(),
            mid: mid.clone(),
            bottom: bottom.clone(),
        };

        assert_eq!(result.top, top, "explicit: top round-trips");
        assert_eq!(result.mid, mid, "explicit: mid round-trips");
        assert_eq!(result.bottom, bottom, "explicit: bottom round-trips");
    }
}
