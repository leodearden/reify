// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration: the user-observable ζ signal (PRD §"Boundary-test sketch",
//! Toolpath-roles row).
//!
//! Parse a committed PrusaSlicer-vocabulary G-code fixture into a [`Toolpath`]
//! and assert the slice-2 signal: every bead carries a real role + a positive
//! width + a positive layer-Z; perimeter, skin (solid infill), and sparse
//! infill bead counts are all > 0; the toolpath spans ≥2 layers with strictly
//! increasing layer-Z; in-layer and inter-layer adjacency are both non-empty;
//! and the parse succeeds despite the fixture's many unknown G/M codes (G21,
//! G28, M73, M201/M204, M115) and free-text comments.
//!
//! The fixture is hand-authored in faithful PrusaSlicer GCodeViewer comment
//! vocabulary (no live slicer — that is task η's precondition, not ζ's).

use reify_fdm::{BeadRole, parse_prusaslicer_gcode};

const BRACKET: &str = include_str!("fixtures/prusaslicer_bracket.gcode");

#[test]
fn bracket_fixture_yields_populated_toolpath() {
    let tp = parse_prusaslicer_gcode(BRACKET)
        .expect("fixture must parse despite unknown G/M codes + free-text comments");

    assert!(!tp.beads.is_empty(), "fixture produced beads");

    // Every bead: a real (non-placeholder) width/height and a positive layer-Z,
    // and an actual polyline (≥2 points).
    for (i, b) in tp.beads.iter().enumerate() {
        assert!(b.width > 0.0, "bead {i} width must be > 0, got {}", b.width);
        assert!(b.height > 0.0, "bead {i} height must be > 0, got {}", b.height);
        assert!(b.layer_z > 0.0, "bead {i} layer_z must be > 0, got {}", b.layer_z);
        assert!(
            b.centerline.len() >= 2,
            "bead {i} must be a real polyline, got {} points",
            b.centerline.len()
        );
    }

    // Role coverage: perimeter, skin (solid infill), and sparse infill present.
    let count = |role: BeadRole| tp.beads.iter().filter(|b| b.role == role).count();
    assert!(count(BeadRole::Perimeter) > 0, "≥1 perimeter bead");
    assert!(count(BeadRole::SolidInfill) > 0, "≥1 solid-infill (skin) bead");
    assert!(count(BeadRole::SparseInfill) > 0, "≥1 sparse-infill bead");

    // ≥2 layers, contiguous indices, strictly increasing z.
    assert!(tp.layers.len() >= 2, "≥2 layers, got {}", tp.layers.len());
    for (k, w) in tp.layers.windows(2).enumerate() {
        assert_eq!(w[1].index, w[0].index + 1, "layer indices contiguous at {k}");
        assert!(
            w[1].z > w[0].z,
            "layer z strictly increasing: {} then {}",
            w[0].z,
            w[1].z
        );
    }

    // Adjacency populated on both axes.
    assert!(
        !tp.in_layer_adjacency.is_empty(),
        "non-empty in-layer adjacency"
    );
    assert!(
        !tp.inter_layer_adjacency.is_empty(),
        "non-empty inter-layer adjacency"
    );
}
