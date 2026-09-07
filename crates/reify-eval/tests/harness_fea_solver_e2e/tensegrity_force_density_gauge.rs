//! Runtime lock on the tensegrity force-density **dimensional bridge** (task
//! #6095). NORMATIVE statement: the "Dimensional bridge" paragraph in
//! `crates/reify-compiler/stdlib/tensegrity.ri` — this module is only its
//! runtime evidence and points at it rather than restating it.
//!
//! What it pins that nothing else does: every other eval assertion on these
//! fields routes through a dimension-BLIND helper (`force_val` / `coord`) taking
//! `Value::Scalar{..}` or `Value::Real(..)` interchangeably, so the trampoline
//! could silently retag either way with no Rust test failing. The extractors
//! below inspect the DIMENSION explicitly and panic on the wrong one — FORCE
//! required on `member_forces`, none tolerated on the qᵢ/σ echoes — across all
//! three emission sites: anchored line-only, anchored surfaces, free-standing.
//!
//! SCOPE — gauge COVARIANCE is asserted at BOTH emitters (the last two tests: anchored
//! via `solve_at`, free-standing via `solve_free_at`), because they reach the property by
//! different mechanisms — algebraic homogeneity of `D_ff x_f = −D_fa x_a` anchored, versus
//! homogeneity of the GroupRatios search that fixes the gauge from `reference_group` when
//! free-standing. Both fixtures are LINE-ONLY; the SURFACES path is the one deliberately
//! scoped out, for TWO reasons. (1) The gauge is the (q, σ) PAIR: `D = CᵀQC + Σ_T σ_T·L_T`
//! is linear in the pair, not in q alone, so a surfaces covariance experiment must rescale
//! every σ_T by λ too — scaling q alone shifts the q/σ balance and MOVES the free nodes,
//! which is physics, not a defect. (2) Even rescaled as a pair, surfaces convergence is
//! judged on an ABSOLUTE tolerance on a residual not normalised by |D| (itself linear in
//! q) — solver-side, outside #6095's scope, filed as #6119 (dup #6124).

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// A 3-component `Value::Point` of SI-metre coordinates — how `point3` lowers.
/// Every `#[path]` sibling in this module directory carries its own copy (`node` in
/// `tensegrity_t1b_form_find_e2e.rs`, `tensegrity_t3b_load.rs`, …) because each was
/// written standalone and the helpers stayed private. Collapsing the family is tracked
/// with the fixture duplication below (#6152) — see that note for what still blocks it.
fn node(x: f64, y: f64, z: f64) -> Value {
    let m = |v: f64| Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH };
    Value::Point(vec![m(x), m(y), m(z)])
}

// DUPLICATION, tracked not silent: the prism geometry + `MEMBERS` topology below is a
// THIRD copy of the canonical triplex (see `canonical_prism_nodes()` /
// `triplex_tensegrity()` in `harness_fea_solver_e2e/tensegrity_t1b_form_find_e2e.rs`
// and the combined fixture in `…/tensegrity_delta_combined_form_find_e2e.rs`), and
// `membrane_tensegrity()` re-derives the kernel's `tent_membrane()` golden. A topology
// or node-order change must be mirrored by hand across all three, so collapsing them
// onto one shared fixture is tracked by task **#6152**.
//
// WHAT BLOCKS IT HERE — narrower than it once was. This module now sits as a `#[path]`
// sibling of the other two INSIDE the same `harness_fea_solver_e2e` compile unit, so
// the dedup no longer needs `reify-test-support` at all: one shared fixture module in
// `harness_fea_solver_e2e/`, or `pub(crate)` on the helpers that already exist, reaches
// every call site. What it does need is DELETING the two existing copies from
// `tensegrity_t1b_form_find_e2e.rs` and `tensegrity_delta_combined_form_find_e2e.rs`,
// and neither file is in #6095's locked module set. Adding a fourth copy in a new
// shared file without removing those two would raise the drift surface, not lower it —
// so #6152 owns the collapse, with both siblings in ITS scope.

/// Struts-then-cables member order — the one index space `force_densities` and
/// `member_forces` share: 3 struts, then top / bottom / vertical cable triples.
const MEMBERS: [(usize, usize); 12] = [
    (0, 4), (1, 5), (2, 3), (0, 1), (1, 2), (2, 0),
    (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5),
];

/// `MEMBERS[..STRUTS]` are the struts (compression, q < 0); the rest are cables
/// (tension, q > 0). That split is what lets `assert_bridge_holds` re-assert the
/// documented sign contract instead of merely checking finiteness.
const STRUTS: usize = 3;

/// Bottom triangle {3,4,5} anchored; top triangle {0,1,2} free.
const ANCHORS: [i64; 3] = [3, 4, 5];

/// Base force densities in `MEMBERS` order; signs honour the hard contract
/// (struts q < 0, cables q > 0). Verticals are 2, not 1, on purpose: at q = 1
/// everywhere `D_ff` has zero row sums and is exactly singular.
const BASE_Q: [f64; 12] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0];

/// The canonical triplex prism (R=1, height=1, twist=30°; top 0,1,2 at z=1,
/// bottom 3,4,5 at z=0) — `canonical_prism_nodes()` in `tensegrity_t1b_…`.
fn prism_nodes() -> Vec<Value> {
    let ring = |i: usize, twist: f64, z: f64| {
        let a = (120.0 * (i as f64) + twist).to_radians();
        node(a.cos(), a.sin(), z)
    };
    let mut v: Vec<Value> = (0..3).map(|i| ring(i, 0.0, 1.0)).collect();
    v.extend((0..3).map(|i| ring(i, 30.0, 0.0)));
    v
}

/// Assemble a `Tensegrity` Value from raw node / strut / cable / surface fields.
fn tensegrity(nodes: Vec<Value>, struts: Value, cables: Value, surfaces: Value) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(nodes)),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
        ("surfaces".to_string(), surfaces),
    ].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// Index-tuple list (`[[j,k], …]` / `[[i,j,k], …]`) as the DSL lowers it.
fn index_lists<const N: usize>(rows: &[[i64; N]]) -> Value {
    let row = |r: &[i64; N]| Value::List(r.iter().map(|&i| Value::Int(i)).collect());
    Value::List(rows.iter().map(row).collect())
}

/// The triplex prism built from `MEMBERS`, carrying the given `surfaces` field.
fn prism_tensegrity_with(surfaces: Value) -> Value {
    let pair = |&(j, k): &(usize, usize)| [j as i64, k as i64];
    let struts: Vec<[i64; 2]> = MEMBERS[..STRUTS].iter().map(pair).collect();
    let cables: Vec<[i64; 2]> = MEMBERS[STRUTS..].iter().map(pair).collect();
    tensegrity(prism_nodes(), index_lists(&struts), index_lists(&cables), surfaces)
}

/// The line-only triplex prism (no surfaces).
fn prism_tensegrity() -> Value {
    prism_tensegrity_with(Value::List(vec![]))
}

/// Both membrane caps of the prism. The top cap spans the three FREE nodes, so it
/// genuinely enters `D_ff` rather than sitting inertly on the anchored side.
fn caps() -> Value {
    index_lists(&[[0, 1, 2], [3, 4, 5]])
}

/// "Tent" membrane: 4 anchored corners plus one free off-plane interior node,
/// fanned by 4 triangles, no struts/cables. Mirrors the kernel's `tent_membrane()`
/// golden — reused solely to reach the NON-EMPTY `surface_stresses` echo branch.
fn membrane_tensegrity() -> Value {
    let nodes = vec![
        node(0.1, 0.1, 0.3),  // 0: free interior — deliberately off-solution
        node(1.0, 0.0, 0.0),  // 1: anchor
        node(0.0, 1.0, 0.0),  // 2: anchor
        node(-1.0, 0.0, 0.0), // 3: anchor
        node(0.0, -1.0, 0.0), // 4: anchor
    ];
    let tris = index_lists(&[[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 1]]);
    tensegrity(nodes, Value::List(vec![]), Value::List(vec![]), tris)
}

type Trampoline = fn(
    &[Value], &[RealizationReadHandle], &Value, Option<&OpaqueState>, &CancellationHandle,
) -> ComputeOutcome;

/// Run a trampoline and return the `FormFindResult` fields, asserting the solve
/// completed cleanly and converged (a non-converged solve is vacuous here).
fn solve_with(trampoline: Trampoline, value_inputs: &[Value]) -> PersistentMap<String, Value> {
    let cancel = CancellationHandle::new();
    let outcome = trampoline(value_inputs, &[], &Value::Undef, None, &cancel);
    let fields = match outcome {
        ComputeOutcome::Completed { result: Value::StructureInstance(d), .. } => {
            assert_eq!(&d.type_name, "FormFindResult", "result must be a FormFindResult");
            d.fields
        }
        other => panic!("expected a Completed FormFindResult for a well-posed solve: {other:?}"),
    };
    let converged = fields.get(&"converged".to_string());
    assert_eq!(converged, Some(&Value::Bool(true)), "fixture must be well posed (converged)");
    fields
}

fn reals(vs: &[f64]) -> Value {
    Value::List(vs.iter().map(|&v| Value::Real(v)).collect())
}

fn ints(vs: impl IntoIterator<Item = i64>) -> Value {
    Value::List(vs.into_iter().map(Value::Int).collect())
}

/// Anchored LINE-ONLY solve of the triplex prism at the given force densities.
fn solve_at(q: &[f64]) -> PersistentMap<String, Value> {
    let inputs = [prism_tensegrity(), reals(q), ints(ANCHORS)];
    solve_with(reify_eval::compute_targets::form_find::solve_form_find_trampoline, &inputs)
}

/// Anchored SURFACES solve of the tent membrane at one isotropic σ per triangle
/// (no struts/cables ⇒ an empty `force_densities`).
fn solve_membrane(sigma: f64) -> PersistentMap<String, Value> {
    let inputs = [membrane_tensegrity(), reals(&[]), ints(1..=4), reals(&[sigma; 4])];
    solve_with(reify_eval::compute_targets::form_find::solve_form_find_trampoline, &inputs)
}

/// Anchored COMBINED solve — the prism PLUS both membrane caps, at one isotropic σ per
/// cap. This is the only fixture that reaches the anchored-SURFACES emission path with
/// a NON-EMPTY member set: `solve_membrane` has zero struts and zero cables, so
/// `member_forces` comes back empty there and neither `force_si` nor the Nᵢ = qᵢ·Lᵢ
/// pairing is exercised on that path. Shape mirrors the combined struts+cables+membrane
/// fixture of `harness_fea_solver_e2e/tensegrity_delta_combined_form_find_e2e.rs`.
fn solve_combined(q: &[f64], sigma: f64) -> PersistentMap<String, Value> {
    let inputs =
        [prism_tensegrity_with(caps()), reals(q), ints(ANCHORS), reals(&[sigma; 2])];
    solve_with(reify_eval::compute_targets::form_find::solve_form_find_trampoline, &inputs)
}

/// Seed ratios for `solve_free`, indexed by group: struts (compression) / horizontal
/// cables / vertical cables. Group 1 is the `reference_group`, so ITS magnitude is what
/// fixes the free path's gauge — the covariance test below rescales all three together.
const BASE_SEED: [f64; 3] = [-1.0, 1.0, 1.0];

/// FREE-STANDING solve of the same prism at the given per-group seed ratios (GroupRatios:
/// struts→0, the six horizontals→1, verticals→2; reference group 1) — the
/// `build_result_free` emission site, which the anchored solves above never reach.
fn solve_free_at(seed: &[f64]) -> PersistentMap<String, Value> {
    let inputs = [
        prism_tensegrity(),
        ints([0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]),
        reals(seed),
        Value::Int(1), // reference_group
    ];
    solve_with(reify_eval::compute_targets::form_find::solve_form_find_free_trampoline, &inputs)
}

/// The free-standing solve at the base gauge.
fn solve_free() -> PersistentMap<String, Value> {
    solve_free_at(&BASE_SEED)
}

fn list_field<'a>(fields: &'a PersistentMap<String, Value>, name: &str) -> &'a Vec<Value> {
    match fields.get(&name.to_string()) {
        Some(Value::List(items)) => items,
        other => panic!("FormFindResult.{name} must be a List, got {other:?}"),
    }
}

// DIMENSION-STRICT extractors — deliberately NOT the dimension-blind `force_val`/`coord`.
// Each pins the declared DIMENSION of its field and panics on anything else; only
// `member_forces` additionally requires the `Value::Scalar` representation, because a
// dimension tag can only be carried by that variant in the first place.

/// SI value of a **FORCE-dimensioned** Scalar; anything else is a violation.
fn force_si(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, dimension } if *dimension == DimensionVector::FORCE => *si_value,
        other => panic!(
            "member_forces entries must be a FORCE-dimensioned (kg·m·s⁻²) Value::Scalar, \
             matching `List<Force>` under the q_ref ≡ 1 N/m gauge of task #6095; got {other:?}"
        ),
    }
}

/// Value of a qᵢ / σ echo, pinning the Leg B DIMENSIONAL claim and nothing more: a
/// dimensioned tag is the violation, and it is the only thing rejected here.
///
/// `Value::Real(r)` and `Value::Scalar { dimension: DIMENSIONLESS }` are BOTH accepted
/// on purpose. Both satisfy the `List<Real>` declaration and both satisfy Leg B, and the
/// value model already treats them as interchangeable (`compute_targets::
/// tensegrity_crack::scalar_f64` accepts either; geometry_ops / modal_ops explicitly
/// accept DIMENSIONLESS scalars). Pinning the bare-Real REPRESENTATION on top of the
/// dimensional claim would fail a contract-preserving normalisation of the echo emitters
/// — e.g. routing them through a `scalar_list` with a dimensionless vector — which is a
/// false regression signal on a change that violates nothing #6095 adjudicated, and it
/// buys nothing the `List<Real>` declaration lock in
/// `crates/reify-compiler/tests/tensegrity_stdlib_tests.rs` does not already cover.
fn dimensionless_echo(field: &str, v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, dimension } if dimension.is_dimensionless() => *si_value,
        Value::Scalar { dimension, .. } => panic!(
            "{field} entries carry a DIMENSIONED tag ({dimension:?}) where the \
             `List<Real>` declaration requires none — THIS is the Leg B violation: the \
             qᵢ/σ are nullity-invariant ratios with no absolute scale, per the \
             dimension-checked-readers ruling upheld by #6095"
        ),
        other => panic!(
            "{field} entries must be a dimensionless number — a bare Value::Real, or a \
             DIMENSIONLESS Value::Scalar — per the `List<Real>` declaration; got a \
             variant that is neither, so the Leg B dimensional claim cannot be judged \
             (#6095): {other:?}"
        ),
    }
}

/// The three SI-metre components of a node, each strictly LENGTH-dimensioned.
fn point_xyz(v: &Value) -> [f64; 3] {
    let coord = |c: &Value| match c {
        Value::Scalar { si_value, dimension } if *dimension == DimensionVector::LENGTH => *si_value,
        other => panic!("node coordinates must be a LENGTH-dimensioned Scalar, got {other:?}"),
    };
    match v {
        Value::Point(c) if c.len() == 3 => [coord(&c[0]), coord(&c[1]), coord(&c[2])],
        other => panic!("nodes entries must be a 3-component Value::Point, got {other:?}"),
    }
}

/// Euclidean length of a member on the returned geometry.
fn member_length(nodes: &[Value], (j, k): (usize, usize)) -> f64 {
    let (a, b) = (point_xyz(&nodes[j]), point_xyz(&nodes[k]));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// THE BRIDGE, asserted end to end on one emission site's result: `member_forces` are
/// strictly FORCE-dimensioned Scalars, `force_densities` strictly dimensionless, and the
/// two pair up as `Nᵢ = qᵢ·Lᵢ·q_ref` on the geometry that same solve returned. EXACT —
/// the kernel evaluates `qi * len` from the very `out_nodes` it emits — so the only
/// error is f64 round-trip + sqrt (~1e-16 relative). One helper, both sites: the
/// anchored and free builders must not be allowed to drift apart.
fn assert_bridge_holds(fields: &PersistentMap<String, Value>, site: &str) {
    let nodes = list_field(fields, "nodes");
    let member_forces = list_field(fields, "member_forces");
    let force_densities = list_field(fields, "force_densities");
    assert_eq!(member_forces.len(), MEMBERS.len(), "{site}: one force per member");
    assert_eq!(force_densities.len(), MEMBERS.len(), "{site}: one echoed density per member");

    for (i, &m) in MEMBERS.iter().enumerate() {
        // Both extractors panic on the wrong variant / dimension.
        let q = dimensionless_echo("force_densities", &force_densities[i]);
        let n = force_si(&member_forces[i]);
        let (l, expected) = { let l = member_length(nodes, m); (l, q * l) };
        // NON-DEGENERACY, via the sign contract rather than mere finiteness: without a
        // magnitude floor, `|N − q·L| ≤ 1e-12·|q·L|` is satisfied VACUOUSLY by N=0, q=0.
        // That is a real hole on the free-standing path, where q is solver-DERIVED by
        // the GroupRatios search: a regression collapsing every searched density to zero
        // would still satisfy 0 == 0·L for every member. Signing it also pins the
        // tension/compression half of the contract for free.
        let (sign, kind) =
            if i < STRUTS { (-1.0, "strut (compression, q < 0)") } else { (1.0, "cable (tension, q > 0)") };
        assert!(
            n.is_finite() && q.is_finite() && l > 1e-6,
            "{site}: entry {i} {m:?} must be finite and non-degenerate: N={n} q={q} L={l}"
        );
        assert!(
            q * sign > 1e-9 && n * sign > 1e-9,
            "{site}: entry {i} {m:?} is a {kind}, so BOTH q and N must carry that sign \
             with magnitude > 1e-9; got q={q} N={n}. A zero/flipped density makes the \
             Nᵢ = qᵢ·Lᵢ·q_ref identity below vacuous (task #6095)"
        );
        assert!(
            (n - expected).abs() <= 1e-12 * expected.abs(),
            "{site}: member_forces[{i}] = {n} must equal qᵢ·Lᵢ·q_ref = {q} · {l} = {expected} \
             on the nodes THIS solve returned, to 1e-12 relative (q_ref ≡ 1 N/m, task #6095)"
        );
    }
}

/// The anchored emission site (`build_result`), on BOTH of its solve paths: line-only,
/// and surfaces carrying a non-empty member set. The surfaces path needs its own
/// fixture because `membrane_tensegrity` has zero struts and zero cables — there
/// `member_forces` is empty, so `force_si` and the qᵢ·Lᵢ pairing never run on it. One
/// solve each, no gauge rescale, so this stays clear of the #6119/#6124 scope-out.
#[test]
fn member_force_is_q_times_solved_length_in_the_unit_gauge() {
    let line_only = solve_at(&BASE_Q);
    assert_bridge_holds(&line_only, "anchored line-only");

    let combined = solve_combined(&BASE_Q, 0.5);
    // NON-VACUITY: prove this really is the surfaces path and not a silent
    // fall-through to the line-only solve — one σ echo per cap, and a top cap
    // spanning the three FREE nodes that measurably moves them.
    assert_eq!(
        list_field(&combined, "surface_stresses").len(),
        2,
        "the combined fixture must reach the surfaces path (one σ echo per cap)"
    );
    let moved = list_field(&line_only, "nodes")
        .iter()
        .zip(list_field(&combined, "nodes"))
        .any(|(a, b)| {
            point_xyz(a).iter().zip(point_xyz(b)).any(|(p, q)| (p - q).abs() > 1e-9)
        });
    assert!(moved, "σ on the free-node top cap must enter D_ff and move the solution");

    assert_bridge_holds(&combined, "anchored surfaces");
}

/// Both echoes carry NO dimension — a dimensioned Scalar is the Leg B violation, and
/// the bare-Real / dimensionless-Scalar representations are equally acceptable (see
/// `dimensionless_echo`). σ is asserted on a fixture that actually CARRIES surfaces, so
/// the extractor genuinely runs on it instead of skipping the line-only empty list.
#[test]
fn force_density_and_surface_stress_echoes_carry_no_dimension() {
    let line_only = solve_at(&BASE_Q);
    let force_densities = list_field(&line_only, "force_densities");
    assert_eq!(force_densities.len(), BASE_Q.len(), "one echoed density per member");
    for (i, (fd, &expected)) in force_densities.iter().zip(BASE_Q.iter()).enumerate() {
        let q = dimensionless_echo("force_densities", fd);
        assert_eq!(q, expected, "force_densities[{i}] must echo the input q exactly");
    }
    let none = list_field(&line_only, "surface_stresses");
    assert!(none.is_empty(), "line-only must echo an EMPTY surface_stresses, got {none:?}");

    const SIGMA: f64 = 2.0;
    let membrane = solve_membrane(SIGMA);
    let surface_stresses = list_field(&membrane, "surface_stresses");
    assert_eq!(surface_stresses.len(), 4, "one echoed σ per triangle — a NON-empty list");
    for (t, ss) in surface_stresses.iter().enumerate() {
        let s = dimensionless_echo("surface_stresses", ss);
        assert_eq!(s, SIGMA, "surface_stresses[{t}] must echo the prescribed σ exactly");
    }
}

/// `build_result_free` obeys the same strict FORCE contract as `build_result` AND the
/// same `Nᵢ = qᵢ·Lᵢ·q_ref` identity — reached only via `solve_form_find_free_trampoline`,
/// where every other assertion on it is dimension-blind. The identity half pins the
/// PAIRING, not just the tag: this builder takes nodes / member_forces / force_densities
/// as three positional `&[f64]`, so a swap still arrives FORCE-tagged and finite.
#[test]
fn free_standing_member_forces_are_strictly_force_dimensioned() {
    assert_bridge_holds(&solve_free(), "free-standing");
}

/// The gauge-rescale factor, shared by both covariance tests. Positive, so the
/// strut-q<0 / cable-q>0 sign contract survives it; not a power of two, so an exact
/// binary rescale cannot mask a real dependence.
const GAUGE_LAMBDA: f64 = 7.0;

/// THE COVARIANCE ASSERTION, shared by both rescale tests: scaling the WHOLE gauge by λ
/// must leave the solved GEOMETRY exactly where it was while scaling every member force
/// by exactly λ. Tolerances are caller-supplied because the two emission paths reach the
/// property by DIFFERENT mechanisms (see each caller), and pretending otherwise would
/// either over-tighten the free path or silently slacken the anchored one.
fn assert_gauge_covariance(
    base: &PersistentMap<String, Value>,
    scaled: &PersistentMap<String, Value>,
    node_tol: f64,
    force_rel_tol: f64,
    site: &str,
) {
    // Geometry is gauge-INVARIANT: the rescale moves no node.
    let base_nodes = list_field(base, "nodes");
    let scaled_nodes = list_field(scaled, "nodes");
    assert_eq!(base_nodes.len(), scaled_nodes.len(), "{site}: same node count from both solves");
    for (i, (b, s)) in base_nodes.iter().zip(scaled_nodes.iter()).enumerate() {
        let (bp, sp) = (point_xyz(b), point_xyz(s));
        for (axis, (bc, sc)) in bp.iter().zip(sp.iter()).enumerate() {
            assert!(
                (bc - sc).abs() <= node_tol,
                "{site}: nodes[{i}][{axis}] moved from {bc} to {sc} under a ×{GAUGE_LAMBDA} gauge \
                 rescale (tol {node_tol} m); the solved geometry is nullity-invariant and \
                 must not move (task #6095)"
            );
        }
    }

    // Forces are gauge-COVARIANT: every one scales by exactly λ.
    let base_forces = list_field(base, "member_forces");
    let scaled_forces = list_field(scaled, "member_forces");
    assert_eq!(base_forces.len(), scaled_forces.len(), "{site}: same member count from both solves");
    assert!(!base_forces.is_empty(), "{site}: a covariance check over zero members is vacuous");
    for (i, (b, s)) in base_forces.iter().zip(scaled_forces.iter()).enumerate() {
        let (bn, sn) = (force_si(b), force_si(s));
        let expected = bn * GAUGE_LAMBDA;
        // NON-VACUITY — the same hole `assert_bridge_holds` closes with its sign contract:
        // `|sn − bn·λ| ≤ tol·|bn·λ|` collapses to `0 ≤ 0` at bn = 0 and then holds for EVERY
        // member at EVERY λ. The `!is_empty()` guard above does not exclude an all-zero list.
        // Live hazard, not hypothetical: on the free path q is solver-DERIVED, so a
        // GroupRatios regression collapsing every derived density to zero would land here
        // green, and the geometry half is equally satisfied by an unchanged node set.
        assert!(
            bn.abs() > 1e-9,
            "{site}: member_forces[{i}] = {bn} at the base gauge makes the ×{GAUGE_LAMBDA} \
             covariance check vacuous — 0 == 0·λ holds for any λ (task #6095)"
        );
        assert!(
            (sn - expected).abs() <= force_rel_tol * expected.abs(),
            "{site}: member_forces[{i}] must scale by exactly {GAUGE_LAMBDA} under the gauge \
             rescale: {bn} · {GAUGE_LAMBDA} = {expected}, got {sn} (rel tol {force_rel_tol}). \
             Member forces are gauge-covariant outputs of a gauge-free input — that is why \
             the absolute force scale comes from q_ref ≡ 1 N/m and not from q (task #6095)"
        );
    }
}

/// ANCHORED GAUGE COVARIANCE — the runtime proof of the adjudication. q is a gauge-free
/// ratio (nothing moves) while `member_forces` is gauge-covariant (everything scales),
/// which is precisely why the force scale must come from a reference factor and cannot
/// come from q. This fixture is line-only, so σ is empty and the whole gauge IS q (see
/// SCOPE above). MECHANISM: an exact algebraic identity — `D` is linear in q, so λ
/// cancels in `D_ff x_f = −D_fa x_a` by inspection. Invariant to ~1e-15 (≈6 orders under
/// the 1e-9 m tolerance) and exactly covariant to f64 round-off, hence 1e-12 relative.
#[test]
fn rescale_q_leaves_geometry_fixed_and_scales_forces() {
    let base = solve_at(&BASE_Q);
    let scaled_q: Vec<f64> = BASE_Q.iter().map(|&q| q * GAUGE_LAMBDA).collect();
    let scaled = solve_at(&scaled_q);
    // The rescaled solve must independently satisfy the bridge — strict FORCE / bare-Real
    // tags, the Nᵢ = qᵢ·Lᵢ·q_ref identity, and the sign contract — so covariance is asserted
    // BETWEEN two solves each known good, not merely between two ratios.
    assert_bridge_holds(&scaled, "anchored line-only (scaled gauge)");
    assert_gauge_covariance(&base, &scaled, 1e-9, 1e-12, "anchored line-only");
}

/// FREE-STANDING GAUGE COVARIANCE — the same claim at the OTHER emitter, where it holds
/// for an entirely different reason and so needs its own case. Anchored covariance is an
/// identity (above). Here the gauge is fixed by `reference_group`, whose magnitude is
/// HELD at its seed while every other group's magnitude is SEARCHED — coordinate descent
/// over a Σλ² eigenvalue objective, on log-spaced brackets seeded from `|seed_ratios|`
/// (`form_find_free.rs`). So "scale every seed ratio by λ ⇒ same geometry, λ-scaled
/// forces" is a property of that search's HOMOGENEITY, not algebra: the brackets and the
/// log grid scale with λ and the objective ordering is λ²-invariant, but a regression in
/// the bracketing or the warm start could break it with nothing else noticing.
///
/// The homogeneity is not EXACT, which is why the relative tolerance is looser than the
/// anchored case's 1e-12: the search's two stopping rules (`OBJ_TOL = 1e-20` and the
/// stall guard's `1e-18·max(before, 1)`) are ABSOLUTE thresholds on an objective that
/// scales as λ², so the two runs stop at slightly different depths and the recovered
/// ratios differ in the last digits. Both still land far below the nullity classifier's
/// threshold, so this is the same solution — search noise, not gauge dependence.
///
/// TOLERANCES ARE MEASURED, not guessed (λ = 7, this fixture): node residual 1.8e-12 m,
/// q and N relative residuals both 1.1e-10 — so 1e-9 m and 1e-8 leave ~2.5 and ~2 orders
/// of margin. A failure at these numbers is a real homogeneity regression; do not slacken
/// them without re-measuring, and do not tighten them to the anchored path's 1e-12, which
/// this path cannot meet by construction.
#[test]
fn free_standing_rescaled_seed_ratios_leave_geometry_fixed_and_scale_forces() {
    let base = solve_free_at(&BASE_SEED);
    let scaled_seed: Vec<f64> = BASE_SEED.iter().map(|&r| r * GAUGE_LAMBDA).collect();
    let scaled = solve_free_at(&scaled_seed);
    // The scaled FREE solve is the one result nothing else pins: the base free gauge is
    // covered by `free_standing_member_forces_are_strictly_force_dimensioned`, but a
    // GroupRatios regression that only bites at λ = 7 would otherwise land here unchecked.
    assert_bridge_holds(&scaled, "free-standing (scaled gauge)");

    // The echoed densities must scale too — that is what makes this a GAUGE rescale and
    // not merely a different search that happened to land on the same shape.
    let base_q = list_field(&base, "force_densities");
    let scaled_q = list_field(&scaled, "force_densities");
    assert_eq!(base_q.len(), scaled_q.len(), "same member count from both free solves");
    for (i, (b, s)) in base_q.iter().zip(scaled_q.iter()).enumerate() {
        let (bq, sq) = (dimensionless_echo("force_densities", b), dimensionless_echo("force_densities", s));
        let expected = bq * GAUGE_LAMBDA;
        // The same non-vacuity floor as `assert_gauge_covariance`, on the quantity the free
        // path actually SEARCHES: at bq = 0 the ×λ claim is trivially true.
        assert!(
            bq.abs() > 1e-9,
            "free-standing: force_densities[{i}] = {bq} at the base gauge makes the \
             ×{GAUGE_LAMBDA} scale check vacuous; the GroupRatios search must not collapse \
             a derived density to zero (task #6095)"
        );
        assert!(
            (sq - expected).abs() <= 1e-8 * expected.abs(),
            "free-standing: force_densities[{i}] must scale by {GAUGE_LAMBDA} under a \
             whole-gauge seed rescale: {bq} · {GAUGE_LAMBDA} = {expected}, got {sq}"
        );
    }

    assert_gauge_covariance(&base, &scaled, 1e-9, 1e-8, "free-standing");
}
