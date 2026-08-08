//! Runtime lock on the tensegrity force-density **dimensional bridge**
//! adjudicated in task #6095. The NORMATIVE statement is the "Dimensional
//! bridge" paragraph in `crates/reify-compiler/stdlib/tensegrity.ri`; this
//! module is the runtime half of its evidence and POINTS at it rather than
//! restating it (one normative home).
//!
//! Nothing pinned that before: every pre-existing eval assertion on
//! `member_forces` routes through a dimension-BLIND helper (`force_val` in
//! `tensegrity_t1b_form_find_e2e.rs`) taking `Value::Scalar{..}` or
//! `Value::Real(..)` interchangeably, so the trampoline could emit either with
//! no Rust test failing — the FORCE tag was held up solely by incidental
//! `m·kg·s^-2` text in a byte-golden. The extractors below therefore match
//! variants EXPLICITLY and panic on the wrong one; reusing `force_val` would
//! defeat the point.
//!
//! SCOPE — line-only, no surfaces. Covariance (test (d)) holds unconditionally
//! only on the anchored line-only path, where `D` is geometry-independent and
//! the single reduced solve is exact. The surfaces fixed point judges
//! convergence against an ABSOLUTE tolerance on a residual not normalised by
//! |D|, and `D` is linear in q — so surfaces-path convergence is
//! gauge-DEPENDENT: a solver-side sensitivity in `crates/reify-solver-elastic`,
//! outside task #6095's module scope, filed as a follow-up, not asserted.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// A 3-component `Value::Point` of LENGTH-dimensioned SI-metre coordinates —
/// how `point3(..m, ..)` lowers.
fn node(x: f64, y: f64, z: f64) -> Value {
    let m = |v: f64| Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH };
    Value::Point(vec![m(x), m(y), m(z)])
}

/// Member connectivity in the struts-then-cables order the whole contract is
/// indexed by (`tensegrity_wires` emission order; the `force_densities` input,
/// the `member_forces` output and this list all share it).
const MEMBERS: [(usize, usize); 12] = [
    (0, 4), (1, 5), (2, 3), // struts
    (0, 1), (1, 2), (2, 0), // top-triangle cables
    (3, 4), (4, 5), (5, 3), // bottom-triangle cables (both ends anchored)
    (0, 3), (1, 4), (2, 5), // vertical cables
];

/// Bottom triangle {3,4,5} anchored; top triangle {0,1,2} free.
const ANCHORS: [i64; 3] = [3, 4, 5];

/// Base force densities, struts-then-cables; signs honour the hard contract
/// (struts q < 0 compression, cables q > 0 tension). q_vert = 2 rather than 1
/// on purpose: at q = 1 everywhere the free-node block `D_ff` has zero row
/// sums and is exactly singular; at 2 it is strictly diagonally dominant.
const BASE_Q: [f64; 12] = [
    -1.0, -1.0, -1.0, // struts
    1.0, 1.0, 1.0, // top cables
    1.0, 1.0, 1.0, // bottom cables
    2.0, 2.0, 2.0, // vertical cables
];

/// The canonical symmetric triplex prism (circumradius R=1, height=1, twist=30°),
/// matching `canonical_prism_nodes()` in `tensegrity_t1b_form_find_e2e.rs`.
/// Node order: top 0,1,2 at z=1; bottom 3,4,5 at z=0.
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

/// Build the line-only `Tensegrity` Value (no `surfaces` field) from `MEMBERS`.
fn prism_tensegrity() -> Value {
    let pair =
        |(j, k): (usize, usize)| Value::List(vec![Value::Int(j as i64), Value::Int(k as i64)]);
    let struts = Value::List(MEMBERS[..3].iter().copied().map(pair).collect());
    let cables = Value::List(MEMBERS[3..].iter().copied().map(pair).collect());
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(prism_nodes())),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// Run the anchored line-only solve at the given force densities and return the
/// `FormFindResult` fields. Panics unless the solve completed cleanly.
fn solve_at(q: &[f64]) -> PersistentMap<String, Value> {
    let value_inputs = vec![
        prism_tensegrity(),
        Value::List(q.iter().map(|&qi| Value::Real(qi)).collect()),
        Value::List(ANCHORS.iter().map(|&a| Value::Int(a)).collect()),
    ];

    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    let outcome = reify_eval::compute_targets::form_find::solve_form_find_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    );

    let fields = match outcome {
        ComputeOutcome::Completed { result: Value::StructureInstance(d), .. } => {
            let name = &d.type_name;
            assert_eq!(name, "FormFindResult", "result must be a FormFindResult");
            d.fields
        }
        other => panic!("expected a Completed FormFindResult for a well-posed solve: {other:?}"),
    };

    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "the fixture must be well posed — a non-converged solve is vacuous here",
    );
    fields
}

fn list_field<'a>(fields: &'a PersistentMap<String, Value>, name: &str) -> &'a Vec<Value> {
    match fields.get(&name.to_string()) {
        Some(Value::List(items)) => items,
        other => panic!("FormFindResult.{name} must be a List, got {other:?}"),
    }
}

// STRICT extractors — deliberately NOT the dimension-blind `force_val`.

/// SI value of a **FORCE-dimensioned** Scalar. A bare `Value::Real`, or a
/// Scalar of any other dimension, is a contract violation here — not an
/// accepted alternative encoding.
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

/// Value of a **bare** `Value::Real`. A dimensioned `Value::Scalar` is a
/// contract violation here — that is exactly what Leg B forbids.
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

/// The three SI-metre components of a solved node, each strictly a
/// LENGTH-dimensioned `Value::Scalar`.
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

/// (a) STRICT DIMENSION ON THE OUTPUT. Every `member_forces` entry is a
/// FORCE-dimensioned `Value::Scalar`; a bare `Value::Real` fails. The first
/// strict lock anywhere on this field.
#[test]
fn member_forces_are_strictly_force_dimensioned() {
    let fields = solve_at(&BASE_Q);
    let member_forces = list_field(&fields, "member_forces");

    assert_eq!(member_forces.len(), MEMBERS.len(), "one force per member");
    for (i, mf) in member_forces.iter().enumerate() {
        // `force_si` panics on a bare Real or a non-FORCE dimension.
        let n = force_si(mf);
        assert!(n.is_finite(), "member_forces[{i}] must be finite, got {n}");
    }
}

/// (b) STRICT DIMENSIONLESSNESS ON THE ECHOES. `force_densities` and
/// `surface_stresses` are bare `Value::Real`s; a dimensioned `Value::Scalar`
/// fails. This pins Leg B at runtime.
#[test]
fn force_density_and_surface_stress_echoes_are_strictly_bare_reals() {
    let fields = solve_at(&BASE_Q);

    let force_densities = list_field(&fields, "force_densities");
    assert_eq!(force_densities.len(), BASE_Q.len(), "one echoed density per member");
    for (i, (fd, &expected)) in force_densities.iter().zip(BASE_Q.iter()).enumerate() {
        let q = bare_real("force_densities", fd);
        assert_eq!(q, expected, "force_densities[{i}] must echo the input q exactly");
    }

    // Line-only path: an EMPTY list, never Undef and never a dimensioned entry.
    let surface_stresses = list_field(&fields, "surface_stresses");
    let empty = surface_stresses.is_empty();
    assert!(empty, "line-only must echo an EMPTY surface_stresses, got {surface_stresses:?}");
}

/// (c) THE GAUGE IDENTITY, NUMERICALLY. `Nᵢ = qᵢ·Lᵢ·q_ref`, `q_ref ≡ 1 N/m`: the
/// FORCE-tagged SI number is exactly the dimensionless qᵢ times the member
/// length in metres on the geometry the same solve returned. An EXACT algebraic
/// identity — the kernel evaluates `qi * len` from the very `out_nodes` it hands
/// back — so the only error is the f64 round-trip plus the recomputed sqrt
/// (~1e-16 relative), ~4 orders of margin under the 1e-12 bound.
#[test]
fn member_force_is_q_times_solved_length_in_the_unit_gauge() {
    let fields = solve_at(&BASE_Q);
    let nodes = list_field(&fields, "nodes");
    let member_forces = list_field(&fields, "member_forces");
    let force_densities = list_field(&fields, "force_densities");

    for (i, &m) in MEMBERS.iter().enumerate() {
        let q = bare_real("force_densities", &force_densities[i]);
        let l = member_length(nodes, m);
        let expected = q * l;
        let got = force_si(&member_forces[i]);

        assert!(
            l > 1e-6,
            "member {i} {m:?} collapsed to length {l} — degenerate fixture"
        );
        assert!(
            (got - expected).abs() <= 1e-12 * expected.abs(),
            "member_forces[{i}] = {got} must equal qᵢ·Lᵢ·q_ref = {q} · {l} = {expected} \
             to 1e-12 relative (q_ref ≡ 1 N/m, task #6095)"
        );
    }
}

/// (d) GAUGE COVARIANCE — the runtime proof of the adjudication. Rescaling every
/// qᵢ by λ = 7 leaves the solved GEOMETRY identical and scales every member
/// force by exactly λ: q is a gauge-free relative ratio (nothing moves) while
/// `member_forces` is gauge-covariant (everything scales) — precisely why the
/// force scale must come from a reference factor and cannot come from q itself.
///
/// λ = 7 is positive (the strut-q<0 / cable-q>0 sign contract still holds) and
/// not a power of two (an exactly representable binary rescale cannot mask a
/// real dependence). Achievability: the anchored solve is `D_ff x_f = −D_fa x_a`
/// with `D` exactly linear in q, so λ multiplies both sides and cancels — the LU
/// solution is invariant to ~1e-15 relative on this well-conditioned O(1 m)
/// prism, ~6 orders under the 1e-9 m bound.
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

    // The echoed densities scale too: the gauge lives in the reference factor,
    // not in some hidden renormalisation of the input.
    let scaled_densities = list_field(&scaled, "force_densities");
    for (i, (fd, &expected)) in scaled_densities.iter().zip(scaled_q.iter()).enumerate() {
        let q = bare_real("force_densities", fd);
        assert_eq!(q, expected, "force_densities[{i}] must echo the rescaled input q");
    }
}
