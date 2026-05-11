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

use reify_mesh_morph::{
    MorphOptions, QualityVerdict, elasticity_morph, quality_check,
};
use reify_types::VolumeMesh;

/// Sentinel pin used by `tests/calibration.rs`'s smoke test to verify the
/// helper module is wired in before the sweep runner lands.
pub const MODULE_OK: bool = true;

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

    let morphed = elasticity_morph(&source, &prescribed_positions, options)
        .unwrap_or_else(|e| {
            panic!(
                "run_sweep: elasticity_morph failed ({:?}) at base={base_param}, \
                 target={target_param}",
                e
            )
        });

    let morph_verdict = quality_check(&morphed, &source, options);
    let (morph_min_scaled_j, morph_max_ar_factor) = extract_metrics(&morphed, &source);
    // For from-scratch: min_scaled_j depends only on the first arg. We pass
    // `source` as second arg to satisfy the signature; the returned AR
    // factor (from_scratch_ar / source_ar) is ignored for this metric path.
    let (from_scratch_min_scaled_j, _) = extract_metrics(&from_scratch, &source);

    SweepReport {
        morph_verdict,
        morph_min_scaled_j,
        from_scratch_min_scaled_j,
        morph_max_ar_factor,
        morphed,
        from_scratch,
    }
}

/// "Materially-better" rule (PRD task #13): a from-scratch remesh is
/// *materially better* than a morph on a given metric when the
/// from-scratch value exceeds the morph value by more than 20 %.
///
/// Encoded as `from_scratch > 1.20 * morph`. Used by the calibration
/// sweep tests (steps 11/13/15) to gate the assertion "morph is rejected
/// only when from-scratch is materially better" — the materiality bar
/// the PRD specifies for threshold calibration.
///
/// The 1.20 factor matches the task's stated >20% improvement threshold.
/// Both inputs are read as "higher is better" metrics (e.g. min scaled
/// Jacobian); for "lower is better" metrics (e.g. AR factor) the caller
/// passes `1.0 / value` or `-value` to flip the polarity. The shared
/// helper keeps the materiality bar pinned in one place rather than
/// scattered through threshold comparisons.
#[allow(dead_code)]
pub fn is_materially_better(morph: f64, from_scratch: f64) -> bool {
    from_scratch > 1.20 * morph
}
