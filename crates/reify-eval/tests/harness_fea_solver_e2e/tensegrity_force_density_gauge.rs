//! Runtime lock on the tensegrity force-density **dimensional bridge**
//! adjudicated in task #6095 (see the "Dimensional bridge" paragraph in
//! `crates/reify-compiler/stdlib/tensegrity.ri`, the normative statement).
//!
//! The adjudication keeps `FormFindResult.member_forces : List<Force>` beside
//! `force_densities : List<Real>`, on the grounds that the qᵢ are
//! nullity-invariant DIMENSIONLESS ratios and the force scale comes from the
//! unit reference force density gauge **q_ref ≡ 1 N/m**, so
//! `Nᵢ = qᵢ · Lᵢ · q_ref` is genuinely Force-dimensioned. This module is the
//! runtime half of that claim's evidence.
//!
//! Nothing pinned any of this before. Every pre-existing eval assertion on
//! `member_forces` routes through a dimension-BLIND helper (`force_val` in
//! `tensegrity_t1b_form_find_e2e.rs` / `tensegrity_delta_combined_form_find_e2e.rs`)
//! that accepts `Value::Scalar{..}` or `Value::Real(..)` interchangeably, so the
//! trampoline could emit bare `Real` for `member_forces` — or a dimensioned
//! `Scalar` for `force_densities` — with no Rust test failing. The FORCE tag was
//! held up solely by the incidental `m·kg·s^-2` text inside a byte-golden.
//! The extractors below therefore match variants EXPLICITLY and panic on the
//! wrong one; that strictness is the point, and reusing `force_val` here would
//! defeat it.
//!
//! SCOPE — deliberately LINE-ONLY (no surfaces). The gauge-covariance law
//! (`rescale_q_leaves_geometry_fixed_and_scales_forces`) is asserted only on the
//! anchored line-only path, where `D` is geometry-independent and the single
//! reduced solve is exact, so covariance is unconditional. On the surfaces path
//! the cotangent fixed point judges convergence with an ABSOLUTE
//! `SURFACE_EQUILIBRIUM_TOL = 1e-10` (`crates/reify-solver-elastic/src/form_find.rs`)
//! against `free_equilibrium_residual`, which normalises by geometry scale but
//! NOT by the magnitude of `D`. Since `D` is linear in q, that residual scales
//! with λ, making surfaces-path convergence gauge-DEPENDENT. That is a genuine
//! solver-side scale-sensitivity living in `crates/reify-solver-elastic`, outside
//! task #6095's module scope — it is filed as a follow-up, not fixed here, and
//! this module must not assert it.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── fixture ──────────────────────────────────────────────────────────────────

/// A Length-typed coordinate Scalar (SI metres) — how `point3(..m, ..)` lowers.
fn length(m: f64) -> Value {
    Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    }
}

/// A 3-component `Value::Point` node.
fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

/// Member connectivity in the struts-then-cables order the whole contract is
/// indexed by (`tensegrity_wires` emission order; the `force_densities` input,
/// the `member_forces` output and this list all share it).
const MEMBERS: [(usize, usize); 12] = [
    // struts
    (0, 4),
    (1, 5),
    (2, 3),
    // top-triangle cables
    (0, 1),
    (1, 2),
    (2, 0),
    // bottom-triangle cables (both ends anchored)
    (3, 4),
    (4, 5),
    (5, 3),
    // vertical cables
    (0, 3),
    (1, 4),
    (2, 5),
];

/// Bottom triangle {3,4,5} anchored; top triangle {0,1,2} free.
const ANCHORS: [i64; 3] = [3, 4, 5];

/// Base force densities, struts-then-cables. Signs honour the hard contract
/// (struts q < 0 compression, cables q > 0 tension).
///
/// The vertical cables carry q = 2 rather than 1 on purpose: with every cable
/// at q = 1 the free-node block `D_ff` has zero row sums and is exactly
/// singular. At q_vert = 2 each row sums to +1, so `D_ff` is strictly
/// diagonally dominant and the reduced solve is well conditioned.
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
    let deg = PI / 180.0;
    let top = |i: usize| {
        let a = 120.0 * (i as f64) * deg;
        node(a.cos(), a.sin(), 1.0)
    };
    let bot = |i: usize| {
        let a = (120.0 * (i as f64) + 30.0) * deg;
        node(a.cos(), a.sin(), 0.0)
    };
    vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
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
        ComputeOutcome::Completed { result, .. } => match result {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name, "FormFindResult",
                    "result should be a FormFindResult, got {:?}",
                    d.type_name
                );
                d.fields
            }
            other => panic!("Completed result should be a StructureInstance, got {other:?}"),
        },
        other => panic!("expected ComputeOutcome::Completed for a well-posed solve, got {other:?}"),
    };

    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "the fixture must be well posed — a non-converged solve invalidates every \
         assertion in this module",
    );
    fields
}

fn list_field<'a>(fields: &'a PersistentMap<String, Value>, name: &str) -> &'a Vec<Value> {
    match fields.get(&name.to_string()) {
        Some(Value::List(items)) => items,
        other => panic!("FormFindResult.{name} must be a List, got {other:?}"),
    }
}

// ── STRICT extractors (deliberately NOT the dimension-blind `force_val`) ─────

/// SI value of a **FORCE-dimensioned** Scalar. A bare `Value::Real` is a
/// contract violation here, not an accepted alternative encoding.
fn force_si(v: &Value) -> f64 {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::FORCE,
                "member_forces entries must be FORCE-dimensioned (kg·m·s⁻²) — the \
                 q_ref ≡ 1 N/m gauge of task #6095; got dimension {dimension:?}"
            );
            *si_value
        }
        other => panic!(
            "member_forces entries must be Value::Scalar{{ dimension: FORCE }}, matching the \
             `List<Force>` declaration; got {other:?}"
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

/// SI metres of a **LENGTH-dimensioned** Scalar node coordinate.
fn coord_si(v: &Value) -> f64 {
    match v {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "node coordinates must be LENGTH-dimensioned; got {dimension:?}"
            );
            *si_value
        }
        other => panic!("node coordinates must be Value::Scalar{{ .. }}, got {other:?}"),
    }
}

/// The three SI-metre components of a solved node.
fn point_xyz(v: &Value) -> [f64; 3] {
    match v {
        Value::Point(c) if c.len() == 3 => [coord_si(&c[0]), coord_si(&c[1]), coord_si(&c[2])],
        other => panic!("nodes entries must be a 3-component Value::Point, got {other:?}"),
    }
}

/// Euclidean length of member `m` on the returned geometry.
fn member_length(nodes: &[Value], m: (usize, usize)) -> f64 {
    let pj = point_xyz(&nodes[m.0]);
    let pk = point_xyz(&nodes[m.1]);
    ((pj[0] - pk[0]).powi(2) + (pj[1] - pk[1]).powi(2) + (pj[2] - pk[2]).powi(2)).sqrt()
}

// ── (a) strict dimension on the output ───────────────────────────────────────

/// Every `member_forces` entry is a FORCE-dimensioned `Value::Scalar`. A bare
/// `Value::Real` fails — the first strict lock anywhere on this field.
#[test]
fn member_forces_are_strictly_force_dimensioned() {
    let fields = solve_at(&BASE_Q);
    let member_forces = list_field(&fields, "member_forces");

    assert_eq!(
        member_forces.len(),
        MEMBERS.len(),
        "one force per member, struts-then-cables"
    );
    for (i, mf) in member_forces.iter().enumerate() {
        // `force_si` panics on a bare Real or a non-FORCE dimension.
        let n = force_si(mf);
        assert!(n.is_finite(), "member_forces[{i}] must be finite, got {n}");
    }
}

// ── (b) strict dimensionlessness on the echoes ───────────────────────────────

/// `force_densities` and `surface_stresses` are bare `Value::Real`s. A
/// dimensioned `Value::Scalar` fails — this pins Leg B at runtime.
#[test]
fn force_density_and_surface_stress_echoes_are_strictly_bare_reals() {
    let fields = solve_at(&BASE_Q);

    let force_densities = list_field(&fields, "force_densities");
    assert_eq!(
        force_densities.len(),
        BASE_Q.len(),
        "force_densities echoes the input, one per member"
    );
    for (i, (fd, &expected)) in force_densities.iter().zip(BASE_Q.iter()).enumerate() {
        let q = bare_real("force_densities", fd);
        assert_eq!(
            q, expected,
            "force_densities[{i}] must echo the input q exactly"
        );
    }

    // Line-only path: an EMPTY list, never Undef and never a dimensioned entry.
    let surface_stresses = list_field(&fields, "surface_stresses");
    assert!(
        surface_stresses.is_empty(),
        "the line-only path must echo an empty surface_stresses list (never Undef), \
         got {surface_stresses:?}"
    );
}

// ── (c) the gauge identity, numerically ──────────────────────────────────────

/// `Nᵢ = qᵢ · Lᵢ · q_ref` with `q_ref ≡ 1 N/m`: the FORCE-tagged SI number is
/// exactly the dimensionless qᵢ times the member length in metres, measured on
/// the geometry the same solve returned.
///
/// This is an exact algebraic identity, not an approximation — the kernel
/// evaluates `qi * len` from the very `out_nodes` it hands back. The only error
/// is the f64 round-trip through `Value::Scalar{ si_value }` plus the recomputed
/// sqrt, i.e. a few ULP (~1e-16 relative), so 1e-12 leaves ~4 orders of margin.
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
            "member {i} {m:?} collapsed to length {l} — the fixture is degenerate \
             and the identity below would be vacuous"
        );
        assert!(
            (got - expected).abs() <= 1e-12 * expected.abs(),
            "member_forces[{i}] = {got} must equal qᵢ·Lᵢ·q_ref = {q} · {l} = {expected} \
             to 1e-12 relative (q_ref ≡ 1 N/m, task #6095)"
        );
    }
}

// ── (d) gauge covariance — the runtime proof of the adjudication ─────────────

/// Rescaling every qᵢ by λ = 7 leaves the solved GEOMETRY identical and scales
/// every member force by exactly λ. This is the adjudicated claim as a
/// behavioural law: q is a gauge-free relative ratio (nothing moves), while
/// `member_forces` is gauge-covariant (everything scales), which is precisely
/// why the force scale has to come from a reference factor and cannot come from
/// q itself.
///
/// λ = 7 is positive (so the strut-q<0 / cable-q>0 sign contract still holds and
/// the solve stays feasible) and is not a power of two (so an exactly
/// representable binary rescale cannot mask a real dependence).
///
/// Achievability: the anchored line-only solve is `D_ff x_f = −D_fa x_a` with
/// `D` exactly linear in q, so λ multiplies both sides and cancels; the LU
/// solution is invariant up to round-off. On a well-conditioned 6-node prism
/// with O(1 m) coordinates that is ~1e-15 relative, so the 1e-9 m absolute bound
/// carries ~6 orders of margin. Forces then scale exactly, being `qᵢ·Lᵢ` with
/// `Lᵢ` unchanged.
#[test]
fn rescale_q_leaves_geometry_fixed_and_scales_forces() {
    const LAMBDA: f64 = 7.0;

    let base = solve_at(&BASE_Q);
    let scaled_q: Vec<f64> = BASE_Q.iter().map(|&q| q * LAMBDA).collect();
    let scaled = solve_at(&scaled_q);

    // Geometry is gauge-INVARIANT: q → λq moves no node.
    let base_nodes = list_field(&base, "nodes");
    let scaled_nodes = list_field(&scaled, "nodes");
    assert_eq!(
        base_nodes.len(),
        scaled_nodes.len(),
        "both solves must return the same node count"
    );
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
    assert_eq!(
        base_forces.len(),
        scaled_forces.len(),
        "both solves must return the same member count"
    );
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

    // The echoed densities scale too — the gauge choice lives in the reference
    // factor, not in some hidden renormalisation of the input.
    let scaled_densities = list_field(&scaled, "force_densities");
    for (i, (fd, &expected)) in scaled_densities.iter().zip(scaled_q.iter()).enumerate() {
        assert_eq!(
            bare_real("force_densities", fd),
            expected,
            "force_densities[{i}] must echo the rescaled input q exactly"
        );
    }
}
