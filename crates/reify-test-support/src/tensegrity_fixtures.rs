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
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

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

/// Assemble a `Tensegrity` structure `Value` from raw node / strut / cable
/// fields.
///
/// `surfaces` is an [`Option`] and the distinction is LOAD-BEARING: `None`
/// OMITS the `surfaces` key from the field map entirely, where `Some(v)`
/// inserts it. A structure with no `surfaces` key is the line-only input; one
/// carrying a PRESENT-but-empty `surfaces` list is a different input. `None`
/// must never be lowered to `Value::Undef` or to an empty list — the combined
/// membrane δ suite asserts the no-surfaces path returns an empty
/// `surface_stresses` echo and never an absent one, so both shapes have to stay
/// reachable and distinguishable.
pub fn tensegrity(
    nodes: Vec<Value>,
    struts: Value,
    cables: Value,
    surfaces: Option<Value>,
) -> Value {
    let mut fields: PersistentMap<String, Value> = PersistentMap::default();
    fields.insert("nodes".to_string(), Value::List(nodes));
    fields.insert("struts".to_string(), struts);
    fields.insert("cables".to_string(), cables);
    if let Some(surfaces) = surfaces {
        fields.insert("surfaces".to_string(), surfaces);
    }
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// The canonical triplex as a `Tensegrity` structure: [`triplex_nodes`] at the
/// requested heights, with [`TRIPLEX_MEMBERS`] split at [`TRIPLEX_STRUTS`] into
/// the `struts` and `cables` fields.
///
/// Both parameters carry a real difference between the call sites this replaced:
/// `triplex_tensegrity(1.0, 0.0, ..)` is the gauge / T1b prism,
/// `triplex_tensegrity(1.0, -1.0, ..)` the taller δ one. See [`tensegrity`] for
/// what `surfaces: None` means.
pub fn triplex_tensegrity(top_z: f64, bottom_z: f64, surfaces: Option<Value>) -> Value {
    let pair = |&(j, k): &(usize, usize)| [j as i64, k as i64];
    let struts: Vec<[i64; 2]> = TRIPLEX_MEMBERS[..TRIPLEX_STRUTS].iter().map(pair).collect();
    let cables: Vec<[i64; 2]> = TRIPLEX_MEMBERS[TRIPLEX_STRUTS..].iter().map(pair).collect();
    tensegrity(
        triplex_nodes(top_z, bottom_z),
        index_lists(&struts),
        index_lists(&cables),
        surfaces,
    )
}

/// Both membrane end caps of the triplex: the top cap over nodes 0, 1, 2 and the
/// bottom cap over nodes 3, 4, 5. The top cap spans the three FREE nodes of the
/// anchored solve, so it genuinely enters `D_ff` rather than sitting inertly on
/// the anchored side.
pub fn triplex_caps() -> Value {
    index_lists(&[[0, 1, 2], [3, 4, 5]])
}

/// Group ids in [`TRIPLEX_MEMBERS`] order: the three struts to group 0, the six
/// horizontals (top and bottom rings) to group 1, the three verticals to group 2.
pub fn triplex_group_ids() -> Value {
    Value::List([0i64, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2].into_iter().map(Value::Int).collect())
}

/// One seed ratio per group, in group-id order: struts compressive (−1),
/// horizontals and verticals tensile (+1).
pub fn triplex_seeds() -> Value {
    Value::List(vec![Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)])
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

    /// Destructure a structure Value into its field map, asserting the
    /// `StructureInstance` shape.
    fn structure_fields(v: &Value) -> &PersistentMap<String, Value> {
        match v {
            Value::StructureInstance(d) => &d.fields,
            other => panic!("expected a Value::StructureInstance, got {other:?}"),
        }
    }

    /// A minimal non-triplex member set, so the `tensegrity` assembler contract
    /// is pinned independently of the triplex topology.
    fn stub_members() -> (Value, Value) {
        (index_lists(&[[0, 4]]), index_lists(&[[0, 1]]))
    }

    /// The `Tensegrity` header every consumer matches on, plus the 3-key field
    /// map T1b's superseded copy built. `None` must OMIT `surfaces`.
    #[test]
    fn tensegrity_with_none_surfaces_omits_the_key_entirely() {
        let (struts, cables) = stub_members();
        let v = tensegrity(canonical_triplex_nodes(), struts, cables, None);

        let d = match &v {
            Value::StructureInstance(d) => d,
            other => panic!("expected a Value::StructureInstance, got {other:?}"),
        };
        assert_eq!(d.type_id, StructureTypeId(0), "Tensegrity fixtures use type_id 0");
        assert_eq!(d.type_name, "Tensegrity", "the type name every consumer matches on");
        assert_eq!(d.version, 1, "Tensegrity is version 1 (not 0 like Provenance)");

        assert_eq!(d.fields.len(), 3, "exactly nodes/struts/cables — no surfaces key");
        for key in ["nodes", "struts", "cables"] {
            assert!(d.fields.get(key).is_some(), "field `{key}` must be present");
        }
        assert_eq!(
            d.fields.get("surfaces"),
            None,
            "None must OMIT `surfaces` — never insert Value::Undef, never an empty list"
        );
    }

    /// `Some(v)` inserts the key, giving the 4-key map the gauge and δ copies
    /// built.
    #[test]
    fn tensegrity_with_some_surfaces_inserts_the_key() {
        let (struts, cables) = stub_members();
        let caps = triplex_caps();
        let v = tensegrity(canonical_triplex_nodes(), struts, cables, Some(caps.clone()));

        let fields = structure_fields(&v);
        assert_eq!(fields.len(), 4, "nodes/struts/cables/surfaces");
        assert_eq!(fields.get("surfaces"), Some(&caps), "`surfaces` is carried through verbatim");
    }

    /// THE distinction that makes a naive one-fixture collapse unsafe: a
    /// PRESENT-but-empty `surfaces` list is a different input from an ABSENT
    /// `surfaces` key. δ's backward-compat test asserts the no-surfaces path
    /// returns an EMPTY echo and never Undef/absent, so both shapes have to stay
    /// reachable and distinguishable.
    #[test]
    fn present_but_empty_surfaces_is_distinct_from_absent_surfaces() {
        let (struts, cables) = stub_members();
        let empty = tensegrity(
            canonical_triplex_nodes(),
            struts.clone(),
            cables.clone(),
            Some(Value::List(vec![])),
        );
        let absent = tensegrity(canonical_triplex_nodes(), struts, cables, None);

        assert_eq!(
            structure_fields(&empty).get("surfaces"),
            Some(&Value::List(vec![])),
            "Some(empty list) must be PRESENT and empty"
        );
        assert_eq!(
            structure_fields(&absent).get("surfaces"),
            None,
            "None must be ABSENT from the field map"
        );
        assert_eq!(structure_fields(&empty).len(), 4);
        assert_eq!(structure_fields(&absent).len(), 3);
        assert_ne!(empty, absent, "the two are different inputs to a form-find solve");
    }

    /// The kernel topology, transcribed from the superseded T1b and δ copies:
    /// struts then top ring, bottom ring, verticals — in that exact order.
    #[test]
    fn triplex_tensegrity_lowers_the_kernel_topology() {
        let v = triplex_tensegrity(1.0, 0.0, None);
        let fields = structure_fields(&v);

        assert_eq!(
            fields.get("nodes"),
            Some(&Value::List(canonical_triplex_nodes())),
            "nodes are the canonical prism at the requested heights"
        );
        assert_eq!(
            fields.get("struts"),
            Some(&Value::List(vec![
                Value::List(vec![Value::Int(0), Value::Int(4)]),
                Value::List(vec![Value::Int(1), Value::Int(5)]),
                Value::List(vec![Value::Int(2), Value::Int(3)]),
            ])),
            "struts are TRIPLEX_MEMBERS[..TRIPLEX_STRUTS]: the three crossing diagonals"
        );
        assert_eq!(
            fields.get("cables"),
            Some(&Value::List(vec![
                // top ring
                Value::List(vec![Value::Int(0), Value::Int(1)]),
                Value::List(vec![Value::Int(1), Value::Int(2)]),
                Value::List(vec![Value::Int(2), Value::Int(0)]),
                // bottom ring
                Value::List(vec![Value::Int(3), Value::Int(4)]),
                Value::List(vec![Value::Int(4), Value::Int(5)]),
                Value::List(vec![Value::Int(5), Value::Int(3)]),
                // verticals
                Value::List(vec![Value::Int(0), Value::Int(3)]),
                Value::List(vec![Value::Int(1), Value::Int(4)]),
                Value::List(vec![Value::Int(2), Value::Int(5)]),
            ])),
            "cables are TRIPLEX_MEMBERS[TRIPLEX_STRUTS..], in that exact order"
        );
    }

    /// δ's instance: taller prism plus both end caps.
    #[test]
    fn triplex_tensegrity_carries_the_requested_geometry_and_surfaces() {
        let v = triplex_tensegrity(1.0, -1.0, Some(triplex_caps()));
        let fields = structure_fields(&v);

        assert_eq!(
            fields.get("nodes"),
            Some(&Value::List(triplex_nodes(1.0, -1.0))),
            "bottom_z reaches the assembled structure, not just triplex_nodes()"
        );
        assert_eq!(fields.get("surfaces"), Some(&triplex_caps()));
        assert_eq!(fields.len(), 4);
    }

    /// The gauge's `caps()` and δ's inline `surfaces` literal were the same
    /// value — top cap over nodes 0,1,2 and bottom cap over nodes 3,4,5.
    #[test]
    fn triplex_caps_are_the_two_end_triangles() {
        assert_eq!(
            triplex_caps(),
            Value::List(vec![
                Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]),
                Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]),
            ]),
            "top cap over nodes 0,1,2 and bottom cap over nodes 3,4,5"
        );
    }

    /// Struts to group 0, the six horizontals to group 1, the three verticals to
    /// group 2 — byte-identical between the superseded T1b and δ copies.
    #[test]
    fn triplex_group_ids_follow_the_member_index_space() {
        assert_eq!(
            triplex_group_ids(),
            Value::List(
                [0i64, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2].into_iter().map(Value::Int).collect()
            ),
            "3 struts then 6 horizontals then 3 verticals, in TRIPLEX_MEMBERS order"
        );
    }

    /// Seed ratios: struts compressive (−1), horizontals and verticals tensile (+1).
    #[test]
    fn triplex_seeds_are_one_compressive_and_two_tensile() {
        assert_eq!(
            triplex_seeds(),
            Value::List(vec![Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)]),
            "one seed per group, in group-id order"
        );
    }
}
