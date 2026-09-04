//! Contract tests for the canonical triplex tensegrity fixture.
//!
//! STEP 1 (RED): this module deliberately carries *only* its `#[cfg(test)]`
//! contract tests. The items they exercise land in step 2; until then the test
//! build fails to compile, which is the RED signal.

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use reify_ir::Value;

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

        for i in 0..3 {
            let a = azimuth_rads(120.0 * (i as f64));
            let got = point_components(&nodes[i]);
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
        for i in 0..3 {
            assert_bits_eq(point_components(&delta[i])[2], 1.0, &format!("top node {i} z"));
        }
        for i in 3..6 {
            assert_bits_eq(point_components(&delta[i])[2], -1.0, &format!("bottom node {i} z"));
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
