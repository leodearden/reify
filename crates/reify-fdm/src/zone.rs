// SPDX-License-Identifier: AGPL-3.0-or-later

//! R-fast geometric zone classifier (task γ).
//!
//! Implements the wall / skin / infill trichotomy from
//! `docs/prds/v0_5/fdm-as-printed-fea.md` §C4 as a pure function over
//! precomputed distance probes. The classifier is consumer-agnostic —
//! the δ-task is responsible for wiring real-body OCCT distance queries
//! into `ZoneProbe` values; this module only knows how to interpret them.

#[cfg(test)]
mod tests {
    use super::*;

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
        let params = ZoneProcessParams {
            walls: 3,
            top_bottom_layers: 4,
            layer_height: 0.0002,
            line_width: 0.0004,
            build_direction: [0.0, 0.0, 1.0],
        };
        assert_eq!(classify_zone(&probe, &params), Zone::Infill);
    }
}
