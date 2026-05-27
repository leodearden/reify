//! Sweep runner + `SweepReport` + materially-better-rule helper for the
//! PRD task #13 calibration suite. The runner drives a fixture across a
//! parameter range, runs `elasticity_morph` against the procedural
//! target-mesh's surface vertices, and compares morph quality against a
//! from-scratch remesh using `quality_check`.
//!
//! ## Probe-options pattern
//!
//! `quality_check`'s public surface returns a [`QualityVerdict`] whose
//! soft-fail metric fields (`SoftFailDetails.{min_scaled_jacobian,
//! pct_below_025, max_aspect_ratio_factor}`) are only populated when the
//! metric *breaches* its threshold. To extract the raw metric values
//! independent of the user-facing thresholds, this module crafts a
//! "probe" [`MorphOptions`] with sentinel thresholds chosen so every
//! metric always exceeds its threshold and therefore always lands in the
//! `Some(_)` arm of `SoftFailDetails`. The probe is used only inside
//! [`run_sweep`] for metric extraction; the user-facing `morph_verdict`
//! is still computed under the caller-supplied `options` so the verdict
//! reflects production thresholds.

use reify_mesh_morph::{MorphOptions, QualityVerdict, elasticity_morph, quality_check};
use reify_ir::VolumeMesh;

/// Canonical materiality factor for the "morph rejected only when from-scratch
/// is materially better" calibration rule (PRD task #13 / task #2950).
///
/// A from-scratch remesh is *materially better* than a morph when its metric
/// value exceeds the morph's by more than 20 %. Pinned as a single shared
/// constant so the bar lives in one place and changes propagate uniformly to
/// every materiality comparison ([`is_materially_better`] for higher-is-better
/// metrics; AR-factor inline in the rule helper for lower-is-better metrics).
pub const MATERIALITY_FACTOR: f64 = 1.20;

/// One step of a calibration parameter sweep: morph quality + from-scratch
/// quality, plus the meshes themselves for downstream inspection.
///
/// Produced by [`run_sweep`]. The fields are populated unconditionally
/// (probe-options pattern — see module-level doc); `morph_verdict` is the
/// only field that depends on the caller-supplied [`MorphOptions`].
#[derive(Debug)]
pub struct SweepReport {
    /// Verdict from `quality_check(morphed, source, options)` under the
    /// caller-supplied options — i.e. the production-facing pass/fail.
    pub morph_verdict: QualityVerdict,

    /// Minimum per-element scaled Jacobian across the morphed mesh.
    /// Extracted via probe options (always-populated SoftFail).
    /// On HardFail (an inverted tet), this is the (negative) scaled J of
    /// the first inverted element — signals inversion via its sign.
    pub morph_min_scaled_j: f64,

    /// Minimum per-element scaled Jacobian across the from-scratch mesh
    /// (no inversion expected — procedural meshers are valid by
    /// construction, so this is always positive).
    pub from_scratch_min_scaled_j: f64,

    /// Maximum aspect-ratio factor `morphed_ar / source_ar` across all
    /// matching tet pairs. Probe-options pattern: always populated.
    pub morph_max_ar_factor: f64,

    /// Maximum aspect-ratio factor `morphed_ar / from_scratch_ar` across all
    /// matching tet pairs — the morph's AR measured against the true
    /// from-scratch baseline (NOT against `source` like `morph_max_ar_factor`).
    /// Probe-options pattern: always populated.
    ///
    /// Used by [`ar_materially_better`] to evaluate whether the morph's AR
    /// quality is materially worse than a fresh remesh of the same target
    /// geometry.
    pub from_scratch_max_ar_factor: f64,

    /// The morphed mesh produced by `elasticity_morph`.
    pub morphed: VolumeMesh,

    /// The from-scratch mesh: the fixture evaluated at `target_param`.
    /// Connectivity is identical to `morphed` by procedural-fixture
    /// construction (same generator function, same `n`).
    pub from_scratch: VolumeMesh,
}

/// Probe-option set: thresholds chosen so `quality_check`'s SoftFail
/// metric fields are always populated when the mesh is non-empty and
/// non-inverted. See module-level doc for rationale.
///
/// Inherits the production [`MorphOptions::default`] for non-threshold
/// fields (stiffness rule, fictitious modulus, Poisson, …) so the metric
/// computations themselves are not perturbed by the probe.
fn probe_options() -> MorphOptions {
    MorphOptions {
        // `global_min_scaled_j < INFINITY` is true for any finite J, so
        // `min_scaled_jacobian` is always `Some(_)`.
        quality_floor_min_scaled_jacobian: f64::INFINITY,
        // `pct >= 0`, threshold `-1.0`, so `pct > -1.0` is always true and
        // `pct_below_025` is always `Some(_)`.
        quality_floor_pct_below_025: -1.0,
        // `max_ar_ratio >= 0`, threshold `-1.0`, so it is always populated
        // (provided connectivity matches — guaranteed by procedural
        // fixtures using the same generator).
        quality_aspect_ratio_factor_max: -1.0,
        ..MorphOptions::default()
    }
}

/// Extract `(min_scaled_j, max_ar_factor)` from a mesh by running
/// [`quality_check`] under [`probe_options`]. Falls back to sentinel
/// values when the verdict is `HardFail` (loop breaks before AR
/// accumulation completes) or `Pass` (only happens for empty meshes
/// since the probe thresholds always trigger SoftFail on non-empty
/// meshes).
fn extract_metrics(mesh: &VolumeMesh, source: &VolumeMesh) -> (f64, f64) {
    let probe = probe_options();
    match quality_check(mesh, source, &probe) {
        QualityVerdict::SoftFail(d) => (
            // `min_scaled_jacobian` is always populated under probe options
            // (threshold = INFINITY); the `unwrap_or` is a defensive fallback
            // for a degenerate empty-mesh case where `global_min_scaled_j`
            // stays at INFINITY and the `.is_finite()` guard rejects it.
            d.min_scaled_jacobian.unwrap_or(0.0),
            d.max_aspect_ratio_factor.unwrap_or(0.0),
        ),
        // HardFail short-circuits after the first inverted element — AR
        // ratios for the remaining elements are never accumulated. Use the
        // (negative) inversion J as `min_scaled_j` to signal the failure
        // numerically; AR factor is meaningless after a break, return 0.0.
        QualityVerdict::HardFail(d) => (d.jacobian, 0.0),
        // Pass only happens on an empty mesh — return zeros (caller is
        // expected to not pass empty meshes; this is purely defensive).
        QualityVerdict::Pass => (0.0, 0.0),
    }
}

/// Run a one-step parameter sweep on a procedural fixture.
///
/// 1. Evaluate the fixture at `base_param` → source mesh.
/// 2. Evaluate the fixture at `target_param` → from-scratch target mesh.
/// 3. Build `prescribed_positions` from `source`'s surface indices and
///    `target`'s corresponding vertex positions (identity correspondence —
///    procedural fixtures preserve connectivity across parameter values).
/// 4. Run [`elasticity_morph`] to produce the morphed mesh.
/// 5. Run [`quality_check`] under the caller-supplied `options` for the
///    user-facing `morph_verdict`.
/// 6. Extract raw metrics from the morphed and from-scratch meshes via the
///    probe-options pattern (see module-level doc).
///
/// ## Panics
///
/// Panics if [`elasticity_morph`] fails. The calibration sweeps in
/// [`tests/calibration.rs`] use tiny parameter steps that are well within
/// the solver's operating range; a failure here is a calibration-rig bug
/// (e.g., a fixture-builder regression), not a tuning concern.
pub fn run_sweep<F>(
    fixture: F,
    base_param: f64,
    target_param: f64,
    options: &MorphOptions,
) -> SweepReport
where
    F: Fn(f64) -> (VolumeMesh, Vec<u32>),
{
    let (source, surface_indices) = fixture(base_param);
    let (from_scratch, _from_scratch_surface) = fixture(target_param);

    // Identity surface correspondence: surface_node_indices is the same Vec
    // for both meshes by procedural-fixture construction, so we read the
    // target positions directly from `from_scratch.vertices` using the
    // source's surface indices. `vertex_f64` is the f32 → f64 widening read
    // helper.
    let prescribed_positions: Vec<(u32, [f64; 3])> = surface_indices
        .iter()
        .map(|&i| {
            let pos = from_scratch.vertex_f64(i).unwrap_or_else(|| {
                panic!(
                    "run_sweep: surface index {i} out of range for from_scratch \
                     mesh (n_vertices = {})",
                    from_scratch.vertices.len() / 3
                )
            });
            (i, pos)
        })
        .collect();

    let morphed = elasticity_morph(&source, &prescribed_positions, options).unwrap_or_else(|e| {
        panic!(
            "run_sweep: elasticity_morph failed ({:?}) at base={base_param}, \
                 target={target_param}",
            e
        )
    });

    let morph_verdict = quality_check(&morphed, &source, options);
    let (morph_min_scaled_j, morph_max_ar_factor) = extract_metrics(&morphed, &source);
    // For from-scratch min-J: second-arg `source` is arbitrary here — only the
    // first tuple element (min_scaled_j of `from_scratch`) is used; the AR
    // ratio (`from_scratch_ar / source_ar`) is discarded (`_`). Contrast with
    // the third call below where the second-arg choice is load-bearing.
    let (from_scratch_min_scaled_j, _) = extract_metrics(&from_scratch, &source);
    // Compute the true morph-vs-from_scratch AR ratio: pass `from_scratch` in
    // the source slot so quality_check computes max(morphed_AR / from_scratch_AR).
    // The first tuple element (min_scaled_j) is discarded — it equals
    // `morph_min_scaled_j` since it's the same `morphed` mesh evaluated again;
    // we capture it as `_` to make the discard explicit.
    let (_, from_scratch_max_ar_factor) = extract_metrics(&morphed, &from_scratch);

    SweepReport {
        morph_verdict,
        morph_min_scaled_j,
        from_scratch_min_scaled_j,
        morph_max_ar_factor,
        from_scratch_max_ar_factor,
        morphed,
        from_scratch,
    }
}

/// "Materially-better" rule (PRD task #13) for *higher-is-better* metrics
/// (e.g. min scaled Jacobian): a from-scratch remesh is *materially better*
/// than a morph when its value exceeds the morph value by more than 20 %.
///
/// Encoded as `from_scratch > MATERIALITY_FACTOR * morph`. Used by the
/// calibration sweep tests (steps 13/15) to gate the assertion "morph is
/// rejected only when from-scratch is materially better" — the materiality
/// bar the PRD specifies for threshold calibration.
///
/// For the *lower-is-better* AR-factor metric, use the companion helper
/// [`ar_materially_better`] which reads [`SweepReport::from_scratch_max_ar_factor`]
/// — the true `max(morphed_AR / from_scratch_AR)` ratio computed directly
/// against the from-scratch baseline in [`run_sweep`].
pub fn is_materially_better(morph: f64, from_scratch: f64) -> bool {
    from_scratch > MATERIALITY_FACTOR * morph
}

/// AR-side materially-better predicate (lower-is-better polarity).
///
/// True when the morph's AR is more than `MATERIALITY_FACTOR` × the from-scratch
/// baseline AR — i.e. the morph is ≥20 % more elongated than a fresh remesh
/// of the same target geometry.
///
/// Uses [`SweepReport::from_scratch_max_ar_factor`], the true
/// `max(morphed_AR / from_scratch_AR)` ratio computed in [`run_sweep`] by
/// calling `extract_metrics(&morphed, &from_scratch)`. This is NOT the old
/// `morph_max_ar_factor` proxy that assumed `source_AR ≈ from_scratch_AR ≈ 1.0`
/// — for wide sweep steps the two ratios diverge significantly.
pub fn ar_materially_better(report: &SweepReport) -> bool {
    report.from_scratch_max_ar_factor > MATERIALITY_FACTOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::ElementOrderTag;

    /// Build a synthetic `SweepReport` with hand-picked metric values and
    /// empty meshes. The predicate functions only read scalar fields, so
    /// the meshes need not contain any elements.
    fn synthetic_report(morph_max_ar_factor: f64, from_scratch_max_ar_factor: f64) -> SweepReport {
        let empty = VolumeMesh {
            vertices: Vec::new(),
            tet_indices: Vec::new(),
            element_order: ElementOrderTag::P1,
            normals: None,
        };
        SweepReport {
            morph_verdict: reify_mesh_morph::QualityVerdict::Pass,
            morph_min_scaled_j: 0.0,
            from_scratch_min_scaled_j: 0.0,
            morph_max_ar_factor,
            from_scratch_max_ar_factor,
            morphed: empty.clone(),
            from_scratch: empty,
        }
    }

    /// `ar_materially_better` uses `from_scratch_max_ar_factor`, NOT
    /// `morph_max_ar_factor` — the two fields must be independent.
    ///
    /// Case (a): old source-aliased check (`morph_max_ar_factor > 1.20`) would
    /// trip, but `from_scratch_max_ar_factor = 1.10 < 1.20` so the new
    /// predicate must return false.
    #[test]
    fn ar_materially_better_predicate_compares_morph_against_from_scratch_baseline_not_source() {
        // (a) morph_max_ar_factor=1.50 (old check trips), from_scratch=1.10 → false
        let report_a = synthetic_report(1.50, 1.10);
        assert!(
            !ar_materially_better(&report_a),
            "from_scratch_max_ar_factor=1.10 < MATERIALITY_FACTOR=1.20 must return false; \
             old source-proxied check would have tripped on morph_max_ar_factor=1.50"
        );

        // (b) morph_max_ar_factor=1.00 (old check doesn't trip), from_scratch=1.30 → true
        let report_b = synthetic_report(1.00, 1.30);
        assert!(
            ar_materially_better(&report_b),
            "from_scratch_max_ar_factor=1.30 > MATERIALITY_FACTOR=1.20 must return true; \
             old source-proxied check would NOT have tripped on morph_max_ar_factor=1.00"
        );

        // Boundary: from_scratch_max_ar_factor == MATERIALITY_FACTOR — strict >
        // comparison must return false (equal is not materially better).
        let report_boundary = synthetic_report(2.00, MATERIALITY_FACTOR);
        assert!(
            !ar_materially_better(&report_boundary),
            "from_scratch_max_ar_factor == MATERIALITY_FACTOR must return false \
             (strict greater-than comparison)"
        );
    }
}
