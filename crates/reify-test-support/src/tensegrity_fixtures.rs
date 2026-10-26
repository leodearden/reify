//! The single definitions of the three form-finding goldens: the triplex
//! tensegrity, the tent membrane and the catenoid tube.
//!
//! The "triplex" is the canonical symmetric triangular T-prism used across the
//! form-finding suites: 6 nodes on a unit circumradius, 3 crossing struts and 9
//! cables, provided **in `Value` form**. Before this module it existed as
//! hand-maintained copies inside
//! `crates/reify-eval/tests/harness_fea_solver_e2e/`, so a topology or node-order
//! change had to be mirrored across all of them or they silently drifted apart.
//! Its raw coordinates are also exposed, as [`triplex_node_coords`], because
//! reify-solver-elastic's integration tests consume the prism below the `Value`
//! layer.
//!
//! The "tent" is the minimal fixed-boundary membrane: four anchored corners in
//! one plane, fanned around one free node seeded off that plane (see
//! [`TENT_NODE_COORDS`]). Besides its `Value` form, [`tent_membrane_tensegrity`],
//! it is exposed as raw consts, because reify-solver-elastic's `form_find` unit
//! tests consume it below the `Value` layer.
//!
//! The "catenoid tube" is the minimal surface of revolution `r(z) = c·cosh(z/c)`
//! between two coaxial anchored rings (see [`catenoid_tube`]). It is raw-only,
//! like the tent's consts, because reify-solver-elastic's integration tests
//! consume it below the `Value` layer.
//!
//! ANTI-DRIFT PROPERTY: changing the topology or node order *here* changes every
//! consuming suite at once. That is the whole point — resist re-inlining a
//! "just this once" local variant in a call site.
//!
//! Copies elsewhere in the workspace are still being collapsed onto this module.
//! Each remaining one is inventoried by exact path in the task that closes it —
//! #7721 for the `#[cfg(test)]`-internal triplex copies, #8106 for the tent
//! mirror in the `harness_fea_solver_e2e` T1a suite, #8152 for the triplex
//! member-topology, group-id, guess-table and closed-form-q copies in
//! reify-solver-elastic’s `tests/`, and #8153 for that directory’s zero-twist
//! prism in `common/mod.rs` and the ε suite’s catenoid-tube builder.
//!
//! Two axes on which the superseded triplex copies genuinely differed are preserved
//! rather than normalised away, because both are load-bearing inputs to a solve:
//!
//!   * bottom-triangle height — [`canonical_triplex_tensegrity`] puts it at
//!     [`CANONICAL_TRIPLEX_BOTTOM_Z`] (the gauge and T1b prism),
//!     [`tall_triplex_tensegrity`] at [`TALL_TRIPLEX_BOTTOM_Z`] (δ’s taller
//!     prism, which feeds a different solve). Named constructors, and callers
//!     of the raw [`triplex_node_coords`] pass one of the two named consts, so
//!     the geometry choice stays legible at the call site instead of becoming
//!     a positional float.
//!   * `surfaces` presence — see [`tensegrity`]: T1b’s structure OMITS the
//!     field where the gauge and δ carry it. Absent and present-but-empty are
//!     different inputs, and at least one test turns on the distinction.

use crate::values::point3;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Struts-then-cables member order — the ONE index space that `force_densities`
/// and `member_forces` share. `TRIPLEX_MEMBERS[..TRIPLEX_STRUTS]` are the three
/// crossing struts (compression, q < 0); the rest are the top, bottom and
/// vertical cable triples (tension, q > 0).
///
/// Reordering this changes the meaning of every per-member array handed to or
/// returned by a form-find solve.
pub const TRIPLEX_MEMBERS: [[i64; 2]; 12] = [
    [0, 4],
    [1, 5],
    [2, 3],
    [0, 1],
    [1, 2],
    [2, 0],
    [3, 4],
    [4, 5],
    [5, 3],
    [0, 3],
    [1, 4],
    [2, 5],
];

/// Split point of [`TRIPLEX_MEMBERS`]: the first `TRIPLEX_STRUTS` entries are
/// struts, the remainder cables.
pub const TRIPLEX_STRUTS: usize = 3;

/// The anchored node set for the anchored (non-free-standing) solves: the bottom
/// triangle {3, 4, 5} is fixed, the top triangle {0, 1, 2} is free.
pub const TRIPLEX_ANCHORS: [i64; 3] = [3, 4, 5];

/// Height of the top triangle, shared by both prisms; only the bottom ring varies.
const TRIPLEX_TOP_Z: f64 = 1.0;

/// Bottom-triangle height of the canonical unit-height prism: the force-density
/// gauge, T1b and T2 prism.
pub const CANONICAL_TRIPLEX_BOTTOM_Z: f64 = 0.0;

/// Bottom-triangle height of the taller, height-2 prism: the combined membrane δ
/// prism.
pub const TALL_TRIPLEX_BOTTOM_Z: f64 = -1.0;

/// The triplex prism at circumradius 1, as raw coordinates: top triangle
/// (nodes 0, 1, 2) at [`TRIPLEX_TOP_Z`] and azimuth 120°·i, bottom triangle
/// (nodes 3, 4, 5) at `bottom_z` and azimuth 120°·i + 30°. Callers pass
/// [`CANONICAL_TRIPLEX_BOTTOM_Z`] or [`TALL_TRIPLEX_BOTTOM_Z`].
pub fn triplex_node_coords(bottom_z: f64) -> Vec<[f64; 3]> {
    let ring = |i: usize, twist: f64, z: f64| {
        let a = (120.0 * (i as f64) + twist).to_radians();
        [a.cos(), a.sin(), z]
    };
    let mut coords: Vec<[f64; 3]> = (0..3).map(|i| ring(i, 0.0, TRIPLEX_TOP_Z)).collect();
    coords.extend((0..3).map(|i| ring(i, 30.0, bottom_z)));
    coords
}

/// Raw coordinates as `Value::Point`s of LENGTH-dimensioned SI-metre
/// `Value::Scalar`s, via [`crate::values::point3`].
fn lift_points(coords: impl IntoIterator<Item = [f64; 3]>) -> Vec<Value> {
    coords
        .into_iter()
        .map(|[x, y, z]| point3(x, y, z))
        .collect()
}

/// [`triplex_node_coords`], lifted by [`lift_points`].
fn triplex_nodes(bottom_z: f64) -> Vec<Value> {
    lift_points(triplex_node_coords(bottom_z))
}

/// Lower index rows (`[[j, k], …]` for struts and cables, `[[i, j, k], …]` for
/// surfaces) the way the DSL lowers them: a `Value::List` of `Value::List`s of
/// `Value::Int`.
fn index_lists<const N: usize>(rows: &[[i64; N]]) -> Value {
    let row = |r: &[i64; N]| Value::List(r.iter().map(|&i| Value::Int(i)).collect());
    Value::List(rows.iter().map(row).collect())
}

/// Assemble a `Tensegrity` structure `Value` from nodes and raw index rows,
/// which it lowers via [`index_lists`].
///
/// `None` surfaces OMITS the `surfaces` key; `Some(rows)` inserts it, even for
/// `Some(&[])`. Absent and present-but-empty are different solver inputs, so
/// `None` is never lowered to `Value::Undef` or an empty list.
pub fn tensegrity(
    nodes: Vec<Value>,
    struts: &[[i64; 2]],
    cables: &[[i64; 2]],
    surfaces: Option<&[[i64; 3]]>,
) -> Value {
    let mut fields: PersistentMap<String, Value> = PersistentMap::default();
    fields.insert("nodes".to_string(), Value::List(nodes));
    fields.insert("struts".to_string(), index_lists(struts));
    fields.insert("cables".to_string(), index_lists(cables));
    if let Some(surfaces) = surfaces {
        fields.insert("surfaces".to_string(), index_lists(surfaces));
    }
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// The triplex as a `Tensegrity` structure: [`triplex_nodes`] at `bottom_z`,
/// with [`TRIPLEX_MEMBERS`] split at [`TRIPLEX_STRUTS`] into struts and cables.
fn triplex_tensegrity_at(bottom_z: f64, surfaces: Option<&[[i64; 3]]>) -> Value {
    let (struts, cables) = TRIPLEX_MEMBERS.split_at(TRIPLEX_STRUTS);
    tensegrity(triplex_nodes(bottom_z), struts, cables, surfaces)
}

/// The canonical triplex: circumradius 1, **unit height** (top `z = +1.0`,
/// bottom `z = 0.0`), 30° twist. This is the force-density gauge and T1b prism.
///
/// See [`tensegrity`] for what `surfaces: None` means — it is not the same input
/// as `Some(&[])`.
pub fn canonical_triplex_tensegrity(surfaces: Option<&[[i64; 3]]>) -> Value {
    triplex_tensegrity_at(CANONICAL_TRIPLEX_BOTTOM_Z, surfaces)
}

/// The taller triplex: circumradius 1, **height 2** (top `z = +1.0`, bottom
/// `z = -1.0`), 30° twist. This is the combined membrane δ prism.
///
/// See [`tensegrity`] for what `surfaces: None` means.
pub fn tall_triplex_tensegrity(surfaces: Option<&[[i64; 3]]>) -> Value {
    triplex_tensegrity_at(TALL_TRIPLEX_BOTTOM_Z, surfaces)
}

/// Both membrane end caps of the triplex, as raw index rows: the top cap over
/// the free nodes 0, 1, 2 and the bottom cap over nodes 3, 4, 5 (=
/// [`TRIPLEX_ANCHORS`]). Consumers size their per-cap σ arrays from
/// `TRIPLEX_CAPS.len()`.
pub const TRIPLEX_CAPS: [[i64; 3]; 2] = [[0, 1, 2], [3, 4, 5]];

/// Group ids in [`TRIPLEX_MEMBERS`] order: the three struts to group 0, the six
/// horizontals (top and bottom rings) to group 1, the three verticals to group 2.
const TRIPLEX_GROUP_IDS: [i64; TRIPLEX_MEMBERS.len()] = [0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2];

/// [`TRIPLEX_GROUP_IDS`] lowered to the `List<Int>` the free-standing solve
/// takes. `Value::Int` is part of the contract: the kernel reads group ids as
/// integers and indexes the seed list BY them.
pub fn triplex_group_ids() -> Value {
    Value::List(TRIPLEX_GROUP_IDS.into_iter().map(Value::Int).collect())
}

/// One seed ratio per group, in group-id order: struts compressive (−1),
/// horizontals and verticals tensile (+1). The distinct group ids must be exactly
/// `0..TRIPLEX_SEEDS.len()`.
pub const TRIPLEX_SEEDS: [f64; 3] = [-1.0, 1.0, 1.0];

/// [`TRIPLEX_SEEDS`] lowered to the dimensionless `List<Real>` the free-standing
/// solve takes.
pub fn triplex_seeds() -> Value {
    Value::List(TRIPLEX_SEEDS.into_iter().map(Value::Real).collect())
}

/// The tent membrane, as raw coordinates: a diamond boundary of four anchored
/// corners ([`TENT_ANCHORS`]) in the `z = 0` plane plus one free interior node
/// seeded OFF that plane, fanned by [`TENT_TRIS`].
///
/// The minimal surface spanning a planar boundary is flat, so a correct
/// cotangent assembly pulls the free node back into the boundary plane
/// (`z → 0`) and leaves a ~0 equilibrium residual, while a wrong assembly drives
/// it off-plane or blows up the residual. The off-plane seed is what makes that
/// a non-circular signal. The in-plane `(x, y)` equilibrium is NOT unique — the
/// flat surface has constant area for any interior position, so the
/// cotangent-Laplacian vanishes across the whole interior — hence consumers
/// assert planarity + residual, never an `(x, y)`.
pub const TENT_NODE_COORDS: [[f64; 3]; 5] = [
    [0.1, 0.1, 0.3],  // 0: free interior — deliberately off-solution
    [1.0, 0.0, 0.0],  // 1: anchor
    [0.0, 1.0, 0.0],  // 2: anchor
    [-1.0, 0.0, 0.0], // 3: anchor
    [0.0, -1.0, 0.0], // 4: anchor
];

/// The tent's triangle fan: one triangle per anchored corner, each hinged on
/// the free node 0. Consumers size their per-triangle σ arrays from
/// `TENT_TRIS.len()`.
pub const TENT_TRIS: [[i64; 3]; 4] = [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 1]];

/// The tent's anchored node set: the four corners, leaving node 0 free.
pub const TENT_ANCHORS: [i64; 4] = [1, 2, 3, 4];

/// The tent as a `Tensegrity` structure: a pure membrane, so struts and cables
/// are present but empty, and `surfaces` carries [`TENT_TRIS`].
pub fn tent_membrane_tensegrity() -> Value {
    tensegrity(lift_points(TENT_NODE_COORDS), &[], &[], Some(&TENT_TRIS))
}

/// Catenoid waist `c` in `r(z) = c·cosh(z/c)`: the minimum radius, at the waist
/// `z = 0`.
const CATENOID_WAIST: f64 = 1.0;

/// Half-height: the two anchored boundary rings sit at `z = ±H`. `H/c = 0.8`
/// keeps the patch on the STABLE, globally area-minimizing catenoid branch, well
/// below the `t·tanh(t) = 1 ⇒ t ≈ 1.2` existence limit and the `t ≈ 1.056`
/// Goldschmidt two-disk threshold, so form-finding has a clean minimum to settle
/// into.
const CATENOID_HALF_HEIGHT: f64 = 0.8;

/// The free-ring seed amplitude both catenoid consumers use: large enough that
/// the solve must do real work, small enough to stay in the catenoid's basin of
/// attraction. The γ golden's measured shape bounds are calibrated at this seed.
pub const CATENOID_SEED_PERTURB: f64 = 0.02;

/// Analytic catenoid radius at height `z`.
pub fn catenoid_radius(z: f64) -> f64 {
    CATENOID_WAIST * (z / CATENOID_WAIST).cosh()
}

/// Deterministic, RNG-free perturbation in `[-1, 1]` keyed on two indices: a
/// fixed trig hash keeps the seed reproducible and well spread.
fn jitter(a: usize, b: usize) -> f64 {
    ((a as f64) * 12.9898 + (b as f64) * 78.233).sin()
}

/// Ring-major node id of azimuth `j` on `ring`, with `j` wrapping mod `n_theta`.
fn tube_node_id(n_theta: usize, ring: usize, j: usize) -> usize {
    ring * n_theta + (j % n_theta)
}

/// A triangulated catenoid tube built by [`catenoid_tube`].
pub struct CatenoidTube {
    /// Ring-major: ring 0 (`z = −H`) first, azimuth fastest.
    pub nodes: Vec<[f64; 3]>,
    /// Two triangles per quad between adjacent rings, facing away from the axis.
    pub surfaces: Vec<(usize, usize, usize)>,
    /// The two boundary rings, bottom then top: the fixed data of the BVP.
    pub anchors: Vec<usize>,
    /// The interior rings, bottom to top.
    pub free: Vec<usize>,
    n_theta: usize,
}

impl CatenoidTube {
    /// Id of azimuth `j` on `ring`. `j` wraps at the ring size, so
    /// `node_id(ring, j + 1)` closes a hoop.
    pub fn node_id(&self, ring: usize, j: usize) -> usize {
        tube_node_id(self.n_theta, ring, j)
    }
}

/// The catenoid tube: `n_theta` nodes per ring and `n_axial` axial segments, so
/// `n_axial + 1` rings from `z = −H` to `z = +H`. The two boundary rings are
/// anchored exactly on the analytic catenoid; the interior rings are free,
/// seeded on it plus a deterministic radial (≤ `perturb`) and axial
/// (≤ `perturb / 2`) offset so a solve must genuinely move them back.
///
/// Raw types rather than `Value`, because the consumers call
/// reify-solver-elastic's anchored surface solves below the `Value` layer.
pub fn catenoid_tube(n_theta: usize, n_axial: usize, perturb: f64) -> CatenoidTube {
    assert!(
        n_theta >= 3,
        "catenoid_tube needs n_theta >= 3 nodes per ring, got {n_theta}"
    );
    assert!(
        n_axial >= 1,
        "catenoid_tube needs n_axial >= 1 segment between the anchored rings, got {n_axial}"
    );
    let h = CATENOID_HALF_HEIGHT;
    let n_rings = n_axial + 1;
    let node_id = |ring: usize, j: usize| tube_node_id(n_theta, ring, j);
    let mut nodes = vec![[0.0_f64; 3]; n_rings * n_theta];
    let mut anchors = Vec::new();
    let mut free = Vec::new();

    for ring in 0..n_rings {
        let z = -h + 2.0 * h * (ring as f64) / (n_axial as f64);
        let r_true = catenoid_radius(z);
        let is_boundary = ring == 0 || ring == n_rings - 1;
        for j in 0..n_theta {
            let theta = 2.0 * std::f64::consts::PI * (j as f64) / (n_theta as f64);
            let id = node_id(ring, j);
            if is_boundary {
                nodes[id] = [r_true * theta.cos(), r_true * theta.sin(), z];
                anchors.push(id);
            } else {
                let r = r_true + perturb * jitter(ring, j);
                let dz = 0.5 * perturb * jitter(j, ring);
                nodes[id] = [r * theta.cos(), r * theta.sin(), z + dz];
                free.push(id);
            }
        }
    }

    let mut surfaces = Vec::new();
    for ring in 0..n_axial {
        for j in 0..n_theta {
            let a = node_id(ring, j);
            let b = node_id(ring, j + 1);
            let c = node_id(ring + 1, j);
            let d = node_id(ring + 1, j + 1);
            surfaces.push((a, b, c));
            surfaces.push((b, d, c));
        }
    }

    CatenoidTube {
        nodes,
        surfaces,
        anchors,
        free,
        n_theta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use std::collections::BTreeMap;

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

    /// The canonical prism: circumradius 1, top ring z = +1.0 at azimuth 120°·i,
    /// bottom ring z = 0.0 at azimuth 120°·i + 30°. All 18 raw components
    /// pinned, and `triplex_nodes` re-checked to carry exactly those components
    /// so the raw seam and the `Value` form cannot drift apart.
    #[test]
    fn canonical_triplex_geometry_is_pinned_bit_for_bit() {
        let coords = triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z);
        let nodes = triplex_nodes(CANONICAL_TRIPLEX_BOTTOM_Z);
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
        for (i, n) in triplex_nodes(CANONICAL_TRIPLEX_BOTTOM_Z).iter().enumerate() {
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
        let canonical = triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z);
        let tall = triplex_node_coords(TALL_TRIPLEX_BOTTOM_Z);
        assert_eq!(tall.len(), 6, "the tall triplex also has exactly 6 nodes");

        for i in 0..6 {
            assert_bits_eq(tall[i][0], canonical[i][0], &format!("node {i} x must not move"));
            assert_bits_eq(tall[i][1], canonical[i][1], &format!("node {i} y must not move"));
            let (ring, want_z) = if i < 3 {
                ("top", 1.0)
            } else {
                ("bottom", TALL_TRIPLEX_BOTTOM_Z)
            };
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
    const STUB_STRUTS: [[i64; 2]; 1] = [[0, 4]];
    const STUB_CABLES: [[i64; 2]; 1] = [[0, 1]];

    /// The `Tensegrity` header every consumer matches on, plus the 3-key field
    /// map T1b's superseded copy built. `None` must OMIT `surfaces`.
    #[test]
    fn tensegrity_with_none_surfaces_omits_the_key_entirely() {
        let v = tensegrity(
            triplex_nodes(CANONICAL_TRIPLEX_BOTTOM_Z),
            &STUB_STRUTS,
            &STUB_CABLES,
            None,
        );

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

    /// `Some(rows)` inserts the key, giving the 4-key map the gauge and δ copies
    /// built, with the rows lowered the way the DSL lowers them.
    #[test]
    fn tensegrity_with_some_surfaces_inserts_the_key() {
        let v = tensegrity(
            triplex_nodes(CANONICAL_TRIPLEX_BOTTOM_Z),
            &STUB_STRUTS,
            &STUB_CABLES,
            Some(&TRIPLEX_CAPS),
        );

        let fields = structure_fields(&v);
        assert_eq!(fields.len(), 4, "nodes/struts/cables/surfaces");
        assert_eq!(
            fields.get("surfaces"),
            Some(&index_lists(&TRIPLEX_CAPS)),
            "`surfaces` carries the caps, lowered to nested Int lists"
        );
    }

    /// THE distinction that makes a naive one-fixture collapse unsafe: a
    /// PRESENT-but-empty `surfaces` list is a different input from an ABSENT
    /// `surfaces` key. δ's backward-compat test asserts the no-surfaces path
    /// returns an EMPTY echo and never Undef/absent, so both shapes have to stay
    /// reachable and distinguishable.
    #[test]
    fn present_but_empty_surfaces_is_distinct_from_absent_surfaces() {
        let empty = canonical_triplex_tensegrity(Some(&[]));
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
            Some(&Value::List(triplex_nodes(CANONICAL_TRIPLEX_BOTTOM_Z))),
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
        let tall = tall_triplex_tensegrity(Some(&TRIPLEX_CAPS));
        let canonical = canonical_triplex_tensegrity(None);
        let tall_fields = structure_fields(&tall);
        let canonical_fields = structure_fields(&canonical);

        assert_eq!(
            tall_fields.get("nodes"),
            Some(&Value::List(triplex_nodes(TALL_TRIPLEX_BOTTOM_Z))),
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
        assert_eq!(tall_fields.get("surfaces"), Some(&index_lists(&TRIPLEX_CAPS)));
    }

    /// A `Value::List` of `Value::Int`, strictly. The `Int` variant is part of the
    /// contract rather than incidental: the kernel reads group ids as integers and
    /// indexes the seed list by them.
    fn int_list(v: &Value, what: &str) -> Vec<i64> {
        match v {
            Value::List(items) => items
                .iter()
                .map(|item| match item {
                    Value::Int(i) => *i,
                    other => panic!("{what}: expected a Value::Int entry, got {other:?}"),
                })
                .collect(),
            other => panic!("{what}: expected a Value::List, got {other:?}"),
        }
    }

    /// A `Value::List` of `Value::Real`, strictly: seed ratios are DIMENSIONLESS
    /// relative ratios, so a dimensioned `Value::Scalar` here would be a different
    /// input to the free-standing solve and must not pass silently.
    fn real_list(v: &Value, what: &str) -> Vec<f64> {
        match v {
            Value::List(items) => items
                .iter()
                .map(|item| match item {
                    Value::Real(r) => *r,
                    other => panic!("{what}: expected a Value::Real entry, got {other:?}"),
                })
                .collect(),
            other => panic!("{what}: expected a Value::List, got {other:?}"),
        }
    }

    /// How [`triplex_group_ids`] and [`triplex_seeds`] have to fit together for the
    /// free-standing solve to be well posed: one id per member, ids that are exactly
    /// the seed list's own index range, and the strut/cable sign contract the gauge
    /// re-asserts downstream on the solved force densities.
    #[test]
    fn triplex_group_ids_index_the_seeds_and_honour_the_sign_contract() {
        let groups = int_list(&triplex_group_ids(), "triplex_group_ids");
        let seeds = real_list(&triplex_seeds(), "triplex_seeds");

        assert_eq!(
            groups.len(),
            TRIPLEX_MEMBERS.len(),
            "one group id per member, in TRIPLEX_MEMBERS order"
        );

        let mut distinct = groups.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct,
            (0..seeds.len() as i64).collect::<Vec<_>>(),
            "group ids must be exactly 0..triplex_seeds().len(): the kernel indexes the seed \
             list BY group id, so a gap or an out-of-range id is a dimension error there"
        );

        for (m, &g) in groups.iter().enumerate() {
            let [j, k] = TRIPLEX_MEMBERS[m];
            let seed = seeds[g as usize];
            let (kind, sign_ok) =
                if m < TRIPLEX_STRUTS { ("strut", seed < 0.0) } else { ("cable", seed > 0.0) };
            assert!(
                sign_ok,
                "{kind} {m} ({j},{k}) is in group {g}, whose seed is {seed}: struts must seed \
                 compressive (< 0) and cables tensile (> 0)"
            );
        }
    }

    /// A group is a SYMMETRY class of the prism, which is what fixes WHICH members
    /// share an id — the counting above cannot tell a horizontals/verticals swap from
    /// the real grouping. Congruent members have equal length, and the three classes
    /// have pairwise different lengths, so the partition is recoverable from the
    /// geometry alone.
    #[test]
    fn triplex_groups_are_the_symmetry_classes_of_the_prism() {
        let groups = int_list(&triplex_group_ids(), "triplex_group_ids");
        let coords = triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z);
        let member_len = |m: usize| {
            let [j, k] = TRIPLEX_MEMBERS[m];
            let (a, b) = (coords[j as usize], coords[k as usize]);
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };

        let mut classes = groups.clone();
        classes.sort_unstable();
        classes.dedup();
        let representative: Vec<f64> = classes
            .iter()
            .map(|&g| {
                let first = groups.iter().position(|&x| x == g).expect("class is inhabited");
                member_len(first)
            })
            .collect();

        for (m, &g) in groups.iter().enumerate() {
            let want = representative[classes.iter().position(|&c| c == g).unwrap()];
            let [j, k] = TRIPLEX_MEMBERS[m];
            assert!(
                (member_len(m) - want).abs() < 1e-12,
                "member {m} ({j},{k}) is in group {g} but its length {} differs from that \
                 group's {want} — members sharing a force density must be congruent",
                member_len(m)
            );
        }

        for (a, la) in representative.iter().enumerate() {
            for (b, lb) in representative.iter().enumerate().skip(a + 1) {
                assert!(
                    (la - lb).abs() > 1e-9,
                    "groups {} and {} are both {la}-long, so the grouping does not distinguish \
                     two genuinely different member families",
                    classes[a],
                    classes[b]
                );
            }
        }
    }

    /// The caps are the prism's two RINGS, read off the geometry instead of restated:
    /// they partition the node set, each lies in one z plane, cap 0 is the FREE top
    /// ring (so the cap enters `D_ff` rather than sitting inertly on the anchored
    /// side) and cap 1 is exactly the anchored set.
    #[test]
    fn triplex_caps_are_the_prisms_two_rings() {
        let coords = triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z);

        let mut covered: Vec<i64> = TRIPLEX_CAPS.concat();
        covered.sort_unstable();
        assert_eq!(
            covered,
            (0..coords.len() as i64).collect::<Vec<_>>(),
            "the caps must partition the node set — every node in exactly one ring"
        );

        for cap in TRIPLEX_CAPS {
            let z = coords[cap[0] as usize][2];
            for &n in &cap {
                assert_bits_eq(
                    coords[n as usize][2],
                    z,
                    &format!("cap {cap:?}: node {n} must share the ring's z plane"),
                );
            }
        }

        assert_bits_eq(
            coords[TRIPLEX_CAPS[0][0] as usize][2],
            TRIPLEX_TOP_Z,
            "cap 0 must be the TOP ring: its three nodes are the free ones of the anchored solve",
        );
        assert_eq!(
            TRIPLEX_CAPS[1], TRIPLEX_ANCHORS,
            "cap 1 must be the bottom ring, which is exactly the anchored set"
        );
    }

    /// The tent lowered to a `Tensegrity` Value: a PURE membrane — `surfaces`
    /// PRESENT and carrying [`TENT_TRIS`], struts and cables present but empty —
    /// over nodes bit-identical to [`TENT_NODE_COORDS`].
    #[test]
    fn tent_membrane_tensegrity_lowers_the_golden() {
        let v = tent_membrane_tensegrity();
        let Value::StructureInstance(d) = &v else {
            panic!("expected a Value::StructureInstance, got {v:?}");
        };
        assert_eq!(
            d.type_name, "Tensegrity",
            "the type name every consumer matches on"
        );

        let fields = &d.fields;
        assert_eq!(
            fields.len(),
            4,
            "nodes/struts/cables/surfaces — `surfaces` must be PRESENT"
        );
        assert_eq!(
            fields.get("surfaces"),
            Some(&index_lists(&TENT_TRIS)),
            "`surfaces` carries the tent fan, lowered to nested Int lists"
        );
        for key in ["struts", "cables"] {
            assert_eq!(
                fields.get(key),
                Some(&Value::List(vec![])),
                "a pure membrane: `{key}` must be present and empty"
            );
        }

        let nodes = match fields.get("nodes") {
            Some(Value::List(nodes)) => nodes,
            other => panic!("`nodes` must be a Value::List, got {other:?}"),
        };
        assert_eq!(
            nodes.len(),
            TENT_NODE_COORDS.len(),
            "one node per TENT_NODE_COORDS row"
        );
        for (i, (node, want)) in nodes.iter().zip(TENT_NODE_COORDS).enumerate() {
            let lifted = point_components(node);
            for (axis, c) in ["x", "y", "z"].iter().zip(0..3) {
                assert_bits_eq(lifted[c], want[c], &format!("tent node {i} {axis}"));
            }
        }
    }

    /// The relations the tent's consumers lean on, read off the consts rather than
    /// restated: a planar anchored boundary, ONE free node seeded off that plane
    /// (without which "the free node returns to z = 0" is vacuous), and a closed
    /// fan of triangles around that free node.
    #[test]
    fn tent_golden_is_a_planar_anchored_fan_around_one_off_plane_free_node() {
        let node_count = TENT_NODE_COORDS.len() as i64;
        for a in TENT_ANCHORS {
            assert!(
                (0..node_count).contains(&a),
                "anchor {a} must index a tent node"
            );
        }
        let mut distinct = TENT_ANCHORS.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            TENT_ANCHORS.len(),
            "anchors must be distinct"
        );

        let free_nodes: Vec<i64> = (0..node_count)
            .filter(|n| !TENT_ANCHORS.contains(n))
            .collect();
        let [free] = free_nodes[..] else {
            panic!("the tent must have exactly one free node, got {free_nodes:?}");
        };

        for a in TENT_ANCHORS {
            assert_bits_eq(
                TENT_NODE_COORDS[a as usize][2],
                0.0,
                &format!("anchor {a} must lie in the z = 0 boundary plane"),
            );
        }
        let free_z = TENT_NODE_COORDS[free as usize][2];
        assert!(
            free_z.abs() > 1e-6,
            "free node {free} must be seeded OFF the boundary plane, got z = {free_z}"
        );

        for tri in TENT_TRIS {
            let corners: Vec<i64> = tri.into_iter().filter(|&n| n != free).collect();
            let [c0, c1] = corners[..] else {
                panic!("triangle {tri:?} must be hinged on free node {free} exactly once");
            };
            assert!(
                TENT_ANCHORS.contains(&c0) && TENT_ANCHORS.contains(&c1) && c0 != c1,
                "triangle {tri:?} must span two distinct anchors besides the free node"
            );
        }
        for a in TENT_ANCHORS {
            let uses = TENT_TRIS.iter().filter(|tri| tri.contains(&a)).count();
            assert_eq!(
                uses, 2,
                "anchor {a} must sit in exactly two triangles for the fan to close"
            );
        }
    }

    /// The tube resolutions the consumers solve at: the gauge-covariance suite's
    /// 8×2, and the γ golden's coarse 16×3 and fine 32×6.
    const TUBE_RESOLUTIONS: [(usize, usize); 3] = [(8, 2), (16, 3), (32, 6)];

    /// Both seeds: exactly on the catenoid, and the shared perturbed seed.
    const TUBE_PERTURBS: [f64; 2] = [0.0, CATENOID_SEED_PERTURB];

    /// Every `(n_theta, n_axial, perturb)` tube of [`TUBE_RESOLUTIONS`] ×
    /// [`TUBE_PERTURBS`], with its label for assertion messages.
    fn every_tube() -> impl Iterator<Item = (usize, usize, String, CatenoidTube)> {
        TUBE_RESOLUTIONS.into_iter().flat_map(|(n_theta, n_axial)| {
            TUBE_PERTURBS.into_iter().map(move |perturb| {
                let at = format!("{n_theta}×{n_axial} tube at perturb {perturb}");
                (
                    n_theta,
                    n_axial,
                    at,
                    catenoid_tube(n_theta, n_axial, perturb),
                )
            })
        })
    }

    /// Distance of a point from the tube's (z) axis.
    fn axial_distance(p: [f64; 3]) -> f64 {
        (p[0] * p[0] + p[1] * p[1]).sqrt()
    }

    /// The ring a node id lies on: the inverse of the ring-major layout pinned by
    /// `catenoid_tube_lays_out_rings_by_azimuth`.
    fn ring_of(n_theta: usize, id: usize) -> usize {
        id / n_theta
    }

    /// How many triangles share each undirected edge.
    fn edge_multiplicities(surfaces: &[(usize, usize, usize)]) -> BTreeMap<(usize, usize), usize> {
        let mut counts = BTreeMap::new();
        for &(a, b, c) in surfaces {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                *counts.entry((u.min(v), u.max(v))).or_insert(0) += 1;
            }
        }
        counts
    }

    fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// Ring-major layout: ring 0 (z = −H) first, azimuth fastest. The boundary
    /// rings are the anchors and the interior rings the free nodes, both in id
    /// order, and `node_id` wraps the azimuth so ring members can close a hoop.
    #[test]
    fn catenoid_tube_lays_out_rings_by_azimuth() {
        for (n_theta, n_axial, at, tube) in every_tube() {
            assert_eq!(
                tube.nodes.len(),
                (n_axial + 1) * n_theta,
                "{at}: n_theta nodes on each of n_axial + 1 rings"
            );
            assert_eq!(
                tube.surfaces.len(),
                2 * n_axial * n_theta,
                "{at}: two triangles per quad"
            );

            let ring_ids = move |ring: usize| (0..n_theta).map(move |j| ring * n_theta + j);
            let want_anchors: Vec<usize> = ring_ids(0).chain(ring_ids(n_axial)).collect();
            let want_free: Vec<usize> = (1..n_axial).flat_map(ring_ids).collect();
            assert_eq!(
                tube.anchors, want_anchors,
                "{at}: anchors are the bottom ring's ids, then the top ring's"
            );
            assert_eq!(
                tube.free, want_free,
                "{at}: free nodes are the interior rings' ids, bottom to top"
            );

            let mut all: Vec<usize> = tube.anchors.iter().chain(&tube.free).copied().collect();
            all.sort_unstable();
            assert_eq!(
                all,
                (0..tube.nodes.len()).collect::<Vec<_>>(),
                "{at}: anchors and free nodes partition the node set"
            );

            for ring in 0..=n_axial {
                for j in 0..n_theta {
                    assert_eq!(
                        tube.node_id(ring, j),
                        ring * n_theta + j,
                        "{at}: ring-major id of ({ring}, {j})"
                    );
                    assert_eq!(
                        tube.node_id(ring, j + n_theta),
                        tube.node_id(ring, j),
                        "{at}: the azimuth of ({ring}, {j}) must wrap at n_theta"
                    );
                }
            }
        }
    }

    /// The anchors are the boundary data of the BVP, so they sit ON the analytic
    /// catenoid: bottom ring at z = −H, top ring at z = +H. Epsilon compares,
    /// not bit: the ring height `−H + 2H·n/n` and `√(cos²θ + sin²θ)` are not
    /// exact for every `n`.
    #[test]
    fn catenoid_tube_anchors_sit_on_the_analytic_catenoid() {
        assert_eq!(
            catenoid_radius(0.0),
            CATENOID_WAIST,
            "the waist radius is c, at z = 0"
        );
        for z in [0.25, CATENOID_HALF_HEIGHT, 1.5] {
            assert_eq!(
                catenoid_radius(z),
                catenoid_radius(-z),
                "the catenoid is symmetric about its waist"
            );
        }

        for (n_theta, _, at, tube) in every_tube() {
            for &a in &tube.anchors {
                let p = tube.nodes[a];
                let want_z = if ring_of(n_theta, a) == 0 {
                    -CATENOID_HALF_HEIGHT
                } else {
                    CATENOID_HALF_HEIGHT
                };
                assert!(
                    (p[2] - want_z).abs() < 1e-15,
                    "{at}: anchor {a} must sit on its boundary plane z = {want_z}, got {}",
                    p[2]
                );
                assert!(
                    (axial_distance(p) - catenoid_radius(p[2])).abs() < 1e-12,
                    "{at}: anchor {a} must lie on the catenoid: r = {}, want {}",
                    axial_distance(p),
                    catenoid_radius(p[2])
                );
            }
        }
    }

    /// The seed moves ONLY the free rings, and by at most its amplitude:
    /// radially by ≤ `perturb`, axially by ≤ `perturb / 2`. The unperturbed tube
    /// is the reference (every node on the catenoid, at its ring's height), and
    /// the non-vacuity floor proves the seed really is off the catenoid.
    #[test]
    fn catenoid_tube_seed_moves_only_the_free_rings() {
        let perturb = CATENOID_SEED_PERTURB;
        for (n_theta, n_axial) in TUBE_RESOLUTIONS {
            let at = format!("{n_theta}×{n_axial} tube");
            let on = catenoid_tube(n_theta, n_axial, 0.0);
            let seeded = catenoid_tube(n_theta, n_axial, perturb);

            for (i, &p) in on.nodes.iter().enumerate() {
                assert!(
                    (axial_distance(p) - catenoid_radius(p[2])).abs() < 1e-12,
                    "{at}: unperturbed node {i} must lie on the catenoid"
                );
            }

            for &a in &seeded.anchors {
                for (axis, c) in ["x", "y", "z"].iter().zip(0..3) {
                    assert_bits_eq(
                        seeded.nodes[a][c],
                        on.nodes[a][c],
                        &format!("{at}: anchor {a} {axis} must not move with the seed"),
                    );
                }
            }

            let mut largest_radial = 0.0_f64;
            for &f in &seeded.free {
                let (p, ring_z) = (seeded.nodes[f], on.nodes[f][2]);
                let radial = (axial_distance(p) - catenoid_radius(ring_z)).abs();
                let axial = (p[2] - ring_z).abs();
                assert!(
                    radial <= perturb,
                    "{at}: free node {f} moved {radial} radially, more than {perturb}"
                );
                assert!(
                    axial <= 0.5 * perturb,
                    "{at}: free node {f} moved {axial} axially, more than {}",
                    0.5 * perturb
                );
                largest_radial = largest_radial.max(radial);
            }
            assert!(
                largest_radial > 0.1 * perturb,
                "{at}: the seed must move some free node off the catenoid, largest radial \
                 offset {largest_radial}"
            );
        }
    }

    /// The free-ring seed, spelled independently and pinned BIT-FOR-BIT: radius
    /// `catenoid_radius(z) + p·sin(12.9898·ring + 78.233·j)`, height
    /// `z + ½·p·sin(12.9898·j + 78.233·ring)`, azimuth `2π·j/n_theta`. The γ
    /// golden's measured shape bounds are calibrated against exactly this seed,
    /// so a changed hash must fail here first.
    #[test]
    fn catenoid_tube_seed_is_the_documented_trig_hash() {
        let (n_theta, n_axial, p) = (8, 2, CATENOID_SEED_PERTURB);
        let tube = catenoid_tube(n_theta, n_axial, p);
        let h = CATENOID_HALF_HEIGHT;

        for ring in 1..n_axial {
            let (ring_f, z) = (ring as f64, -h + 2.0 * h * (ring as f64) / (n_axial as f64));
            for j in 0..n_theta {
                let j_f = j as f64;
                let r = catenoid_radius(z) + p * (12.9898 * ring_f + 78.233 * j_f).sin();
                let seeded_z = z + 0.5 * p * (12.9898 * j_f + 78.233 * ring_f).sin();
                let theta = 2.0 * std::f64::consts::PI * j_f / (n_theta as f64);
                let got = tube.nodes[tube.node_id(ring, j)];
                assert_bits_eq(
                    got[0],
                    r * theta.cos(),
                    &format!("free node ({ring}, {j}) x"),
                );
                assert_bits_eq(
                    got[1],
                    r * theta.sin(),
                    &format!("free node ({ring}, {j}) y"),
                );
                assert_bits_eq(got[2], seeded_z, &format!("free node ({ring}, {j}) z"));
            }
        }
    }

    /// The triangulation is a closed strip between the two boundary rings: no
    /// duplicate face, an edge on a boundary ring borders exactly ONE triangle
    /// and every other edge exactly TWO (no gap, no seam), and every triangle's
    /// normal `(b − a) × (c − a)` points away from the axis.
    #[test]
    fn catenoid_tube_is_a_closed_consistently_oriented_strip() {
        for (n_theta, n_axial, at, tube) in every_tube() {
            let mut faces: Vec<[usize; 3]> = tube
                .surfaces
                .iter()
                .map(|&(a, b, c)| {
                    let mut face = [a, b, c];
                    face.sort_unstable();
                    face
                })
                .collect();
            faces.sort_unstable();
            faces.dedup();
            assert_eq!(faces.len(), tube.surfaces.len(), "{at}: no face may repeat");

            let mut boundary_edges = 0;
            for ((u, v), count) in edge_multiplicities(&tube.surfaces) {
                let ring = ring_of(n_theta, u);
                let on_boundary_ring =
                    ring == ring_of(n_theta, v) && (ring == 0 || ring == n_axial);
                let want = if on_boundary_ring { 1 } else { 2 };
                assert_eq!(
                    count, want,
                    "{at}: edge ({u}, {v}) borders {count} triangles"
                );
                boundary_edges += usize::from(on_boundary_ring);
            }
            assert_eq!(
                boundary_edges,
                2 * n_theta,
                "{at}: both boundary rings are closed"
            );

            for &(a, b, c) in &tube.surfaces {
                let (pa, pb, pc) = (tube.nodes[a], tube.nodes[b], tube.nodes[c]);
                let normal = cross3(sub3(pb, pa), sub3(pc, pa));
                let centroid_xy = [(pa[0] + pb[0] + pc[0]) / 3.0, (pa[1] + pb[1] + pc[1]) / 3.0];
                let outward = normal[0] * centroid_xy[0] + normal[1] * centroid_xy[1];
                assert!(
                    outward > 0.0,
                    "{at}: triangle ({a}, {b}, {c}) must face away from the axis"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "catenoid_tube needs n_axial >= 1")]
    fn catenoid_tube_rejects_a_degenerate_axial_resolution() {
        catenoid_tube(8, 0, 0.0);
    }

    #[test]
    #[should_panic(expected = "catenoid_tube needs n_theta >= 3")]
    fn catenoid_tube_rejects_a_degenerate_azimuthal_resolution() {
        catenoid_tube(2, 2, 0.0);
    }
}
