//! Shell-classification + extraction-failure policy and the reify-eval glue
//! that bridges the neutral `reify-solver-elastic` shell driver output into the
//! DSL `ShellChannels` / `ShellStress` value (PRD task δ,
//! `docs/prds/v0_4/shell-extract-engine-bridge.md` §3/§5/§7/§11 OQ-1/OQ-2).
//!
//! This module is the reify-eval-side host for the ShellChannels-production glue
//! (the task names `shell_result.rs`, but `ShellChannels` is defined in
//! reify-eval and the crate dependency direction is `reify-eval →
//! reify-solver-elastic`, so naming it in the solver crate would close a
//! dependency cycle — see the task δ design decisions).
//!
//! ## Wiring
//!
//! - [`classify_shell`] / [`resolve_extraction_failure`] are the routing +
//!   failure-policy site, consumed by BOTH the FEA trampoline's shell branch
//!   (`elastic_static::solve_elastic_static_trampoline`) and the `@optimized`→
//!   ComputeNode lowering (`engine_eval::insert_shell_extract_upstream`, step-12)
//!   so the graph wiring and the actual solve route always agree.
//! - [`build_slab_sdf`] feeds the upstream `shell-extract::extract` ComputeNode a
//!   *synthetic* slab SDF so it `Completes` and satisfies the graph/segmentation
//!   contract; per PRD §11 OQ-2 it is NOT the geometry source for the v0.4 stress
//!   solve (that mesh is synthesized inside `solve_flat_plate_shell`). Consuming
//!   the live extracted mid-surface (OQ-1 per-element persistence; OQ-2 live
//!   producer) is gated on GR-003 and is a follow-up.
//! - [`solve_shell_static`] drives the neutral solver-elastic flat-plate kernel
//!   and wraps its buffers into the DSL `ShellStress`. Its accuracy is the
//!   bare-MITC3 honest band (esc-3594-10 re-spec): max top-channel von Mises
//!   within one order of magnitude of `σ = 6PL/(bh²)`, NOT a tight tolerance.

use std::sync::atomic::AtomicBool;

use reify_ir::{InterpolationKind, SampledField, SampledGridKind, Value};
use reify_solver_elastic::IsotropicElastic;

use super::sampled_stress_field;
use crate::persistent_cache::ShellChannels;

/// Tri-state shell-formulation control — the Rust mirror of the stdlib
/// `ShellForce` enum (`crates/reify-compiler/stdlib/solver_elastic.ri:70`,
/// `param shell_force : ShellForce = ShellForce.Auto`).
///
/// `On` is the proxy for an `@shell` annotation: it forces the shell route and
/// hard-errors on extraction failure (no tet fallback). `Auto` auto-classifies
/// by the thickness/extent ratio and falls back softly. `Off` forces the tet
/// route. See the task δ design decisions (PRD §3 failure-semantics table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellForce {
    /// Force the tet/solid route; never run shell extraction.
    Off,
    /// Auto-classify by `height/min(length,width)` vs `shell_threshold`
    /// (height is the thickness axis by DSL contract); soft tet fallback on
    /// failure.
    Auto,
    /// Force the shell route (proxy for `@shell`); hard-error on failure.
    On,
}

/// Resolved FEA route for a body: shell-kernel assembly vs. tet/solid assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRoute {
    /// Route through the MITC3 shell kernel (`solve_flat_plate_shell`).
    Shell,
    /// Route through the tet/solid path (`solve_cantilever_fea`, task 4084/α).
    Tet,
}

/// What to do when the upstream `shell-extract::extract` step fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Surface the extraction error and abort (no fallback). `ShellForce::On`.
    HardError,
    /// Fall back to tet meshing and emit a warning diagnostic. `Auto`/`Off`.
    TetFallbackWithWarning,
}

/// Classify a body's FEA route from its shell-force setting and geometry.
///
/// - `ShellForce::On`  → always [`ShellRoute::Shell`] (proxy for `@shell`).
/// - `ShellForce::Off` → always [`ShellRoute::Tet`].
/// - `ShellForce::Auto` → [`ShellRoute::Shell`] iff `height / min(length, width)
///   < shell_threshold`, else [`ShellRoute::Tet`]. The comparison is strict `<`,
///   so a ratio exactly equal to the threshold classifies `Tet`.
///
/// # Thickness-axis invariant (esc-3594 suggestion 1)
///
/// `height` is the thickness axis **by DSL contract** — the FEA fn signature is
/// `solve_elastic_static(law, length, width, height, …)` and the shell driver
/// [`reify_solver_elastic::solve_flat_plate_shell`] binds its `thickness`
/// parameter to `height`. Classification therefore divides by `height` on the
/// same axis the solve treats as the thickness, so the route and the solve can
/// never disagree on which axis is through-thickness. The earlier axis-agnostic
/// `min(L,W,H) / median(L,W,H)` metric could classify a body whose thinnest dim
/// was length or width as `Shell`, after which the driver would mesh a
/// wrong-geometry plate (height as thickness) and silently emit bogus stress.
///
/// Dividing by the **smaller** in-plane dimension `min(length, width)` (a shell
/// is thin relative to BOTH in-plane dims) keeps a slender square-section *beam*
/// out of the shell route: a 1000×100×100 mm beam has `height == width`, giving
/// ratio 1.0 → `Tet` (esc-3594-216). A non-positive in-plane dimension
/// (degenerate geometry) classifies `Tet` rather than dividing by zero.
///
/// The fixture body (50 mm × 10 mm × 1 mm: height 1 mm, `min(L,W)` 10 mm,
/// ratio 0.1 < the default threshold 0.2) auto-classifies `Shell` under a bare
/// `ElasticOptions()`; the 1000×100×100 mm cantilever (ratio 1.0) stays `Tet`.
pub fn classify_shell(
    shell_force: ShellForce,
    length: f64,
    width: f64,
    height: f64,
    shell_threshold: f64,
) -> ShellRoute {
    match shell_force {
        ShellForce::On => ShellRoute::Shell,
        ShellForce::Off => ShellRoute::Tet,
        ShellForce::Auto => {
            // `height` IS the thickness axis BY DSL CONTRACT: the FEA fn
            // signature is `solve_elastic_static(law, length, width, height, …)`
            // and the shell driver `solve_flat_plate_shell(length, width,
            // height=thickness, …)` maps height→thickness. Classification MUST
            // use the SAME axis as the solve, or a body whose thinnest dim is
            // length/width would auto-classify Shell and then be solved with the
            // wrong axis as the thickness — physically wrong geometry, silent
            // bogus stress (esc-3594 reviewer suggestion 1).
            //
            // A shell is thin relative to BOTH in-plane dims, so compare the
            // thickness (height) against the SMALLER in-plane dimension
            // `min(length, width)`. This keeps the esc-3594-216 beam-vs-shell
            // split — a slender square-section beam (e.g. 1000×100×100 mm,
            // height==width) has ratio 1.0 → Tet — and classifies a body whose
            // true thin axis is length/width (large height/in-plane ratio) as
            // Tet, since it is not a height-thickness shell. A non-positive
            // in-plane dim (degenerate geometry) classifies Tet rather than
            // dividing by zero.
            let in_plane = length.min(width);
            if in_plane > 0.0 && height / in_plane < shell_threshold {
                ShellRoute::Shell
            } else {
                ShellRoute::Tet
            }
        }
    }
}

/// Resolve the extraction-failure policy from the shell-force setting.
///
/// - `ShellForce::On`  → [`FailurePolicy::HardError`] (proxy for `@shell`:
///   the user explicitly demanded a shell solve, so a failed extraction is a
///   hard error with no silent fallback).
/// - `ShellForce::Auto`/`Off` → [`FailurePolicy::TetFallbackWithWarning`]: a
///   failed (or never-attempted) extraction degrades gracefully to the tet path
///   with a warning diagnostic.
///
/// The user-facing extraction-failure CLI fixtures are owned by task ε; this
/// helper is the policy site (unit-tested here, wired by the engine lowering in
/// step-12).
pub fn resolve_extraction_failure(shell_force: ShellForce) -> FailurePolicy {
    match shell_force {
        ShellForce::On => FailurePolicy::HardError,
        ShellForce::Auto | ShellForce::Off => FailurePolicy::TetFallbackWithWarning,
    }
}

/// Test whether a body is too thick for the shell route, returning the
/// thickness/extent ratio when it is.
///
/// Returns `Some(ratio)` when the body is too thick (where `ratio =
/// height / min(length, width)`, or [`f64::INFINITY`] for degenerate
/// in-plane dimensions), and `None` when the body is thin enough for the
/// shell path.  Both the binary decision and the ratio come from the same
/// call so the message at the dispatch site and the routing decision can
/// never disagree (esc-3837 reviewer suggestion 4).
///
/// # Thickness-axis invariant (esc-3594 suggestion 1 / esc-3837 suggestion 1)
///
/// The decision delegates to [`classify_shell`] with [`ShellForce::Auto`],
/// making the "never disagree" invariant **structurally** true: if
/// [`classify_shell`] ever changes its metric (different divisor, strict `<`
/// boundary, etc.) this helper automatically stays in sync — there is no
/// copy to drift.
///
/// - `Some(ratio)` ↔ `classify_shell(Auto, …) == ShellRoute::Tet`.
/// - `None`        ↔ `classify_shell(Auto, …) == ShellRoute::Shell`.
///
/// `pub(crate)` — called by `elastic_static::solve_elastic_static_trampoline`.
pub(crate) fn is_too_thick_for_shell(
    length: f64,
    width: f64,
    height: f64,
    shell_threshold: f64,
) -> Option<f64> {
    // Delegate to classify_shell for the binary decision.  Structural
    // delegation (not a re-implementation of the criterion) guarantees that
    // the route and the too-thick gate are always consistent.
    let too_thick = classify_shell(ShellForce::Auto, length, width, height, shell_threshold)
        == ShellRoute::Tet;
    if too_thick {
        // Compute the ratio for the error/warning message using the same
        // formula as classify_shell's Auto branch so the displayed value
        // matches the routing decision.
        let in_plane = length.min(width);
        let ratio = if in_plane > 0.0 { height / in_plane } else { f64::INFINITY };
        Some(ratio)
    } else {
        None
    }
}

// ── driver-output → DSL-value glue ───────────────────────────────────────────
//
// These builders bridge the neutral `reify-solver-elastic` flat-plate driver
// output (plain `Vec<f64>` buffers from `flatten_shell_channels`) into the DSL
// `ShellStress` value. `ShellChannels` lives here in reify-eval (the crate
// dependency direction is reify-eval → reify-solver-elastic), so the wrapping
// happens on this side of the seam — no dependency cycle (PRD §11 OQ-2 and the
// task δ design decisions).

/// Wrap the three per-element local-frame stress/frame buffers into a
/// [`ShellChannels`] struct.
///
/// The `mid` layer is deliberately **not** a field of `ShellChannels`: it is
/// routed into the result `stress` field via [`build_mid_stress_field`] so that
/// the I-2 alias `result.stress == result.shell_channels.mid` holds (PRD §3).
/// `top` / `bottom` / `frame` are each `9 * n_elem` long (row-major 3×3 per
/// element, element-major), per [`reify_solver_elastic::flatten_shell_channels`].
pub(crate) fn build_shell_channels(top: Vec<f64>, bottom: Vec<f64>, frame: Vec<f64>) -> ShellChannels {
    ShellChannels { top, bottom, frame }
}

/// Build the mid-surface stress `Value::Field { source: Sampled }` from the
/// flattened mid buffer (`9 * n_elem` row-major 3×3 per element).
///
/// The field is a flat per-element 1D `SampledField` whose single axis grid has
/// exactly `mid.len()` nodes, so its grid node count equals the data length.
/// That is the precondition [`super::elastic_static::shell_channels_to_value`]'s
/// `build_channel_field` checks before cloning this grid for the `top` /
/// `bottom` channels (its `debug_assert_eq!(data.len(), grid_node_count)`).
///
/// This is the single field instance that becomes **both** `result.stress` and
/// `result.shell_channels.mid` (the I-2 alias).
pub(crate) fn build_mid_stress_field(mid: Vec<f64>) -> Value {
    let n = mid.len();
    let axis_grid: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let sf = SampledField {
        name: "shell_channels_mid".to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![0.0],
        bounds_max: vec![n.saturating_sub(1) as f64],
        spacing: vec![1.0],
        axis_grids: vec![axis_grid],
        interpolation: InterpolationKind::Linear,
        data: mid,
        oob_emitted: AtomicBool::new(false),
    };
    // Stress codomain (Tensor<2,3,Pressure>) — matches `solver_elastic.ri:327`
    // and the tet-path `stress` field, so the alias is type-consistent.
    sampled_stress_field(sf)
}

/// Build a synthetic Regular3D slab signed-distance `Value::SampledField` from
/// the body dimensions, for the upstream `shell-extract::extract` ComputeNode.
///
/// The slab is centred on `z = 0` with half-thickness `height / 2`; the SDF is
/// `|z| − height/2` (negative inside the slab, positive outside), sampled on a
/// small structured grid spanning `[0, length] × [0, width] × [−h/2, h/2]`.
/// Mirrors the synthetic-slab construction in γ's
/// `tests/shell_extract_compute_integration.rs` (row-major: z outermost, then y,
/// then x).
///
/// Per PRD §11 OQ-2 this field exists only to drive the upstream
/// `shell-extract::extract` node so it `Completes` and satisfies the
/// graph/segmentation contract; it is **not** the geometry source for the v0.4
/// flat-plate stress solve (that mesh is trampoline-synthesized by
/// [`reify_solver_elastic::solve_flat_plate_shell`]).
//
// Wired into the `@optimized`→ComputeNode lowering (step-12,
// `engine_eval::insert_shell_extract_upstream`) to feed the upstream
// `shell-extract::extract` node's `value_inputs[1]`.
pub(crate) fn build_slab_sdf(length: f64, width: f64, height: f64) -> Value {
    let half_t = 0.5 * height;
    // A modest fixed grid: the field only has to satisfy the shell-extract
    // contract (Regular3D, non-empty axis grids, finite data), not resolve the
    // stress solve.
    const NX: usize = 5;
    const NY: usize = 5;
    const NZ: usize = 3;
    let x_grid: Vec<f64> = (0..NX).map(|i| length * i as f64 / (NX - 1) as f64).collect();
    let y_grid: Vec<f64> = (0..NY).map(|i| width * i as f64 / (NY - 1) as f64).collect();
    let z_grid: Vec<f64> = (0..NZ)
        .map(|i| -half_t + height * i as f64 / (NZ - 1) as f64)
        .collect();

    // Row-major flatten: z outermost, then y, then x (γ's slab convention).
    let mut data = Vec::with_capacity(NX * NY * NZ);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - half_t);
            }
        }
    }

    let sf = SampledField {
        name: "shell_slab_sdf".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -half_t],
        bounds_max: vec![length, width, half_t],
        spacing: vec![
            length / (NX - 1) as f64,
            width / (NY - 1) as f64,
            height / (NZ - 1) as f64,
        ],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    Value::SampledField(sf)
}

/// Rotation-invariant von Mises stress of a 3×3 Cauchy tensor.
///
/// `σ_vm = sqrt(0.5·((σxx−σyy)² + (σyy−σzz)² + (σzz−σxx)² + 6·(σxy²+σyz²+σzx²)))`.
fn von_mises_3x3(t: &[[f64; 3]; 3]) -> f64 {
    let (sxx, syy, szz) = (t[0][0], t[1][1], t[2][2]);
    let (sxy, syz, szx) = (t[0][1], t[1][2], t[2][0]);
    (0.5 * ((sxx - syy).powi(2)
        + (syy - szz).powi(2)
        + (szz - sxx).powi(2)
        + 6.0 * (sxy * sxy + syz * syz + szx * szx)))
        .sqrt()
}

/// Orchestrate a flat-plate MITC3+ shell solve and produce the reify-eval-side
/// outputs the FEA trampoline needs for a shell-classified body.
///
/// Calls [`reify_solver_elastic::solve_flat_plate_shell`] (synthesized
/// `length × width` mid-surface mesh, thickness `height`, `-Z` tip load), then
/// [`reify_solver_elastic::flatten_shell_channels`], and wraps the buffers into
/// the DSL value pieces.
///
/// Returns `(channels, mid_field, max_von_mises, converged, iterations)`:
/// - `channels` — per-element top/bottom/frame ([`ShellChannels`]).
/// - `mid_field` — the mid-surface stress `Value::Field` (becomes both
///   `result.stress` and `result.shell_channels.mid`, the I-2 alias).
/// - `max_von_mises` — max over elements of the von Mises across **all three**
///   through-thickness channels (top/mid/bottom): the body's TRUE peak stress,
///   regardless of channel. This is the `result.max_von_mises` scalar summary
///   and is semantically aligned with the tet path's `fea.max_von_mises` (peak
///   over the solid stress field), so the field means "peak von Mises in the
///   body" on both routes (esc-3594 suggestion 4). For a bending-dominated flat
///   plate the peak rides on the top/bottom fibres; the mid layer is near the
///   neutral plane, but folding it in costs nothing and avoids a route-dependent
///   summary.
/// - `converged` / `iterations` — CG solve status.
//
// `#[allow(dead_code)]`: wired into the FEA trampoline's shell branch by step-10
// (`compute_targets/elastic_static.rs`). Marking this live root also keeps its
// callees (`build_shell_channels`, `build_mid_stress_field`, `von_mises_3x3`)
// from tripping the non-test lib build's dead_code lint before then.
#[allow(dead_code)]
pub(crate) fn solve_shell_static(
    length: f64,
    width: f64,
    height: f64,
    material: &IsotropicElastic,
    tip_force: f64,
) -> (ShellChannels, Value, f64, bool, u32) {
    let solve = reify_solver_elastic::solve_flat_plate_shell(length, width, height, material, tip_force);

    // True peak von Mises over ALL THREE through-thickness channels
    // (top/mid/bottom), so `result.max_von_mises` is the body's actual peak
    // stress regardless of channel — semantically aligned with the tet path's
    // `fea.max_von_mises` (peak over the solid field), per esc-3594 suggestion 4.
    let max_von_mises = solve
        .stresses
        .iter()
        .flat_map(|s| {
            [
                von_mises_3x3(&s.top),
                von_mises_3x3(&s.mid),
                von_mises_3x3(&s.bottom),
            ]
        })
        .fold(0.0_f64, f64::max);

    let (top, mid, bottom, frame) =
        reify_solver_elastic::flatten_shell_channels(&solve.stresses, &solve.frames);

    let channels = build_shell_channels(top, bottom, frame);
    let mid_field = build_mid_stress_field(mid);

    (channels, mid_field, max_von_mises, solve.converged, solve.iterations as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute_targets::elastic_static::shell_channels_to_value;
    use crate::persistent_cache::ShellChannels;
    use reify_ir::{FieldSourceKind, SampledGridKind, Value};

    /// RED (task δ step-5): pin the shell-classification routing policy.
    ///
    /// `classify_shell(shell_force, length, width, height, shell_threshold)`
    /// resolves the FEA route for a body:
    /// - `ShellForce::On`  → always `Shell` (proxy for an `@shell` annotation).
    /// - `ShellForce::Off` → always `Tet`.
    /// - `ShellForce::Auto` → `Shell` iff `height / min(length, width) <
    ///   shell_threshold`, else `Tet`. The comparison is strict `<`, so a ratio
    ///   exactly equal to the threshold classifies `Tet`. `height` is the
    ///   thickness axis by DSL contract, so classification divides on the same
    ///   axis the solve treats as the thickness (esc-3594 suggestion 1);
    ///   dividing by the smaller in-plane dim keeps a slender square-section
    ///   beam out of the shell route (esc-3594-216).
    #[test]
    fn classify_shell_routes_by_force_and_threshold() {
        // Fixture body 50mm × 10mm × 1mm: height=1mm, min(L,W)=10mm,
        // ratio = 1/10 = 0.1 < 0.2 → shell under Auto.
        let (l, w, h) = (0.050_f64, 0.010_f64, 0.001_f64);
        let threshold = 0.2_f64;

        // Forced On always routes Shell regardless of geometry.
        assert_eq!(
            classify_shell(ShellForce::On, l, w, h, threshold),
            ShellRoute::Shell,
            "ShellForce::On must force the shell route on a thin plate"
        );
        assert_eq!(
            classify_shell(ShellForce::On, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Shell,
            "ShellForce::On forces Shell even for a thick (cube-ish) body"
        );

        // Forced Off always routes Tet regardless of geometry.
        assert_eq!(
            classify_shell(ShellForce::Off, l, w, h, threshold),
            ShellRoute::Tet,
            "ShellForce::Off must force the tet route even for a thin plate"
        );

        // Auto + thin plate (height/min(L,W) = 1/10 = 0.1 < 0.2) → Shell.
        assert_eq!(
            classify_shell(ShellForce::Auto, l, w, h, threshold),
            ShellRoute::Shell,
            "Auto with height/min(L,W) ratio 0.1 < 0.2 must classify Shell"
        );

        // Auto + thick body 10×10×8 (height/min(L,W) = 8/10 = 0.8 >= 0.2) → Tet.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Tet,
            "Auto with height/min(L,W) ratio 0.8 >= 0.2 must classify Tet"
        );

        // Boundary: ratio exactly == threshold is NOT < threshold → Tet.
        // 10×10×2 → height/min(L,W) = 2/10 = 0.2 == threshold.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.002, threshold),
            ShellRoute::Tet,
            "Auto at ratio exactly == threshold must classify Tet (strict <)"
        );

        // Regression (esc-3594-216): a slender SQUARE-CROSS-SECTION beam
        // (1000×100×100 mm cantilever: length=1.0, width=height=0.1) has
        // height=100mm, min(L,W)=100mm, ratio = 1.0 >= 0.2 → Tet. The old
        // min/max metric (100/1000 = 0.1) wrongly classified it Shell, breaking
        // the 4084/α tet pins.
        assert_eq!(
            classify_shell(ShellForce::Auto, 1.000, 0.100, 0.100, threshold),
            ShellRoute::Tet,
            "Auto must classify a slender square-section beam as Tet, not Shell"
        );

        // Regression (esc-3594 suggestion 1): `height` is the thickness axis by
        // DSL contract, so a body whose ACTUAL thinnest dim is length or width
        // (NOT height) must NOT auto-classify Shell — otherwise the solve would
        // mesh a wrong-geometry plate treating height as the thickness. A
        // 1mm×50mm×10mm body (length=0.001, width=0.05, height=0.01) has
        // height/min(L,W) = 0.01/0.001 = 10 >= 0.2 → Tet. The old axis-agnostic
        // min/median metric (min=0.001, median=0.01 → 0.1 < 0.2) wrongly
        // classified it Shell.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.001, 0.05, 0.01, threshold),
            ShellRoute::Tet,
            "Auto must classify a body whose thin axis is NOT height as Tet \
             (height is the thickness axis by DSL contract)"
        );
    }

    /// RED (task δ step-5): pin the extraction-failure fallback policy.
    ///
    /// `resolve_extraction_failure(shell_force)` decides what happens when the
    /// upstream shell-extract step fails:
    /// - `ShellForce::On`  → `HardError` (proxy for `@shell`: no fallback).
    /// - `ShellForce::Auto`/`Off` → `TetFallbackWithWarning` (soft fallback).
    #[test]
    fn resolve_extraction_failure_maps_force_to_policy() {
        assert_eq!(
            resolve_extraction_failure(ShellForce::On),
            FailurePolicy::HardError,
            "ShellForce::On must hard-error on extraction failure (no fallback)"
        );
        assert_eq!(
            resolve_extraction_failure(ShellForce::Auto),
            FailurePolicy::TetFallbackWithWarning,
            "ShellForce::Auto must fall back to tet meshing with a warning"
        );
        assert_eq!(
            resolve_extraction_failure(ShellForce::Off),
            FailurePolicy::TetFallbackWithWarning,
            "ShellForce::Off must not hard-error (never attempts shell extraction)"
        );
    }

    // ── step-7 RED: driver-output → DSL-value glue ──────────────────────────
    //
    // Pin the three reify-eval glue builders that bridge the neutral
    // `reify-solver-elastic` flat-plate driver output into the DSL ShellStress
    // value, and their interaction with the existing 4067-shipped
    // `shell_channels_to_value` mapping helper:
    //   (a) build_shell_channels(top, bottom, frame) -> ShellChannels
    //       — wraps the three per-element local-frame buffers into the struct.
    //         `mid` is deliberately NOT a struct field: it is routed into the
    //         stress field (build_mid_stress_field), per PRD §3 (result.stress
    //         aliases ShellStress.mid).
    //   (b) build_mid_stress_field(mid) -> Value::Field{Sampled}
    //       — a flat per-element field of len 9*n whose axis-grid node count
    //         equals the data length, so build_channel_field can clone its grid
    //         for the top/bottom channels.
    //   (c) shell_channels_to_value(&Some(ch), &mid_field) consumes both and
    //       yields a "ShellStress" StructureInstance with mid==mid_field and
    //       finite top/bottom Sampled fields of data length 9*n.
    //   (d) build_slab_sdf(L, W, H) -> Value::SampledField
    //       — a Regular3D SDF accepted by the shell-extract::extract contract.
    //
    // RED: build_shell_channels / build_mid_stress_field / build_slab_sdf do
    // not exist yet, so this module fails to compile.

    /// Extract the `SampledField.data` vec from a `Value::Field { Sampled }`,
    /// panicking on any other shape. Mirrors the `extract_sampled_field_data`
    /// idiom in `tests/solve_elastic_static_e2e.rs`.
    fn sampled_data(v: &Value) -> Vec<f64> {
        match v {
            Value::Field { source, lambda, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "expected a Sampled field source, got {source:?}"
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => sf.data.clone(),
                    other => panic!("field lambda must be Value::SampledField, got {other:?}"),
                }
            }
            other => panic!("expected Value::Field, got {other:?}"),
        }
    }

    /// (a) `build_shell_channels` routes the three local-frame buffers into the
    /// `ShellChannels` struct verbatim; `mid` has no home in the struct.
    #[test]
    fn build_shell_channels_routes_buffers_into_struct() {
        let n = 2usize; // 2 elements → 9 f64 per element per channel.
        let top: Vec<f64> = (0..9 * n).map(|i| i as f64 + 1.0).collect();
        let bottom: Vec<f64> = (0..9 * n).map(|i| i as f64 + 100.0).collect();
        let frame: Vec<f64> = (0..9 * n).map(|i| i as f64 * 0.5).collect();

        let ch: ShellChannels = build_shell_channels(top.clone(), bottom.clone(), frame.clone());

        assert_eq!(ch.top, top, "top buffer must pass through unchanged");
        assert_eq!(ch.bottom, bottom, "bottom buffer must pass through unchanged");
        assert_eq!(ch.frame, frame, "frame buffer must pass through unchanged");
    }

    /// (b) `build_mid_stress_field` wraps the mid buffer as a flat Sampled
    /// field whose grid node count == data length (so the channel grid clones).
    #[test]
    fn build_mid_stress_field_wraps_mid_as_flat_sampled_field() {
        let n = 3usize;
        let mid: Vec<f64> = (0..9 * n).map(|i| (i as f64) * 1.5 - 4.0).collect();

        let field = build_mid_stress_field(mid.clone());

        match &field {
            Value::Field { source, lambda, .. } => {
                assert!(
                    matches!(source, FieldSourceKind::Sampled),
                    "mid field must be a Sampled source, got {source:?}"
                );
                match lambda.as_ref() {
                    Value::SampledField(sf) => {
                        assert_eq!(
                            sf.data, mid,
                            "mid field data must equal the input buffer bit-for-bit"
                        );
                        assert_eq!(
                            sf.data.len(),
                            9 * n,
                            "mid field is a flat per-element field of len 9*n"
                        );
                        let node_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
                        assert_eq!(
                            node_count,
                            9 * n,
                            "axis-grid node count must equal data len so build_channel_field's \
                             length check passes when top/bottom reuse this grid"
                        );
                        assert!(!sf.axis_grids.is_empty(), "axis_grids must be non-empty");
                    }
                    other => panic!("mid field lambda must be Value::SampledField, got {other:?}"),
                }
            }
            other => panic!("build_mid_stress_field must return Value::Field, got {other:?}"),
        }
    }

    /// (c) The two glue outputs feed the existing `shell_channels_to_value`,
    /// producing a "ShellStress" StructureInstance with the mid alias and
    /// finite top/bottom channels of length 9*n.
    #[test]
    fn glue_outputs_feed_shell_channels_to_value() {
        let n = 4usize;
        let top: Vec<f64> = (0..9 * n).map(|i| i as f64 + 1.0).collect();
        let mid: Vec<f64> = (0..9 * n).map(|i| i as f64 + 1000.0).collect();
        let bottom: Vec<f64> = (0..9 * n).map(|i| i as f64 + 9000.0).collect();
        let frame: Vec<f64> = (0..9 * n).map(|i| i as f64 * 0.25).collect();

        let channels = build_shell_channels(top.clone(), bottom.clone(), frame);
        let mid_field = build_mid_stress_field(mid.clone());

        let value = shell_channels_to_value(&Some(channels), &mid_field);

        let data = match &value {
            Value::StructureInstance(d) => d,
            other => panic!("shell_channels_to_value must return a StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name.as_str(),
            "ShellStress",
            "shell channels value must be a ShellStress instance"
        );

        // .mid is the mid field bit-for-bit (it is the alias source for
        // result.stress — the I-2 invariant).
        let mid_v = data
            .fields
            .get(&"mid".to_string())
            .expect("ShellStress must carry a `mid` field");
        assert_eq!(
            sampled_data(mid_v),
            mid,
            "ShellStress.mid must carry the mid buffer bit-for-bit"
        );

        // .top / .bottom are finite Sampled fields whose data length == 9*n,
        // reusing the mid grid (build_channel_field).
        let top_data = sampled_data(
            data.fields
                .get(&"top".to_string())
                .expect("ShellStress must carry a `top` field"),
        );
        let bottom_data = sampled_data(
            data.fields
                .get(&"bottom".to_string())
                .expect("ShellStress must carry a `bottom` field"),
        );
        assert_eq!(top_data.len(), 9 * n, "top channel data length must be 9*n");
        assert_eq!(bottom_data.len(), 9 * n, "bottom channel data length must be 9*n");
        assert!(
            top_data.iter().all(|x| x.is_finite()),
            "top channel must be all-finite"
        );
        assert!(
            bottom_data.iter().all(|x| x.is_finite()),
            "bottom channel must be all-finite"
        );
        assert_eq!(top_data, top, "top channel data must equal the top buffer");
        assert_eq!(
            bottom_data, bottom,
            "bottom channel data must equal the bottom buffer"
        );
    }

    /// (d) `build_slab_sdf` returns a Regular3D Sampled SDF accepted by the
    /// shell-extract::extract trampoline contract (non-empty axis grids, finite
    /// signed-distance data, row-major data length == grid node count).
    #[test]
    fn build_slab_sdf_is_regular3d_sampled_field() {
        // Fixture body dims (SI metres): 50mm × 10mm × 1mm.
        let value = build_slab_sdf(0.050, 0.010, 0.001);
        match &value {
            Value::SampledField(sf) => {
                assert!(
                    matches!(sf.kind, SampledGridKind::Regular3D),
                    "slab SDF must be a Regular3D field, got {:?}",
                    sf.kind
                );
                assert_eq!(sf.axis_grids.len(), 3, "Regular3D field has three axis grids");
                assert!(
                    sf.axis_grids.iter().all(|g| !g.is_empty()),
                    "every axis grid must be non-empty (shell-extract contract)"
                );
                assert!(!sf.data.is_empty(), "signed-distance data must be non-empty");
                assert!(
                    sf.data.iter().all(|d| d.is_finite()),
                    "all signed-distance samples must be finite"
                );
                let node_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
                assert_eq!(
                    sf.data.len(),
                    node_count,
                    "data length must equal the product of axis-grid lengths (row-major grid)"
                );
            }
            other => panic!("build_slab_sdf must return Value::SampledField, got {other:?}"),
        }
    }

    // ── step-3 RED (task ε #3837): is_too_thick_for_shell helper ────────────
    //
    // Pins the `is_too_thick_for_shell(length, width, height, shell_threshold)
    // -> Option<f64>` helper that delegates to `classify_shell` for the binary
    // decision and returns the ratio for use in the dispatch-site message:
    //   thin  ↔  classify_shell(Auto, …) == Shell → None (not too thick)
    //   thick ↔  classify_shell(Auto, …) == Tet   → Some(ratio)
    //
    // Degenerate (non-positive in-plane dim) → Some(INFINITY) (cannot shell-mesh).
    // Boundary ratio == threshold → Some(ratio) (NOT thin under strict `<`).
    //
    // RED: `is_too_thick_for_shell` does not exist yet → compile fail.
    // GREEN after step-4 adds the helper to this module.
    //
    // NOTE (esc-3837 amendment, suggestions 1+4): the return type was changed
    // from `bool` to `Option<f64>` so the dispatch site can use the ratio from
    // one source without re-deriving `length.min(width)` locally.

    /// `is_too_thick_for_shell` correctly classifies thin vs. thick bodies and
    /// returns the thickness/extent ratio for thick bodies.
    ///
    /// - Thin flexure (50×10×1 mm, ratio 0.1 < threshold 0.2) → `None`
    ///   (not too thick; the shell path is valid for this body).
    /// - Thick block (50×20×20 mm, ratio 1.0 ≥ threshold 0.2) → `Some(1.0)`
    ///   (too thick; shell route would be inappropriate).
    /// - Boundary case (ratio == threshold) → `Some(ratio)` (classify_shell's
    ///   strict `<` makes ratio==threshold a Tet → helper must agree: too thick).
    /// - Degenerate (non-positive in-plane dimension) → `Some(INFINITY)` (cannot
    ///   shell-mesh a body with zero or negative in-plane extent).
    #[test]
    fn is_too_thick_for_shell_classifies_correctly() {
        // Thin flexure: height=1mm, min(L,W)=10mm → ratio 0.1 < 0.2 → NOT too thick
        assert!(
            is_too_thick_for_shell(0.050, 0.010, 0.001, 0.2).is_none(),
            "thin flexure (ratio 0.1 < 0.2) must return None (not too thick)"
        );
        // Thick block: height=20mm, min(L,W)=20mm → ratio 1.0 ≥ 0.2 → too thick
        assert_eq!(
            is_too_thick_for_shell(0.050, 0.020, 0.020, 0.2),
            Some(1.0),
            "thick block (ratio 1.0 ≥ 0.2) must return Some(1.0)"
        );
        // Boundary: height/min(L,W) == threshold exactly → too thick
        // (classify_shell uses strict `<`, so == threshold routes Tet)
        let boundary = is_too_thick_for_shell(0.010, 0.010, 0.002, 0.2);
        assert!(
            boundary.is_some(),
            "boundary ratio==threshold must return Some(_) (not strictly thin)"
        );
        // Degenerate: zero in-plane dimension → Some(INFINITY)
        let degen_zero = is_too_thick_for_shell(0.050, 0.0, 0.001, 0.2);
        assert_eq!(
            degen_zero,
            Some(f64::INFINITY),
            "zero width (degenerate in-plane) must return Some(INFINITY)"
        );
        // Degenerate: negative in-plane dimension → Some(INFINITY)
        let degen_neg = is_too_thick_for_shell(0.050, -1.0, 0.001, 0.2);
        assert_eq!(
            degen_neg,
            Some(f64::INFINITY),
            "negative width (degenerate in-plane) must return Some(INFINITY)"
        );
    }
}
