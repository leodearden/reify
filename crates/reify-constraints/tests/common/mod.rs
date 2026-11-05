//! Synthetic realized-datum and relation builders shared by the kernel-free
//! relate-solve test binaries (`relate_solve_tests`, `relate_frame_outcome_tests`).

use reify_constraints::relate_solve::{
    FrameUnknown, Operand, Pose, RelationInstance, solve_frame_outcome, solve_scaling,
};
use reify_constraints::scaling::{RelateTolerance, SolveScaling};
use reify_constraints::trust_region::SolveBudget;
use reify_ir::{SolveResult, Value};

pub fn point3(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![Value::length(x), Value::length(y), Value::length(z)])
}

pub fn vec3(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

/// A `Value::Axis` from an origin point (metres) + a direction vector.
pub fn axis(o: (f64, f64, f64), d: (f64, f64, f64)) -> Value {
    Value::Axis {
        origin: Box::new(point3(o.0, o.1, o.2)),
        direction: Box::new(vec3(d.0, d.1, d.2)),
    }
}

/// A `Value::Plane` from an origin point (metres) + a unit normal vector.
pub fn plane(o: (f64, f64, f64), n: (f64, f64, f64)) -> Value {
    Value::Plane {
        origin: Box::new(point3(o.0, o.1, o.2)),
        normal: Box::new(vec3(n.0, n.1, n.2)),
    }
}

/// A dimensionless `Value::Direction`.
pub fn dir(x: f64, y: f64, z: f64) -> Value {
    Value::Direction { x, y, z }
}

/// A datum operand belonging to sub `sub` (so the partition transforms it by the
/// witness Frame iff `sub` is the auto unknown).
pub fn datum(sub: &str, value: Value) -> Operand {
    Operand {
        sub: Some(sub.to_string()),
        datum: value,
    }
}

/// A relation instance: name, ordered operands, and the γ-published nominal ΔDOF
/// (codimension) the partition cross-checks its measured per-relation rank against.
pub fn relation(name: &str, operands: Vec<Operand>, nominal_delta_dof: u32) -> RelationInstance {
    RelationInstance {
        name: name.to_string(),
        operands,
        nominal_delta_dof: Some(nominal_delta_dof),
    }
}

/// The auto Frame unknown for sub `"bolt"` (the §1 moving sub).
pub fn bolt_unknown() -> FrameUnknown {
    FrameUnknown {
        sub: "bolt".to_string(),
        free: false,
    }
}

/// A Frame unknown for `sub` with the given `free` flag (`auto` vs `auto(free)`).
pub fn unknown(sub: &str, free: bool) -> FrameUnknown {
    FrameUnknown {
        sub: sub.to_string(),
        free,
    }
}

/// A fully-determined (rank-6) synthetic scenario for the moving sub `"m"`:
/// `coincident(point,point)` pins 3 translation DOF; `parallel(+z,+z)` pins 2 tilt
/// DOF; `parallel(+x,+x)` pins the remaining spin — all 6 Frame DOF determined, all
/// already satisfied at the identity witness.
pub fn fully_determined_relations() -> Vec<RelationInstance> {
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
            vec![
                datum("m", dir(0.0, 0.0, 1.0)),
                datum("a", dir(0.0, 0.0, 1.0)),
            ],
            2,
        ),
        relation(
            "parallel",
            vec![
                datum("m", dir(1.0, 0.0, 0.0)),
                datum("a", dir(1.0, 0.0, 0.0)),
            ],
            2,
        ),
    ]
}

/// B5 — a single `concentric(axis,axis)` leaving a residual DOF. The moving bolt's
/// LOCAL shank axis sits at the origin; the fixed plate hole axis is offset by
/// `(0.10, 0.20)` in-plane, both `+z`. concentric pins {2 tilt, 2 perp-position} = 4
/// DOF; the 2 residual DOF are slide-along-axis (`tz`) + spin-about-axis (`rot_z`).
pub fn b5_relations() -> Vec<RelationInstance> {
    vec![relation(
        "concentric",
        vec![
            datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis((0.10, 0.20, 0.0), (0.0, 0.0, 1.0))),
        ],
        4,
    )]
}

/// The kernel-default scaling a scope partition or solve of `relations` from
/// `witness` uses. Its rank cutoff acts on normalized scaled rows; an unconstrained
/// DOF shows up as an exactly-zero column (equal residuals up to float noise), so
/// the cutoff cleanly separates rank-5 from rank-6.
pub fn scaling_for(
    relations: &[RelationInstance],
    unknown: &FrameUnknown,
    witness: &Pose,
) -> SolveScaling {
    solve_scaling(
        relations,
        unknown,
        witness,
        RelateTolerance::kernel_default(),
    )
}

/// The Frame solve within the default budget, read through the
/// [`SolveResult`] contract
/// ([`FrameSolveOutcome::into_solve_result`]).
pub fn solve_frame_result(
    driving: &[RelationInstance],
    unknown: &FrameUnknown,
    seed: &Pose,
    scaling: &SolveScaling,
) -> SolveResult {
    solve_frame_outcome(driving, unknown, seed, scaling, SolveBudget::default())
        .into_solve_result(unknown)
}
