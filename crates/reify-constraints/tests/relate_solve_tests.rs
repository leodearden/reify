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
    FrameUnknown, Operand, Pose, RelateTolerance, RelationInstance, ResidualUnit,
    comparable_datum_operands, max_relation_residual, partition_driving_set, pose_from_frame,
    solve_frame, static_relation_residuals,
};
use reify_ir::{SolveResult, Value};

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
        free: false,
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
        free: false,
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

// ── step-9 RED: solve_frame drives the auto Frame to satisfy the driving set ──
//
// `solve_frame(driving, frame_unknown, seed, tol)` takes the partition's driving set
// over realized datums + the 6-DOF Frame unknown + a seed pose, and returns a
// `SolveResult::Solved { values, unique }` whose solved Frame satisfies every driving
// relation within the solver convergence tolerance. The synthetic §1 scenario starts
// the bolt's LOCAL datums at the origin; the fixed plate anchor is offset, so a
// non-trivial transform is required — the solve must FIND it (not merely confirm an
// already-satisfied identity witness as the partition tests do). RED until step-10
// adds `solve_frame` / `max_relation_residual` / `pose_from_frame` / `RelateTolerance`.

/// The §1 concentric+flush relations posed as a SOLVE scenario (B1). The bolt's LOCAL
/// shank axis + seat plane sit at the origin; the fixed plate anchor is offset by
/// `(0.10, 0.20)` in-plane (hole axis) and seats `0.045 m` along the axis (top plane
/// at `z = 0.050`, seat local `z = 0.005`). The unique solving transform is therefore
/// translation `(0.10, 0.20, 0.045)` with identity rotation; spin about the shank
/// axis is a residual gauge freedom (concentric+flush leave it open).
fn b1_solve_relations() -> Vec<RelationInstance> {
    vec![
        relation(
            "concentric",
            vec![
                datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                datum("plate", axis((0.10, 0.20, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "flush",
            vec![
                datum("bolt", plane((0.0, 0.0, 0.005), (0.0, 0.0, 1.0))),
                datum("plate", plane((0.0, 0.0, 0.050), (0.0, 0.0, 1.0))),
            ],
            3,
        ),
    ]
}

/// B1 — the driving-set solve converges: the returned `Solved` Frame seats the bolt
/// coaxial+flush, with the relation residual at the solved pose ≤ the solver
/// convergence tolerance (a method guarantee of returning `Solved`, not a guessed
/// epsilon), and the solved value is a `Value::Frame`.
#[test]
fn solve_frame_b1_converges_within_solver_tol() {
    let relations = b1_solve_relations();
    let tol = RelateTolerance::kernel_default();
    let result = solve_frame(
        &relations,
        &bolt_unknown(),
        &Pose::identity(),
        tol.solver_convergence(),
    );

    let values = match result {
        SolveResult::Solved { values, .. } => values,
        other => panic!("expected Solved for the feasible §1 scenario, got {other:?}"),
    };
    assert_eq!(values.len(), 1, "exactly one solved Frame for the single auto sub");
    let frame = values.values().next().expect("a solved Frame value");
    assert!(
        matches!(frame, Value::Frame { .. }),
        "the 6 solved scalars assemble into a Value::Frame, got {frame:?}"
    );

    // Method guarantee: at the solved pose every driving relation is satisfied within
    // the solver convergence tolerance (concentric coaxial + flush coplanar).
    let pose = pose_from_frame(frame).expect("solved Frame converts back to a Pose");
    let resid = max_relation_residual(&relations, &bolt_unknown(), &pose);
    assert!(
        resid <= tol.solver_convergence(),
        "solved residual {resid} must be ≤ solver convergence {}",
        tol.solver_convergence()
    );

    // The recovered transform is the expected seat (translation pinned; spin is the
    // lone gauge freedom, left at the identity seed). Checked within the (looser)
    // assertion tolerance per the single-knob hierarchy.
    assert!((pose.translation[0] - 0.10).abs() <= tol.assertion(), "tx: {:?}", pose.translation);
    assert!((pose.translation[1] - 0.20).abs() <= tol.assertion(), "ty: {:?}", pose.translation);
    assert!((pose.translation[2] - 0.045).abs() <= tol.assertion(), "tz: {:?}", pose.translation);
}

/// The single kernel-defaulted `Length` knob governs the whole tolerance hierarchy:
/// `kernel_local ≤ solver_convergence ≤ assertion/dedup` (PRD §7.1 coherence law).
/// Numeric boundary assertions test against the solver's OWN convergence guarantee,
/// never a hand-picked epsilon — so the ordering must hold by construction.
#[test]
fn solve_frame_single_knob_tolerance_hierarchy() {
    let tol = RelateTolerance::kernel_default();
    assert!(
        tol.kernel_local() <= tol.solver_convergence(),
        "kernel-local {} must be ≤ solver convergence {}",
        tol.kernel_local(),
        tol.solver_convergence()
    );
    assert!(
        tol.solver_convergence() <= tol.assertion(),
        "solver convergence {} must be ≤ assertion/dedup {}",
        tol.solver_convergence(),
        tol.assertion()
    );
    // All three are strictly-positive lengths (metres).
    assert!(tol.kernel_local() > 0.0, "tolerances are positive lengths");
}

// ── step-11 RED: auto(free) + residual seeding + seed bias (B5) ───────────────
//
// The `free` flag on the Frame unknown + the seed `Pose` change how `solve_frame`
// reports and seeds a residual DOF (PRD §7.1 step 3):
//   * `auto(free)` waives the uniqueness check — even a fully-determined system
//     returns `unique:false`; a residual DOF is seeded to a CONCRETE value (the
//     solved Frame is always fully numeric, NEVER a free/NaN variable).
//   * strict `auto` (free=false) reports `unique:true` only when all 6 DOF are
//     pinned; a genuine residual leaves `unique:false` — the under-determined
//     signal, DISTINCT from the unique case — with the residual DOF count available
//     from the partition (step-8's `free`), NOT a unique placement.
//   * `auto(seed=…)` (the seed `Pose`) biases each residual DOF toward the seed.
// RED until step-12 adds the `free` field to `FrameUnknown` + the free/seed wiring
// (the `unknown(sub, free)` helper references the not-yet-existing field).

/// A Frame unknown for `sub` with the given `free` flag (`auto` vs `auto(free)`).
fn unknown(sub: &str, free: bool) -> FrameUnknown {
    FrameUnknown {
        sub: sub.to_string(),
        free,
    }
}

/// A fully-determined (rank-6) synthetic scenario for the moving sub `"m"`:
/// `coincident(point,point)` pins 3 translation DOF; `parallel(+z,+z)` pins 2 tilt
/// DOF; `parallel(+x,+x)` pins the remaining spin — all 6 Frame DOF determined, all
/// already satisfied at the identity witness.
fn fully_determined_relations() -> Vec<RelationInstance> {
    vec![
        relation(
            "coincident",
            vec![
                datum("m", point3(0.0, 0.0, 0.0)),
                datum("a", point3(0.0, 0.0, 0.0)),
            ],
            3,
        ),
        relation(
            "parallel",
            vec![datum("m", dir(0.0, 0.0, 1.0)), datum("a", dir(0.0, 0.0, 1.0))],
            2,
        ),
        relation(
            "parallel",
            vec![datum("m", dir(1.0, 0.0, 0.0)), datum("a", dir(1.0, 0.0, 0.0))],
            2,
        ),
    ]
}

/// B5 — a single `concentric(axis,axis)` leaving a residual DOF. The moving bolt's
/// LOCAL shank axis sits at the origin; the fixed plate hole axis is offset by
/// `(0.10, 0.20)` in-plane, both `+z`. concentric pins {2 tilt, 2 perp-position} = 4
/// DOF; the 2 residual DOF are slide-along-axis (`tz`) + spin-about-axis (`rot_z`).
fn b5_relations() -> Vec<RelationInstance> {
    vec![relation(
        "concentric",
        vec![
            datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis((0.10, 0.20, 0.0), (0.0, 0.0, 1.0))),
        ],
        4,
    )]
}

/// The `free` flag waives uniqueness: a rank-6 system is `unique:true` under strict
/// `auto` but `unique:false` under `auto(free)` (the perturbation check is skipped).
#[test]
fn solve_frame_free_flag_waives_uniqueness() {
    let relations = fully_determined_relations();
    let tol = RelateTolerance::kernel_default();

    let strict = solve_frame(
        &relations,
        &unknown("m", false),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    match strict {
        SolveResult::Solved { unique, .. } => {
            assert!(unique, "a rank-6 strict auto is uniquely determined")
        }
        other => panic!("expected Solved, got {other:?}"),
    }

    let free = solve_frame(
        &relations,
        &unknown("m", true),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    match free {
        SolveResult::Solved { unique, .. } => {
            assert!(!unique, "auto(free) waives the uniqueness check")
        }
        other => panic!("expected Solved, got {other:?}"),
    }
}

/// `auto(free)` with a residual DOF: `Solved{unique:false}` and the residual is
/// seeded to a CONCRETE value — every solved Frame scalar is finite (never a free
/// variable). The residual DOF count comes from the partition (step-8's `free`).
#[test]
fn solve_frame_free_residual_is_fully_numeric() {
    let relations = b5_relations();
    let tol = RelateTolerance::kernel_default();

    let result = solve_frame(
        &relations,
        &unknown("bolt", true),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    let (values, unique) = match result {
        SolveResult::Solved { values, unique } => (values, unique),
        other => panic!("expected Solved for an auto(free) residual, got {other:?}"),
    };
    assert!(!unique, "a residual DOF under auto(free) is not uniquely determined");

    let frame = values.values().next().expect("a solved Frame");
    let pose = pose_from_frame(frame).expect("solved Frame → Pose");
    for c in pose.translation.iter().chain(pose.rotation.iter()) {
        assert!(c.is_finite(), "every solved Frame scalar is concrete (finite), got {c}");
    }

    let part = partition_driving_set(
        &relations,
        &unknown("bolt", true),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    assert_eq!(part.free, 2, "concentric leaves slide + spin = 2 residual DOF");
}

/// Strict `auto` with a genuine residual surfaces the under-determined signal
/// (`unique:false`) — DISTINCT from the rank-6 `unique:true` case — carrying the
/// residual DOF count via the partition, NOT a unique placement.
#[test]
fn solve_frame_strict_residual_signals_under_determined() {
    let relations = b5_relations();
    let tol = RelateTolerance::kernel_default();

    let result = solve_frame(
        &relations,
        &unknown("bolt", false),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    match result {
        SolveResult::Solved { unique, .. } => {
            assert!(!unique, "a strict residual is under-determined (unique:false)")
        }
        other => panic!("expected Solved, got {other:?}"),
    }

    let part = partition_driving_set(
        &relations,
        &unknown("bolt", false),
        &Pose::identity(),
        tol.solver_convergence(),
    );
    assert_eq!(part.free, 2, "the under-determined signal carries the residual DOF count");
}

/// `auto(seed=…)` biases the residual DOF toward the seed: the constrained DOF solve
/// to the anchor (perp position → the plate hole's `(0.10, 0.20)`), while the slide
/// (`tz`) and spin (`rot_z`) residual DOF stay AT the seed's concrete values.
#[test]
fn solve_frame_seed_biases_residual_dof() {
    let relations = b5_relations();
    let tol = RelateTolerance::kernel_default();

    let seed = Pose {
        translation: [0.0, 0.0, 0.05],
        rotation: [0.0, 0.0, 0.3],
    };
    let result = solve_frame(&relations, &unknown("bolt", true), &seed, tol.solver_convergence());
    let values = match result {
        SolveResult::Solved { values, .. } => values,
        other => panic!("expected Solved, got {other:?}"),
    };
    let pose = pose_from_frame(values.values().next().unwrap()).unwrap();

    // Constrained perp-position DOF solve to the anchor offset.
    assert!((pose.translation[0] - 0.10).abs() <= tol.assertion(), "tx pinned: {:?}", pose.translation);
    assert!((pose.translation[1] - 0.20).abs() <= tol.assertion(), "ty pinned: {:?}", pose.translation);
    // Residual DOF biased toward the seed (slide + spin).
    assert!((pose.translation[2] - 0.05).abs() <= tol.assertion(), "tz biased to seed: {:?}", pose.translation);
    assert!((pose.rotation[2] - 0.3).abs() <= tol.assertion(), "spin biased to seed: {:?}", pose.rotation);
}

// ── Residual-form robustness (amendments) ────────────────────────────────────
//
// `angle` and `distance` are not exercised by the B1/B2/B3/B5 partition/solve
// scenarios above; these kernel-free units pin the two residual-form corrections a
// review surfaced (a non-unit direction operand for `angle`; an axial-slide-coupled
// origin distance for `distance` over axes). Both drive `max_relation_residual`
// directly so the residual algebra is checked without a solver round-trip.

/// `angle(a, b, θ)` must normalize its direction operands before comparing the dot
/// product against `cos θ` — a NON-unit operand otherwise reads the residual zero at
/// the wrong angle. Moving operand: a magnitude-2 `+x` direction; anchor: a unit 45°
/// direction. The true angle is 45°, so an `angle(.., 45°)` relation is satisfied
/// (residual ≈ 0) ONLY if the magnitude-2 operand is normalized first — a raw
/// `dot(da, db) − cos 45°` would leave a spurious `2·cos45° − cos45° = cos45° ≈ 0.707`.
#[test]
fn angle_residual_normalizes_non_unit_direction_operands() {
    let s = 1.0 / 2.0_f64.sqrt();
    let rel = RelationInstance {
        name: "angle".to_string(),
        operands: vec![
            datum("m", dir(2.0, 0.0, 0.0)),
            datum("anchor", dir(s, s, 0.0)),
            Operand {
                sub: None,
                datum: Value::Real(std::f64::consts::FRAC_PI_4),
            },
        ],
        nominal_delta_dof: Some(1),
    };
    let resid =
        max_relation_residual(std::slice::from_ref(&rel), &unknown("m", false), &Pose::identity());
    assert!(
        resid < 1e-9,
        "angle residual must be ≈0 at the true 45° angle even for a NON-unit operand \
         (normalized before the dot); got {resid}"
    );
}

/// `parallel` / `antiparallel` / `coincident`-over-Direction must distinguish the two
/// senses: a `parallel` request is satisfied only by a SAME-sense pair, `antiparallel`
/// only by an OPPOSITE-sense pair, and `coincident` over Direction only by a same-sense
/// pair. The earlier tangent-plane-only residual was sign-blind — its residual vanished
/// for BOTH senses, so an `antiparallel` relation was (wrongly) satisfied by a parallel
/// solution and vice-versa. All checks at the identity witness so the moving operand is
/// the literal direction given.
#[test]
fn direction_sense_disambiguates_parallel_antiparallel_coincident() {
    let u = unknown("m", false);
    let id = Pose::identity();
    // A wrong-sense residual is the unit-difference norm (≈2 per component); 0.5 cleanly
    // separates it from the satisfied (≈0) case without pinning a tuned epsilon.
    let wrong = 0.5;

    let rel = |name: &str, m: Value, a: Value| RelationInstance {
        name: name.to_string(),
        operands: vec![datum("m", m), datum("a", a)],
        nominal_delta_dof: None,
    };

    // parallel: same sense satisfied, opposite sense NOT.
    assert!(
        max_relation_residual(&[rel("parallel", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, 1.0))], &u, &id)
            < 1e-9,
        "parallel(+z,+z) is satisfied (same sense)"
    );
    assert!(
        max_relation_residual(&[rel("parallel", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, -1.0))], &u, &id)
            > wrong,
        "parallel(+z,−z) must NOT be satisfied — antiparallel pair is not parallel"
    );

    // antiparallel: opposite sense satisfied, same sense NOT.
    assert!(
        max_relation_residual(
            &[rel("antiparallel", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, -1.0))],
            &u,
            &id
        ) < 1e-9,
        "antiparallel(+z,−z) is satisfied (opposite sense)"
    );
    assert!(
        max_relation_residual(
            &[rel("antiparallel", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, 1.0))],
            &u,
            &id
        ) > wrong,
        "antiparallel(+z,+z) must NOT be satisfied — parallel pair is not antiparallel"
    );

    // coincident over Direction: same sense satisfied, opposite sense NOT.
    assert!(
        max_relation_residual(&[rel("coincident", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, 1.0))], &u, &id)
            < 1e-9,
        "coincident(+z,+z) is satisfied (same sense)"
    );
    assert!(
        max_relation_residual(
            &[rel("coincident", dir(0.0, 0.0, 1.0), dir(0.0, 0.0, -1.0))],
            &u,
            &id
        ) > wrong,
        "coincident over Direction must require same sense — an antiparallel pair is not coincident"
    );
}

/// `distance(a, b, d)` over two AXES must measure the perpendicular line-to-line
/// distance, NOT the origin-to-origin distance — so an axial slide along the axes
/// does not couple into the metric. Two parallel `+z` axes offset perpendicularly by
/// `p = 0.10 m` and axially by `L = 0.50 m` have line distance `p`; an origin-to-origin
/// measure would read `√(p²+L²) ≈ 0.51 m`. A `distance(.., .., p)` relation is therefore
/// satisfied (residual ≈ 0) only under the perpendicular measure.
#[test]
fn distance_over_axes_is_perpendicular_not_origin_to_origin() {
    let p = 0.10;
    let axial_slide = 0.50;
    let rel = RelationInstance {
        name: "distance".to_string(),
        operands: vec![
            datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis((p, 0.0, axial_slide), (0.0, 0.0, 1.0))),
            Operand {
                sub: None,
                datum: Value::length(p),
            },
        ],
        nominal_delta_dof: Some(1),
    };
    let resid =
        max_relation_residual(std::slice::from_ref(&rel), &bolt_unknown(), &Pose::identity());
    assert!(
        resid < 1e-9,
        "distance over two parallel axes must measure the perpendicular offset \
         ({p} m), independent of the {axial_slide} m axial slide between origins; got {resid}"
    );
}

// ── step-15 RED: fasten (frame coincidence, codim 6) ─────────────────────────
//
// `fasten(a.frame, b.frame)` is coincident-OVER-Frame (η): a 6-component residual
// (3 origin-delta + 3 orientation-delta) that is zero exactly when the two frames
// coincide, locking ALL 6 DOF (codim 6, kinds (3,3) — 3 translational + 3 rotational).
// It is the residual `ground(sub)` desugars onto, so a grounded sub fastened to
// `self.frame` (identity) must solve to identity. RED until step-16 adds the
// `"fasten"` arm to `residual_dispatch` + the (Frame, Frame) branch to
// `coincident_residual` (and admits a moving Frame operand in is_datum/transform_datum);
// today `Value::Frame` is excluded from `is_datum`, so a fasten relation contributes
// NO residual rows — every assertion below is RED.

/// A `Value::Frame` from an origin (metres) + a basis quaternion `(w, x, y, z)`.
fn frame(o: (f64, f64, f64), q: (f64, f64, f64, f64)) -> Value {
    Value::Frame {
        origin: Box::new(point3(o.0, o.1, o.2)),
        basis: Box::new(Value::Orientation {
            w: q.0,
            x: q.1,
            y: q.2,
            z: q.3,
        }),
    }
}

/// The identity frame: world origin + identity basis quaternion.
fn identity_frame() -> Value {
    frame((0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0))
}

/// A `fasten(a.frame, anchor.frame)` relation over two Frame operands: moving sub
/// `"a"`, fixed anchor sub `"b"`, with γ's nominal ΔDOF 6 carried for the cross-check.
fn fasten_relation(a_frame: Value, anchor_frame: Value) -> RelationInstance {
    relation("fasten", vec![datum("a", a_frame), datum("b", anchor_frame)], 6)
}

/// The auto Frame unknown for the moving sub `"a"`.
fn a_unknown() -> FrameUnknown {
    FrameUnknown {
        sub: "a".to_string(),
        free: false,
    }
}

/// The fasten residual is ~0 when the two frames coincide and moves off zero under
/// BOTH a pure-translation and a pure-rotation perturbation — proving it carries a
/// 6-component residual with both a translational and a rotational sub-block.
#[test]
fn fasten_residual_zero_at_coincidence_nonzero_when_perturbed() {
    let rel = fasten_relation(identity_frame(), identity_frame());
    let u = a_unknown();

    // Coincident at the identity witness: the moving frame transformed by the identity
    // pose equals the identity anchor ⇒ all 6 residual components vanish.
    let at_id = max_relation_residual(std::slice::from_ref(&rel), &u, &Pose::identity());
    assert!(at_id < 1e-9, "fasten residual ≈0 when the frames coincide; got {at_id}");

    // A pure-translation perturbation moves the 3 origin-delta components (the
    // translational sub-block) off zero.
    let translated = Pose {
        translation: [0.05, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0],
    };
    let r_t = max_relation_residual(std::slice::from_ref(&rel), &u, &translated);
    assert!(
        r_t > 1e-3,
        "a translation perturbation must move the origin-delta block; got {r_t}"
    );

    // A pure-rotation perturbation moves the 3 orientation-delta components (the
    // rotational sub-block) off zero — fasten carries BOTH a translational and a
    // rotational residual block (kinds (3,3)).
    let rotated = Pose {
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.2],
    };
    let r_r = max_relation_residual(std::slice::from_ref(&rel), &u, &rotated);
    assert!(
        r_r > 1e-3,
        "a rotation perturbation must move the orientation-delta block; got {r_r}"
    );
}

/// The fasten residual's measured codimension is 6 — its Jacobian at the witness has
/// full rank 6 (all 6 Frame DOF independently constrained), cross-checking γ's
/// nominal ΔDOF 6 (the codim-law sum-invariant: 3 translational + 3 rotational).
#[test]
fn fasten_codimension_is_six() {
    let rel = fasten_relation(identity_frame(), identity_frame());
    let p = partition_driving_set(
        std::slice::from_ref(&rel),
        &a_unknown(),
        &Pose::identity(),
        TOL,
    );

    assert_eq!(p.per_relation.len(), 1);
    assert_eq!(
        p.per_relation[0].individual_rank, 6,
        "fasten locks all 6 Frame DOF (3 translational + 3 rotational): measured rank {}",
        p.per_relation[0].individual_rank
    );
    assert_eq!(p.spent, 6, "fasten spends all 6 DOF");
    assert_eq!(p.free, 0, "a fastened sub has no residual DOF");
    // Cross-check the measured codimension against γ's nominal ΔDOF for fasten.
    assert_eq!(
        p.per_relation[0].nominal_delta_dof,
        Some(6),
        "γ publishes ΔDOF 6 for fasten"
    );
    assert_eq!(
        p.per_relation[0].individual_rank,
        p.per_relation[0].nominal_delta_dof.unwrap(),
        "measured codimension must match the nominal ΔDOF"
    );
}

/// `{ fasten(a.frame=identity, self.frame=identity) }` grounds the sub: the solve
/// drives all 6 DOF back to the identity pose. Seeded from a NON-identity pose so the
/// solver has to FIND identity (not merely confirm an already-satisfied seed), and
/// reports `unique:true` (fasten pins every DOF).
#[test]
fn fasten_solve_grounds_sub_to_identity() {
    let rel = fasten_relation(identity_frame(), identity_frame());
    let tol = RelateTolerance::kernel_default();
    let seed = Pose {
        translation: [0.05, 0.03, -0.02],
        rotation: [0.1, -0.05, 0.2],
    };

    let result = solve_frame(
        std::slice::from_ref(&rel),
        &a_unknown(),
        &seed,
        tol.solver_convergence(),
    );
    let (values, unique) = match result {
        SolveResult::Solved { values, unique } => (values, unique),
        other => panic!("expected Solved for a fully-grounded fasten, got {other:?}"),
    };
    assert!(
        unique,
        "fasten pins all 6 DOF ⇒ the grounded sub is uniquely determined"
    );

    let solved = values.values().next().expect("a solved Frame");
    assert!(
        matches!(solved, Value::Frame { .. }),
        "the solved pose assembles into a Value::Frame, got {solved:?}"
    );
    let pose = pose_from_frame(solved).expect("solved Frame → Pose");

    // Converged to identity: every DOF driven back to ~0 within the assertion tol.
    for t in pose.translation.iter() {
        assert!(
            t.abs() <= tol.assertion(),
            "translation grounded to identity: {:?}",
            pose.translation
        );
    }
    for r in pose.rotation.iter() {
        assert!(
            r.abs() <= tol.assertion(),
            "rotation grounded to identity: {:?}",
            pose.rotation
        );
    }

    // The residual at the solved pose meets the solver-convergence guarantee.
    let resid = max_relation_residual(std::slice::from_ref(&rel), &a_unknown(), &pose);
    assert!(
        resid <= tol.solver_convergence(),
        "solved residual {resid} must be ≤ solver convergence {}",
        tol.solver_convergence()
    );
}

// ── task 5540 step-5 RED: tangent residuals + rank ───────────────────────────
//
// `tangent` is the one curated relation `residual_dispatch` has no arm for: it
// falls through the catch-all and contributes ZERO rows. That is not a loud gap
// but a SILENT one — no rows ⇒ `partition_driving_set` files the relation as
// redundant with `rank_contribution: 0`, and `max_relation_residual` reads 0.0,
// so a tangency request is wholly ignored yet reported satisfied. These units pin
// the four residual forms and their exact codimensions so the gap cannot reopen.
//
// Radii travel as trailing `Length` scalar operands (`tangent(a, b, r)` /
// `tangent(a, b, r1, r2)`), reusing the metric-operand plumbing `distance` /
// `offset` / `angle` already run — the surface-carried `<HasAxis & HasRadius>`
// form is sibling task #5588's, not this one's. Sign convention: the target
// separation is `|r1 + r2|`, so two positive radii mean EXTERNAL tangency and a
// negative second radius means INTERNAL (`|r1 − |r2||`), with no branch.
//
// Every scenario is placed to be exactly satisfied at `Pose::identity()` per the
// module convention above, so the expected values are exact closed forms.
//
// RED until step-6 adds the `"tangent"` arm (and the multi-scalar plumbing the
// two-radius combos need): today every assertion below reads residual 0.0 / rank 0.

/// A trailing radius operand — a bare `Length` scalar with no owning sub, exactly
/// the shape `build_relation_instances` pushes for a metric argument.
fn radius(r: f64) -> Operand {
    Operand {
        sub: None,
        datum: Value::length(r),
    }
}

/// A `tangent` relation over `operands`, carrying the γ-published ΔDOF for its
/// combo (1 for cyl/cyl, sphere/plane and sphere/sphere; 2 for cyl/plane).
fn tangent(operands: Vec<Operand>, nominal_delta_dof: u32) -> RelationInstance {
    relation("tangent", operands, nominal_delta_dof)
}

/// The exact-algebra tolerance for a residual closed form. Unlike the rank
/// tolerance [`TOL`] (which absorbs finite-difference noise in the Jacobian),
/// residual evaluation is straight-line arithmetic on the operand values, so the
/// only slack needed is float round-off.
const EXACT: f64 = 1e-12;

/// The moving sub used by every tangent scenario.
fn tangent_unknown() -> FrameUnknown {
    unknown("m", false)
}

/// Residual of a single relation at the identity witness.
fn resid_at_identity(rel: &RelationInstance) -> f64 {
    max_relation_residual(std::slice::from_ref(rel), &tangent_unknown(), &Pose::identity())
}

/// The measured individual Jacobian rank of a single relation at identity — the
/// geometry's own codimension, cross-checked against the published ΔDOF.
fn measured_rank(rel: &RelationInstance) -> u32 {
    partition_driving_set(
        std::slice::from_ref(rel),
        &tangent_unknown(),
        &Pose::identity(),
        TOL,
    )
    .per_relation[0]
        .individual_rank
}

/// cylinder/cylinder `tangent(axis_a, axis_b, r1, r2)` is ONE row:
/// `line_line_distance(a, b) − |r1 + r2|`. Two parallel `+z` axes separated
/// perpendicularly by exactly `r1 + r2 = 12 mm` are tangent (residual 0); widening
/// the separation to 20 mm leaves the exact excess `20 − 12 = 8 mm`.
///
/// Measuring LINE distance (not origin-to-origin) is what keeps an axial slide
/// along the cylinders out of the metric, matching `distance` over axes.
#[test]
fn tangent_cyl_cyl_residual_is_line_distance_minus_summed_radii() {
    let (r1, r2) = (0.005, 0.007);
    let touching = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("anchor", axis((r1 + r2, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r1),
            radius(r2),
        ],
        1,
    );
    let got = resid_at_identity(&touching);
    assert!(
        got < EXACT,
        "two parallel cylinders whose axes are {} m apart with radii {r1} + {r2} are \
         externally tangent ⇒ residual 0; got {got}",
        r1 + r2
    );

    // An axial slide of 0.5 m along both axes must not move the residual — the
    // metric is the perpendicular line distance, not the origin separation.
    let slid = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("anchor", axis((r1 + r2, 0.0, 0.5), (0.0, 0.0, 1.0))),
            radius(r1),
            radius(r2),
        ],
        1,
    );
    let got = resid_at_identity(&slid);
    assert!(
        got < EXACT,
        "an axial slide along parallel cylinder axes must not couple into the tangency \
         metric (perpendicular line distance); got {got}"
    );

    // Pulled apart to 20 mm: the residual is the exact 8 mm excess.
    let apart = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("anchor", axis((0.020, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r1),
            radius(r2),
        ],
        1,
    );
    let got = resid_at_identity(&apart);
    let want = 0.020 - (r1 + r2);
    assert!(
        (got - want).abs() < EXACT,
        "separation 0.020 m against a {} m tangency target leaves residual {want}; got {got}",
        r1 + r2
    );
}

/// A NEGATIVE radius selects INTERNAL tangency without a branch: the target
/// separation is `|r1 + r2|`, which for `r2 < 0` collapses to `|r1 − |r2||` — the
/// small cylinder running inside the large one. A 20 mm cylinder with an 8 mm
/// cylinder inside it touches when the axes are 12 mm apart, and the SAME geometry
/// read with both radii positive would demand 28 mm.
#[test]
fn tangent_cyl_cyl_negative_radius_encodes_internal_tangency() {
    let (r_big, r_small) = (0.020, 0.008);
    let internal = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("anchor", axis((r_big - r_small, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r_big),
            radius(-r_small),
        ],
        1,
    );
    let got = resid_at_identity(&internal);
    assert!(
        got < EXACT,
        "a negative second radius means INTERNAL tangency: |r1 + r2| = |{r_big} − {r_small}| \
         = {} m, which the {} m axis separation meets exactly ⇒ residual 0; got {got}",
        r_big - r_small,
        r_big - r_small
    );

    // The same geometry with both radii POSITIVE is the external form, which wants
    // 28 mm — proving the sign genuinely selects the branch rather than being
    // absorbed by an `.abs()` on each radius.
    let external = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("anchor", axis((r_big - r_small, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r_big),
            radius(r_small),
        ],
        1,
    );
    let got = resid_at_identity(&external);
    let want = (r_big - r_small) - (r_big + r_small);
    assert!(
        (got - want.abs()).abs() < EXACT,
        "with both radii positive the same {} m separation is short of the {} m external \
         target by {want}; got {got}",
        r_big - r_small,
        r_big + r_small
    );
}

/// cylinder/plane `tangent(axis, plane, r)` is TWO rows — the axis/normal
/// perpendicularity and the SIGNED axis-origin-to-plane offset minus `r`. A `+x`
/// axis floating 5 mm above the `z = 0` plane with `r = 5 mm` satisfies both;
/// raising it to 9 mm leaves exactly the 4 mm offset error in row 2 while row 1
/// stays 0.
#[test]
fn tangent_cyl_plane_residual_has_perpendicularity_and_signed_offset_rows() {
    let r = 0.005;
    let seated = tangent(
        vec![
            datum("m", axis((0.0, 0.0, r), (1.0, 0.0, 0.0))),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        2,
    );
    let got = resid_at_identity(&seated);
    assert!(
        got < EXACT,
        "a +x cylinder axis {r} m above the z=0 plane with radius {r} rests on it ⇒ both \
         rows 0; got {got}"
    );

    // Lifted to 9 mm with the SAME (parallel) orientation: row 1 stays 0, so the max
    // residual is exactly row 2's 4 mm offset error.
    let lifted = tangent(
        vec![
            datum("m", axis((0.0, 0.0, 0.009), (1.0, 0.0, 0.0))),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        2,
    );
    let got = resid_at_identity(&lifted);
    let want = 0.009 - r;
    assert!(
        (got - want).abs() < EXACT,
        "a parallel axis at 0.009 m with radius {r} overshoots tangency by {want}; got {got}"
    );

    // A centre BELOW the plane is NOT tangent from above: the offset row is SIGNED,
    // so `−r` against a `+r` request reads `−2r`, not 0. An `.abs()` form (as
    // `distance_residual` uses) would wrongly report this satisfied.
    let below = tangent(
        vec![
            datum("m", axis((0.0, 0.0, -r), (1.0, 0.0, 0.0))),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        2,
    );
    let got = resid_at_identity(&below);
    assert!(
        (got - 2.0 * r).abs() < EXACT,
        "the cylinder/plane offset row must be SIGNED so the radius sign picks the side \
         of the plane: an axis at −{r} against a +{r} request reads {}, not 0; got {got}",
        2.0 * r
    );
}

/// REGRESSION GUARD for the failure the ΔDOF table exists to prevent: a cylinder
/// TILTED out of parallel but still at the right distance would sit at exactly zero
/// residual if the cylinder/plane form carried only the offset row. Row 1 (the
/// perpendicularity of the axis direction against the plane normal) is what makes
/// this codimension 2 rather than 1.
///
/// The axis is tilted 45° in the xz-plane with its origin still `r` above the plane,
/// so the offset row is exactly 0 and the whole residual IS row 1: `dot(û, n̂)` for
/// a 45° axis is `1/√2`. Asserting that exact value also pins the normalization —
/// an un-normalized `dot((1,0,1), (0,0,1))` would read 1.
#[test]
fn tangent_cyl_plane_tilted_axis_at_correct_distance_is_not_satisfied() {
    let r = 0.005;
    let tilted = tangent(
        vec![
            // Deliberately NON-unit direction: (1,0,1) has norm √2.
            datum("m", axis((0.0, 0.0, r), (1.0, 0.0, 1.0))),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        2,
    );
    let got = resid_at_identity(&tilted);
    let want = 1.0 / 2.0_f64.sqrt();
    assert!(
        (got - want).abs() < EXACT,
        "a 45°-tilted cylinder at the correct {r} m offset must NOT read satisfied — the \
         perpendicularity row is dot(û, n̂) = {want} for a UNIT-normalized axis direction \
         (an un-normalized dot would read 1.0); got {got}"
    );
}

/// sphere/plane `tangent(centre, plane, r)` is ONE row: the SIGNED
/// centre-to-plane offset minus `r`. A centre 5 mm above the `z = 0` plane with
/// `r = 5 mm` is tangent; the mirrored centre 5 mm BELOW is not — an absolute-value
/// form would wrongly accept it, which is the whole reason the row is signed.
#[test]
fn tangent_sphere_plane_residual_is_signed_not_absolute() {
    let r = 0.005;
    let resting = tangent(
        vec![
            datum("m", point3(0.0, 0.0, r)),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        1,
    );
    let got = resid_at_identity(&resting);
    assert!(
        got < EXACT,
        "a sphere centre {r} m above the z=0 plane with radius {r} rests on it ⇒ residual 0; \
         got {got}"
    );

    let mirrored = tangent(
        vec![
            datum("m", point3(0.0, 0.0, -r)),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(r),
        ],
        1,
    );
    let got = resid_at_identity(&mirrored);
    assert!(
        (got - 2.0 * r).abs() < EXACT,
        "the sphere/plane row must be SIGNED: a centre at −{r} against a +{r} request reads \
         {}, not the 0 an `.abs()` form would give; got {got}",
        2.0 * r
    );
}

/// A NEGATIVE sphere radius selects the FAR side of the plane: the signed row
/// `dot(c − o, n̂) − r` is satisfied at `c·n̂ = r`, so `r = −5 mm` places the centre
/// 5 mm BELOW the plane. This is the same sign convention the cylinder combos use,
/// and it stays differentiable through zero.
#[test]
fn tangent_sphere_plane_negative_radius_lands_on_the_far_side() {
    let r = 0.005;
    let far_side = tangent(
        vec![
            datum("m", point3(0.0, 0.0, -r)),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(-r),
        ],
        1,
    );
    let got = resid_at_identity(&far_side);
    assert!(
        got < EXACT,
        "a negative radius selects the far side of the plane: centre at −{r} with r = −{r} \
         ⇒ residual 0; got {got}"
    );

    // Contrast: the NEAR-side centre against the same negative request is off by 2r.
    // Without this the satisfied case above would also hold for a form that ignored
    // the radius sign entirely (or produced no rows at all).
    let near_side = tangent(
        vec![
            datum("m", point3(0.0, 0.0, r)),
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            radius(-r),
        ],
        1,
    );
    let got = resid_at_identity(&near_side);
    assert!(
        (got - 2.0 * r).abs() < EXACT,
        "r = −{r} requests the far side, so a centre at +{r} is off by {}; got {got}",
        2.0 * r
    );
}

/// sphere/sphere `tangent(a, b, r1, r2)` is ONE row:
/// `‖pa − pb‖ − |r1 + r2|`. Centres 12 mm apart with radii 5 mm + 7 mm are
/// externally tangent; pushed to 20 mm the residual is the exact 8 mm excess.
#[test]
fn tangent_sphere_sphere_residual_is_centre_distance_minus_summed_radii() {
    let (r1, r2) = (0.005, 0.007);
    let touching = tangent(
        vec![
            datum("m", point3(0.0, 0.0, 0.0)),
            datum("anchor", point3(r1 + r2, 0.0, 0.0)),
            radius(r1),
            radius(r2),
        ],
        1,
    );
    let got = resid_at_identity(&touching);
    assert!(
        got < EXACT,
        "centres {} m apart with radii {r1} + {r2} are externally tangent ⇒ residual 0; got {got}",
        r1 + r2
    );

    let apart = tangent(
        vec![
            datum("m", point3(0.0, 0.0, 0.0)),
            datum("anchor", point3(0.020, 0.0, 0.0)),
            radius(r1),
            radius(r2),
        ],
        1,
    );
    let got = resid_at_identity(&apart);
    let want = 0.020 - (r1 + r2);
    assert!(
        (got - want).abs() < EXACT,
        "centre separation 0.020 m against a {} m tangency target leaves residual {want}; \
         got {got}",
        r1 + r2
    );
}

/// The plane combos must be operand-order symmetric: `tangent(axis, plane, r)` and
/// `tangent(plane, axis, r)` denote the same tangency, and the type-side classifier
/// accepts both orders — so the residual must too. `tangent_residual` reads its
/// operands POSITIONALLY (like `on_residual`), which is exactly where an
/// order-blind implementation silently produces the wrong rows.
#[test]
fn tangent_plane_combos_are_operand_order_symmetric() {
    let r = 0.005;
    let cyl_plane_reversed = tangent(
        vec![
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("m", axis((0.0, 0.0, r), (1.0, 0.0, 0.0))),
            radius(r),
        ],
        2,
    );
    let got = resid_at_identity(&cyl_plane_reversed);
    assert!(
        got < EXACT,
        "tangent(plane, axis, r) is the same relation as tangent(axis, plane, r) ⇒ \
         residual 0 for a seated cylinder; got {got}"
    );
    assert_eq!(
        measured_rank(&cyl_plane_reversed),
        2,
        "the reversed cylinder/plane order must still measure codimension 2"
    );

    let sphere_plane_reversed = tangent(
        vec![
            datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("m", point3(0.0, 0.0, r)),
            radius(r),
        ],
        1,
    );
    let got = resid_at_identity(&sphere_plane_reversed);
    assert!(
        got < EXACT,
        "tangent(plane, centre, r) is the same relation as tangent(centre, plane, r) ⇒ \
         residual 0 for a resting sphere; got {got}"
    );
    assert_eq!(
        measured_rank(&sphere_plane_reversed),
        1,
        "the reversed sphere/plane order must still measure codimension 1"
    );
}

/// DOF accounting — each combo's MEASURED Jacobian rank at a satisfying witness
/// must equal the ΔDOF `relation_delta_dof` publishes for it: cyl/cyl 1,
/// cyl/plane 2, sphere/plane 1, sphere/sphere 1.
///
/// This is the drift guard between the two `TangentCombo` classifiers — the
/// type-side one in `reify-compiler` (which publishes the count) and the
/// value-side one here (which produces the rows). They cannot share code
/// (`reify-constraints` is kernel- and compiler-free), so the binding check is
/// this end-to-end one: rows measured, not tables compared. A relation whose rank
/// is 0 is the silent no-solve this task exists to remove.
#[test]
fn tangent_measured_rank_matches_the_published_delta_dof_table() {
    let (r1, r2) = (0.005, 0.007);
    let cases: Vec<(&str, RelationInstance, u32)> = vec![
        (
            "cylinder/cylinder",
            tangent(
                vec![
                    datum("m", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                    datum("anchor", axis((r1 + r2, 0.0, 0.0), (0.0, 0.0, 1.0))),
                    radius(r1),
                    radius(r2),
                ],
                1,
            ),
            1,
        ),
        (
            "cylinder/plane",
            tangent(
                vec![
                    datum("m", axis((0.0, 0.0, r1), (1.0, 0.0, 0.0))),
                    datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                    radius(r1),
                ],
                2,
            ),
            2,
        ),
        (
            "sphere/plane",
            tangent(
                vec![
                    datum("m", point3(0.0, 0.0, r1)),
                    datum("anchor", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                    radius(r1),
                ],
                1,
            ),
            1,
        ),
        (
            "sphere/sphere",
            tangent(
                vec![
                    datum("m", point3(0.0, 0.0, 0.0)),
                    datum("anchor", point3(r1 + r2, 0.0, 0.0)),
                    radius(r1),
                    radius(r2),
                ],
                1,
            ),
            1,
        ),
    ];

    for (label, rel, want) in cases {
        // Precondition: the witness genuinely satisfies the relation, so the rank is
        // measured at the tangency configuration and not at an arbitrary pose.
        let resid = resid_at_identity(&rel);
        assert!(
            resid < EXACT,
            "{label}: the witness must satisfy the relation before its rank is meaningful; \
             residual {resid}"
        );

        let part = partition_driving_set(
            std::slice::from_ref(&rel),
            &tangent_unknown(),
            &Pose::identity(),
            TOL,
        );
        let measured = part.per_relation[0].individual_rank;
        assert_eq!(
            measured, want,
            "{label}: measured codimension must equal the published ΔDOF {want}; got {measured} \
             (0 means the relation contributes NO Jacobian rows — the silent no-solve)"
        );
        assert_eq!(
            part.per_relation[0].nominal_delta_dof,
            Some(want),
            "{label}: the carried nominal ΔDOF must agree with the measured rank"
        );
        assert_eq!(
            part.driving,
            vec![0],
            "{label}: a rank-{want} tangency must be DRIVING, never filed as redundant"
        );
        assert_eq!(
            part.spent, want,
            "{label}: spent DOF must be the combo's codimension {want}"
        );
    }
}

// ── static (ZERO-AUTO) relation residuals — DIC α (task 5415), step-1 ────────
//
// `static_relation_residuals` is the witness primitive for relate scopes with NO
// `at auto` sub: nothing moves, so there is no pose to solve for, only a verdict
// to render on the datums as they already sit.
//
// ## Why this cannot be `max_relation_residual(rels, &sentinel, &identity)`
//
// THE degeneracy these tests exist to pin. `relation_residual` marks an operand
// "moving" iff `op.sub == unknown.sub`, and `pick_ab` then resolves
//
//     a = first MOVING operand, else datums.first()
//     b = first NON-MOVING operand, else datums.last()
//
// With a sentinel unknown naming no real sub, NOTHING is moving — so `a` falls
// through to `datums[0]` and `b` resolves to the first non-moving operand, which
// is ALSO `datums[0]`. The relation is compared against ITSELF, and the damage
// runs in both directions:
//
//   * `concentric`/`flush`/`coincident`/`fasten`/`parallel` → identically 0.0,
//     i.e. exactly the false green this task exists to kill; and
//   * `perpendicular` → `d·d` = 1.0, `antiparallel` → 2.0, `distance`/`offset` →
//     `|d|`, `angle` → `1 − cos θ` — false VIOLATIONS on correct models.
//
// Only `on`/`tangent`, which read `datums` positionally, survive it. So the
// tests below deliberately cover one relation from each degeneracy direction:
// (a)/(b)/(e) would read as satisfied under the naive form, (c)/(c') would read
// as violated.
//
// ## Why the return type is a row VECTOR, not a collapsed `f64`
//
// (d) is the reason. An EMPTY row vector ("no residual model for this
// name/operand-kind combination") and an all-zero row vector ("measured, and
// satisfied") are different facts, and the caller must be able to tell them
// apart: the first is UNVERIFIABLE and must be said out loud, the second is
// silent. A `-> f64` signature can only render both as 0.0, which would trade the
// false green for a quieter one (INV-SF-3).

/// The `dic_relate_static_violated` offset, in metres — the plate's datums sit
/// here while the bushing's sit at the origin. Taken from the committed fixture
/// `docs/prds/v0_6/fixtures/dic_relate_static_violated.ri` (`30mm, 20mm, 5mm`),
/// not invented for the test.
const VIOLATED_OFFSET: (f64, f64, f64) = (0.030, 0.020, 0.005);

/// (a) The B1 shape: `concentric` over two axes separated by the fixture's
/// (30, 20, 5) mm split must measure a residual of **0.03 m**, not zero.
///
/// This is the anti-`pick_ab` test in the false-GREEN direction. The naive
/// sentinel-unknown implementation returns exactly 0.0 here and would report the
/// PRD's deliberately-violated fixture as satisfied.
///
/// The expected 0.03 is DERIVED, not observed. `axis_coincidence_residual`
/// returns `[ûa·e1, ûa·e2, off·e1, off·e2]` in the ANCHOR's tangent frame; for
/// the anchor direction `+z` that frame is exactly `e1 = (0,−1,0)`,
/// `e2 = (1,0,0)`. With `off = oa − ob = (−0.03, −0.02, −0.005)` the four rows
/// are `[0, 0, 0.02, −0.03]`, so the max magnitude is the x-split, 0.03 m. The
/// z-split does not appear: an axis constrains only the two components
/// PERPENDICULAR to itself, and sliding along `+z` is not a coincidence error.
///
/// Against `RelateTolerance::kernel_default().assertion()` = 1e-5 m that is a
/// 3000× margin — the verdict is not sensitive to the tolerance's exact value.
#[test]
fn static_residuals_measure_the_gap_between_two_offset_axes() {
    let rel = relation(
        "concentric",
        vec![
            datum("bush", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis(VIOLATED_OFFSET, (0.0, 0.0, 1.0))),
        ],
        4,
    );

    let rows = static_relation_residuals(&rel);
    assert!(
        !rows.is_empty(),
        "concentric over two Axis operands has a residual model; an empty row \
         vector would mean UNVERIFIABLE, which is a different (and here wrong) \
         verdict from violated"
    );

    let max = rows.iter().fold(0.0_f64, |m, r| m.max(r.value.abs()));
    assert!(
        (max - 0.03).abs() < 1e-9,
        "the measured residual must be the 30 mm x-split of the fixture's \
         datums, 0.03 m; got {max} from rows {rows:?}. A measured 0.0 means the \
         relation was compared against ITSELF — the `pick_ab` degeneracy that \
         makes a zero-auto relate block a silent no-op."
    );
}

/// (b) The B2 shape: `concentric` over two BIT-IDENTICAL axes measures exactly
/// zero on every row.
///
/// This mirrors `dic_relate_static_ok.ri`, whose two structures are built from
/// the identical `translate(...)` expression — so `resolve_operands`, which keys
/// realized datums by `(structure, member)`, hands both operands bit-identical
/// f64s.
///
/// The exactness is asserted rather than an epsilon because it is derivable, and
/// derivable in two separate ways: the two POSITION rows are `off = oa − ob` with
/// `oa` and `ob` bitwise equal, so they are exactly `±0.0`; and for the `+z`
/// direction the anchor tangent frame is exactly `(0,−1,0)`/`(1,0,0)`, both of
/// which have a zero z-component, so the two TILT rows are exact zeros too.
///
/// The operative bound for the caller is of course the far looser assertion
/// tolerance (1e-5 m); this test pins the stronger true statement, so that a
/// future change which introduces float noise here surfaces as a question rather
/// than silently eating margin.
#[test]
fn static_residuals_are_exactly_zero_for_bit_identical_axes() {
    let colocated = axis(VIOLATED_OFFSET, (0.0, 0.0, 1.0));
    let rel = relation(
        "concentric",
        vec![
            datum("bush", colocated.clone()),
            datum("plate", colocated),
        ],
        4,
    );

    let rows = static_relation_residuals(&rel);
    assert!(
        !rows.is_empty(),
        "a satisfied relation must still MEASURE — an empty row vector means \
         unverifiable, not satisfied"
    );
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(
            r.value, 0.0,
            "row {i} of {rows:?} must be exactly zero for bit-identical operands"
        );
    }
}

/// (c) The anti-`pick_ab` test in the false-VIOLATION direction:
/// `perpendicular` over two genuinely perpendicular unit `Direction`s on
/// DISTINCT subs must measure ~0.
///
/// `perpendicular`'s residual is `dot(a, b)`, so the naive sentinel-unknown form
/// — which collapses `a` and `b` onto the same operand — returns `d·d` = **1.0**,
/// a confident violation of a correct model. The bound is 1e-12 rather than exact
/// only because the operands need not be axis-aligned in general; for these two
/// the dot is exactly 0.
#[test]
fn static_residuals_do_not_self_compare_perpendicular_directions() {
    let rel = relation(
        "perpendicular",
        vec![
            datum("m", dir(0.0, 0.0, 1.0)),
            datum("a", dir(1.0, 0.0, 0.0)),
        ],
        1,
    );

    let rows = static_relation_residuals(&rel);
    assert!(
        !rows.is_empty(),
        "perpendicular over two Directions has a residual model"
    );
    let max = rows.iter().fold(0.0_f64, |m, r| m.max(r.value.abs()));
    assert!(
        max <= 1e-12,
        "two genuinely perpendicular directions must measure as SATISFIED; got \
         {max} from rows {rows:?}. A measured 1.0 is the `pick_ab` self-compare \
         (`d·d`), i.e. a false violation on a correct model."
    );
}

/// (c′) The same false-violation direction for `antiparallel`, whose residual is
/// the unit difference `â − sign·b̂`. Self-comparing yields `â + â`, norm **2.0**.
#[test]
fn static_residuals_do_not_self_compare_antiparallel_directions() {
    let rel = relation(
        "antiparallel",
        vec![
            datum("m", dir(0.0, 0.0, 1.0)),
            datum("a", dir(0.0, 0.0, -1.0)),
        ],
        2,
    );

    let rows = static_relation_residuals(&rel);
    assert!(
        !rows.is_empty(),
        "antiparallel over two Directions has a residual model"
    );
    let max = rows.iter().fold(0.0_f64, |m, r| m.max(r.value.abs()));
    assert!(
        max <= 1e-12,
        "two genuinely antiparallel directions must measure as SATISFIED; got \
         {max} from rows {rows:?}. A measured 2.0 is the `pick_ab` self-compare."
    );
}

/// (d1) UNVERIFIABLE, source 1: an uncurated relation name contributes no
/// residual rows, and that must surface as an EMPTY vector rather than as a
/// zero-valued one.
///
/// `residual_dispatch`'s catch-all arm returns no rows for a name it does not
/// model. Collapsing that to `0.0` would report an unmodelled relation as
/// satisfied — the same class of silent failure the compile-time
/// `E_TANGENT_OPERANDS_UNSUPPORTED` gate exists to prevent one layer up.
#[test]
fn static_residuals_are_empty_for_an_uncurated_relation_name() {
    let rel = relation(
        "wibbly",
        vec![
            datum("bush", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis(VIOLATED_OFFSET, (0.0, 0.0, 1.0))),
        ],
        1,
    );

    assert!(
        static_relation_residuals(&rel).is_empty(),
        "an uncurated relation name has no residual model, so the row vector must \
         be EMPTY (⇒ unverifiable). Any zero-valued row would read as satisfied."
    );
}

/// (d2) UNVERIFIABLE, source 2: an operand that did not realize to a datum
/// (`Value::Undef`) leaves the relation with fewer than two datums to compare.
///
/// This case is sharper than it looks, and is why the implementation cannot
/// simply nominate a witness and call through. With ONE datum operand surviving,
/// `pick_ab` resolves `a` and `b` to that same lone datum and `concentric`
/// returns four exact zeros — a *fully confident* "satisfied" verdict derived
/// from a relation half of whose inputs are missing. Requiring two datum operands
/// is what makes this honest.
#[test]
fn static_residuals_are_empty_when_an_operand_did_not_realize() {
    let rel = relation(
        "concentric",
        vec![
            datum("bush", Value::Undef),
            datum("plate", axis(VIOLATED_OFFSET, (0.0, 0.0, 1.0))),
        ],
        4,
    );

    assert!(
        static_relation_residuals(&rel).is_empty(),
        "a relation with only ONE realized datum cannot be verified, so the row \
         vector must be EMPTY. Comparing the lone datum against itself yields \
         four exact zeros — a confident false green built from missing input."
    );
}

/// (e) Both operands on ONE sub (`concentric(a.x, a.y)`) still measures the two
/// DISTINCT datums rather than self-comparing.
///
/// This is the case where nominating "the first datum operand's sub" as the
/// witness makes EVERY operand moving, so `pick_ab` finds no non-moving operand
/// and falls back to `datums.last()`. That fallback is correct here — last is a
/// genuinely different operand from first — which is why the guard is on the
/// datum COUNT (≥ 2) and not on the subs being distinct.
#[test]
fn static_residuals_compare_two_datums_of_the_same_sub() {
    let rel = relation(
        "concentric",
        vec![
            datum("a", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("a", axis((0.030, 0.0, 0.0), (0.0, 0.0, 1.0))),
        ],
        4,
    );

    let rows = static_relation_residuals(&rel);
    let max = rows.iter().fold(0.0_f64, |m, r| m.max(r.value.abs()));
    assert!(
        (max - 0.03).abs() < 1e-9,
        "two datums of the SAME sub must still be compared against each other; \
         got {max} from rows {rows:?} (0.0 means the first datum was compared \
         against itself)"
    );
}

// ── Residual-row units (the fabricated-length guard) ─────────────────────────
//
// A residual row vector is NOT dimensionally homogeneous. Before every row
// carried its `ResidualUnit`, the static-verification renderer took `max |row|`
// and printed it through `fmt_mm`, so any relation whose dominant row is
// angular/dimensionless reported a length that does not exist. These pin the
// UNIT of each row at its source, which is what the renderer now dispatches on.

/// `parallel` measures a unit-vector difference: three PURE NUMBERS. A renderer
/// that reads these as metres states a fabricated length.
#[test]
fn parallel_residual_rows_are_dimensionless() {
    // 90° apart — maximally violated for a parallel demand.
    let rel = relation(
        "parallel",
        vec![
            datum("m", dir(1.0, 0.0, 0.0)),
            datum("a", dir(0.0, 1.0, 0.0)),
        ],
        2,
    );

    let rows = static_relation_residuals(&rel);
    assert!(!rows.is_empty(), "parallel over two Directions has a residual model");
    assert!(
        rows.iter().all(|r| r.unit == ResidualUnit::Dimensionless),
        "every row of a direction-alignment residual is a pure number; got {rows:?}"
    );
}

/// `perpendicular` measures a dot product — likewise a pure number, and the
/// violated magnitude here (1.0 for two parallel directions) is precisely the
/// value that used to render as "off by 1000 mm".
#[test]
fn perpendicular_residual_row_is_dimensionless() {
    let rel = relation(
        "perpendicular",
        vec![
            datum("m", dir(1.0, 0.0, 0.0)),
            datum("a", dir(1.0, 0.0, 0.0)),
        ],
        1,
    );

    let rows = static_relation_residuals(&rel);
    assert_eq!(rows.len(), 1, "perpendicular contributes one dot-product row");
    assert_eq!(
        rows[0].unit,
        ResidualUnit::Dimensionless,
        "a dot product has no length reading; got {rows:?}"
    );
}

/// `angle` measures `dot(â, b̂) − cos θ` — a cosine difference, not a length.
#[test]
fn angle_residual_row_is_dimensionless() {
    let mut rel = relation(
        "angle",
        vec![
            datum("m", dir(1.0, 0.0, 0.0)),
            datum("a", dir(0.0, 1.0, 0.0)),
        ],
        1,
    );
    // The demanded angle rides as a trailing scalar (non-datum) operand.
    rel.operands.push(Operand {
        sub: None,
        datum: Value::Real(0.0),
    });

    let rows = static_relation_residuals(&rel);
    assert_eq!(rows.len(), 1, "angle contributes one cosine-difference row");
    assert_eq!(
        rows[0].unit,
        ResidualUnit::Dimensionless,
        "a cosine difference has no length reading; got {rows:?}"
    );
}

/// `concentric` is the MIXED case that makes a per-relation-family split
/// insufficient: two dimensionless tilt rows followed by two metre rows, in that
/// order. A pair of axes that are CO-LOCATED but TILTED therefore has its
/// dominant row in the dimensionless block — the exact shape that used to print a
/// fabricated millimetre figure.
#[test]
fn concentric_residual_rows_are_tilt_then_length() {
    let rel = relation(
        "concentric",
        vec![
            // Same origin, 45° apart: the position rows are exactly zero and the
            // tilt rows dominate.
            datum("bush", axis((0.0, 0.0, 0.0), (1.0, 0.0, 1.0))),
            datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
        ],
        4,
    );

    let rows = static_relation_residuals(&rel);
    let units: Vec<ResidualUnit> = rows.iter().map(|r| r.unit).collect();
    assert_eq!(
        units,
        vec![
            ResidualUnit::Dimensionless,
            ResidualUnit::Dimensionless,
            ResidualUnit::Length,
            ResidualUnit::Length,
        ],
        "axis-coincidence is 2 tilt rows then 2 position rows; got {rows:?}"
    );

    let dominant = rows
        .iter()
        .copied()
        .reduce(|m, r| if r.value.abs() > m.value.abs() { r } else { m })
        .expect("non-empty");
    assert_eq!(
        dominant.unit,
        ResidualUnit::Dimensionless,
        "co-located but tilted axes are violated in TILT, so the dominant row \
         carries no length reading; got {rows:?}"
    );
}

/// `fasten` (coincident over Frame) is the other mixed form: three metre origin
/// rows then three RADIAN orientation rows.
#[test]
fn frame_coincidence_rows_are_length_then_angle() {
    let identity_q = (1.0, 0.0, 0.0, 0.0);
    let rel = relation(
        "fasten",
        vec![
            datum("m", frame((0.010, 0.0, 0.0), identity_q)),
            datum("a", frame((0.0, 0.0, 0.0), identity_q)),
        ],
        6,
    );

    let rows = static_relation_residuals(&rel);
    let units: Vec<ResidualUnit> = rows.iter().map(|r| r.unit).collect();
    assert_eq!(
        units,
        vec![
            ResidualUnit::Length,
            ResidualUnit::Length,
            ResidualUnit::Length,
            ResidualUnit::Angle,
            ResidualUnit::Angle,
            ResidualUnit::Angle,
        ],
        "frame-coincidence is 3 origin-delta rows then 3 orientation-delta rows; \
         got {rows:?}"
    );
}

// ── Per-unit assertion rungs + scale-free dimensionless rows (DIC α amendment) ──
//
// A residual row vector is not dimensionally homogeneous, so ONE rung cannot judge
// all of it. The zero-auto static verifier compares each row against the rung for
// its own unit; these pin the rungs’ derivation and the one residual form whose
// row was not scale-free until now.

/// The three assertion rungs derive from the SAME base length, so an edit to the
/// hierarchy moves all three together rather than leaving two hand-picked epsilons
/// behind.
///
/// The angular rung is the angle that displaces a feature at the documented 1 m
/// reference radius by exactly the length rung; the dimensionless rung is its sine,
/// because every dimensionless form here measures the sine of a misalignment
/// between two unit directions. At the kernel default the three therefore coincide
/// to within float noise — which is the point: the numbers agreeing is a
/// CONSEQUENCE of the derivation, not the licence to compare radians against metres
/// that the single-rung code was taking.
#[test]
fn assertion_rungs_are_derived_per_unit_from_one_base_length() {
    let tol = RelateTolerance::kernel_default();

    assert_eq!(
        tol.assertion_angle(),
        tol.assertion() / 1.0,
        "the angular rung is the length rung over the 1 m reference radius"
    );
    assert_eq!(
        tol.assertion_dimensionless(),
        tol.assertion_angle().sin(),
        "the dimensionless rung is the sine of the angular one"
    );
    assert!(
        tol.assertion_dimensionless() < tol.assertion_angle(),
        "sin θ < θ for θ > 0, so the dimensionless rung is never the looser of the two"
    );
    assert!(
        tol.kernel_local() <= tol.solver_convergence() && tol.solver_convergence() <= tol.assertion(),
        "the length hierarchy is unchanged by the per-unit rungs"
    );
}

/// A geometrically EXACT `perpendicular` measures zero however long its operands’
/// direction vectors are.
///
/// The residual is `dot(a, b)`, which is the sine of the misalignment only for UNIT
/// operands — and `dir_of` reads whatever direction vector realization produced.
/// Unnormalized, the pair below reads `10 × 10 = 100`: six orders of magnitude past
/// any assertion rung, on geometry that is exactly right. The solve path never
/// noticed (its zero set is magnitude-invariant), but the static verifier compares
/// the row against a fixed rung and fails the BUILD, so the scale sensitivity had
/// to go. `angle` already normalized for the same reason.
#[test]
fn perpendicular_residual_is_scale_free_in_its_operands() {
    let exact = |da: (f64, f64, f64), db: (f64, f64, f64)| {
        let rel = relation(
            "perpendicular",
            vec![datum("m", vec3(da.0, da.1, da.2)), datum("a", vec3(db.0, db.1, db.2))],
            1,
        );
        static_relation_residuals(&rel)
    };

    let unit_rows = exact((1.0, 0.0, 0.0), (0.0, 0.0, 1.0));
    let scaled_rows = exact((10.0, 0.0, 0.0), (0.0, 0.0, 10.0));
    assert_eq!(
        unit_rows.len(),
        1,
        "perpendicular contributes exactly one dimensionless row; got {unit_rows:?}"
    );
    assert_eq!(
        scaled_rows, unit_rows,
        "scaling either operand must not move the residual — an exact \
         perpendicular reads zero at every magnitude; got {scaled_rows:?}"
    );

    // And a genuinely misaligned pair still reads its SINE, not a scaled one: 30°
    // off perpendicular is sin 30° = 0.5 whatever the operand lengths.
    let misaligned = exact((10.0, 0.0, 0.0), (5.0, 0.0, 8.660_254_037_844_387));
    assert!(
        (misaligned[0].value - 0.5).abs() < 1e-12,
        "a 30° misalignment reads sin 30° = 0.5 independently of operand scale; \
         got {misaligned:?}"
    );
}

/// `comparable_datum_operands` is the arity `static_relation_residuals` guards on,
/// exposed so a caller can say WHICH source of an empty row vector it hit.
///
/// The two are pinned together here because the zero-auto verifier reports a
/// DIFFERENT reason for each (“only one operand to compare” vs. “no residual model
/// for these operand kinds”), and a reason that disagrees with the guard that
/// actually fired is a confidently wrong “why” on a diagnostic whose entire value
/// is its why.
#[test]
fn comparable_datum_operands_is_the_arity_the_residual_guard_applies() {
    let identity_q = (1.0, 0.0, 0.0, 0.0);
    let lone = relation(
        "fasten",
        vec![datum("m", frame((0.010, 0.0, 0.0), identity_q))],
        6,
    );
    assert_eq!(
        comparable_datum_operands(&lone), 1,
        "one Frame operand — the `ground(sub)` desugar’s shape once `self.frame` \
         has dropped out"
    );
    assert!(
        static_relation_residuals(&lone).is_empty(),
        "below two datum operands there is no pair to compare, so no rows"
    );

    let pair = relation(
        "fasten",
        vec![
            datum("m", frame((0.010, 0.0, 0.0), identity_q)),
            datum("a", frame((0.0, 0.0, 0.0), identity_q)),
        ],
        6,
    );
    assert_eq!(comparable_datum_operands(&pair), 2);
    assert!(
        !static_relation_residuals(&pair).is_empty(),
        "`fasten` over two Frames IS modelled — the empty vector above is an arity \
         verdict, not a missing residual model"
    );
}
