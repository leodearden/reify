//! THE single definition of the canonical triplex tensegrity fixture.
//!
//! The "triplex" is the canonical symmetric triangular T-prism used across the
//! form-finding suites: 6 nodes on a unit circumradius, 3 crossing struts and 9
//! cables. Before this module it existed as three hand-maintained copies inside
//! `crates/reify-eval/tests/harness_fea_solver_e2e/` (the force-density gauge,
//! the T1b free-standing form-find, and the combined membrane δ suite), so a
//! topology or node-order change had to be mirrored by hand across all three or
//! they silently drifted apart.
//!
//! ANTI-DRIFT PROPERTY: changing the topology or node order *here* changes every
//! consuming suite at once. That is the whole point — resist re-inlining a
//! "just this once" local variant in a call site.
//!
//! The two axes on which the three copies genuinely differed are preserved as
//! explicit parameters rather than normalised away, because both are
//! load-bearing:
//!
//!   * `bottom_z` — the gauge and T1b prisms have their bottom triangle at
//!     `z = 0.0`; δ's sits at `z = -1.0`, a taller prism that feeds a different
//!     solve.
//!   * `surfaces` — see [`tensegrity`]: T1b's structure OMITS the `surfaces`
//!     field entirely, where the gauge and δ carry it. Absent and present-but-
//!     empty are different inputs, and at least one test turns on the
//!     distinction.

use crate::values::point3;
use reify_ir::Value;

/// Struts-then-cables member order — the ONE index space that `force_densities`
/// and `member_forces` share. `TRIPLEX_MEMBERS[..TRIPLEX_STRUTS]` are the three
/// crossing struts (compression, q < 0); the rest are the top, bottom and
/// vertical cable triples (tension, q > 0).
///
/// Reordering this changes the meaning of every per-member array handed to or
/// returned by a form-find solve.
pub const TRIPLEX_MEMBERS: [(usize, usize); 12] = [
    (0, 4),
    (1, 5),
    (2, 3),
    (0, 1),
    (1, 2),
    (2, 0),
    (3, 4),
    (4, 5),
    (5, 3),
    (0, 3),
    (1, 4),
    (2, 5),
];

/// Split point of [`TRIPLEX_MEMBERS`]: the first `TRIPLEX_STRUTS` entries are
/// struts, the remainder cables. That split is what lets consumers re-assert the
/// documented sign contract (struts q < 0, cables q > 0) instead of merely
/// checking finiteness.
pub const TRIPLEX_STRUTS: usize = 3;

/// The anchored node set for the anchored (non-free-standing) solves: the bottom
/// triangle {3, 4, 5} is fixed, the top triangle {0, 1, 2} is free.
pub const TRIPLEX_ANCHORS: [i64; 3] = [3, 4, 5];

/// The canonical symmetric triplex prism at circumradius 1: top triangle
/// (nodes 0, 1, 2) at `top_z` and azimuth 120°·i, bottom triangle (nodes 3, 4,
/// 5) at `bottom_z` and azimuth 120°·i + 30°.
///
/// `bottom_z` is a parameter, not a constant, because the pre-existing fixtures
/// genuinely disagreed on it — gauge and T1b use `0.0`, δ uses `-1.0`. Silently
/// picking one would change the geometry a solve converges from.
///
/// Coordinates are built with [`crate::values::point3`], so each node is a
/// `Value::Point` of three LENGTH-dimensioned SI-metre `Value::Scalar`s.
pub fn triplex_nodes(top_z: f64, bottom_z: f64) -> Vec<Value> {
    // `.to_radians()` is `self * (PI / 180.0)`; the contract tests below pin
    // that it agrees bit-for-bit with the explicit `* (PI / 180.0)` spelling the
    // superseded T1b/δ copies used, so this collapse is not a numerical change.
    let ring = |i: usize, twist: f64, z: f64| {
        let a = (120.0 * (i as f64) + twist).to_radians();
        point3(a.cos(), a.sin(), z)
    };
    let mut nodes: Vec<Value> = (0..3).map(|i| ring(i, 0.0, top_z)).collect();
    nodes.extend((0..3).map(|i| ring(i, 30.0, bottom_z)));
    nodes
}

/// The canonical triplex geometry: `triplex_nodes(1.0, 0.0)` — circumradius 1,
/// height 1, 30° twist. This is the gauge / T1b prism.
pub fn canonical_triplex_nodes() -> Vec<Value> {
    triplex_nodes(1.0, 0.0)
}

/// Lower a list of index tuples (`[[j, k], …]` for struts and cables,
/// `[[i, j, k], …]` for surfaces) the way the DSL lowers them: a `Value::List`
/// of `Value::List`s of `Value::Int`.
pub fn index_lists<const N: usize>(rows: &[[i64; N]]) -> Value {
    let row = |r: &[i64; N]| Value::List(r.iter().map(|&i| Value::Int(i)).collect());
    Value::List(rows.iter().map(row).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    /// The azimuth formulation used by the two *pre-refactor* inline copies
    /// (`tensegrity_t1b_form_find_e2e.rs` and
    /// `tensegrity_delta_combined_form_find_e2e.rs`): an explicit
    /// `* (PI / 180.0)` rather than `f64::to_radians`.
    ///
    /// Expectations below are built with THIS form while the fixture itself
    /// uses `.to_radians()`, so the geometry assertions are a real cross-check
    /// between the two spellings rather than a tautology against the
    /// implementation.
    fn azimuth_rads(degrees: f64) -> f64 {
        degrees * (std::f64::consts::PI / 180.0)
    }

    /// Destructure a node into its three SI-metre components, asserting the
    /// `Value::Point`-of-LENGTH-`Scalar` shape every consumer relies on.
    fn point_components(v: &Value) -> [f64; 3] {
        match v {
            Value::Point(cs) => {
                assert_eq!(cs.len(), 3, "a triplex node must be a 3-component Point, got {cs:?}");
                let mut out = [0.0f64; 3];
                for (slot, c) in out.iter_mut().zip(cs) {
                    match c {
                        Value::Scalar { si_value, dimension } => {
                            assert_eq!(
                                *dimension,
                                DimensionVector::LENGTH,
                                "node coordinates must carry LENGTH, got {dimension:?}"
                            );
                            *slot = *si_value;
                        }
                        other => panic!("expected a LENGTH Scalar coordinate, got {other:?}"),
                    }
                }
                out
            }
            other => panic!("expected a Value::Point node, got {other:?}"),
        }
    }

    /// Bit-exact f64 comparison. The whole point of these assertions is
    /// bit-identity with the pre-refactor inline fixtures, so an epsilon
    /// compare would not catch the drift this module exists to prevent.
    #[track_caller]
    fn assert_bits_eq(actual: f64, expected: f64, what: &str) {
        assert_eq!(
            actual.to_bits(),
            expected.to_bits(),
            "{what}: not bit-identical — got {actual:?} ({:#x}), want {expected:?} ({:#x})",
            actual.to_bits(),
            expected.to_bits()
        );
    }

    /// `f64::to_radians` is documented as `self * (PI / 180.0)`. The three
    /// pre-refactor copies are split across both spellings — the gauge module
    /// uses `.to_radians()`, t1b and delta use `* (PI / 180.0)` — so collapsing
    /// them onto ONE spelling is only behaviour-preserving if the two agree to
    /// the bit. Lock that here rather than assuming it.
    #[test]
    fn to_radians_bit_equals_the_explicit_pi_over_180_spelling() {
        for i in 0..3 {
            for twist in [0.0f64, 30.0] {
                let degrees = 120.0 * (i as f64) + twist;
                assert_eq!(
                    degrees.to_radians().to_bits(),
                    (degrees * (std::f64::consts::PI / 180.0)).to_bits(),
                    "azimuth {degrees}°: .to_radians() and * (PI/180.0) must agree bit-for-bit"
                );
            }
        }
    }

    /// The canonical prism: circumradius 1, top ring z = +1.0 at azimuth 120°·i,
    /// bottom ring z = 0.0 at azimuth 120°·i + 30°. All 18 components pinned.
    #[test]
    fn canonical_triplex_nodes_pins_the_prism_geometry_bit_for_bit() {
        let nodes = canonical_triplex_nodes();
        assert_eq!(nodes.len(), 6, "the canonical triplex has exactly 6 nodes");

        for (i, n) in nodes.iter().take(3).enumerate() {
            let a = azimuth_rads(120.0 * (i as f64));
            let got = point_components(n);
            assert_bits_eq(got[0], a.cos(), &format!("top node {i} x"));
            assert_bits_eq(got[1], a.sin(), &format!("top node {i} y"));
            assert_bits_eq(got[2], 1.0, &format!("top node {i} z"));
        }

        for i in 0..3 {
            let a = azimuth_rads(120.0 * (i as f64) + 30.0);
            let got = point_components(&nodes[3 + i]);
            assert_bits_eq(got[0], a.cos(), &format!("bottom node {i} x"));
            assert_bits_eq(got[1], a.sin(), &format!("bottom node {i} y"));
            assert_bits_eq(got[2], 0.0, &format!("bottom node {i} z"));
        }
    }

    /// Circumradius is 1 for every node. Necessarily an epsilon compare, not a
    /// bit compare: cos²θ + sin²θ is not exactly 1.0 in binary floating point.
    #[test]
    fn canonical_triplex_nodes_lie_on_the_unit_circumradius() {
        for (i, n) in canonical_triplex_nodes().iter().enumerate() {
            let [x, y, _] = point_components(n);
            assert!(
                (x * x + y * y - 1.0).abs() < 1e-15,
                "node {i} must sit at circumradius 1, got r² = {}",
                x * x + y * y
            );
        }
    }

    /// `canonical_triplex_nodes()` is exactly the `bottom_z = 0.0` instance —
    /// the gauge / t1b geometry.
    #[test]
    fn canonical_triplex_nodes_is_the_bottom_z_zero_instance() {
        assert_eq!(
            triplex_nodes(1.0, 0.0),
            canonical_triplex_nodes(),
            "canonical_triplex_nodes() must equal triplex_nodes(1.0, 0.0)"
        );
    }

    /// delta's variant: same x/y ring, but the bottom triangle sits at z = −1.0,
    /// a genuinely taller prism than gauge / t1b solve. Parameterising
    /// `bottom_z` is what keeps that difference explicit instead of silently
    /// normalised away by the collapse.
    #[test]
    fn triplex_nodes_parameterises_bottom_z_without_moving_the_rings() {
        let canonical = canonical_triplex_nodes();
        let delta = triplex_nodes(1.0, -1.0);
        assert_eq!(delta.len(), 6, "the delta triplex also has exactly 6 nodes");

        for i in 0..6 {
            let c = point_components(&canonical[i]);
            let d = point_components(&delta[i]);
            assert_bits_eq(d[0], c[0], &format!("node {i} x must not move with bottom_z"));
            assert_bits_eq(d[1], c[1], &format!("node {i} y must not move with bottom_z"));
        }
        for (i, n) in delta.iter().enumerate() {
            let (ring, want_z) = if i < 3 { ("top", 1.0) } else { ("bottom", -1.0) };
            assert_bits_eq(point_components(n)[2], want_z, &format!("{ring} node {i} z"));
        }
    }

    /// The ONE index space `force_densities` and `member_forces` share:
    /// struts first, then top / bottom / vertical cable triples.
    #[test]
    fn triplex_member_index_space_is_struts_then_cables() {
        assert_eq!(
            TRIPLEX_MEMBERS,
            [
                (0, 4),
                (1, 5),
                (2, 3),
                (0, 1),
                (1, 2),
                (2, 0),
                (3, 4),
                (4, 5),
                (5, 3),
                (0, 3),
                (1, 4),
                (2, 5)
            ],
            "member order is load-bearing: force_densities and member_forces are indexed by it"
        );
        assert_eq!(TRIPLEX_STRUTS, 3, "the first three members are the struts");
        assert_eq!(TRIPLEX_ANCHORS, [3i64, 4, 5], "the bottom triangle is the anchored set");
    }

    /// Index-tuple lowering, at both widths the fixtures use: 2-wide for
    /// struts / cables, 3-wide for surfaces.
    #[test]
    fn index_lists_lowers_rows_to_nested_int_lists() {
        assert_eq!(
            index_lists(&[[0, 1], [2, 3]]),
            Value::List(vec![
                Value::List(vec![Value::Int(0), Value::Int(1)]),
                Value::List(vec![Value::Int(2), Value::Int(3)]),
            ]),
            "2-wide rows lower to 2-element Int lists"
        );
        assert_eq!(
            index_lists(&[[0, 1, 2], [3, 4, 5]]),
            Value::List(vec![
                Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]),
                Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]),
            ]),
            "3-wide rows lower to 3-element Int lists"
        );
    }
}
