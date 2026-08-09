//! Runtime lock on the tensegrity force-density **dimensional bridge** (task
//! #6095). NORMATIVE statement: the "Dimensional bridge" paragraph in
//! `crates/reify-compiler/stdlib/tensegrity.ri` — this module is only its
//! runtime evidence and points at it rather than restating it.
//!
//! What it pins that nothing else does: every other eval assertion on these
//! fields routes through a dimension-BLIND helper (`force_val` / `coord`) taking
//! `Value::Scalar{..}` or `Value::Real(..)` interchangeably, so the trampoline
//! could silently retag either way with no Rust test failing. The extractors
//! below match variants EXPLICITLY and panic on the wrong one, across all three
//! emission sites: anchored line-only, anchored surfaces, free-standing.
//!
//! SCOPE — gauge COVARIANCE (the last test) is line-only on purpose: surfaces
//! convergence is judged against an ABSOLUTE tolerance on a residual not
//! normalised by |D|, and `D` is linear in q, so it is gauge-DEPENDENT there.
//! Solver-side defect outside #6095's scope; filed as #6119 (dup #6124).

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// A 3-component `Value::Point` of SI-metre coordinates — how `point3` lowers.
fn node(x: f64, y: f64, z: f64) -> Value {
    let m = |v: f64| Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH };
    Value::Point(vec![m(x), m(y), m(z)])
}

/// Struts-then-cables member order — the one index space `force_densities` and
/// `member_forces` share: 3 struts, then top / bottom / vertical cable triples.
const MEMBERS: [(usize, usize); 12] = [
    (0, 4), (1, 5), (2, 3), (0, 1), (1, 2), (2, 0),
    (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5),
];

/// Bottom triangle {3,4,5} anchored; top triangle {0,1,2} free.
const ANCHORS: [i64; 3] = [3, 4, 5];

/// Base force densities in `MEMBERS` order; signs honour the hard contract
/// (struts q < 0, cables q > 0). Verticals are 2, not 1, on purpose: at q = 1
/// everywhere `D_ff` has zero row sums and is exactly singular.
const BASE_Q: [f64; 12] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0];

/// The canonical triplex prism (R=1, height=1, twist=30°; top 0,1,2 at z=1,
/// bottom 3,4,5 at z=0) — `canonical_prism_nodes()` in `tensegrity_t1b_…`.
fn prism_nodes() -> Vec<Value> {
    use std::f64::consts::PI;
    let ring = |i: usize, twist: f64, z: f64| {
        let a = (120.0 * (i as f64) + twist) * PI / 180.0;
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

/// The line-only triplex prism (no surfaces), built from `MEMBERS`.
fn prism_tensegrity() -> Value {
    let pair = |&(j, k): &(usize, usize)| [j as i64, k as i64];
    let struts: Vec<[i64; 2]> = MEMBERS[..3].iter().map(pair).collect();
    let cables: Vec<[i64; 2]> = MEMBERS[3..].iter().map(pair).collect();
    tensegrity(prism_nodes(), index_lists(&struts), index_lists(&cables), Value::List(vec![]))
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

/// FREE-STANDING solve of the same prism (GroupRatios: struts→0, the six
/// horizontals→1, verticals→2; reference group 1) — the `build_result_free`
/// emission site, which the anchored solves above never reach.
fn solve_free() -> PersistentMap<String, Value> {
    let inputs = [
        prism_tensegrity(),
        ints([0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]),
        reals(&[-1.0, 1.0, 1.0]), // seed ratios
        Value::Int(1),            // reference_group
    ];
    solve_with(reify_eval::compute_targets::form_find::solve_form_find_free_trampoline, &inputs)
}

fn list_field<'a>(fields: &'a PersistentMap<String, Value>, name: &str) -> &'a Vec<Value> {
    match fields.get(&name.to_string()) {
        Some(Value::List(items)) => items,
        other => panic!("FormFindResult.{name} must be a List, got {other:?}"),
    }
}

// STRICT extractors — deliberately NOT the dimension-blind `force_val`/`coord`.

/// SI value of a **FORCE-dimensioned** Scalar; anything else is a violation.
fn force_si(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, dimension } if *dimension == DimensionVector::FORCE => *si_value,
        other => panic!(
            "member_forces entries must be a FORCE-dimensioned (kg·m·s⁻²) Value::Scalar, \
             matching the `List<Force>` declaration under the q_ref ≡ 1 N/m gauge of task \
             #6095; got {other:?}"
        ),
    }
}

/// Value of a **bare** `Value::Real`; a dimensioned Scalar is what Leg B forbids.
fn bare_real(field: &str, v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        other => panic!(
            "{field} entries must be a bare Value::Real (dimensionless), matching the \
             `List<Real>` declaration — the qᵢ/σ are nullity-invariant ratios per the \
             dimension-checked-readers Leg B ruling upheld by task #6095; got {other:?}"
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

/// Anchored line-only `member_forces` are strictly FORCE-dimensioned Scalars AND
/// numerically equal `Nᵢ = qᵢ·Lᵢ·q_ref` on the geometry the same solve returned.
/// An EXACT identity — the kernel evaluates `qi * len` from the very `out_nodes`
/// it returns — so the only error is f64 round-trip + sqrt (~1e-16 relative).
#[test]
fn member_force_is_q_times_solved_length_in_the_unit_gauge() {
    let fields = solve_at(&BASE_Q);
    let nodes = list_field(&fields, "nodes");
    let member_forces = list_field(&fields, "member_forces");
    let force_densities = list_field(&fields, "force_densities");
    assert_eq!(member_forces.len(), MEMBERS.len(), "one force per member");

    for (i, &m) in MEMBERS.iter().enumerate() {
        let q = bare_real("force_densities", &force_densities[i]);
        let l = member_length(nodes, m);
        let expected = q * l;
        // `force_si` panics on a bare Real or a non-FORCE dimension.
        let got = force_si(&member_forces[i]);

        assert!(got.is_finite(), "member_forces[{i}] must be finite, got {got}");
        assert!(l > 1e-6, "member {i} {m:?} collapsed to length {l} — degenerate fixture");
        assert!(
            (got - expected).abs() <= 1e-12 * expected.abs(),
            "member_forces[{i}] = {got} must equal qᵢ·Lᵢ·q_ref = {q} · {l} = {expected} \
             to 1e-12 relative (q_ref ≡ 1 N/m, task #6095)"
        );
    }
}

/// Both echoes are strictly bare `Value::Real`s — a dimensioned Scalar is the
/// Leg B violation. σ is asserted on a fixture that actually CARRIES surfaces, so
/// `bare_real` genuinely runs on it instead of skipping the line-only empty list;
/// σ shares q's gauge (it enters `D` additively with dimensionless cotangent
/// weights), so the same lock is the right one for both.
#[test]
fn force_density_and_surface_stress_echoes_are_strictly_bare_reals() {
    let line_only = solve_at(&BASE_Q);
    let force_densities = list_field(&line_only, "force_densities");
    assert_eq!(force_densities.len(), BASE_Q.len(), "one echoed density per member");
    for (i, (fd, &expected)) in force_densities.iter().zip(BASE_Q.iter()).enumerate() {
        let q = bare_real("force_densities", fd);
        assert_eq!(q, expected, "force_densities[{i}] must echo the input q exactly");
    }
    let none = list_field(&line_only, "surface_stresses");
    assert!(none.is_empty(), "line-only must echo an EMPTY surface_stresses, got {none:?}");

    const SIGMA: f64 = 2.0;
    let membrane = solve_membrane(SIGMA);
    let surface_stresses = list_field(&membrane, "surface_stresses");
    assert_eq!(surface_stresses.len(), 4, "one echoed σ per triangle — a NON-empty list");
    for (t, ss) in surface_stresses.iter().enumerate() {
        let s = bare_real("surface_stresses", ss);
        assert_eq!(s, SIGMA, "surface_stresses[{t}] must echo the prescribed σ exactly");
    }
}

/// `build_result_free` obeys the same strict FORCE contract as `build_result`.
/// It is reached only via `solve_form_find_free_trampoline`, and every other
/// assertion on its `member_forces` is dimension-blind — so without this it could
/// regress to bare `Value::Real` with no test failing.
#[test]
fn free_standing_member_forces_are_strictly_force_dimensioned() {
    let fields = solve_free();

    let member_forces = list_field(&fields, "member_forces");
    assert_eq!(member_forces.len(), MEMBERS.len(), "one force per member");
    for (i, mf) in member_forces.iter().enumerate() {
        let n = force_si(mf); // panics on a bare Real or a non-FORCE dimension
        assert!(n.is_finite(), "member_forces[{i}] must be finite, got {n}");
    }
    for (i, fd) in list_field(&fields, "force_densities").iter().enumerate() {
        let q = bare_real("force_densities", fd);
        assert!(q.is_finite(), "force_densities[{i}] must be finite, got {q}");
    }
}

/// GAUGE COVARIANCE — the runtime proof of the adjudication. Rescaling every qᵢ
/// by λ = 7 leaves the solved GEOMETRY identical and scales every member force by
/// exactly λ: q is a gauge-free ratio (nothing moves) while `member_forces` is
/// gauge-covariant (everything scales), which is precisely why the force scale
/// must come from a reference factor and cannot come from q.
///
/// λ = 7 is positive (the strut-q<0 / cable-q>0 contract holds) and not a power
/// of two (an exact binary rescale cannot mask a real dependence). `D` is exactly
/// linear in q, so λ cancels in `D_ff x_f = −D_fa x_a` — the LU solution is
/// invariant to ~1e-15 relative here, ~6 orders under the 1e-9 m bound.
#[test]
fn rescale_q_leaves_geometry_fixed_and_scales_forces() {
    const LAMBDA: f64 = 7.0;

    let base = solve_at(&BASE_Q);
    let scaled_q: Vec<f64> = BASE_Q.iter().map(|&q| q * LAMBDA).collect();
    let scaled = solve_at(&scaled_q);

    // Geometry is gauge-INVARIANT: q → λq moves no node.
    let base_nodes = list_field(&base, "nodes");
    let scaled_nodes = list_field(&scaled, "nodes");
    assert_eq!(base_nodes.len(), scaled_nodes.len(), "same node count from both solves");
    for (i, (b, s)) in base_nodes.iter().zip(scaled_nodes.iter()).enumerate() {
        let (bp, sp) = (point_xyz(b), point_xyz(s));
        for (axis, (bc, sc)) in bp.iter().zip(sp.iter()).enumerate() {
            assert!(
                (bc - sc).abs() <= 1e-9,
                "nodes[{i}][{axis}] moved from {bc} to {sc} under q → {LAMBDA}·q; the \
                 solved geometry is nullity-invariant and must not move (task #6095)"
            );
        }
    }

    // Forces are gauge-COVARIANT: every one scales by exactly λ.
    let base_forces = list_field(&base, "member_forces");
    let scaled_forces = list_field(&scaled, "member_forces");
    assert_eq!(base_forces.len(), scaled_forces.len(), "same member count from both solves");
    for (i, (b, s)) in base_forces.iter().zip(scaled_forces.iter()).enumerate() {
        let (bn, sn) = (force_si(b), force_si(s));
        let expected = bn * LAMBDA;
        assert!(
            (sn - expected).abs() <= 1e-12 * expected.abs(),
            "member_forces[{i}] must scale by exactly {LAMBDA} under q → {LAMBDA}·q: \
             {bn} · {LAMBDA} = {expected}, got {sn}. Member forces are gauge-covariant \
             outputs of a gauge-free input — that is why the absolute force scale comes \
             from q_ref ≡ 1 N/m and not from q (task #6095)"
        );
    }
}
