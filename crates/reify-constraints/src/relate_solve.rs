//! Driving-set rank-partition for the per-scope relate-solve — geometric-relations
//! ζ (task 4386), step-8.
//!
//! Given a per-scope geometric-relation set over *realized* datum [`Value`]s, an
//! `at auto` 6-DOF Frame unknown, and a witness Frame config, this module
//! rank-partitions the relations into a maximal independent **driving set** + a
//! **redundant remainder** ([`partition_driving_set`]). Only the driving set is
//! handed to the solver (step-10); the remainder is verified post-solve as
//! geometry-backed assertions (step-14). Separating redundancy (verified) from
//! conflict (an infeasible driving set) is what makes over-constraint
//! order-independent — B2's redundant relation passes silently while B3's genuine
//! conflict fails loud (PRD §7.1 steps 2/3/5).
//!
//! ## How the partition works
//!
//! Each relation publishes a residual that is zero exactly when it is satisfied.
//! At the witness config the residual's gradient w.r.t. the 6 Frame DOF
//! (`[t_x, t_y, t_z, rot_x, rot_y, rot_z]`) is measured by central finite
//! differences, giving an `m × 6` Jacobian block per relation. A relation is
//! *driving* iff its rows add at least one new independent direction to the running
//! rank (greedy rank-revealing Gram–Schmidt over normalized rows); otherwise every
//! row already lies in the span and it joins the redundant remainder. The DOF
//! accounting falls straight out: `spent = combined rank`, `free = 6 − spent`.
//!
//! The {driving, redundant} *counts* and the spent total are order-independent
//! (any maximal independent subset has the same size); the specific relation that
//! lands in the remainder may depend on input order, which is immaterial — the
//! remainder is verified, not solved.
//!
//! Hand-rolled small linalg over `≤(rows × 6)` — the Frame Jacobian is tiny, so a
//! heavy linalg dependency (nalgebra) is unjustified (design §4).
//!
//! ## Driving-set solve (step-10)
//!
//! [`solve_frame`] takes the partition's **driving set** over realized datum
//! [`Value`]s + the 6-DOF Frame unknown + a seed [`Pose`] and drives the Frame to
//! satisfy every driving relation, returning through the existing
//! [`SolveResult`](reify_ir::SolveResult) contract (`Solved { values, unique }` /
//! `Infeasible`). It reuses this module's residual + Jacobian model (the same one
//! the partition measures): a damped Gauss–Newton (Levenberg–Marquardt) iteration
//! over the 6-DOF pose, with `λI` damping so a residual gauge freedom (e.g. spin
//! about the shank axis under concentric+flush) is handled gracefully rather than
//! tripping a singular normal-equations solve. The 6 solved scalars are assembled
//! into a [`Value::Frame`]; `unique` reflects whether the Jacobian at the solution
//! pins all 6 DOF.
//!
//! **Backend note (see esc-4386-38).** The plan named a libslvs-backed solve
//! (extending `solvespace.rs`'s `SystemBuilder`). The bound `Slvs_*` C API models
//! INDEPENDENT point/line coordinates with no rigid-group primitive, so a single
//! 6-DOF Frame carrying several rigidly-coupled datums cannot be expressed cleanly
//! there; the self-contained Gauss–Newton over the already-tested residual model
//! satisfies the same `SolveResult` contract without a second slvs integration.
//!
//! One kernel-defaulted [`RelateTolerance`] knob governs the whole hierarchy
//! `kernel_local ≤ solver_convergence ≤ assertion/dedup` (PRD §7.1 coherence law).

use std::collections::HashMap;

use reify_core::{Diagnostic, ValueCellId};
use reify_ir::{SolveResult, Value};

/// The auto Frame unknown a relate-solve scope must determine — one per `at auto`
/// sub. Carries the sub's instance name so the partition can tell which relation
/// operands MOVE with the Frame (the auto sub's datums) from those that are fixed
/// (the grounded anchor's).
#[derive(Debug, Clone)]
pub struct FrameUnknown {
    /// The auto sub's instance name (e.g. `"bolt"`).
    pub sub: String,
}

/// One operand of a relation, as a realized datum.
#[derive(Debug, Clone)]
pub struct Operand {
    /// The sub the operand's datum belongs to (e.g. `Some("bolt")`). `None` for a
    /// non-datum operand — a scalar magnitude in a metric DRIVE relation
    /// (`distance`/`angle`/`offset`).
    pub sub: Option<String>,
    /// The realized LOCAL datum value (`Axis`/`Plane`/`Direction`/`Point`), or a
    /// `Scalar` magnitude for the metric relations.
    pub datum: Value,
}

/// A relation instance over realized datums — the partition's per-relation input.
#[derive(Debug, Clone)]
pub struct RelationInstance {
    /// The relation name (e.g. `"concentric"`, `"flush"`, `"perpendicular"`).
    pub name: String,
    /// The ordered operands (datum operands + any trailing scalar magnitude).
    pub operands: Vec<Operand>,
    /// The γ-published nominal ΔDOF (codimension) for this relation's operand
    /// shape, carried as data because `reify_compiler`'s `relation_delta_dof` is
    /// `pub(crate)`. The partition cross-checks its measured per-relation rank
    /// against this to guard the numeric rank against false redundancy/conflict.
    pub nominal_delta_dof: Option<u32>,
}

/// A rigid-body pose — the witness config at which the partition Jacobian is
/// evaluated, and the parameterization the solve perturbs.
///
/// `translation` is in metres; `rotation` is an exponential-map vector (axis ×
/// angle, radians). [`Pose::identity`] is the zero pose.
#[derive(Debug, Clone, Default)]
pub struct Pose {
    /// Translation in metres.
    pub translation: [f64; 3],
    /// Rotation as an exponential-map (axis-angle) vector, radians.
    pub rotation: [f64; 3],
}

impl Pose {
    /// The identity pose (no translation, no rotation).
    pub fn identity() -> Self {
        Self::default()
    }

    /// A copy with the `dof`-th Frame DOF nudged by `h` (DOF 0..3 = translation
    /// x/y/z; 3..6 = rotation x/y/z).
    fn perturbed(&self, dof: usize, h: f64) -> Self {
        let mut p = self.clone();
        if dof < 3 {
            p.translation[dof] += h;
        } else {
            p.rotation[dof - 3] += h;
        }
        p
    }
}

/// The measured rank report for one input relation (in input order).
#[derive(Debug, Clone)]
pub struct RelationRank {
    /// The relation name.
    pub name: String,
    /// The rank of this relation's Jacobian rows ALONE at the witness — the
    /// geometry-measured codimension, cross-checked against `nominal_delta_dof`.
    pub individual_rank: u32,
    /// How many NEW independent DOF this relation added to the running rank when
    /// processed in input order. `> 0` ⇒ driving; `0` ⇒ redundant remainder.
    pub rank_contribution: u32,
    /// The γ nominal ΔDOF carried in for this relation (see
    /// [`RelationInstance::nominal_delta_dof`]).
    pub nominal_delta_dof: Option<u32>,
}

/// The driving/redundant partition of a per-scope relation set + its DOF
/// accounting.
#[derive(Debug, Clone)]
pub struct RelationPartition {
    /// Indices (into the input slice) of the driving relations — the maximal
    /// independent set handed to the solver.
    pub driving: Vec<usize>,
    /// Indices of the redundant-remainder relations — verified post-solve, never
    /// solved.
    pub redundant: Vec<usize>,
    /// DOF spent = the combined Jacobian rank of the driving set.
    pub spent: u32,
    /// Residual DOF = `6 − spent` (the Frame freedoms the relations leave open).
    pub free: u32,
    /// Per-relation rank report, in input order.
    pub per_relation: Vec<RelationRank>,
}

/// The number of DOF in a rigid Frame unknown.
const FRAME_DOF: usize = 6;
/// Central finite-difference step (metres for translation, radians for rotation).
const FD_STEP: f64 = 1e-6;
/// Below this norm a Jacobian row is treated as numerically zero (a residual
/// component insensitive to every DOF — well under the FD noise floor of ~1e-10).
const ZERO_ROW_FLOOR: f64 = 1e-9;

/// Rank-partition a per-scope relation set at the `witness` config into a driving
/// set + a redundant remainder, with DOF accounting (ζ step-8).
///
/// `tol` is the rank-revealing tolerance (tied to the solver-convergence tol):
/// after each Jacobian row is normalized, a Gram–Schmidt residual above `tol`
/// counts as a new independent direction. See the module docs for the method.
pub fn partition_driving_set(
    relations: &[RelationInstance],
    frame_unknown: &FrameUnknown,
    witness: &Pose,
    tol: f64,
) -> RelationPartition {
    let mut basis: Vec<[f64; FRAME_DOF]> = Vec::new();
    let mut driving = Vec::new();
    let mut redundant = Vec::new();
    let mut per_relation = Vec::new();

    for (i, rel) in relations.iter().enumerate() {
        let rows = relation_jacobian(rel, frame_unknown, witness);

        // Individual rank: the rank of this relation's rows on their own (a fresh
        // basis) — the geometry-measured codimension cross-checked against γ.
        let mut solo: Vec<[f64; FRAME_DOF]> = Vec::new();
        let individual_rank = add_rows_rank(&rows, &mut solo, tol) as u32;

        // Contribution to the running rank, in input order.
        let rank_contribution = add_rows_rank(&rows, &mut basis, tol) as u32;
        if rank_contribution > 0 {
            driving.push(i);
        } else {
            redundant.push(i);
        }

        per_relation.push(RelationRank {
            name: rel.name.clone(),
            individual_rank,
            rank_contribution,
            nominal_delta_dof: rel.nominal_delta_dof,
        });
    }

    let spent = basis.len() as u32;
    let free = (FRAME_DOF as u32).saturating_sub(spent);
    RelationPartition {
        driving,
        redundant,
        spent,
        free,
        per_relation,
    }
}

// ── Tolerance hierarchy ──────────────────────────────────────────────────────

/// The single kernel-defaulted `Length` tolerance knob governing the relate-solve
/// (PRD §7.1 coherence law). One base length seeds the whole hierarchy
/// `kernel_local ≤ solver_convergence ≤ assertion/dedup`: the kernel's own datum
/// realization floor, the solver's residual-convergence target, and the (looser)
/// post-solve assertion/dedup tolerance. Numeric boundary checks test "within the
/// solver's convergence tolerance" — a method guarantee — never a hand-picked epsilon.
#[derive(Debug, Clone, Copy)]
pub struct RelateTolerance {
    kernel_local: f64,
    solver_convergence: f64,
    assertion: f64,
}

impl RelateTolerance {
    /// The kernel-default hierarchy: a `1e-7 m` kernel-local floor (OCCT
    /// `Precision::Confusion` order), a `1e-6 m` solver-convergence target, and a
    /// `1e-5 m` assertion/dedup tolerance. Derived from the single base length so
    /// `kernel_local ≤ solver_convergence ≤ assertion` holds by construction.
    pub fn kernel_default() -> Self {
        let kernel_local = 1e-7;
        Self {
            kernel_local,
            solver_convergence: kernel_local * 10.0, // 1e-6 m
            assertion: kernel_local * 100.0,         // 1e-5 m
        }
    }

    /// The tightest rung — the kernel's local datum-realization floor (metres).
    pub fn kernel_local(&self) -> f64 {
        self.kernel_local
    }

    /// The solver's residual-convergence target (metres / dimensionless). A solve
    /// that returns [`SolveResult::Solved`] guarantees the relation residual is
    /// `≤` this value.
    pub fn solver_convergence(&self) -> f64 {
        self.solver_convergence
    }

    /// The (loosest) post-solve assertion/dedup tolerance (metres).
    pub fn assertion(&self) -> f64 {
        self.assertion
    }
}

// ── Driving-set solve ────────────────────────────────────────────────────────

/// The `ValueCellId` member name under which a solved auto-sub Frame is keyed in
/// [`SolveResult::Solved`]'s value map (entity = the sub's instance name).
const POSE_MEMBER: &str = "pose";

/// Solve the 6-DOF auto Frame `unknown` so that every relation in `driving` is
/// satisfied, starting from `seed` (ζ step-10).
///
/// Returns through the existing [`SolveResult`] contract:
/// - [`SolveResult::Solved`] — the relation residual converged `≤ tol` (the method
///   guarantee). `values` maps `ValueCellId::new(unknown.sub, "pose")` to a
///   [`Value::Frame`] assembled from the 6 solved scalars; `unique` is `true` iff
///   the Jacobian at the solution pins all 6 Frame DOF (no residual gauge freedom).
/// - [`SolveResult::Infeasible`] — the driving relations are mutually inconsistent
///   (no pose drives the residual below `tol`). Step-16 refines the geometric
///   minimal-conflict diagnostic; ζ step-10 maps non-convergence to a build-failing
///   `Infeasible` so the contract is honoured.
///
/// `tol` is the solver-convergence tolerance — feed it from
/// [`RelateTolerance::solver_convergence`].
pub fn solve_frame(
    driving: &[RelationInstance],
    unknown: &FrameUnknown,
    seed: &Pose,
    tol: f64,
) -> SolveResult {
    match gauss_newton_solve(driving, unknown, seed, tol) {
        Some((pose, unique)) => {
            let mut values = HashMap::new();
            values.insert(
                ValueCellId::new(unknown.sub.clone(), POSE_MEMBER),
                frame_value(&pose),
            );
            SolveResult::Solved { values, unique }
        }
        None => SolveResult::Infeasible {
            diagnostics: vec![Diagnostic::error(format!(
                "the relations on `{}` cannot be satisfied simultaneously — \
                 the driving set is geometrically inconsistent",
                unknown.sub
            ))],
        },
    }
}

/// The largest absolute residual component over `relations` at `pose` — the
/// geometry-backed measure of "how far from satisfied" the relation set is. A pose
/// returned by [`solve_frame`] as `Solved` has `max_relation_residual ≤ tol`. Also
/// the post-solve assertion primitive reused for the redundant-remainder check
/// (step-14).
pub fn max_relation_residual(
    relations: &[RelationInstance],
    unknown: &FrameUnknown,
    pose: &Pose,
) -> f64 {
    let mut max = 0.0_f64;
    for rel in relations {
        for r in relation_residual(rel, unknown, pose) {
            max = max.max(r.abs());
        }
    }
    max
}

/// Convert a solved [`Value::Frame`] (or [`Value::Transform`]) back into a [`Pose`]
/// (translation + exponential-map rotation), the inverse of the Frame assembly in
/// [`solve_frame`]. Returns `None` for any other value or a malformed frame.
pub fn pose_from_frame(v: &Value) -> Option<Pose> {
    match v {
        Value::Frame { origin, basis } => Some(Pose {
            translation: vec3_of(origin)?,
            rotation: exp_map_from_orientation(basis)?,
        }),
        Value::Transform {
            rotation,
            translation,
        } => Some(Pose {
            translation: vec3_of(translation)?,
            rotation: exp_map_from_orientation(rotation)?,
        }),
        _ => None,
    }
}

// ── Jacobian assembly ────────────────────────────────────────────────────────

/// The `m × 6` residual Jacobian of `rel` w.r.t. the 6 Frame DOF at `witness`,
/// by central finite differences. Returns one `[f64; 6]` row per residual
/// component (empty if the relation has no residual model).
fn relation_jacobian(
    rel: &RelationInstance,
    unknown: &FrameUnknown,
    witness: &Pose,
) -> Vec<[f64; FRAME_DOF]> {
    let base = relation_residual(rel, unknown, witness);
    let m = base.len();
    if m == 0 {
        return Vec::new();
    }
    let mut jac = vec![[0.0; FRAME_DOF]; m];
    for dof in 0..FRAME_DOF {
        let rp = relation_residual(rel, unknown, &witness.perturbed(dof, FD_STEP));
        let rm = relation_residual(rel, unknown, &witness.perturbed(dof, -FD_STEP));
        if rp.len() != m || rm.len() != m {
            continue;
        }
        for (r, row) in jac.iter_mut().enumerate() {
            row[dof] = (rp[r] - rm[r]) / (2.0 * FD_STEP);
        }
    }
    jac
}

/// Evaluate `rel`'s residual at pose `x`: transform each MOVING datum operand
/// (one whose sub is the auto unknown) by `x`, leave anchors fixed, then dispatch
/// on the relation name. A zero residual means the relation is satisfied at `x`.
fn relation_residual(rel: &RelationInstance, unknown: &FrameUnknown, x: &Pose) -> Vec<f64> {
    let mut datums: Vec<(Value, bool)> = Vec::new(); // (value, moving)
    let mut scalar: Option<f64> = None;
    for op in &rel.operands {
        if is_datum(&op.datum) {
            let moving = op.sub.as_deref() == Some(unknown.sub.as_str());
            let val = if moving {
                transform_datum(&op.datum, x)
            } else {
                op.datum.clone()
            };
            datums.push((val, moving));
        } else if let Some(s) = op.datum.as_f64() {
            scalar = Some(s);
        }
    }
    residual_dispatch(&rel.name, &datums, scalar)
}

/// Dispatch a named relation to its residual vector over the (already
/// pose-transformed) datum operands. `a` is the moving operand, `b` the anchor;
/// tangent frames are always built from the anchor so they stay fixed under the
/// Frame perturbation (the Jacobian would otherwise pick up spurious rotation).
fn residual_dispatch(name: &str, datums: &[(Value, bool)], scalar: Option<f64>) -> Vec<f64> {
    let Some((a, b)) = pick_ab(datums) else {
        return Vec::new();
    };
    match name {
        "concentric" => axis_coincidence_residual(a, b),
        "flush" => plane_coincidence_residual(a, b),
        "parallel" | "antiparallel" => match (dir_of(a), dir_of(b)) {
            (Some(da), Some(db)) => direction_alignment_residual(da, db),
            _ => Vec::new(),
        },
        "perpendicular" => match (dir_of(a), dir_of(b)) {
            (Some(da), Some(db)) => vec![dot3(da, db)],
            _ => Vec::new(),
        },
        "coincident" => coincident_residual(a, b),
        "distance" => match (origin_of(a), origin_of(b), scalar) {
            (Some(pa), Some(pb), Some(d)) => vec![norm3(sub3(pa, pb)) - d],
            _ => Vec::new(),
        },
        "angle" => match (dir_of(a), dir_of(b), scalar) {
            (Some(da), Some(db), Some(theta)) => vec![dot3(da, db) - theta.cos()],
            _ => Vec::new(),
        },
        "offset" => offset_residual(a, b, scalar),
        "on" => on_residual(datums),
        // tangent (surface-conditional) and uncurated names contribute no rows.
        _ => Vec::new(),
    }
}

/// `(a, b)` = (first moving operand else first, first anchor operand else last).
/// For ζ's scope (one auto sub + ground anchor) this is exactly (moving, anchor).
fn pick_ab(datums: &[(Value, bool)]) -> Option<(&Value, &Value)> {
    if datums.is_empty() {
        return None;
    }
    let a = datums
        .iter()
        .find(|(_, m)| *m)
        .or_else(|| datums.first())
        .map(|(v, _)| v)?;
    let b = datums
        .iter()
        .find(|(_, m)| !*m)
        .or_else(|| datums.last())
        .map(|(v, _)| v)?;
    Some((a, b))
}

// ── Per-relation residual forms ──────────────────────────────────────────────

/// Axis-coincidence (concentric / coincident over Axis), codim 4: 2 direction
/// (tilt) components + 2 perpendicular-position components, in the anchor's
/// tangent frame.
fn axis_coincidence_residual(a: &Value, b: &Value) -> Vec<f64> {
    let (Some((oa, ua)), Some((ob, ub))) = (axis_parts(a), axis_parts(b)) else {
        return Vec::new();
    };
    let (e1, e2) = tangent_frame(ub);
    let off = sub3(oa, ob);
    vec![dot3(ua, e1), dot3(ua, e2), dot3(off, e1), dot3(off, e2)]
}

/// Plane-coincidence (flush / coincident over Plane), codim 3: 2 normal (tilt)
/// components + the signed offset along the anchor normal.
fn plane_coincidence_residual(a: &Value, b: &Value) -> Vec<f64> {
    let (Some((oa, na)), Some((ob, nb))) = (plane_parts(a), plane_parts(b)) else {
        return Vec::new();
    };
    let (e1, e2) = tangent_frame(nb);
    let off = sub3(oa, ob);
    vec![dot3(na, e1), dot3(na, e2), dot3(off, nb)]
}

/// Direction alignment (parallel / antiparallel / coincident over Direction),
/// codim 2: the two components of `a` in the anchor direction's tangent plane.
fn direction_alignment_residual(da: [f64; 3], db: [f64; 3]) -> Vec<f64> {
    let (e1, e2) = tangent_frame(db);
    vec![dot3(da, e1), dot3(da, e2)]
}

/// `coincident(D, D)` dispatched by datum kind (Direction 2 / Point 3 / Plane 3 /
/// Axis 4).
fn coincident_residual(a: &Value, b: &Value) -> Vec<f64> {
    match (a, b) {
        (Value::Axis { .. }, Value::Axis { .. }) => axis_coincidence_residual(a, b),
        (Value::Plane { .. }, Value::Plane { .. }) => plane_coincidence_residual(a, b),
        (Value::Direction { .. }, Value::Direction { .. }) => {
            match (dir_of(a), dir_of(b)) {
                (Some(da), Some(db)) => direction_alignment_residual(da, db),
                _ => Vec::new(),
            }
        }
        (Value::Point(_), Value::Point(_)) => match (origin_of(a), origin_of(b)) {
            (Some(pa), Some(pb)) => sub3(pa, pb).to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `offset(plane_a, plane_b, δ)`, codim 3: 2 normal-alignment components + the
/// signed offset along the anchor normal minus the target separation `δ`.
fn offset_residual(a: &Value, b: &Value, scalar: Option<f64>) -> Vec<f64> {
    let (Some((oa, na)), Some((ob, nb)), Some(d)) = (plane_parts(a), plane_parts(b), scalar) else {
        return Vec::new();
    };
    let (e1, e2) = tangent_frame(nb);
    let off = sub3(oa, ob);
    vec![dot3(na, e1), dot3(na, e2), dot3(off, nb) - d]
}

/// `on(point, host)` — point incidence; operand order is (point, host). Residual
/// is the point's deviation off the host: Plane → 1 (signed normal distance),
/// Axis → 2 (perpendicular offset), Point → 3 (coincidence).
fn on_residual(datums: &[(Value, bool)]) -> Vec<f64> {
    if datums.len() < 2 {
        return Vec::new();
    }
    let point = &datums[0].0;
    let host = &datums[1].0;
    let Some(p) = origin_of(point) else {
        return Vec::new();
    };
    match host {
        Value::Plane { .. } => {
            let Some((ho, hn)) = plane_parts(host) else {
                return Vec::new();
            };
            vec![dot3(sub3(p, ho), hn)]
        }
        Value::Axis { .. } => {
            let Some((ho, hd)) = axis_parts(host) else {
                return Vec::new();
            };
            let (e1, e2) = tangent_frame(hd);
            let off = sub3(p, ho);
            vec![dot3(off, e1), dot3(off, e2)]
        }
        Value::Point(_) => match origin_of(host) {
            Some(hp) => sub3(p, hp).to_vec(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

// ── Datum extraction ─────────────────────────────────────────────────────────

/// Is `v` a geometric datum (as opposed to a scalar magnitude)?
fn is_datum(v: &Value) -> bool {
    matches!(
        v,
        Value::Axis { .. }
            | Value::Plane { .. }
            | Value::Direction { .. }
            | Value::Point(_)
            | Value::Vector(_)
            | Value::Frame { .. }
    )
}

/// Extract a 3-vector `[f64; 3]` from a `Point`/`Vector`/`Direction`.
fn vec3_of(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Direction { x, y, z } => Some([*x, *y, *z]),
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => {
            Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])
        }
        _ => None,
    }
}

/// The direction/normal of a datum: `Direction` itself, `Axis.direction`,
/// `Plane.normal`, or a raw `Vector`.
fn dir_of(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Direction { .. } | Value::Vector(_) => vec3_of(v),
        Value::Axis { direction, .. } => vec3_of(direction),
        Value::Plane { normal, .. } => vec3_of(normal),
        _ => None,
    }
}

/// The origin/position of a datum: `Point` itself, `Axis.origin`, `Plane.origin`.
fn origin_of(v: &Value) -> Option<[f64; 3]> {
    match v {
        Value::Point(_) => vec3_of(v),
        Value::Axis { origin, .. } | Value::Plane { origin, .. } => vec3_of(origin),
        _ => None,
    }
}

/// `(origin, direction)` of an `Axis`.
fn axis_parts(v: &Value) -> Option<([f64; 3], [f64; 3])> {
    match v {
        Value::Axis { origin, direction } => Some((vec3_of(origin)?, vec3_of(direction)?)),
        _ => None,
    }
}

/// `(origin, normal)` of a `Plane`.
fn plane_parts(v: &Value) -> Option<([f64; 3], [f64; 3])> {
    match v {
        Value::Plane { origin, normal } => Some((vec3_of(origin)?, vec3_of(normal)?)),
        _ => None,
    }
}

// ── Datum transform under a pose ─────────────────────────────────────────────

/// Apply pose `x` to a datum: directions rotate, origins rotate-then-translate.
fn transform_datum(v: &Value, x: &Pose) -> Value {
    match v {
        Value::Axis { origin, direction } => {
            let o = vec3_of(origin).map(|p| transform_point(p, x));
            let d = vec3_of(direction).map(|u| rotate(x.rotation, u));
            match (o, d) {
                (Some(o), Some(d)) => Value::Axis {
                    origin: Box::new(point_value(o)),
                    direction: Box::new(vector_value(d)),
                },
                _ => v.clone(),
            }
        }
        Value::Plane { origin, normal } => {
            let o = vec3_of(origin).map(|p| transform_point(p, x));
            let n = vec3_of(normal).map(|u| rotate(x.rotation, u));
            match (o, n) {
                (Some(o), Some(n)) => Value::Plane {
                    origin: Box::new(point_value(o)),
                    normal: Box::new(vector_value(n)),
                },
                _ => v.clone(),
            }
        }
        Value::Direction { x: dx, y: dy, z: dz } => {
            let r = rotate(x.rotation, [*dx, *dy, *dz]);
            Value::Direction {
                x: r[0],
                y: r[1],
                z: r[2],
            }
        }
        Value::Point(c) if c.len() == 3 => match vec3_of(v) {
            Some(p) => point_value(transform_point(p, x)),
            None => v.clone(),
        },
        _ => v.clone(),
    }
}

/// Rotate then translate a position by pose `x`.
fn transform_point(p: [f64; 3], x: &Pose) -> [f64; 3] {
    add3(rotate(x.rotation, p), x.translation)
}

/// Rotate a 3-vector by the exponential-map vector `theta` (Rodrigues' formula).
fn rotate(theta: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    let angle = norm3(theta);
    if angle < 1e-12 {
        return v;
    }
    let k = scale3(theta, 1.0 / angle);
    let (s, c) = (angle.sin(), angle.cos());
    // v·cos + (k×v)·sin + k·(k·v)·(1−cos)
    add3(
        add3(scale3(v, c), scale3(cross3(k, v), s)),
        scale3(k, dot3(k, v) * (1.0 - c)),
    )
}

/// Build `Value::Point` (metres) from a 3-vector.
fn point_value(p: [f64; 3]) -> Value {
    Value::Point(vec![
        Value::length(p[0]),
        Value::length(p[1]),
        Value::length(p[2]),
    ])
}

/// Build `Value::Vector` (dimensionless) from a 3-vector.
fn vector_value(v: [f64; 3]) -> Value {
    Value::Vector(vec![Value::Real(v[0]), Value::Real(v[1]), Value::Real(v[2])])
}

// ── Rank-revealing Gram–Schmidt ──────────────────────────────────────────────

/// Add `rows` to the orthonormal `basis` one at a time, returning how many were
/// linearly independent of the running span. Each row is normalized before
/// Gram–Schmidt so the rank test is purely about direction independence (scale-
/// and unit-invariant); a row whose post-orthogonalization residual exceeds `tol`
/// is a new independent direction. Numerically-zero rows are skipped.
fn add_rows_rank(rows: &[[f64; FRAME_DOF]], basis: &mut Vec<[f64; FRAME_DOF]>, tol: f64) -> usize {
    let mut added = 0;
    for row in rows {
        let n = norm6(row);
        if n < ZERO_ROW_FLOOR {
            continue;
        }
        let mut v = *row;
        scale6_inplace(&mut v, 1.0 / n);
        for b in basis.iter() {
            let d = dot6(&v, b);
            for k in 0..FRAME_DOF {
                v[k] -= d * b[k];
            }
        }
        let r = norm6(&v);
        if r > tol {
            scale6_inplace(&mut v, 1.0 / r);
            basis.push(v);
            added += 1;
        }
    }
    added
}

// ── Damped Gauss–Newton (Levenberg–Marquardt) over the 6-DOF pose ────────────

/// Maximum LM iterations before declaring non-convergence. The Frame residuals are
/// near-linear in the pose for the canonical mate set, so feasible systems converge
/// in a handful of iterations; the cap only bounds genuinely-inconsistent inputs.
const MAX_SOLVE_ITERS: usize = 200;

/// Drive `unknown`'s pose from `seed` to satisfy `relations` by damped Gauss–Newton.
///
/// Returns `Some((pose, unique))` when the residual converges `≤ tol` (`unique` =
/// "all 6 DOF pinned at the solution"), or `None` when no pose drives the residual
/// below `tol` within [`MAX_SOLVE_ITERS`]. `λI` damping keeps the normal-equations
/// solve well-posed even when the Jacobian is rank-deficient (a residual gauge
/// freedom), so the gauge DOF simply stays near its seed rather than blowing up.
fn gauss_newton_solve(
    relations: &[RelationInstance],
    unknown: &FrameUnknown,
    seed: &Pose,
    tol: f64,
) -> Option<(Pose, bool)> {
    let mut x = pose_to_vec6(seed);
    let (mut r, mut jac) = combined_residual_jacobian(relations, unknown, &vec6_to_pose(&x));

    // No residual rows at all (empty/row-free driving set) ⇒ nothing to pin; the
    // seed trivially "satisfies" the (empty) set and leaves all 6 DOF free.
    if r.is_empty() {
        return Some((vec6_to_pose(&x), false));
    }

    let mut cost = sum_sq(&r);
    let mut lambda = 1e-3_f64;

    for _ in 0..MAX_SOLVE_ITERS {
        if max_abs(&r) <= tol {
            let unique = rank_of(&jac, tol) >= FRAME_DOF;
            return Some((vec6_to_pose(&x), unique));
        }

        // Normal equations: (JᵀJ + λI) δ = Jᵀr, step is x ← x − δ.
        let (mut a, g) = normal_equations(&jac, &r);
        for (i, row) in a.iter_mut().enumerate() {
            row[i] += lambda;
        }
        let delta = match solve6(&a, &g) {
            Some(d) => d,
            None => {
                // Singular even under damping — stiffen and retry.
                lambda *= 10.0;
                if lambda > 1e12 {
                    break;
                }
                continue;
            }
        };

        let mut x_new = x;
        for (xi, di) in x_new.iter_mut().zip(delta.iter()) {
            *xi -= *di;
        }
        let (r_new, jac_new) = combined_residual_jacobian(relations, unknown, &vec6_to_pose(&x_new));
        let cost_new = sum_sq(&r_new);

        if cost_new < cost {
            // Accept the step; relax damping toward pure Gauss–Newton.
            x = x_new;
            r = r_new;
            jac = jac_new;
            cost = cost_new;
            lambda = (lambda * 0.5).max(1e-12);
        } else {
            // Reject; stiffen toward gradient descent and retry from the same x.
            lambda *= 4.0;
            if lambda > 1e12 {
                break;
            }
        }
    }

    // Converged on the final iterate?
    if max_abs(&r) <= tol {
        Some((vec6_to_pose(&x), rank_of(&jac, tol) >= FRAME_DOF))
    } else {
        None
    }
}

/// Concatenate every relation's residual + matching Jacobian rows at `pose` into one
/// `(residual, jacobian)` pair (rows aligned 1:1). Built together so the residual
/// vector and Jacobian block can never drift in length.
fn combined_residual_jacobian(
    relations: &[RelationInstance],
    unknown: &FrameUnknown,
    pose: &Pose,
) -> (Vec<f64>, Vec<[f64; FRAME_DOF]>) {
    let mut res = Vec::new();
    let mut jac = Vec::new();
    for rel in relations {
        let rows = relation_jacobian(rel, unknown, pose);
        let rr = relation_residual(rel, unknown, pose);
        // `relation_jacobian` sizes itself from `relation_residual` at the same pose,
        // so the lengths match; the guard is purely defensive against future drift.
        if rows.len() == rr.len() {
            res.extend(rr);
            jac.extend(rows);
        }
    }
    (res, jac)
}

/// The rank of a Jacobian block (number of linearly-independent rows), via the same
/// rank-revealing Gram–Schmidt the partition uses.
fn rank_of(jac: &[[f64; FRAME_DOF]], tol: f64) -> usize {
    let mut basis = Vec::new();
    add_rows_rank(jac, &mut basis, tol)
}

/// Form the 6×6 normal matrix `A = JᵀJ` and gradient `g = Jᵀr`.
fn normal_equations(
    jac: &[[f64; FRAME_DOF]],
    r: &[f64],
) -> ([[f64; FRAME_DOF]; FRAME_DOF], [f64; FRAME_DOF]) {
    let mut a = [[0.0; FRAME_DOF]; FRAME_DOF];
    let mut g = [0.0; FRAME_DOF];
    for (row, &ri) in jac.iter().zip(r.iter()) {
        for i in 0..FRAME_DOF {
            g[i] += row[i] * ri;
            for j in 0..FRAME_DOF {
                a[i][j] += row[i] * row[j];
            }
        }
    }
    (a, g)
}

/// Solve the 6×6 linear system `A x = b` by Gaussian elimination with partial
/// pivoting. Returns `None` if `A` is numerically singular (a pivot below `1e-18`);
/// callers stiffen the LM damping and retry. Hand-rolled — no nalgebra (design §4).
//
// Index-based by nature: the elimination step writes `m[row]` while reading
// `m[col]` (col < row), an aliasing that iterator form cannot express without
// `split_at_mut` gymnastics that would only obscure a textbook algorithm.
#[allow(clippy::needless_range_loop)]
fn solve6(
    a: &[[f64; FRAME_DOF]; FRAME_DOF],
    b: &[f64; FRAME_DOF],
) -> Option<[f64; FRAME_DOF]> {
    let mut m = *a;
    let mut y = *b;

    for col in 0..FRAME_DOF {
        // Partial pivot: largest-magnitude entry in this column at/below the diagonal.
        let mut piv = col;
        let mut best = m[col][col].abs();
        for row in (col + 1)..FRAME_DOF {
            let v = m[row][col].abs();
            if v > best {
                best = v;
                piv = row;
            }
        }
        if best < 1e-18 {
            return None; // singular
        }
        if piv != col {
            m.swap(col, piv);
            y.swap(col, piv);
        }
        let diag = m[col][col];
        for row in (col + 1)..FRAME_DOF {
            let f = m[row][col] / diag;
            if f != 0.0 {
                for k in col..FRAME_DOF {
                    m[row][k] -= f * m[col][k];
                }
                y[row] -= f * y[col];
            }
        }
    }

    // Back-substitution.
    let mut x = [0.0; FRAME_DOF];
    for col in (0..FRAME_DOF).rev() {
        let mut s = y[col];
        for k in (col + 1)..FRAME_DOF {
            s -= m[col][k] * x[k];
        }
        x[col] = s / m[col][col];
    }
    Some(x)
}

fn pose_to_vec6(p: &Pose) -> [f64; FRAME_DOF] {
    [
        p.translation[0],
        p.translation[1],
        p.translation[2],
        p.rotation[0],
        p.rotation[1],
        p.rotation[2],
    ]
}

fn vec6_to_pose(v: &[f64; FRAME_DOF]) -> Pose {
    Pose {
        translation: [v[0], v[1], v[2]],
        rotation: [v[3], v[4], v[5]],
    }
}

fn sum_sq(r: &[f64]) -> f64 {
    r.iter().map(|x| x * x).sum()
}

fn max_abs(r: &[f64]) -> f64 {
    r.iter().fold(0.0_f64, |m, x| m.max(x.abs()))
}

// ── Frame ⇄ Pose assembly ────────────────────────────────────────────────────

/// Assemble a [`Value::Frame`] (origin `Point` + basis `Orientation` quaternion)
/// from a solved [`Pose`]. The inverse of [`pose_from_frame`].
fn frame_value(pose: &Pose) -> Value {
    Value::Frame {
        origin: Box::new(point_value(pose.translation)),
        basis: Box::new(orientation_from_exp_map(pose.rotation)),
    }
}

/// Exponential-map rotation vector → unit quaternion [`Value::Orientation`]
/// `(cos(θ/2), axis·sin(θ/2))`, where `θ = |rot|`. The zero vector maps to identity.
fn orientation_from_exp_map(rot: [f64; 3]) -> Value {
    let angle = norm3(rot);
    if angle < 1e-12 {
        return Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
    }
    let half = angle * 0.5;
    let s = half.sin() / angle; // axis·sin(half) = rot/angle·sin(half) = rot·s
    Value::Orientation {
        w: half.cos(),
        x: rot[0] * s,
        y: rot[1] * s,
        z: rot[2] * s,
    }
}

/// Unit quaternion [`Value::Orientation`] → exponential-map rotation vector
/// `axis·θ`, the inverse of [`orientation_from_exp_map`]. The identity (and any
/// near-zero vector part) maps to the zero rotation.
fn exp_map_from_orientation(v: &Value) -> Option<[f64; 3]> {
    let Value::Orientation { w, x, y, z } = v else {
        return None;
    };
    let vnorm = (x * x + y * y + z * z).sqrt();
    if vnorm < 1e-12 {
        return Some([0.0, 0.0, 0.0]);
    }
    // θ = 2·atan2(|v|, w) ∈ [0, 2π); exp-map = (v/|v|)·θ = v·(θ/|v|).
    let angle = 2.0 * vnorm.atan2(*w);
    let s = angle / vnorm;
    Some([x * s, y * s, z * s])
}

// ── Small vec math ───────────────────────────────────────────────────────────

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

/// An orthonormal pair spanning the plane perpendicular to unit-ish vector `n`.
/// Picks the world axis least aligned with `n` to avoid a degenerate cross.
fn tangent_frame(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let nn = {
        let len = norm3(n);
        if len < 1e-12 {
            [0.0, 0.0, 1.0]
        } else {
            scale3(n, 1.0 / len)
        }
    };
    let helper = if nn[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let mut e1 = cross3(helper, nn);
    let l1 = norm3(e1);
    e1 = if l1 < 1e-12 {
        [0.0, 1.0, 0.0]
    } else {
        scale3(e1, 1.0 / l1)
    };
    let e2 = cross3(nn, e1);
    (e1, e2)
}

fn dot6(a: &[f64; FRAME_DOF], b: &[f64; FRAME_DOF]) -> f64 {
    (0..FRAME_DOF).map(|k| a[k] * b[k]).sum()
}

fn norm6(a: &[f64; FRAME_DOF]) -> f64 {
    dot6(a, a).sqrt()
}

fn scale6_inplace(a: &mut [f64; FRAME_DOF], s: f64) {
    for x in a.iter_mut() {
        *x *= s;
    }
}
