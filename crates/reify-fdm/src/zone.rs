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
/// Default cosine threshold for the top/bottom-vs-side face test:
/// cos(45°) = √2/2.
///
/// Per `docs/prds/v0_5/fdm-as-printed-fea.md` open-Q2, the R-fast tier
/// uses a normal-vs-build-direction threshold to decide whether a face
/// belongs to the top/bottom (solid-skin) population or the side
/// (perimeter-wall) population. 45° is the natural midpoint — a face
/// counts as top/bottom when its outward normal is within 45° of the
/// build axis. Downstream callers can pass a different threshold per
/// call for geometries where the default is unsuitable.
pub const DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Returns true when `normal` is aligned with `build_direction` within
/// the given cosine threshold (`|normal · build_direction| ≥
/// cos_threshold`).
///
/// Both vectors are assumed to be unit-length; it is the caller's
/// responsibility to normalise. The absolute value lets both the top
/// face (normal parallel to `+build_direction`) and the bottom face
/// (normal anti-parallel) count as "top/bottom".
///
/// Use [`DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD`] as a sensible default.
pub fn is_top_or_bottom_normal(
    normal: [f64; 3],
    build_direction: [f64; 3],
    cos_threshold: f64,
) -> bool {
    debug_assert!(
        cos_threshold > 0.0 && cos_threshold <= 1.0,
        "cos_threshold must be in (0, 1]; got {cos_threshold}"
    );
    let dot = normal[0] * build_direction[0]
        + normal[1] * build_direction[1]
        + normal[2] * build_direction[2];
    dot.abs() >= cos_threshold
}

/// Axis-aligned bounding box, used as a test fixture / R-fast analytic
/// helper for zone probing.
///
/// NOT a kernel-geometry handle wrapper — it carries no topology, just
/// the two corner points in SI metres. Real-body distance probes
/// (OCCT-backed) live downstream in the δ-task; this helper keeps γ's
/// integration test self-contained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisAlignedBox {
    /// Lower corner (component-wise minimum).
    pub min: [f64; 3],
    /// Upper corner (component-wise maximum).
    pub max: [f64; 3],
}

impl AxisAlignedBox {
    /// Distance from `p` to the nearest face whose outward normal counts
    /// as top/bottom under [`is_top_or_bottom_normal`] with the given
    /// threshold. Returns `None` if no axis-aligned face qualifies.
    pub fn min_top_bottom_distance(
        &self,
        p: [f64; 3],
        build_direction: [f64; 3],
        cos_threshold: f64,
    ) -> Option<f64> {
        let mut best: Option<f64> = None;
        for axis in 0..3 {
            for (coord, sign) in [(self.min[axis], -1.0_f64), (self.max[axis], 1.0_f64)] {
                let mut normal = [0.0_f64; 3];
                normal[axis] = sign;
                if !is_top_or_bottom_normal(normal, build_direction, cos_threshold) {
                    continue;
                }
                let d = (p[axis] - coord).abs();
                best = Some(match best {
                    Some(b) if b < d => b,
                    _ => d,
                });
            }
        }
        best
    }
}

pub fn classify_zone(probe: &ZoneProbe, params: &ZoneProcessParams) -> Zone {
    let wall_thickness = params.walls as f64 * params.line_width;
    if let Some(d) = probe.min_side_distance {
        if d <= wall_thickness {
            return Zone::Wall;
        }
    }
    let skin_thickness = params.top_bottom_layers as f64 * params.layer_height;
    if let Some(d) = probe.min_top_bottom_distance {
        if d <= skin_thickness {
            return Zone::Skin;
        }
    }
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

    #[test]
    fn classify_zone_skin_branch_and_none_handling() {
        // skin_thickness = top_bottom_layers × layer_height = 4 × 0.2mm = 0.8mm.
        // Side distance set to 10mm in all four cases so Wall cannot fire.
        let params = fdm_default_params();

        // (a) 0.5 mm from top/bottom — inside skin band → Skin.
        let probe_a = ZoneProbe {
            min_side_distance: Some(0.010),
            min_top_bottom_distance: Some(0.0005),
        };
        assert_eq!(classify_zone(&probe_a, &params), Zone::Skin);

        // (b) exactly at threshold (0.8 mm) — Skin (≤).
        let probe_b = ZoneProbe {
            min_side_distance: Some(0.010),
            min_top_bottom_distance: Some(0.0008),
        };
        assert_eq!(classify_zone(&probe_b, &params), Zone::Skin);

        // (c) no top/bottom face at all — Skin cannot fire; falls through to Infill.
        let probe_c = ZoneProbe {
            min_side_distance: Some(0.010),
            min_top_bottom_distance: None,
        };
        assert_eq!(classify_zone(&probe_c, &params), Zone::Infill);

        // (d) 5 mm from top/bottom — past skin band → Infill.
        let probe_d = ZoneProbe {
            min_side_distance: Some(0.010),
            min_top_bottom_distance: Some(0.005),
        };
        assert_eq!(classify_zone(&probe_d, &params), Zone::Infill);
    }

    /// Tolerance for AxisAlignedBox floating-point distance assertions.
    /// Tight (1e-12) — these distances are computed by a single subtraction
    /// + abs() of doubles with no accumulation.
    const EPS: f64 = 1e-12;

    fn assert_approx_eq(actual: Option<f64>, expected: f64) {
        let a = actual.expect("expected Some(_) distance");
        assert!(
            (a - expected).abs() < EPS,
            "actual {a} != expected {expected}"
        );
    }

    #[test]
    fn axis_aligned_box_min_top_bottom_distance() {
        // 40×40×10 mm tall-cap cube; Z is the build axis.
        let bx = AxisAlignedBox {
            min: [0.0, 0.0, 0.0],
            max: [0.040, 0.040, 0.010],
        };
        let build_z = [0.0, 0.0, 1.0];
        let t = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;

        // (a) center — 5 mm to top or bottom; side faces excluded.
        assert_approx_eq(
            bx.min_top_bottom_distance([0.020, 0.020, 0.005], build_z, t),
            0.005,
        );

        // (b) near +Z face — 1 mm.
        assert_approx_eq(
            bx.min_top_bottom_distance([0.020, 0.020, 0.009], build_z, t),
            0.001,
        );

        // (c) point nearer to -X side than to top/bottom; -X side IGNORED
        // (it is a side face, not top/bottom). Top/bottom dist = 5 mm.
        assert_approx_eq(
            bx.min_top_bottom_distance([0.0005, 0.020, 0.005], build_z, t),
            0.005,
        );

        // (d) Y-up build axis: ±Y faces now count as top/bottom, ±X/±Z
        // are sides. Center distance to ±Y faces = 20 mm.
        let build_y = [0.0, 1.0, 0.0];
        assert_approx_eq(
            bx.min_top_bottom_distance([0.020, 0.020, 0.005], build_y, t),
            0.020,
        );
    }

    #[test]
    fn is_top_or_bottom_normal_predicate() {
        let build = [0.0, 0.0, 1.0];
        let threshold = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;

        // (a) +Z face normal — aligned → true.
        assert!(is_top_or_bottom_normal([0.0, 0.0, 1.0], build, threshold));

        // (b) -Z face normal — anti-aligned; |dot|=1 → true.
        assert!(is_top_or_bottom_normal([0.0, 0.0, -1.0], build, threshold));

        // (c) +X face normal — perpendicular; |dot|=0 → false.
        assert!(!is_top_or_bottom_normal([1.0, 0.0, 0.0], build, threshold));

        // (d) tilted normal 60° from horizontal — |dot|=0.8 > 0.7071 → true.
        assert!(is_top_or_bottom_normal([0.6, 0.0, 0.8], build, threshold));
    }
}
