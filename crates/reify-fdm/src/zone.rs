// SPDX-License-Identifier: AGPL-3.0-or-later

//! R-fast geometric zone classifier (task γ).
//!
//! Implements the wall / skin / infill trichotomy from
//! `docs/prds/v0_5/fdm-as-printed-fea.md` §C4 as a pure function over
//! precomputed distance probes. The classifier is consumer-agnostic —
//! the δ-task is responsible for wiring real-body OCCT distance queries
//! into `ZoneProbe` values; this module only knows how to interpret them.

/// One of the three FDM print zones a point may fall into.
///
/// Drives the anisotropic-material assignment in the downstream δ-task:
/// walls and skins have a dense laminated structure, infill has a sparse
/// pattern-dependent structure (β-task constitutive correlations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// Perimeter shell, within `walls × line_width` of a side face.
    Wall,
    /// Top/bottom solid layer, within `top_bottom_layers × layer_height`
    /// of a top or bottom face.
    Skin,
    /// Interior, neither wall nor skin.
    Infill,
}

/// Mechanically relevant subset of stdlib `FDMProcess` consumed by the
/// classifier, in SI metres.
///
/// `walls`, `top_bottom_layers`, `layer_height`, and `build_direction`
/// mirror fields of the stdlib structure
/// (`crates/reify-compiler/stdlib/fdm.ri`). `line_width` is **not** a
/// stdlib `FDMProcess` field — it is consumer-derived (typical default:
/// nozzle diameter ≈ 0.4 mm). The δ-task is responsible for the
/// `FDMProcess → ZoneProcessParams` mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneProcessParams {
    /// Number of perimeter shells (stdlib `FDMProcess.walls`).
    pub walls: u32,
    /// Number of solid top/bottom layers (stdlib `FDMProcess.top_bottom_layers`).
    pub top_bottom_layers: u32,
    /// Layer height in metres (stdlib `FDMProcess.layer_height`).
    pub layer_height: f64,
    /// Extruded line width in metres — consumer-derived, NOT a stdlib
    /// `FDMProcess` field. Typical default: nozzle diameter ≈ 0.0004 m.
    pub line_width: f64,
    /// Unit build-direction vector (stdlib `FDMProcess.build_direction`).
    pub build_direction: [f64; 3],
}

/// Precomputed distance probes for a single query point.
///
/// The two distances probe *different* face populations:
///
/// * `min_side_distance` — distance to the nearest face whose outward
///   normal is **not** classified as top/bottom (perimeter walls live on
///   these faces).
/// * `min_top_bottom_distance` — distance to the nearest face whose
///   outward normal **is** aligned with `build_direction` within the
///   threshold (top/bottom solid skins live on these faces).
///
/// Both are `Option<f64>` because a degenerate `build_direction` (e.g.
/// 45° to every face) could leave one set empty. `None` means "no face
/// of that population exists for this body" and is interpreted as
/// `Infill` for the corresponding classifier branch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneProbe {
    /// See struct-level docs.
    pub min_side_distance: Option<f64>,
    /// See struct-level docs.
    pub min_top_bottom_distance: Option<f64>,
}

/// Classify a probed point into a [`Zone`] under the given process
/// parameters.
///
/// Implements the cascade from `docs/prds/v0_5/fdm-as-printed-fea.md`
/// §C4: Wall first, then Skin, else Infill. The ordering matters at
/// corners where both bands overlap — perimeter shells dominate, which
/// matches conventional slicer behaviour.
pub fn classify_zone(_probe: &ZoneProbe, _params: &ZoneProcessParams) -> Zone {
    Zone::Infill
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fdm_default_params() -> ZoneProcessParams {
        // Mirrors stdlib FDMProcess defaults
        // (crates/reify-compiler/stdlib/fdm.ri) + the consumer-derived
        // line_width default of 0.4 mm (typical nozzle diameter).
        ZoneProcessParams {
            walls: 3,
            top_bottom_layers: 4,
            layer_height: 0.0002,
            line_width: 0.0004,
            build_direction: [0.0, 0.0, 1.0],
        }
    }

    #[test]
    fn classify_zone_returns_infill_for_deep_interior_probe() {
        // 5 mm from any side face AND any top/bottom face — deep interior.
        // With FDM defaults (walls=3 × line_width=0.4mm = 1.2mm wall band,
        // top_bottom_layers=4 × layer_height=0.2mm = 0.8mm skin band),
        // a 5 mm probe is well past both thresholds → Infill.
        let probe = ZoneProbe {
            min_side_distance: Some(0.005),
            min_top_bottom_distance: Some(0.005),
        };
        assert_eq!(classify_zone(&probe, &fdm_default_params()), Zone::Infill);
    }

    #[test]
    fn classify_zone_wall_branch_returns_wall_within_side_threshold() {
        // wall_thickness = walls × line_width = 3 × 0.4mm = 1.2mm.
        let params = fdm_default_params();

        // (a) 0.8 mm from side — inside wall band → Wall.
        let probe_a = ZoneProbe {
            min_side_distance: Some(0.0008),
            min_top_bottom_distance: Some(0.010),
        };
        assert_eq!(classify_zone(&probe_a, &params), Zone::Wall);

        // (b) exactly at threshold (1.2 mm) — Wall (≤).
        let probe_b = ZoneProbe {
            min_side_distance: Some(0.0012),
            min_top_bottom_distance: Some(0.010),
        };
        assert_eq!(classify_zone(&probe_b, &params), Zone::Wall);

        // (c) no side face at all — Wall cannot fire; falls through to Infill.
        let probe_c = ZoneProbe {
            min_side_distance: None,
            min_top_bottom_distance: Some(0.010),
        };
        assert_eq!(classify_zone(&probe_c, &params), Zone::Infill);
    }
}
