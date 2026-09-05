//! The single definition of the triplex tensegrity fixture **in `Value` form**.
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
//! REMAINING DRIFT SURFACE — the kernel side, deliberately NOT collapsed here.
//! `crates/reify-solver-elastic` carries five further copies of this prism's
//! topology and geometry as raw `Vec<[f64; 3]>` / `Vec<(usize, usize)>` rather
//! than `Value`, so they are outside the "single definition" this module can
//! claim: `src/form_find_free.rs` (`triplex_topology` + `canonical_prism`),
//! `src/prestress_stability.rs` (`canonical_prism`),
//! `tests/tensegrity_t1b_form_find_free.rs`,
//! `tests/tensegrity_delta_combined_form_find.rs` and
//! `tests/tensegrity_t2_stability.rs`. The two `src/` ones sit inside a
//! `#[cfg(test)] mod tests` and are unreachable from another crate; the three
//! `tests/` ones ARE reachable — reify-solver-elastic already dev-deps
//! reify-test-support — and [`triplex_node_coords`] exists as the raw-coordinate
//! seam they can collapse onto without any dependency change. Recording that
//! surface here replaces the note the force-density gauge used to carry.
//!
//! The two axes on which the three harness copies genuinely differed are
//! preserved rather than normalised away, because both are load-bearing:
//!
//!   * bottom-triangle height — the gauge and T1b prisms have their bottom
//!     triangle at `z = 0.0`; δ's sits at `z = -1.0`, a taller prism that feeds
//!     a different solve. The two live behind the named constructors
//!     [`canonical_triplex_tensegrity`] and [`tall_triplex_tensegrity`] so the
//!     geometry choice stays legible at the call site instead of becoming a
//!     positional float.
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

/// Height of the top triangle. Not a parameter: every fixture this module
/// replaced put the top ring at `z = +1.0`, and the only axis they disagreed on
/// is the bottom ring (see [`canonical_triplex_tensegrity`] /
/// [`tall_triplex_tensegrity`]). Keeping it a constant is what stops the two
/// heights from becoming a transposable pair of bare `f64` arguments.
const TRIPLEX_TOP_Z: f64 = 1.0;

/// The canonical symmetric triplex prism at circumradius 1, as RAW coordinates:
/// top triangle (nodes 0, 1, 2) at [`TRIPLEX_TOP_Z`] and azimuth 120°·i, bottom
/// triangle (nodes 3, 4, 5) at `bottom_z` and azimuth 120°·i + 30°.
///
/// SEAM, kept `pub` deliberately even though nothing outside this crate calls it
/// yet: reify-solver-elastic's five `canonical_prism()` copies are exactly this
/// value in exactly this `Vec<[f64; 3]>` shape (see the module header), and its
/// three `tests/` copies can collapse onto this function without a dependency
/// change. [`triplex_nodes`] is the `Value`-form counterpart for consumers that
/// need dimensioned coordinates.
///
/// `bottom_z` is a parameter, not a constant, because the pre-existing fixtures
/// genuinely disagreed on it — gauge and T1b use `0.0`, δ uses `-1.0`. Silently
/// picking one would change the geometry a solve converges from.
pub fn triplex_node_coords(bottom_z: f64) -> Vec<[f64; 3]> {
    // `.to_radians()` is `self * (PI / 180.0)`; the contract tests below pin
    // that it agrees bit-for-bit with the explicit `* (PI / 180.0)` spelling the
    // superseded T1b/δ copies used, so this collapse is not a numerical change.
    let ring = |i: usize, twist: f64, z: f64| {
        let a = (120.0 * (i as f64) + twist).to_radians();
        [a.cos(), a.sin(), z]
    };
    let mut coords: Vec<[f64; 3]> = (0..3).map(|i| ring(i, 0.0, TRIPLEX_TOP_Z)).collect();
    coords.extend((0..3).map(|i| ring(i, 30.0, bottom_z)));
    coords
}

/// [`triplex_node_coords`] lifted into the DSL's `Value` domain via
/// [`crate::values::point3`], so each node is a `Value::Point` of three
/// LENGTH-dimensioned SI-metre `Value::Scalar`s.
///
/// SEAM, kept `pub` deliberately: today every consuming suite goes through
/// [`canonical_triplex_tensegrity`] / [`tall_triplex_tensegrity`] and needs only
/// the assembled structure, but a suite that wants the bare node list — to
/// perturb it, or to build a non-`Tensegrity` structure over the same geometry —
/// should reach for this rather than re-deriving the ring math.
pub fn triplex_nodes(bottom_z: f64) -> Vec<Value> {
    triplex_node_coords(bottom_z).into_iter().map(|[x, y, z]| point3(x, y, z)).collect()
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

/// The triplex as a `Tensegrity` structure: [`triplex_nodes`] at the requested
/// bottom height, with [`TRIPLEX_MEMBERS`] split at [`TRIPLEX_STRUTS`] into the
/// `struts` and `cables` fields.
///
/// Private on purpose. `bottom_z` has exactly two inhabitants across the repo
/// and both have a named constructor ([`canonical_triplex_tensegrity`],
/// [`tall_triplex_tensegrity`]); routing every call site through those keeps the
/// geometry choice readable and keeps a third variant from being introduced at a
/// call site instead of here, where the anti-drift property lives.
fn triplex_tensegrity_at(bottom_z: f64, surfaces: Option<Value>) -> Value {
    let pair = |&(j, k): &(usize, usize)| [j as i64, k as i64];
    let struts: Vec<[i64; 2]> = TRIPLEX_MEMBERS[..TRIPLEX_STRUTS].iter().map(pair).collect();
    let cables: Vec<[i64; 2]> = TRIPLEX_MEMBERS[TRIPLEX_STRUTS..].iter().map(pair).collect();
    tensegrity(
        triplex_nodes(bottom_z),
        index_lists(&struts),
        index_lists(&cables),
        surfaces,
    )
}

/// The canonical triplex: circumradius 1, **unit height** (top `z = +1.0`,
/// bottom `z = 0.0`), 30° twist. This is the force-density gauge and T1b prism.
///
/// See [`tensegrity`] for what `surfaces: None` means — it is not the same input
/// as `Some(Value::List(vec![]))`.
pub fn canonical_triplex_tensegrity(surfaces: Option<Value>) -> Value {
    triplex_tensegrity_at(0.0, surfaces)
}

/// The taller triplex: circumradius 1, **height 2** (top `z = +1.0`, bottom
/// `z = -1.0`), 30° twist. This is the combined membrane δ prism, and the height
/// difference is a real one — it changes the geometry the combined solve
/// converges from, so it is named rather than left as a positional float.
///
/// See [`tensegrity`] for what `surfaces: None` means.
pub fn tall_triplex_tensegrity(surfaces: Option<Value>) -> Value {
    triplex_tensegrity_at(-1.0, surfaces)
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

    /// The `bottom_z` of [`canonical_triplex_tensegrity`] / the gauge and T1b
    /// prism, and of [`tall_triplex_tensegrity`] / the δ prism. Named here so the
    /// tests below assert against the same two heights the constructors pick.
    const CANONICAL_BOTTOM_Z: f64 = 0.0;
    const TALL_BOTTOM_Z: f64 = -1.0;

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
    /// bottom ring z = 0.0 at azimuth 120°·i + 30°. All 18 raw components
    /// pinned, and `triplex_nodes` re-checked to carry exactly those components
    /// so the raw seam and the `Value` form cannot drift apart.
    #[test]
    fn canonical_triplex_geometry_is_pinned_bit_for_bit() {
        let coords = triplex_node_coords(CANONICAL_BOTTOM_Z);
        let nodes = triplex_nodes(CANONICAL_BOTTOM_Z);
        assert_eq!(coords.len(), 6, "the canonical triplex has exactly 6 nodes");
        assert_eq!(nodes.len(), 6, "the Value form has the same 6 nodes");

        for i in 0..6 {
            let (twist, want_z) = if i < 3 { (0.0, 1.0) } else { (30.0, 0.0) };
            let a = azimuth_rads(120.0 * ((i % 3) as f64) + twist);
            let ring = if i < 3 { "top" } else { "bottom" };
            assert_bits_eq(coords[i][0], a.cos(), &format!("{ring} node {i} x"));
            assert_bits_eq(coords[i][1], a.sin(), &format!("{ring} node {i} y"));
            assert_bits_eq(coords[i][2], want_z, &format!("{ring} node {i} z"));

            let lifted = point_components(&nodes[i]);
            for (axis, c) in ["x", "y", "z"].iter().zip(0..3) {
                assert_bits_eq(
                    lifted[c],
                    coords[i][c],
                    &format!("node {i} {axis}: triplex_nodes must lift triplex_node_coords"),
                );
            }
        }
    }

    /// Circumradius is 1 for every node. Necessarily an epsilon compare, not a
    /// bit compare: cos²θ + sin²θ is not exactly 1.0 in binary floating point.
    #[test]
    fn triplex_nodes_lie_on_the_unit_circumradius() {
        for (i, n) in triplex_nodes(CANONICAL_BOTTOM_Z).iter().enumerate() {
            let [x, y, _] = point_components(n);
            assert!(
                (x * x + y * y - 1.0).abs() < 1e-15,
                "node {i} must sit at circumradius 1, got r² = {}",
                x * x + y * y
            );
        }
    }

    /// δ's variant: same x/y ring, but the bottom triangle sits at z = −1.0,
    /// a genuinely taller prism than gauge / t1b solve. Parameterising the
    /// bottom height is what keeps that difference explicit instead of silently
    /// normalised away by the collapse.
    #[test]
    fn triplex_nodes_parameterises_bottom_z_without_moving_the_rings() {
        let canonical = triplex_node_coords(CANONICAL_BOTTOM_Z);
        let tall = triplex_node_coords(TALL_BOTTOM_Z);
        assert_eq!(tall.len(), 6, "the tall triplex also has exactly 6 nodes");

        for i in 0..6 {
            assert_bits_eq(tall[i][0], canonical[i][0], &format!("node {i} x must not move"));
            assert_bits_eq(tall[i][1], canonical[i][1], &format!("node {i} y must not move"));
            let (ring, want_z) = if i < 3 { ("top", 1.0) } else { ("bottom", TALL_BOTTOM_Z) };
            assert_bits_eq(tall[i][2], want_z, &format!("{ring} node {i} z"));
        }
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
        let v = tensegrity(triplex_nodes(CANONICAL_BOTTOM_Z), struts, cables, None);

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
        let v =
            tensegrity(triplex_nodes(CANONICAL_BOTTOM_Z), struts, cables, Some(caps.clone()));

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
        let empty = canonical_triplex_tensegrity(Some(Value::List(vec![])));
        let absent = canonical_triplex_tensegrity(None);

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
    fn canonical_triplex_tensegrity_lowers_the_kernel_topology() {
        let v = canonical_triplex_tensegrity(None);
        let fields = structure_fields(&v);

        assert_eq!(
            fields.get("nodes"),
            Some(&Value::List(triplex_nodes(CANONICAL_BOTTOM_Z))),
            "the canonical constructor is the bottom z = 0.0 prism"
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

    /// The two named constructors must differ in exactly one way — the bottom
    /// ring height — and `tall` must be the δ prism. Getting this backwards is
    /// the mistake the named constructors exist to make unrepresentable, so it
    /// is asserted rather than assumed.
    #[test]
    fn tall_and_canonical_constructors_differ_only_in_bottom_height() {
        let tall = tall_triplex_tensegrity(Some(triplex_caps()));
        let canonical = canonical_triplex_tensegrity(None);
        let tall_fields = structure_fields(&tall);
        let canonical_fields = structure_fields(&canonical);

        assert_eq!(
            tall_fields.get("nodes"),
            Some(&Value::List(triplex_nodes(TALL_BOTTOM_Z))),
            "tall_triplex_tensegrity is the bottom z = -1.0 prism"
        );
        assert_ne!(
            tall_fields.get("nodes"),
            canonical_fields.get("nodes"),
            "the two prisms are genuinely different geometry, not an alias"
        );
        for key in ["struts", "cables"] {
            assert_eq!(
                tall_fields.get(key),
                canonical_fields.get(key),
                "`{key}` topology is shared — only the bottom height differs"
            );
        }
        assert_eq!(tall_fields.get("surfaces"), Some(&triplex_caps()));
    }

    /// The remaining literal goldens, in one place. Each is a value the
    /// superseded copies agreed on byte-for-byte, so this is the lockstep-edit
    /// site if the fixture is ever deliberately changed.
    #[test]
    fn triplex_fixture_goldens() {
        assert_eq!(TRIPLEX_STRUTS, 3, "the first three members are the struts");
        assert_eq!(TRIPLEX_ANCHORS, [3i64, 4, 5], "the bottom triangle is the anchored set");
        assert_eq!(
            TRIPLEX_MEMBERS.len(),
            12,
            "3 struts + 9 cables; the order itself is pinned by \
             canonical_triplex_tensegrity_lowers_the_kernel_topology"
        );
        assert_eq!(
            triplex_caps(),
            index_lists(&[[0, 1, 2], [3, 4, 5]]),
            "top cap over nodes 0,1,2 and bottom cap over nodes 3,4,5"
        );
        assert_eq!(
            triplex_group_ids(),
            Value::List(
                [0i64, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2].into_iter().map(Value::Int).collect()
            ),
            "3 struts then 6 horizontals then 3 verticals, in TRIPLEX_MEMBERS order"
        );
        assert_eq!(
            triplex_seeds(),
            Value::List(vec![Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)]),
            "one seed per group, in group-id order: compressive, tensile, tensile"
        );
    }
}
