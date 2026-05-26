// SPDX-License-Identifier: AGPL-3.0-or-later

//! User-observable signal for FDM γ — the classifier on an axis-aligned
//! box assigns the expected Wall/Skin/Infill zones under stdlib
//! FDMProcess defaults. See `docs/prds/v0_5/fdm-as-printed-fea.md` §C4.

use reify_fdm::{
    AxisAlignedBox, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, Zone, ZoneProcessParams, classify_zone,
};

#[test]
fn classifier_on_box_assigns_expected_zones_for_fdm_defaults() {
    // 40×40×10 mm tall-cap cube. Chosen so the top/bottom skin band can
    // fire without the side wall band: sides 20 mm from centre,
    // top/bottom 5 mm.
    let bx = AxisAlignedBox {
        min: [0.0, 0.0, 0.0],
        max: [0.040, 0.040, 0.010],
    };
    // Stdlib FDMProcess defaults + consumer-derived line_width default.
    let params = ZoneProcessParams {
        walls: 3,
        top_bottom_layers: 4,
        layer_height: 0.0002,
        line_width: 0.0004,
        build_direction: [0.0, 0.0, 1.0],
    };
    let t = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;
    // Derived thresholds (documentary):
    //   wall_thickness = 3 × 0.4mm = 1.2 mm
    //   skin_thickness = 4 × 0.2mm = 0.8 mm

    // (a) centre — side 20 mm > 1.2 mm, top/bottom 5 mm > 0.8 mm → Infill.
    let p_a = [0.020, 0.020, 0.005];
    assert_eq!(
        classify_zone(&bx.build_zone_probe(p_a, &params, t), &params),
        Zone::Infill,
    );

    // (b) near side wall — side 0.3 mm ≤ 1.2 mm → Wall (top/bottom 5 mm irrelevant).
    let p_b = [0.0003, 0.020, 0.005];
    assert_eq!(
        classify_zone(&bx.build_zone_probe(p_b, &params, t), &params),
        Zone::Wall,
    );

    // (c) near top skin — side 20 mm > 1.2 mm, top/bottom 0.5 mm ≤ 0.8 mm → Skin.
    let p_c = [0.020, 0.020, 0.0095];
    assert_eq!(
        classify_zone(&bx.build_zone_probe(p_c, &params, t), &params),
        Zone::Skin,
    );

    // (d) near top corner — side 0.3 mm ≤ 1.2 mm (Wall takes precedence per
    // PRD §C4 sequential order), top/bottom 0.6 mm is irrelevant.
    let p_d = [0.0003, 0.020, 0.0094];
    assert_eq!(
        classify_zone(&bx.build_zone_probe(p_d, &params, t), &params),
        Zone::Wall,
    );
}
