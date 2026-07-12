//! Deterministic algorithm-level chain-degradation harness (PRD
//! `docs/prds/v0_3/mesh-morphing.md` task #14). Models the engine's
//! "morph-source = from-most-recent-in-memory only" policy explicitly over
//! the task #13 calibration fixtures (`calibration/fixtures.rs`) and the
//! `elasticity_morph`/`quality_check` primitives, without driving the real
//! (OCCT/gmsh-fragile) Engine — see `tests/chain_degradation.rs`'s module
//! docs for the full rationale.
//!
//! ## Chain-source policy
//!
//! - tick 0: `mesh = fixture(params[0])` — the fresh root of the chain.
//! - tick i ≥ 1: `prescribed` = the previous tick's surface nodes mapped to
//!   `fixture(params[i])`'s vertex positions (identity surface
//!   correspondence — the calibration fixtures are connectivity-invariant
//!   across their swept parameter). `candidate = elasticity_morph(prev,
//!   prescribed, opts)`. `quality_check(candidate, prev, opts)` then decides:
//!   - `Pass` ⇒ accept: `mesh_i = candidate` (the chain continues from the
//!     morphed mesh — the from-most-recent-in-memory policy under test).
//!   - `HardFail` / `SoftFail` / `Unsupported`, or an `elasticity_morph`
//!     `Err`, ⇒ remesh fallback: `mesh_i = fixture(params[i])` (fresh — the
//!     chain RESETS from this tick). Mirrors task #4744's morph-or-remesh
//!     decision tree (quality-reject / solver-error ⇒ gmsh remesh).
//!
//! ## Probe-options pattern
//!
//! [`probe_min_scaled_j`] duplicates the probe-options technique from
//! `calibration/sweep.rs` (see that module's docs) rather than importing it
//! — `sweep.rs`'s `probe_options`/`extract_metrics` are private to that
//! module, and this harness deliberately avoids modifying the task #13
//! calibration files. `quality_check`'s `min_scaled_jacobian` metric is a
//! per-element property of its first (`morphed`) argument only (independent
//! of the second `source` argument), so pairing `mesh` with itself as the
//! `source` argument is safe — the same trick `run_sweep` uses for its
//! from-scratch probe call.

use reify_ir::VolumeMesh;
use reify_mesh_morph::{MorphOptions, QualityVerdict, elasticity_morph, quality_check};

/// One tick of a chain-degradation sweep.
#[derive(Debug, Clone)]
pub struct TickRecord {
    /// The fixture parameter value at this tick.
    pub param: f64,
    /// Probe-extracted minimum scaled Jacobian of the tick's in-use mesh —
    /// the accepted morph on a `fell_back == false` tick, or the fresh
    /// remesh on a `fell_back == true` tick (equal to
    /// `from_scratch_min_scaled_j` in that case).
    ///
    /// On a tick whose in-use mesh has an inverted element (`quality_check`
    /// would report `HardFail`), this is [`probe_min_scaled_j`]'s HardFail-arm
    /// value: the (negative) scaled Jacobian of the *first* inverted element
    /// only, not the true minimum over every element — mirrors
    /// `calibration/sweep.rs`'s `SweepReport::morph_min_scaled_j` doc, which
    /// documents the identical caveat. Still a reliable negative "inverted"
    /// signal, just not a global minimum in that case.
    pub min_scaled_j: f64,
    /// Probe-extracted minimum scaled Jacobian of `fixture(param)` evaluated
    /// from scratch — always populated, independent of `fell_back`.
    pub from_scratch_min_scaled_j: f64,
    /// `true` if this tick fell back to a fresh remesh (quality reject or
    /// solver error); `false` if the morph from the previous tick's mesh was
    /// accepted. Always `false` for tick 0 (the chain root is fresh by
    /// definition, not a morph decision).
    pub fell_back: bool,
}

/// Full chain report: one [`TickRecord`] per swept parameter.
#[derive(Debug, Clone)]
pub struct ChainReport {
    pub ticks: Vec<TickRecord>,
}

impl ChainReport {
    /// Count of ticks that fell back to a fresh remesh, among the morph
    /// ticks (index ≥ 1 — tick 0 is always the fresh chain root, not a morph
    /// decision).
    pub fn fallback_count(&self) -> usize {
        self.ticks.iter().skip(1).filter(|t| t.fell_back).count()
    }

    /// Fraction of morph ticks (index ≥ 1) that fell back. `0.0` for a chain
    /// with no morph ticks (`ticks.len() <= 1`) rather than dividing by
    /// zero.
    pub fn fallback_rate(&self) -> f64 {
        let morph_ticks = self.ticks.len().saturating_sub(1);
        if morph_ticks == 0 {
            return 0.0;
        }
        self.fallback_count() as f64 / morph_ticks as f64
    }

    /// The minimum `min_scaled_j` observed across every tick (including the
    /// tick-0 root).
    pub fn min_scaled_j_floor(&self) -> f64 {
        self.ticks
            .iter()
            .map(|t| t.min_scaled_j)
            .fold(f64::INFINITY, f64::min)
    }
}

/// Extract the raw minimum scaled Jacobian of `mesh` via the probe-options
/// pattern (see module docs): sentinel thresholds so
/// `quality_check`'s `SoftFailDetails.min_scaled_jacobian` is always
/// populated for a non-empty, non-inverted mesh, independent of `opts`'s own
/// production thresholds. Non-threshold fields (stiffness rule, fictitious
/// modulus, Poisson, …) are inherited from the caller's `opts` so the probe
/// never perturbs anything the metric computation might someday depend on
/// (today `quality_check` reads only the three threshold fields).
fn probe_min_scaled_j(mesh: &VolumeMesh, opts: &MorphOptions) -> f64 {
    let probe = MorphOptions {
        // `global_min_scaled_j < INFINITY` is true for any finite J, so
        // `min_scaled_jacobian` is always `Some(_)`.
        quality_floor_min_scaled_jacobian: f64::INFINITY,
        // `pct >= 0`, threshold `-1.0`, so `pct > -1.0` is always true.
        quality_floor_pct_below_025: -1.0,
        // `max_ar_ratio >= 0`, threshold `-1.0`, so it is always populated
        // (provided connectivity matches — irrelevant here since only
        // min_scaled_jacobian is read below).
        quality_aspect_ratio_factor_max: -1.0,
        ..opts.clone()
    };
    match quality_check(mesh, mesh, &probe) {
        QualityVerdict::SoftFail(d) => d.min_scaled_jacobian.unwrap_or(0.0),
        // HardFail short-circuits at the FIRST inverted element (the
        // quality_check loop `break`s immediately on the first sj < 0.0 —
        // quality.rs), so `d.jacobian` — despite `InversionDetails.jacobian`
        // being a genuinely *scaled* Jacobian (types.rs) — is that single
        // element's (negative) value only, NOT necessarily the true minimum
        // scaled Jacobian over the whole mesh (elements after the first
        // inversion are never evaluated). It still reliably signals
        // "inverted" via its sign for this probe's callers (see
        // `TickRecord::min_scaled_j`'s doc for the identical caveat) —
        // mirrors `calibration/sweep.rs::extract_metrics`'s and
        // `SweepReport::morph_min_scaled_j`'s documented behavior.
        QualityVerdict::HardFail(d) => d.jacobian,
        // Unreachable with these sentinel thresholds: `pct_below_025` is
        // always `Some(_)` (`pct >= 0.0 > quality_floor_pct_below_025 ==
        // -1.0` unconditionally, even for an empty mesh where `pct` defaults
        // to 0.0 — quality.rs), so `quality_check` never returns `Pass`
        // under this probe. Purely defensive fallback, not a reachable case.
        QualityVerdict::Pass => 0.0,
        // Unsupported requires hex/wedge connectivity (quality.rs); every
        // chain fixture is tet-only by construction. See `run_chain`'s
        // decision-match comment (below) for why this arm is intentionally
        // untested at the `run_chain` level.
        QualityVerdict::Unsupported => {
            unreachable!("probe_min_scaled_j: chain fixtures are always tet, got Unsupported")
        }
    }
}

/// Run a deterministic chain-degradation sweep over `params`, modelling the
/// engine's "morph-source = from-most-recent-in-memory only" policy — see
/// module docs for the full per-tick decision tree.
///
/// `fixture` must be connectivity-invariant across `params` (as
/// `calibration/fixtures.rs::plate_with_hole`/`bracket` are documented to
/// be) — `run_chain` relies on identity surface-node correspondence between
/// consecutive ticks.
///
/// # Panics
///
/// Panics if `params` is empty, or if a surface-node index from one tick is
/// out of range for the next tick's fixture mesh (a fixture
/// connectivity-invariance violation — a harness bug, not a tuning concern).
pub fn run_chain<F>(fixture: F, params: &[f64], opts: &MorphOptions) -> ChainReport
where
    F: Fn(f64) -> (VolumeMesh, Vec<u32>),
{
    assert!(!params.is_empty(), "run_chain requires at least one param");

    let (mut prev_mesh, mut prev_surface) = fixture(params[0]);
    let root_min_sj = probe_min_scaled_j(&prev_mesh, opts);
    let mut ticks = Vec::with_capacity(params.len());
    ticks.push(TickRecord {
        param: params[0],
        min_scaled_j: root_min_sj,
        from_scratch_min_scaled_j: root_min_sj,
        fell_back: false,
    });

    for &param in &params[1..] {
        let (target_mesh, target_surface) = fixture(param);
        let from_scratch_min_sj = probe_min_scaled_j(&target_mesh, opts);

        // Identity surface correspondence: prev_surface indexes the SAME
        // physical nodes in target_mesh (connectivity-invariant fixture).
        let prescribed: Vec<(u32, [f64; 3])> = prev_surface
            .iter()
            .map(|&i| {
                let pos = target_mesh.vertex_f64(i).unwrap_or_else(|| {
                    panic!(
                        "run_chain: surface index {i} out of range for fixture({param}) \
                         (n_vertices = {}) — fixture is not connectivity-invariant",
                        target_mesh.vertices.len() / 3
                    )
                });
                (i, pos)
            })
            .collect();

        // Morph FROM THE PREVIOUS TICK'S MESH — the "from-most-recent-in-
        // memory" policy under test. A quality reject or solver Err falls
        // back to a fresh remesh, resetting the chain (see module docs).
        //
        // Test coverage of this decision tree (task 2951 amendment): this
        // module's `mod tests` (below) directly exercises the `Err(_) =>
        // None` arm
        // (`run_chain_falls_back_to_fresh_remesh_when_elasticity_morph_returns_err`)
        // and the `QualityVerdict::HardFail(_) => None` arm
        // (`run_chain_falls_back_to_fresh_remesh_when_quality_check_hard_fails`),
        // in addition to the pre-existing `SoftFail` coverage in
        // `tests/chain_degradation.rs`. `QualityVerdict::Unsupported` is the
        // one arm intentionally left uncovered here: reaching it requires a
        // hex/wedge `mesh`, but `probe_min_scaled_j` — called on every
        // tick's from-scratch fixture, including tick 0's root, before this
        // match ever runs — `unreachable!()`s on `Unsupported` first, so a
        // hex/wedge fixture regression is still caught, just by that panic
        // rather than by an assertion on this specific arm.
        let accepted_candidate = match elasticity_morph(&prev_mesh, &prescribed, opts) {
            Ok(candidate) => match quality_check(&candidate, &prev_mesh, opts) {
                QualityVerdict::Pass => Some(candidate),
                QualityVerdict::HardFail(_)
                | QualityVerdict::SoftFail(_)
                | QualityVerdict::Unsupported => None,
            },
            Err(_) => None,
        };

        let (mesh_i, surface_i, fell_back, min_scaled_j) = match accepted_candidate {
            Some(candidate) => {
                let min_sj = probe_min_scaled_j(&candidate, opts);
                (candidate, prev_surface, false, min_sj)
            }
            None => (target_mesh, target_surface, true, from_scratch_min_sj),
        };

        ticks.push(TickRecord {
            param,
            min_scaled_j,
            from_scratch_min_scaled_j: from_scratch_min_sj,
            fell_back,
        });

        prev_mesh = mesh_i;
        prev_surface = surface_i;
    }

    ChainReport { ticks }
}

// ── amendment (task 2951, reviewer_comprehensive finding #1) ────────────────
//
// `chain_degradation.rs`'s tests only drive `run_chain`'s fallback through
// the quality-reject `SoftFail` path. The two tests below directly exercise
// the other two reachable fallback triggers — `elasticity_morph`'s `Err`
// path and `quality_check`'s `HardFail` verdict — using small synthetic
// fixtures (not the calibration plate/bracket geometries) purpose-built to
// force each path deterministically. See the comment on `run_chain`'s
// decision match (above) for why `QualityVerdict::Unsupported` is not
// (and cannot be) covered here.
#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};

    /// Canonical right-handed unit tet — the same fixture `elasticity.rs`
    /// and `quality.rs` use for their own smoke/inversion tests.
    fn unit_tet_vertices() -> Vec<f32> {
        vec![
            0.0, 0.0, 0.0, // node 0
            1.0, 0.0, 0.0, // node 1
            0.0, 1.0, 0.0, // node 2
            0.0, 0.0, 1.0, // node 3
        ]
    }

    /// The same 4 nodes with corners 2 and 3 swapped — left-handed
    /// (inverted). The exact fixture
    /// `quality_check_with_single_inverted_tet_returns_hard_fail_with_element_index_and_negative_jacobian`
    /// (quality.rs) uses to pin `QualityVerdict::HardFail`.
    fn inverted_tet_vertices() -> Vec<f32> {
        vec![
            0.0, 0.0, 0.0, // node 0
            1.0, 0.0, 0.0, // node 1
            0.0, 0.0, 1.0, // node 2  (swapped)
            0.0, 1.0, 0.0, // node 3  (swapped)
        ]
    }

    /// Forces `elasticity_morph`'s `Err` path deterministically via a
    /// P2-tagged fixture: `elasticity_morph_with_cg_opts` rejects any
    /// `old_mesh.element_order() != Some(P1)` (elasticity.rs:237-247)
    /// *before* it ever inspects `prescribed_positions` — the same
    /// precedence
    /// `elasticity_morph_rejects_p2_element_order_with_unsupported_element_order_failure`
    /// (elasticity.rs) exercises directly. Tagging the fixture's `Tet`
    /// connectivity as `P2` therefore forces
    /// `Err(ElasticityFailure::UnsupportedElementOrder(P2))` on every morph
    /// tick, independent of any solver behavior — no need to construct a
    /// pathological FEA system.
    ///
    /// `quality_check`'s tet-only pipeline reads `tet_indices()` (populated
    /// for `Tet` connectivity regardless of `order` — reify-ir
    /// `geometry.rs`) and never reads `order`, so `probe_min_scaled_j`
    /// evaluates this fixture exactly as it would a P1 mesh; only
    /// `elasticity_morph`'s P1 gate rejects it.
    #[test]
    fn run_chain_falls_back_to_fresh_remesh_when_elasticity_morph_returns_err() {
        let fixture = |_param: f64| {
            let mesh = VolumeMesh {
                vertices: unit_tet_vertices(),
                connectivity: VolumeConnectivity::Tet {
                    indices: vec![0, 1, 2, 3],
                    order: ElementOrderTag::P2,
                },
                normals: None,
                boundary: None,
            };
            (mesh, vec![0_u32, 1, 2, 3])
        };

        let opts = MorphOptions::default();
        let report = run_chain(fixture, &[0.0, 1.0], &opts);

        assert_eq!(report.ticks.len(), 2);
        assert!(
            !report.ticks[0].fell_back,
            "tick 0 (chain root) must not be marked fell_back"
        );
        assert!(
            report.ticks[1].fell_back,
            "elasticity_morph must Err on a P2-tagged old_mesh (the \
             element_order gate fires before prescribed_positions is even \
             inspected), forcing run_chain's Err(_) => None fallback"
        );
        assert!(
            (report.ticks[1].min_scaled_j - report.ticks[1].from_scratch_min_scaled_j).abs() < 1e-9,
            "fallback tick's min_scaled_j ({}) must equal its \
             from_scratch_min_scaled_j ({}) — the fallback mesh IS the fresh \
             remesh",
            report.ticks[1].min_scaled_j,
            report.ticks[1].from_scratch_min_scaled_j
        );
    }

    /// Forces `quality_check`'s `HardFail` verdict deterministically via a
    /// fully-Dirichlet-pinned single tet whose target positions are the
    /// inverted (corner-swapped) fixture: with all 4 of the tet's nodes
    /// prescribed, `apply_dirichlet_row_elimination` pins all 12 DOFs
    /// exactly (post-Dirichlet `K = diag(1.0)`, CG converges trivially — the
    /// same all-pinned regime
    /// `elasticity_morph_with_zero_displacement_bcs_on_single_tet_returns_input_positions_within_fp_tolerance`
    /// (elasticity.rs) already exercises with a zero displacement), so
    /// `elasticity_morph` returns `Ok` with `candidate.vertices` equal to
    /// the prescribed (inverted) positions — deterministically producing an
    /// inverted morphed element without depending on any particular
    /// solver-convergence edge case.
    #[test]
    fn run_chain_falls_back_to_fresh_remesh_when_quality_check_hard_fails() {
        let fixture = |param: f64| {
            let vertices = if param == 0.0 {
                unit_tet_vertices()
            } else {
                inverted_tet_vertices()
            };
            let mesh = VolumeMesh {
                vertices,
                connectivity: VolumeConnectivity::Tet {
                    indices: vec![0, 1, 2, 3],
                    order: ElementOrderTag::P1,
                },
                normals: None,
                boundary: None,
            };
            // Every node is "surface" — all 4 are prescribed on the next tick.
            (mesh, vec![0_u32, 1, 2, 3])
        };

        let opts = MorphOptions::default();
        let report = run_chain(fixture, &[0.0, 1.0], &opts);

        assert_eq!(report.ticks.len(), 2);
        assert!(!report.ticks[0].fell_back);
        assert!(
            report.ticks[1].fell_back,
            "a fully-pinned morph to an inverted target must be \
             quality-rejected (HardFail) and fall back to a fresh remesh"
        );
        assert!(
            (report.ticks[1].min_scaled_j - report.ticks[1].from_scratch_min_scaled_j).abs() < 1e-9,
            "fallback tick's min_scaled_j must equal its \
             from_scratch_min_scaled_j — the fallback mesh IS the fresh remesh"
        );
        // The probe's HardFail arm reports a NEGATIVE value (an inversion
        // signal, not a global minimum — see probe_min_scaled_j's doc) for
        // both the rejected candidate and the (also inverted, by fixture
        // construction) fresh remesh.
        assert!(
            report.ticks[1].min_scaled_j < 0.0,
            "expected a negative inversion signal, got {}",
            report.ticks[1].min_scaled_j
        );
    }
}
