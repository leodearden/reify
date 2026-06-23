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
    FrameUnknown, Operand, Pose, RelateTolerance, RelationInstance, max_relation_residual,
    partition_driving_set, pose_from_frame, solve_frame,
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
