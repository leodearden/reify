//! Kernel-free unit tests for the driving-set rank-partition — geometric-relations
//! ζ (task 4386), step-7 RED / step-8 GREEN.
//!
//! [`partition_driving_set`] rank-partitions a per-scope geometric-relation set at
//! a *witness* Frame config into a maximal independent **driving set** + a
//! **redundant remainder**, reporting the DOF accounting (spent = combined rank,
//! free = 6 − rank). These tests drive it over *synthetic* realized datum `Value`s
//! (no geometry kernel) — the fast unit slice of ζ's test layering (design §7:
//! partition + Frame-solve units are kernel-free; only datum realization +
//! B1/B2/B3/B5 e2e need real OCCT).
//!
//! The partition operates on each relation's residual gradient w.r.t. the 6 Frame
//! DOF (3 translation + 3 rotation) at the witness config: a relation is *driving*
//! iff its rows add at least one new independent direction to the running rank;
//! otherwise it joins the *redundant remainder*. Crucially the {driving, redundant}
//! split and the total spent DOF are **order-independent** — that is what makes
//! over-constraint robust (B2's silent-redundant relation, B3's loud conflict).
//!
//! ## Witness configs are exactly-satisfying identity poses
//!
//! Every synthetic scenario places the moving sub's LOCAL datums so the relations
//! are already satisfied at the identity witness (`Pose::identity()`). That keeps
//! the residual ≈ 0 and the Jacobian rows clean closed-forms, so the expected ranks
//! are exact integer codimensions (design §5: DOF figures are exact codimensions,
//! never tuned epsilons). The numeric rank tolerance [`TOL`] only has to separate a
//! genuinely-zero column (an unconstrained DOF) from an O(1) gradient entry.

use reify_constraints::relate_solve::{
    FrameUnknown, Operand, Pose, RelationInstance, partition_driving_set,
};
use reify_ir::Value;

/// Numeric rank-revealing tolerance handed to the partition. The Jacobian entries
/// are O(1) (unit directions) to O(1) (metre translations); an unconstrained DOF
/// shows up as an exactly-zero column (equal residuals up to float noise), so a
/// coarse 1e-6 cleanly separates rank-5 from rank-6.
const TOL: f64 = 1e-6;

// ── synthetic realized-datum builders ───────────────────────────────────────

fn point3(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
}

fn vec3(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

/// A `Value::Axis` from an origin point (metres) + a direction vector.
fn axis(o: (f64, f64, f64), d: (f64, f64, f64)) -> Value {
    Value::Axis {
        origin: Box::new(point3(o.0, o.1, o.2)),
        direction: Box::new(vec3(d.0, d.1, d.2)),
    }
}

/// A `Value::Plane` from an origin point (metres) + a unit normal vector.
fn plane(o: (f64, f64, f64), n: (f64, f64, f64)) -> Value {
    Value::Plane {
        origin: Box::new(point3(o.0, o.1, o.2)),
        normal: Box::new(vec3(n.0, n.1, n.2)),
    }
}

/// A dimensionless `Value::Direction`.
fn dir(x: f64, y: f64, z: f64) -> Value {
    Value::Direction { x, y, z }
}

/// A datum operand belonging to sub `sub` (so the partition transforms it by the
/// witness Frame iff `sub` is the auto unknown).
fn datum(sub: &str, value: Value) -> Operand {
    Operand {
        sub: Some(sub.to_string()),
        datum: value,
    }
}

/// A relation instance: name, ordered operands, and the γ-published nominal ΔDOF
/// (codimension) the partition cross-checks its measured per-relation rank against.
fn relation(name: &str, operands: Vec<Operand>, nominal_delta_dof: u32) -> RelationInstance {
    RelationInstance {
        name: name.to_string(),
        operands,
        nominal_delta_dof: Some(nominal_delta_dof),
    }
}

/// The auto Frame unknown for sub `"bolt"` (the §1 moving sub).
fn bolt_unknown() -> FrameUnknown {
    FrameUnknown {
        sub: "bolt".to_string(),
    }
}

/// The §1 driving relations (B1): `concentric(bolt.shank, plate.hole)` +
/// `flush(bolt.seat, plate.top)`. The bolt is the moving auto sub; the plate is the
/// fixed anchor.
///
/// Geometry (already coaxial+flush at the identity witness):
/// - bolt shank axis (local): origin `(0,0,0)`, dir `+z` — coaxial with the hole.
/// - plate hole axis (anchor): origin `(0,0,0)`, dir `+z`.
/// - bolt seat plane (local): origin `(0,0,5mm)`, normal `+z` — normal ∥ shank dir
///   (the coaxial-bolt property: the head seat is perpendicular to the shank, so
///   its normal lies along the shank axis).
/// - plate top plane (anchor): origin `(0,0,0)`, normal `+z`.
///
/// Expected partition: concentric pins {tilt_x, tilt_y, trans_x, trans_y} (rank 4);
/// flush pins {tilt_x, tilt_y} (redundant with concentric) + the normal offset
/// trans_z (new) → adds 1. Combined rank 5; the lone residual DOF is spin about the
/// shank axis (rot_z). Both relations drive.
fn b1_relations() -> Vec<RelationInstance> {
    vec![
        relation(
            "concentric",
            vec![
                datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "flush",
            vec![
                datum("bolt", plane((0.0, 0.0, 0.005), (0.0, 0.0, 1.0))),
                datum("plate", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            3,
        ),
    ]
}

/// A 3-relation set whose combined Jacobian has rank 2 (B2 partition mechanics):
/// three `perpendicular` relations pinning the SAME moving direction `m.dir = +z`
/// against three fixed anchor directions in the xy-plane.
///
/// A `perpendicular(u, v)` row is the rotational gradient `u × v` (translations do
/// not move a bare Direction); all three rows live in the 2-plane ⊥ `m.dir`, so at
/// most two are independent and the third is necessarily a linear combination —
/// regardless of which two are seen first. The two diagonal-free anchors `+x`, `+y`
/// give independent rows; the diagonal `(x+y)/√2` is their average → redundant.
///
/// Expected partition: driving 2, redundant 1, spent 2, free 4 — and the {2,1}
/// counts + spent are invariant to input order (any two of the three rows are
/// independent).
fn b2_relations() -> Vec<RelationInstance> {
    let s = 1.0 / 2.0_f64.sqrt();
    vec![
        relation(
            "perpendicular",
            vec![datum("m", dir(0.0, 0.0, 1.0)), datum("a", dir(1.0, 0.0, 0.0))],
            1,
        ),
        relation(
            "perpendicular",
            vec![datum("m", dir(0.0, 0.0, 1.0)), datum("a", dir(0.0, 1.0, 0.0))],
            1,
        ),
        relation(
            "perpendicular",
            vec![datum("m", dir(0.0, 0.0, 1.0)), datum("a", dir(s, s, 0.0))],
            1,
        ),
    ]
}

/// The auto Frame unknown for the B2 moving sub `"m"`.
fn m_unknown() -> FrameUnknown {
    FrameUnknown {
        sub: "m".to_string(),
    }
}

// ── B1: concentric + flush — both driving, spent 5, residual 1 ───────────────

/// B1 — `concentric(axis,axis) + flush(plane,plane)`: both relations are driving
/// (flush's two rotational rows are redundant with concentric's, but its
/// translational normal-offset row is independent), the combined system spends 5
/// DOF, and exactly 1 residual DOF (spin about the shank axis) remains.
#[test]
fn partition_b1_concentric_flush_spends_five_residual_one() {
    let relations = b1_relations();
    let p = partition_driving_set(&relations, &bolt_unknown(), &Pose::identity(), TOL);

    assert_eq!(
        p.driving.len(),
        2,
        "both concentric and flush must drive (each adds rank): {:?}",
        p.driving
    );
    assert_eq!(
        p.redundant.len(),
        0,
        "neither relation is wholly redundant in B1: {:?}",
        p.redundant
    );
    assert_eq!(p.spent, 5, "concentric(4) + flush's independent normal offset(1) = 5");
    assert_eq!(p.free, 1, "the residual DOF is spin about the shank axis");
    assert_eq!(p.spent + p.free, 6, "spent + free must account for all 6 Frame DOF");
}

// ── B2: three relations, combined rank 2 — driving 2, redundant 1 ────────────

/// B2 — three `perpendicular` relations whose combined Jacobian has rank 2: the
/// partition keeps two as the driving set and drops the third into the redundant
/// remainder (DOF spent = 2, free = 4).
#[test]
fn partition_b2_three_relations_rank_two() {
    let relations = b2_relations();
    let p = partition_driving_set(&relations, &m_unknown(), &Pose::identity(), TOL);

    assert_eq!(p.driving.len(), 2, "two independent rows ⇒ driving set size 2: {:?}", p.driving);
    assert_eq!(
        p.redundant.len(),
        1,
        "the third relation is rank-redundant: {:?}",
        p.redundant
    );
    assert_eq!(p.spent, 2, "combined Jacobian rank is 2");
    assert_eq!(p.free, 4, "6 − rank(2) = 4 residual DOF");
    // driving ∪ redundant partition every input relation exactly once.
    assert_eq!(p.driving.len() + p.redundant.len(), relations.len());
    let mut all: Vec<usize> = p.driving.iter().chain(p.redundant.iter()).copied().collect();
    all.sort_unstable();
    assert_eq!(all, vec![0, 1, 2], "every relation is classified exactly once");
}

// ── Order-independence ───────────────────────────────────────────────────────

/// Permuting the input relation order yields the SAME {driving count, redundant
/// count} and the SAME total spent DOF — for both the B1 (rank-5) and B2 (rank-2)
/// scenarios. (The specific relation that lands in the remainder may differ; only
/// the counts and the spent total are invariant — that is the property the
/// over-constraint design relies on.)
#[test]
fn partition_is_order_independent() {
    // B2: reverse order [rel2, rel1, rel0].
    let mut b2 = b2_relations();
    let forward = partition_driving_set(&b2, &m_unknown(), &Pose::identity(), TOL);
    b2.reverse();
    let reversed = partition_driving_set(&b2, &m_unknown(), &Pose::identity(), TOL);
    assert_eq!(
        (forward.driving.len(), forward.redundant.len(), forward.spent),
        (reversed.driving.len(), reversed.redundant.len(), reversed.spent),
        "B2 partition counts + spent must be order-independent"
    );
    assert_eq!(reversed.spent, 2, "B2 spends 2 DOF regardless of order");

    // B1: swap order [flush, concentric].
    let mut b1 = b1_relations();
    b1.reverse();
    let swapped = partition_driving_set(&b1, &bolt_unknown(), &Pose::identity(), TOL);
    assert_eq!(swapped.driving.len(), 2, "both relations still drive when swapped");
    assert_eq!(swapped.redundant.len(), 0);
    assert_eq!(swapped.spent, 5, "B1 spends 5 DOF regardless of order");
    assert_eq!(swapped.free, 1);
}

// ── Per-relation ΔDOF cross-check vs γ's relation_delta_dof ──────────────────

/// Each relation's MEASURED individual Jacobian rank (computed at the witness from
/// its own rows alone) must equal its γ-published nominal ΔDOF codimension —
/// concentric removes 4, flush removes 3, perpendicular removes 1. This is the
/// cross-check that guards the numeric rank against false redundancy/conflict: a
/// measured rank below the nominal codimension would signal a degenerate operand
/// config, not a real relation.
///
/// (The nominal values carried here are exactly what `reify_compiler`'s γ
/// `relation_delta_dof` returns for these operand shapes — concentric(Axis,Axis)=4,
/// flush(Plane,Plane)=3, perpendicular(Direction,Direction)=1 — but that fn is
/// `pub(crate)` to reify-compiler, so the partition consumes the codimension as
/// carried data rather than calling it across the crate boundary.)
#[test]
fn partition_per_relation_delta_dof_cross_checks_gamma() {
    // B1: concentric (nominal 4) + flush (nominal 3).
    let b1 = b1_relations();
    let p1 = partition_driving_set(&b1, &bolt_unknown(), &Pose::identity(), TOL);
    assert_eq!(p1.per_relation.len(), 2);
    for rr in &p1.per_relation {
        let nominal = rr.nominal_delta_dof.expect("γ codimension is carried for §1 relations");
        assert_eq!(
            rr.individual_rank, nominal,
            "{}: measured individual rank {} must match γ ΔDOF {}",
            rr.name, rr.individual_rank, nominal
        );
    }
    // Pin the concrete γ codimensions explicitly so a drift in the table is caught.
    assert_eq!(p1.per_relation[0].nominal_delta_dof, Some(4), "concentric removes 4");
    assert_eq!(p1.per_relation[1].nominal_delta_dof, Some(3), "flush removes 3");
    assert_eq!(p1.per_relation[0].individual_rank, 4);
    assert_eq!(p1.per_relation[1].individual_rank, 3);

    // B2: each perpendicular has nominal + measured rank 1.
    let b2 = b2_relations();
    let p2 = partition_driving_set(&b2, &m_unknown(), &Pose::identity(), TOL);
    assert_eq!(p2.per_relation.len(), 3);
    for rr in &p2.per_relation {
        assert_eq!(rr.individual_rank, 1, "perpendicular removes 1 angular DOF");
        assert_eq!(rr.nominal_delta_dof, Some(1));
    }
}
