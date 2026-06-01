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
    /// Auto-classify by `shell_threshold`; soft tet fallback on failure.
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
/// - `ShellForce::Auto` → [`ShellRoute::Shell`] iff `thickness / extent <
///   shell_threshold`, else [`ShellRoute::Tet`], where `thickness = min(L,W,H)`
///   and `extent = max(L,W,H)`. The comparison is strict `<`, so a ratio exactly
///   equal to the threshold classifies `Tet`.
///
/// A non-positive `extent` (degenerate geometry) classifies `Tet` rather than
/// dividing by zero.
///
/// The fixture body (50 mm × 10 mm × 1 mm, ratio 0.02 < the default threshold
/// 0.2) auto-classifies `Shell` under a bare `ElasticOptions()`.
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
            let thickness = length.min(width).min(height);
            let extent = length.max(width).max(height);
            if extent > 0.0 && thickness / extent < shell_threshold {
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
    /// - `ShellForce::Auto` → `Shell` iff `thickness/extent < shell_threshold`
    ///   (`thickness = min(L,W,H)`, `extent = max(L,W,H)`), else `Tet`.
    ///   The comparison is strict `<`, so a ratio exactly equal to the
    ///   threshold classifies `Tet`.
    #[test]
    fn classify_shell_routes_by_force_and_threshold() {
        // Fixture body 50mm × 10mm × 1mm: thickness=1mm, extent=50mm,
        // ratio = 1/50 = 0.02 < 0.2 → shell under Auto.
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

        // Auto + thin plate (ratio 0.02 < 0.2) → Shell.
        assert_eq!(
            classify_shell(ShellForce::Auto, l, w, h, threshold),
            ShellRoute::Shell,
            "Auto with thickness/extent ratio 0.02 < 0.2 must classify Shell"
        );

        // Auto + thick body 10×10×8 (ratio 8/10 = 0.8 >= 0.2) → Tet.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.008, threshold),
            ShellRoute::Tet,
            "Auto with thickness/extent ratio 0.8 >= 0.2 must classify Tet"
        );

        // Boundary: ratio exactly == threshold is NOT < threshold → Tet.
        // 10×10×2 → ratio 2/10 = 0.2 == threshold.
        assert_eq!(
            classify_shell(ShellForce::Auto, 0.010, 0.010, 0.002, threshold),
            ShellRoute::Tet,
            "Auto at ratio exactly == threshold must classify Tet (strict <)"
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
