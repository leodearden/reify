//! Integration tests for the 2D constrained-sketch solver substrate.
//!
//! Every test here drives a real `Slvs_Solve` through the crate's public
//! `solve_sketch` seam — nothing is stubbed, so a passing assertion is evidence
//! about libslvs' actual behaviour, not about a mock.
//!
//! Fixture helpers live at the top of the file and are shared by the assertion
//! blocks below.

// Fixture helpers are shared across the assertion blocks in this file, and each
// one is exercised by only a subset of them (a points-only fixture never reads a
// radius, a radius fixture never resolves a failing set).  Allowing dead_code
// file-wide keeps the helper surface complete instead of pruned to whichever
// tests happen to exist right now.
#![allow(dead_code)]

use reify_constraints::{
    SketchConstraint, SketchConstraintDef, SketchConstraintId, SketchEntity, SketchEntityDef,
    SketchEntityId, SketchSolveResult, SketchSystem, SolvedSketchEntity,
};
use reify_core::SourceSpan;

/// Absolute tolerance for solved-geometry assertions, in SI metres for
/// positional residuals and dimensionless for normalized ones.
///
/// Matches the landed precedent for this exact solver + workplane path
/// (`crates/reify-constraints/tests/solvespace_tests.rs` uses `1e-6` at every
/// residual site).  The residuals these fixtures actually produce were measured
/// against the same `libslvs` this crate links at `<= 1e-10`, so `TOL` carries
/// roughly four orders of margin — it is a floor on "the solver converged",
/// not a bound tuned to make a particular fixture pass.
///
/// Exact-integer facts — `dof` counts, failing-set membership — are asserted
/// with no tolerance at all: they are combinatorial (`params - rank`), not
/// numeric.
const TOL: f64 = 1e-6;

// ---------------------------------------------------------------------------
// Fixture builder
// ---------------------------------------------------------------------------

/// Test-side builder for a [`SketchSystem`].
///
/// Hands out `SketchEntityId` / `SketchConstraintId` values sequentially from 1
/// and stamps each constraint with a synthetic span derived from its id, so a
/// test can check that a failing constraint came back paired with *its own*
/// span rather than with some other constraint's.
struct Sketch {
    system: SketchSystem,
    next_entity: u32,
    next_constraint: u32,
}

/// The synthetic span this fixture stamps on constraint `id`.
///
/// A bijection between constraint ids and spans: distinct ids get
/// non-overlapping spans, so `(id, span)` pairs coming back out of the solver
/// can be checked for *internal consistency*, not merely for plausibility.
fn span_for(id: SketchConstraintId) -> SourceSpan {
    let start = id.0 * 10;
    SourceSpan::new(start, start + 4)
}

impl Sketch {
    fn new() -> Self {
        Self {
            system: SketchSystem::default(),
            next_entity: 1,
            next_constraint: 1,
        }
    }

    /// Append an entity, returning its freshly allocated id.
    fn entity(&mut self, entity: SketchEntity) -> SketchEntityId {
        self.entity_with_aux(entity, false)
    }

    /// Append an entity, marking it as auxiliary (construction) geometry.
    fn aux_entity(&mut self, entity: SketchEntity) -> SketchEntityId {
        self.entity_with_aux(entity, true)
    }

    fn entity_with_aux(&mut self, entity: SketchEntity, aux: bool) -> SketchEntityId {
        let id = SketchEntityId(self.next_entity);
        self.next_entity += 1;
        self.system
            .entities
            .push(SketchEntityDef { id, entity, aux });
        id
    }

    /// Append a point seeded at `(x, y)` metres.
    fn point(&mut self, x: f64, y: f64) -> SketchEntityId {
        self.entity(SketchEntity::Point { x, y })
    }

    /// Append a line between two already-declared points.
    fn line(&mut self, start: SketchEntityId, end: SketchEntityId) -> SketchEntityId {
        self.entity(SketchEntity::Line { start, end })
    }

    /// Append a circle around an already-declared center, seeded at `radius` metres.
    fn circle(&mut self, center: SketchEntityId, radius: f64) -> SketchEntityId {
        self.entity(SketchEntity::Circle { center, radius })
    }

    /// Append an arc over three already-declared points.
    fn arc(
        &mut self,
        center: SketchEntityId,
        start: SketchEntityId,
        end: SketchEntityId,
    ) -> SketchEntityId {
        self.entity(SketchEntity::Arc { center, start, end })
    }

    /// Append a constraint, returning its freshly allocated id.
    ///
    /// The constraint's span is [`span_for`] of that id.
    fn constrain(&mut self, constraint: SketchConstraint) -> SketchConstraintId {
        let id = SketchConstraintId(self.next_constraint);
        self.next_constraint += 1;
        self.system.constraints.push(SketchConstraintDef {
            id,
            constraint,
            span: span_for(id),
        });
        id
    }

    /// Append an entity with a caller-chosen id, bypassing the allocator.
    ///
    /// Only for malformed-input fixtures (duplicate ids, dangling references)
    /// that the well-behaved allocator cannot express.
    fn entity_with_id(&mut self, id: SketchEntityId, entity: SketchEntity) -> SketchEntityId {
        self.system.entities.push(SketchEntityDef {
            id,
            entity,
            aux: false,
        });
        id
    }

    fn system(&self) -> &SketchSystem {
        &self.system
    }
}

// ---------------------------------------------------------------------------
// Readback accessors
// ---------------------------------------------------------------------------

/// The solved entity list, or a descriptive panic naming the actual arm.
fn solved(result: &SketchSolveResult) -> &Vec<(SketchEntityId, SolvedSketchEntity)> {
    match result {
        SketchSolveResult::Solved { entities, .. } => entities,
        other => panic!("expected SketchSolveResult::Solved, got {other:?}"),
    }
}

/// libslvs' reported degrees of freedom, or a descriptive panic.
fn dof_of(result: &SketchSolveResult) -> i32 {
    match result {
        SketchSolveResult::Solved { dof, .. } => *dof,
        other => panic!("expected SketchSolveResult::Solved to read dof, got {other:?}"),
    }
}

/// The solved `(x, y)` of the point entity `id`.
///
/// Panics — descriptively — if the result is not `Solved`, if `id` is absent
/// from the readback, or if `id` names something other than a point.
fn point_of(result: &SketchSolveResult, id: SketchEntityId) -> (f64, f64) {
    match lookup(result, id) {
        SolvedSketchEntity::Point { x, y } => (*x, *y),
        other => panic!("entity {id:?} is not a Point in the solved readback: {other:?}"),
    }
}

/// The solved `(start, end)` of the line entity `id`.
///
/// Panics — descriptively — if the result is not `Solved`, if `id` is absent
/// from the readback, or if `id` names something other than a line.
fn line_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), (f64, f64)) {
    match lookup(result, id) {
        SolvedSketchEntity::Line { start, end } => (*start, *end),
        other => panic!("entity {id:?} is not a Line in the solved readback: {other:?}"),
    }
}

/// The solved radius of the circle entity `id`.
///
/// Panics — descriptively — if the result is not `Solved`, if `id` is absent
/// from the readback, or if `id` names something other than a circle.
fn radius_of(result: &SketchSolveResult, id: SketchEntityId) -> f64 {
    match lookup(result, id) {
        SolvedSketchEntity::Circle { radius, .. } => *radius,
        other => panic!("entity {id:?} is not a Circle in the solved readback: {other:?}"),
    }
}

fn lookup(result: &SketchSolveResult, id: SketchEntityId) -> &SolvedSketchEntity {
    let entities = solved(result);
    match entities.iter().find(|(eid, _)| *eid == id) {
        Some((_, e)) => e,
        None => panic!(
            "entity {id:?} is absent from the solved readback; present ids: {:?}",
            entities.iter().map(|(eid, _)| *eid).collect::<Vec<_>>()
        ),
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// Euclidean distance between two solved 2D positions.
fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// `a` normalized to unit length. Panics if `a` is degenerate — a zero-length
/// direction in a tangency or orientation fixture means the fixture itself is
/// broken, and silently returning `(0, 0)` would make the residual assertion
/// pass for the wrong reason.
fn unit(a: (f64, f64)) -> (f64, f64) {
    let n = (a.0 * a.0 + a.1 * a.1).sqrt();
    assert!(
        n > TOL,
        "degenerate direction {a:?}: length {n} is within TOL of zero"
    );
    (a.0 / n, a.1 / n)
}

/// Vector from `from` to `to`.
fn delta(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    (to.0 - from.0, to.1 - from.1)
}

/// 2D dot product.
fn dot(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

/// 2D cross-product z component (zero iff `a` and `b` are collinear).
fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// Assert `actual` is within [`TOL`] of `expected`, naming both in the failure.
fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < TOL,
        "{what}: expected {expected}, got {actual} (delta {})",
        (actual - expected).abs()
    );
}

/// Assert a solved point is within [`TOL`] of `(x, y)`.
fn assert_point_near(actual: (f64, f64), expected: (f64, f64), what: &str) {
    assert!(
        dist(actual, expected) < TOL,
        "{what}: expected {expected:?}, got {actual:?} (distance {})",
        dist(actual, expected)
    );
}

// ---------------------------------------------------------------------------
// The direct-build spine: entry point, DOF surfacing, determinism
// ---------------------------------------------------------------------------

/// A sketch of two free points with *no* constraints solves, and reports its
/// degrees of freedom honestly.
///
/// `dof == 4` is an exact combinatorial identity, not a measurement: a 2D point
/// contributes two params to the solved group, there are two points, and there
/// are no equations to reduce the rank. libslvs itself reports 4 for exactly
/// this system.
///
/// This is the case a hardcoded `dof: 0` short-circuit would get wrong, and
/// getting it wrong is not cosmetic — it reports a wholly unconstrained sketch
/// as fully constrained.
#[test]
fn points_only_sketch_solves_and_reports_honest_dof() {
    let mut s = Sketch::new();
    let a = s.point(0.003, 0.004);
    let b = s.point(-0.002, 0.007);

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "an unconstrained sketch is solvable, got {result:?}"
    );
    assert_eq!(
        dof_of(&result),
        4,
        "two free 2D points and no constraints = 4 DOF"
    );

    // Nothing constrains the points, so the solver must leave them at their seeds.
    assert_point_near(point_of(&result, a), (0.003, 0.004), "point a");
    assert_point_near(point_of(&result, b), (-0.002, 0.007), "point b");
}

/// Solving the same `SketchSystem` twice yields bit-identical coordinates.
///
/// Compared with `f64::to_bits`, not within `TOL`: O9 determinism is a claim
/// about reproducibility, and a tolerance-based check would pass for a solver
/// that wandered by 1e-12 between runs. Handle allocation walks the declaration
/// -ordered `Vec`s, so the same input must produce the same slvs system and
/// therefore the same output.
#[test]
fn solve_sketch_is_bit_deterministic() {
    let mut s = Sketch::new();
    let a = s.point(0.003, 0.004);
    let b = s.point(-0.002, 0.007);

    let first = reify_constraints::solve_sketch(s.system());
    let second = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        dof_of(&first),
        dof_of(&second),
        "dof must be reproducible across identical solves"
    );

    for id in [a, b] {
        let (x1, y1) = point_of(&first, id);
        let (x2, y2) = point_of(&second, id);
        assert_eq!(
            (x1.to_bits(), y1.to_bits()),
            (x2.to_bits(), y2.to_bits()),
            "{id:?} must be bit-identical across identical solves: \
             ({x1}, {y1}) vs ({x2}, {y2})"
        );
    }

    // The readback order is the declaration order, not whatever a hash map
    // iteration happened to produce.
    let ids_first: Vec<_> = solved(&first).iter().map(|(id, _)| *id).collect();
    let ids_second: Vec<_> = solved(&second).iter().map(|(id, _)| *id).collect();
    assert_eq!(ids_first, ids_second, "readback order must be stable");
    assert_eq!(
        ids_first,
        vec![a, b],
        "readback must follow declaration order"
    );
}

// ---------------------------------------------------------------------------
// Lines: anchoring, orientation, dimensioning
// ---------------------------------------------------------------------------

/// A line anchored at one end, held horizontal, and dimensioned is fully
/// constrained.
///
/// `dof == 0` is combinatorial: four params (two 2D points), four independent
/// equations (`Fix` pins two, `Horizontal` one, `Distance` one).  The endpoint
/// positions are not "wherever the solver went" either — with the anchor at the
/// origin and the line held horizontal, `(0.010, 0)` is the only solution on the
/// seed's side of the origin.
#[test]
fn anchored_horizontal_dimensioned_line_is_fully_constrained() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.008, 0.002);
    let line = s.line(a, b);
    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::Horizontal(line));
    s.constrain(SketchConstraint::Distance { a, b, value: 0.010 });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        dof_of(&result),
        0,
        "4 params less 4 independent equations = 0 DOF"
    );
    assert_point_near(point_of(&result, a), (0.0, 0.0), "anchored end");
    assert_point_near(point_of(&result, b), (0.010, 0.0), "dimensioned free end");

    // A composite entity reads back as geometry, not as another round of id
    // chasing: the line's endpoints are the solved positions of its points.
    let (start, end) = line_of(&result, line);
    assert_point_near(start, point_of(&result, a), "line start vs point a");
    assert_point_near(end, point_of(&result, b), "line end vs point b");
}

/// Dropping the dimension leaves exactly one degree of freedom.
///
/// The same fixture minus `Distance`: `b` may still slide along the horizontal
/// through the anchor, and nothing else. `dof == 1` distinguishes "under-
/// constrained by exactly one" from both "fully constrained" and "the solver
/// lost track of the constraint" — which a tolerance-based check could not.
#[test]
fn undimensioned_horizontal_line_leaves_one_dof() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.008, 0.002);
    let line = s.line(a, b);
    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::Horizontal(line));

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        dof_of(&result),
        1,
        "4 params less 3 independent equations = 1 DOF (b slides along the horizontal)"
    );
    // The orientation constraint still holds even though the length is free.
    assert_near(
        point_of(&result, b).1,
        0.0,
        "free end stays on the horizontal",
    );
}

/// `Vertical` drives the free end along the v axis rather than the u axis.
///
/// The mirror of the horizontal fixture, and the reason both exist: a mapping
/// that emitted `SLVS_C_HORIZONTAL` for both variants would pass the horizontal
/// test alone.
#[test]
fn anchored_vertical_dimensioned_line_drives_the_free_end_up() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.002, 0.018);
    let line = s.line(a, b);
    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::Vertical(line));
    s.constrain(SketchConstraint::Distance { a, b, value: 0.020 });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(dof_of(&result), 0, "anchored + oriented + dimensioned");
    assert_point_near(point_of(&result, a), (0.0, 0.0), "anchored end");
    assert_point_near(
        point_of(&result, b),
        (0.0, 0.020),
        "free end above the anchor",
    );
}

/// `Coincident` drives two points seeded apart onto the same location.
#[test]
fn coincident_drives_two_seeded_apart_points_together() {
    let mut s = Sketch::new();
    let p = s.point(0.005, 0.003);
    let q = s.point(-0.001, 0.008);
    s.constrain(SketchConstraint::Fix(p));
    s.constrain(SketchConstraint::Coincident { a: p, b: q });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(dof_of(&result), 0, "4 params less 4 independent equations");
    // The anchored point did not move to meet the free one; the free one moved.
    assert_point_near(point_of(&result, p), (0.005, 0.003), "anchored point");
    assert_point_near(point_of(&result, q), (0.005, 0.003), "coincident point");
}

// ---------------------------------------------------------------------------
// Lines: relative orientation
// ---------------------------------------------------------------------------

/// `Fix` on a line pins both of its endpoints, and `Parallel` zeroes the cross
/// product of the two line directions.
///
/// The residual is the normalized 2D cross product, which is exactly zero iff
/// the directions are collinear — the formulation already used for the legacy
/// parallel test in `solvespace_tests.rs`.
#[test]
fn parallel_lines_solve_to_a_zero_cross_product() {
    let mut s = Sketch::new();
    let a1 = s.point(0.0, 0.0);
    let a2 = s.point(0.010, 0.010);
    let la = s.line(a1, a2);
    let b1 = s.point(0.020, 0.0);
    // Seeded deliberately off-parallel, so a no-op mapping cannot pass.
    let b2 = s.point(0.030, 0.012);
    let lb = s.line(b1, b2);
    s.constrain(SketchConstraint::Fix(la));
    s.constrain(SketchConstraint::Fix(b1));
    s.constrain(SketchConstraint::Parallel { a: la, b: lb });

    let result = reify_constraints::solve_sketch(s.system());

    // `Fix` applied to a line really did anchor both endpoints.
    assert_point_near(point_of(&result, a1), (0.0, 0.0), "anchored line start");
    assert_point_near(point_of(&result, a2), (0.010, 0.010), "anchored line end");
    assert_point_near(point_of(&result, b1), (0.020, 0.0), "anchored second start");

    let da = unit(delta(point_of(&result, a1), point_of(&result, a2)));
    let db = unit(delta(point_of(&result, b1), point_of(&result, b2)));
    assert!(
        cross(da, db).abs() < TOL,
        "parallel: cross({da:?}, {db:?}) = {} should be 0",
        cross(da, db)
    );
}

/// `Perpendicular` zeroes the dot product of the two normalized directions.
#[test]
fn perpendicular_lines_solve_to_a_zero_dot_product() {
    let mut s = Sketch::new();
    let a1 = s.point(0.0, 0.0);
    let a2 = s.point(0.010, 0.010);
    let la = s.line(a1, a2);
    let b1 = s.point(0.020, 0.0);
    // Seeded near — but not at — the perpendicular direction (-1, 1).
    let b2 = s.point(0.013, 0.008);
    let lb = s.line(b1, b2);
    s.constrain(SketchConstraint::Fix(la));
    s.constrain(SketchConstraint::Fix(b1));
    s.constrain(SketchConstraint::Perpendicular { a: la, b: lb });

    let result = reify_constraints::solve_sketch(s.system());

    let da = unit(delta(point_of(&result, a1), point_of(&result, a2)));
    let db = unit(delta(point_of(&result, b1), point_of(&result, b2)));
    assert!(
        dot(da, db).abs() < TOL,
        "perpendicular: dot({da:?}, {db:?}) = {} should be 0",
        dot(da, db)
    );
}

/// `Angle` drives the direction cosine between two lines to `cos(degrees)`.
///
/// Degrees, not radians: libslvs' `SLVS_C_ANGLE` takes `valA` in degrees, and
/// the surface vocabulary matches it rather than converting at the boundary. A
/// mapping that passed 45 as radians would land the lines ~2.6 degrees apart,
/// which this assertion catches.
#[test]
fn angle_constraint_drives_the_direction_cosine() {
    let mut s = Sketch::new();
    let a1 = s.point(0.0, 0.0);
    let a2 = s.point(0.010, 0.0);
    let la = s.line(a1, a2);
    let b1 = s.point(0.020, 0.0);
    // Seeded at roughly 32 degrees, so the solver has to move it to 45.
    let b2 = s.point(0.028, 0.005);
    let lb = s.line(b1, b2);
    s.constrain(SketchConstraint::Fix(la));
    s.constrain(SketchConstraint::Fix(b1));
    s.constrain(SketchConstraint::Angle {
        a: la,
        b: lb,
        degrees: 45.0,
    });

    let result = reify_constraints::solve_sketch(s.system());

    let da = unit(delta(point_of(&result, a1), point_of(&result, a2)));
    let db = unit(delta(point_of(&result, b1), point_of(&result, b2)));
    assert_near(
        dot(da, db),
        45.0_f64.to_radians().cos(),
        "direction cosine between the two lines",
    );
}
