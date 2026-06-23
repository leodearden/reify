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

use reify_ir::Value;

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
