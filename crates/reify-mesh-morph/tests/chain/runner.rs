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
        // HardFail short-circuits after the first inverted element; its
        // (negative) jacobian signals the inversion numerically — mirrors
        // `calibration/sweep.rs::extract_metrics`.
        QualityVerdict::HardFail(d) => d.jacobian,
        // Pass only happens for an empty mesh — defensive fallback (callers
        // are not expected to pass empty meshes).
        QualityVerdict::Pass => 0.0,
        // The chain fixtures (plate-with-hole, bracket) are always tet.
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
