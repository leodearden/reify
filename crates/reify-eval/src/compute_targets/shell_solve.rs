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
    /// Auto-classify by `thickness/median` vs `shell_threshold`; soft tet
    /// fallback on failure.
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
/// - `ShellForce::Auto` → [`ShellRoute::Shell`] iff `thickness / median <
///   shell_threshold`, else [`ShellRoute::Tet`], where `thickness = min(L,W,H)`
///   and `median` is the middle of the three sorted dimensions. The comparison
///   is strict `<`, so a ratio exactly equal to the threshold classifies `Tet`.
///
/// The metric divides by the **median** dimension, not the max extent (`max`),
/// so a slender square-cross-section *beam* is not misclassified as a shell.
/// A shell is thin relative to its *two* in-plane dimensions, so `thickness`
/// must be small relative to the median (second-largest) dimension; a beam
/// (e.g. 100×100×1000 mm) has `thickness == median`, giving ratio 1.0 → `Tet`.
/// The earlier `thickness / extent` (min/max) metric only required thinness vs.
/// a *single* dimension, which a slender beam also satisfies (esc-3594-216).
///
/// A non-positive `median` (degenerate geometry) classifies `Tet` rather than
/// dividing by zero.
///
/// The fixture body (50 mm × 10 mm × 1 mm: thickness 1 mm, median 10 mm,
/// ratio 0.1 < the default threshold 0.2) auto-classifies `Shell` under a bare
/// `ElasticOptions()`; the 100×100×1000 mm cantilever (ratio 1.0) stays `Tet`.
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
            // Median = middle of the three sorted dimensions. A shell is thin
            // relative to BOTH in-plane dims, so compare thickness (the min)
            // against the median rather than the max extent.
            let mut dims = [length, width, height];
            dims.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let thickness = dims[0];
            let median = dims[1];
            if median > 0.0 && thickness / median < shell_threshold {
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
// `#[allow(dead_code)]`: wired into the `@optimized`→ComputeNode lowering by
// step-12 (`engine_eval.rs`) to feed the upstream `shell-extract::extract`
// node. Until then it is reachable only from the `#[cfg(test)]` module, which
// the non-test lib build does not see.
#[allow(dead_code)]
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
/// - `max_von_mises` — max over elements of the **top**-channel von Mises (peak
///   bending at the clamped root); the `result.max_von_mises` scalar summary.
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

    let max_von_mises = solve
        .stresses
        .iter()
        .map(|s| von_mises_3x3(&s.top))
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
    /// - `ShellForce::Auto` → `Shell` iff `thickness/median < shell_threshold`
    ///   (`thickness = min(L,W,H)`, `median` = middle sorted dim), else `Tet`.
    ///   The comparison is strict `<`, so a ratio exactly equal to the
    ///   threshold classifies `Tet`. (The metric divides by the median rather
    ///   than the max extent so a slender beam is not misread as a shell —
    ///   esc-3594-216.)
    #[test]
    fn classify_shell_routes_by_force_and_threshold() {
        // Fixture body 50mm × 10mm × 1mm: thickness=1mm, median=10mm,
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

        // Auto + thin plate (thickness/median = 1/10 = 0.1 < 0.2) → Shell.
        assert_eq!(
            classify_shell(ShellForce::Auto, l, w, h, threshold),
            ShellRoute::Shell,
            "Auto with thickness/median ratio 0.1 < 0.2 must classify Shell"
        );

        // Auto + thick body 10×10×8 (thickness/median = 8/10 = 0.8 >= 0.2) → Tet.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Tet,
            "Auto with thickness/median ratio 0.8 >= 0.2 must classify Tet"
        );

        // Boundary: ratio exactly == threshold is NOT < threshold → Tet.
        // 10×10×2 → thickness/median = 2/10 = 0.2 == threshold.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.002, threshold),
            ShellRoute::Tet,
            "Auto at ratio exactly == threshold must classify Tet (strict <)"
        );

        // Regression (esc-3594-216): a slender SQUARE-CROSS-SECTION beam
        // (100×100×1000 mm cantilever) has thickness=100mm, median=100mm,
        // ratio = 1.0 >= 0.2 → Tet. The old min/max metric (100/1000 = 0.1)
        // wrongly classified it Shell, breaking the 4084/α tet pins.
        assert_eq!(
            classify_shell(ShellForce::Auto, 1.000, 0.100, 0.100, threshold),
            ShellRoute::Tet,
            "Auto must classify a slender square-section beam as Tet, not Shell"
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
}
