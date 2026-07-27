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
    SketchBuildError, SketchConstraint, SketchConstraintDef, SketchConstraintId, SketchEntity,
    SketchEntityDef, SketchEntityId, SketchEntityKind, SketchSlotKind, SketchSolveResult,
    SketchSystem, SketchValueField, SolvedSketchEntity,
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

    /// Append a constraint with a caller-chosen id, bypassing the allocator.
    ///
    /// The constraint twin of [`Sketch::entity_with_id`]: for duplicate-id
    /// fixtures, and for tests that need an id distinctive enough that finding
    /// it in a rendered string cannot be a coincidence.
    fn constrain_with_id(
        &mut self,
        id: SketchConstraintId,
        constraint: SketchConstraint,
    ) -> SketchConstraintId {
        self.system.constraints.push(SketchConstraintDef {
            id,
            constraint,
            span: span_for(id),
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

/// The solved `(center, radius)` of the circle entity `id`.
fn circle_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), f64) {
    match lookup(result, id) {
        SolvedSketchEntity::Circle { center, radius } => (*center, *radius),
        other => panic!("entity {id:?} is not a Circle in the solved readback: {other:?}"),
    }
}

/// The solved `(center, start, end)` of the arc entity `id`.
fn arc_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), (f64, f64), (f64, f64)) {
    match lookup(result, id) {
        SolvedSketchEntity::Arc { center, start, end } => (*center, *start, *end),
        other => panic!("entity {id:?} is not an Arc in the solved readback: {other:?}"),
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

/// An unconstrained arc has five degrees of freedom, not six.
///
/// The arc's three points contribute six params, but libslvs emits an implicit
/// `|c-s| = |c-e|` equation for every `SLVS_E_ARC_OF_CIRCLE` (the same equation
/// `arc_endpoints_share_a_radius_implicitly` observes acting on the geometry),
/// so the rank of a *zero-constraint* system containing an arc is 1, not 0.
///
/// `dof` is therefore only trustworthy when it comes from libslvs itself. Any
/// Rust-side recomputation as "count the free params" must disagree here — it
/// has no way to know about an equation the C library added on its own behalf.
///
/// Exact integer, no tolerance: `params - rank` is combinatorial.
#[test]
fn unconstrained_arc_reports_five_dof_not_its_param_count() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let start = s.point(0.005, 0.0);
    let end = s.point(0.0, 0.005);
    s.arc(c, start, end);

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "an unconstrained arc is solvable, got {result:?}"
    );
    assert_eq!(
        dof_of(&result),
        5,
        "an arc's 6 point params less libslvs' one implicit equal-radius equation"
    );
}

/// Two unconstrained arcs have ten degrees of freedom, not eleven or twelve.
///
/// The companion to the single-arc case, and the reason it is not redundant:
/// twelve params less *one* equation would be eleven. Ten pins that libslvs
/// contributes the implicit equation once **per arc**, so the discrepancy
/// scales with the arc count rather than being a single fixed offset that a
/// constant correction could absorb.
#[test]
fn implicit_arc_equation_is_per_arc_not_a_fixed_offset() {
    let mut s = Sketch::new();
    let c1 = s.point(0.0, 0.0);
    let s1 = s.point(0.005, 0.0);
    let e1 = s.point(0.0, 0.005);
    s.arc(c1, s1, e1);

    let c2 = s.point(0.020, 0.0);
    let s2 = s.point(0.024, 0.0);
    let e2 = s.point(0.020, 0.004);
    s.arc(c2, s2, e2);

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "two unconstrained arcs are solvable, got {result:?}"
    );
    assert_eq!(
        dof_of(&result),
        10,
        "12 point params less one implicit equal-radius equation per arc"
    );
}

/// An unconstrained circle has three degrees of freedom — centre x, centre y,
/// and radius.
///
/// An over-correction guard rather than a restatement of the arc cases: a
/// circle carries a radius param of its own and libslvs adds *no* implicit
/// equation for it, so any fix for the arc discrepancy that subtracts per
/// curved entity — rather than per arc — lands here as 2 instead of 3.
#[test]
fn unconstrained_circle_reports_three_dof() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    s.circle(c, 0.004);

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "an unconstrained circle is solvable, got {result:?}"
    );
    assert_eq!(
        dof_of(&result),
        3,
        "circle centre (2 params) plus its radius param, with no implicit equation"
    );
}

/// An unconstrained line has four degrees of freedom — its two endpoints.
///
/// The second over-correction guard. A line segment introduces no params of its
/// own and no implicit equation: its DOF is exactly its endpoints', so this
/// must read the same as the two-free-points case above. A blanket "subtract
/// one per non-point entity" correction reads 3 here.
#[test]
fn unconstrained_line_reports_its_endpoints_dof() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.010, 0.002);
    s.line(a, b);

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "an unconstrained line is solvable, got {result:?}"
    );
    assert_eq!(
        dof_of(&result),
        4,
        "two free endpoints; the line segment itself adds neither param nor equation"
    );
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

// ---------------------------------------------------------------------------
// Circles and arcs: the radius family
// ---------------------------------------------------------------------------

/// Solve a single circle with a fixed centre, seeded at 4 mm, under one
/// dimensional constraint, and report the radius it lands on.
///
/// The seed is deliberately neither of the answers the callers expect, so a
/// mapping that dropped the dimension entirely would read back 0.004 and fail.
fn solved_radius(dimension: impl Fn(SketchEntityId) -> SketchConstraint) -> f64 {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let circle = s.circle(c, 0.004);
    s.constrain(SketchConstraint::Fix(c));
    s.constrain(dimension(circle));

    let result = reify_constraints::solve_sketch(s.system());
    radius_of(&result, circle)
}

/// `Diameter` drives the circle's radius to half its value.
#[test]
fn diameter_drives_the_circle_radius() {
    let r = solved_radius(|circle| SketchConstraint::Diameter {
        circle,
        value: 0.010,
    });
    assert_near(r, 0.005, "radius solved from a 10 mm diameter");
}

/// `Radius` and `Diameter` are the same constraint with a factor of two.
///
/// libslvs has no radius constraint — `SLVS_C_DIAMETER` is the only member of
/// the radius family — so the doubling has to happen somewhere. Pinning both
/// spellings against each other is what keeps the surface vocabulary honest:
/// `radius` has to mean radius, not "diameter under another name".
#[test]
fn radius_is_half_the_diameter() {
    let from_diameter = solved_radius(|circle| SketchConstraint::Diameter {
        circle,
        value: 0.010,
    });
    let from_radius = solved_radius(|circle| SketchConstraint::Radius {
        circle,
        value: 0.005,
    });

    assert_near(from_diameter, 0.005, "radius solved from a 10 mm diameter");
    assert_near(from_radius, 0.005, "radius solved from a 5 mm radius");
    assert_near(
        from_radius,
        from_diameter,
        "radius(5 mm) and diameter(10 mm) name the same circle",
    );
}

/// `PtOnCircle` lands a point on the solved circumference.
///
/// Checked against the circle's *own* readback centre, not just the centre
/// point's, so the composite readback has to resolve its operand rather than
/// report placeholder geometry.
#[test]
fn point_on_circle_lands_on_the_solved_circumference() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let circle = s.circle(c, 0.004);
    // Seeded well off the circumference, in both radius and angle.
    let p = s.point(0.009, 0.001);
    s.constrain(SketchConstraint::Fix(c));
    s.constrain(SketchConstraint::Diameter {
        circle,
        value: 0.010,
    });
    s.constrain(SketchConstraint::PtOnCircle { pt: p, circle });

    let result = reify_constraints::solve_sketch(s.system());

    let (center, radius) = circle_of(&result, circle);
    assert_point_near(
        center,
        point_of(&result, c),
        "circle centre vs centre point",
    );
    assert_near(radius, 0.005, "solved radius");
    assert_near(
        dist(point_of(&result, p), center),
        0.005,
        "distance from the constrained point to the centre",
    );
}

/// `EqualRadius` propagates one circle's dimension to another.
#[test]
fn equal_radius_propagates_a_single_dimension() {
    let mut s = Sketch::new();
    let c1 = s.point(0.0, 0.0);
    let k1 = s.circle(c1, 0.004);
    let c2 = s.point(0.020, 0.0);
    // Seeded at a different radius, so equality has to be driven, not inherited.
    let k2 = s.circle(c2, 0.007);
    s.constrain(SketchConstraint::Fix(c1));
    s.constrain(SketchConstraint::Fix(c2));
    s.constrain(SketchConstraint::Diameter {
        circle: k1,
        value: 0.010,
    });
    s.constrain(SketchConstraint::EqualRadius { a: k1, b: k2 });

    let result = reify_constraints::solve_sketch(s.system());

    assert_near(radius_of(&result, k1), 0.005, "dimensioned circle");
    assert_near(radius_of(&result, k2), 0.005, "circle equated to it");
}

/// An arc's two endpoints are equidistant from its centre without anyone
/// saying so.
///
/// libslvs contributes that equation itself for every `ARC_OF_CIRCLE`, which is
/// why the fixture dimensions only the *start* radius and still expects the end
/// to follow. It is also why DOF accounting over arcs must not count the
/// equal-radius relation a second time.
#[test]
fn arc_endpoints_share_a_radius_implicitly() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let start = s.point(0.004, 0.0);
    // Seeded at a different distance from the centre than the start.
    let end = s.point(0.0, 0.006);
    let arc = s.arc(c, start, end);
    s.constrain(SketchConstraint::Fix(c));
    s.constrain(SketchConstraint::Distance {
        a: c,
        b: start,
        value: 0.005,
    });

    let result = reify_constraints::solve_sketch(s.system());

    let (center, arc_start, arc_end) = arc_of(&result, arc);
    assert_point_near(center, (0.0, 0.0), "anchored arc centre");
    assert_near(dist(center, arc_start), 0.005, "dimensioned start radius");
    assert_near(
        dist(center, arc_end),
        0.005,
        "end radius, from libslvs' implicit equal-radius equation",
    );
}

// ---------------------------------------------------------------------------
// Tangency
// ---------------------------------------------------------------------------

/// `ArcLineTangent` squares a line against the arc radius at the touch point.
///
/// The residual asserted here *is* the constraint's definition — libslvs
/// generates exactly "line direction · (centre − endpoint) = 0" — rather than a
/// proxy for it, so a solved direction cosine of zero cannot be produced by any
/// other constraint in the fixture.
///
/// The arc's start is seeded on the dimensioned circle but with the line running
/// well off tangent (direction cosine ≈ 0.83 at the seed), so the assertion is
/// only reachable by moving the touch point — an emit path that dropped the
/// constraint leaves the seed alone and reads back 0.83.
///
/// `at_end: false` selects the arc's *start* as the tangent point; the arc's end
/// stays free, which is why the fixture says nothing about where it lands.
#[test]
fn arc_line_tangent_squares_the_line_against_the_radius() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let start = s.point(0.005, 0.0);
    let end = s.point(0.0, 0.005);
    let arc = s.arc(c, start, end);
    // The line runs from the arc's own start point out to an anchored far end,
    // which is what makes tangency a statement about this arc's touch point
    // rather than about two unrelated pieces of geometry.
    let far = s.point(0.020, 0.010);
    let line = s.line(start, far);

    s.constrain(SketchConstraint::Fix(c));
    s.constrain(SketchConstraint::Fix(far));
    s.constrain(SketchConstraint::Distance {
        a: c,
        b: start,
        value: 0.005,
    });
    s.constrain(SketchConstraint::ArcLineTangent {
        arc,
        line,
        at_end: false,
    });

    let result = reify_constraints::solve_sketch(s.system());
    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "a tangent arc and line are solvable, got {result:?}"
    );

    let (center, arc_start, _) = arc_of(&result, arc);
    let far_end = point_of(&result, far);
    assert_point_near(center, (0.0, 0.0), "anchored arc centre");
    assert_point_near(far_end, (0.020, 0.010), "anchored far endpoint");
    assert_near(dist(center, arc_start), 0.005, "dimensioned arc radius");
    // The line still runs through the arc's start point: it is defined by that
    // very entity, so tangency has to hold at a point on both curves.
    assert_point_near(
        line_of(&result, line).0,
        arc_start,
        "line start vs the arc's start point",
    );

    let radius_dir = unit(delta(center, arc_start));
    let line_dir = unit(delta(arc_start, far_end));
    assert_near(
        dot(radius_dir, line_dir),
        0.0,
        "direction cosine between the radius and the line at the tangent point",
    );
}

/// `at_end: true` selects the arc's *end* — the other slvs endpoint slot.
///
/// The mirror image of the fixture above, and the reason it exists: every other
/// tangency fixture in this suite selects slot 0, so an inverted `at_end`
/// mapping (`if at_end { 0 } else { 1 }`) would leave the whole suite green. Here
/// the line meets the arc at its `end` point, so an inverted mapping constrains
/// the arc's *start* against this line instead — a condition the solver can
/// satisfy by swinging `start` around the circle, leaving `end` at its seed and
/// this assertion reading the seed's direction cosine of ≈ 0.83 rather than 0.
///
/// That the emitted `other` really is 1 and not 0 is pinned separately by
/// `with_other_sets_only_the_endpoint_selectors` in `slvs_sys.rs`; what this
/// test adds is the link from the Rust `bool` to slvs' meaning of that number.
#[test]
fn arc_line_tangent_at_the_arc_end_selects_the_other_endpoint() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    // Declared start-then-end as always; it is `end` that the line touches.
    let start = s.point(0.0, 0.005);
    let end = s.point(0.005, 0.0);
    let arc = s.arc(c, start, end);
    let far = s.point(0.020, 0.010);
    let line = s.line(end, far);

    s.constrain(SketchConstraint::Fix(c));
    s.constrain(SketchConstraint::Fix(far));
    s.constrain(SketchConstraint::Distance {
        a: c,
        b: end,
        value: 0.005,
    });
    s.constrain(SketchConstraint::ArcLineTangent {
        arc,
        line,
        at_end: true,
    });

    let result = reify_constraints::solve_sketch(s.system());
    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "a line tangent at the arc's end is solvable, got {result:?}"
    );

    let (center, _, arc_end) = arc_of(&result, arc);
    let far_end = point_of(&result, far);
    assert_point_near(center, (0.0, 0.0), "anchored arc centre");
    assert_point_near(far_end, (0.020, 0.010), "anchored far endpoint");
    assert_near(
        dist(center, arc_end),
        0.005,
        "dimensioned arc radius at the end",
    );
    assert_point_near(
        line_of(&result, line).0,
        arc_end,
        "line start vs the arc's end point",
    );

    let radius_dir = unit(delta(center, arc_end));
    let line_dir = unit(delta(arc_end, far_end));
    assert_near(
        dot(radius_dir, line_dir),
        0.0,
        "direction cosine between the radius at the arc's END and the line",
    );
}

/// `CurveCurveTangent` puts both centres and the shared point on one line.
///
/// Two arcs touch tangentially exactly when their radius vectors at the touch
/// point are collinear, which in the plane is the cross product asserted here.
/// The touch point itself is held by a separate `Coincident`: the tangency
/// constraint only fixes the *directions*, so without the coincidence the two
/// arcs would be parallel-radius'd but apart.
///
/// Only arc A is dimensioned — B's radius follows from the shared point — so the
/// fixture cannot be satisfied by a lucky pair of consistent dimensions.
#[test]
fn curve_curve_tangent_lines_up_both_centres_with_the_shared_point() {
    let mut s = Sketch::new();
    let c1 = s.point(0.0, 0.0);
    let a_start = s.point(0.005, 0.001);
    let a_end = s.point(0.0, 0.005);
    let arc_a = s.arc(c1, a_start, a_end);

    let c2 = s.point(0.015, 0.0);
    // Seeded apart from arc A's start, and off the centre line, so both the
    // coincidence and the collinearity have to be driven rather than inherited.
    let b_start = s.point(0.0055, 0.0012);
    let b_end = s.point(0.015, 0.010);
    let arc_b = s.arc(c2, b_start, b_end);

    s.constrain(SketchConstraint::Fix(c1));
    s.constrain(SketchConstraint::Fix(c2));
    s.constrain(SketchConstraint::Coincident {
        a: a_start,
        b: b_start,
    });
    s.constrain(SketchConstraint::Distance {
        a: c1,
        b: a_start,
        value: 0.005,
    });
    s.constrain(SketchConstraint::CurveCurveTangent {
        a: arc_a,
        a_at_end: false,
        b: arc_b,
        b_at_end: false,
    });

    let result = reify_constraints::solve_sketch(s.system());
    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "two tangent arcs are solvable, got {result:?}"
    );

    let centre_a = point_of(&result, c1);
    let centre_b = point_of(&result, c2);
    let touch = point_of(&result, a_start);
    assert_point_near(centre_a, (0.0, 0.0), "anchored centre A");
    assert_point_near(centre_b, (0.015, 0.0), "anchored centre B");
    assert_point_near(point_of(&result, b_start), touch, "the shared touch point");
    assert_near(dist(centre_a, touch), 0.005, "dimensioned arc A radius");

    assert_near(
        cross(unit(delta(centre_a, touch)), unit(delta(touch, centre_b))),
        0.0,
        "collinearity of centre A, the touch point and centre B",
    );
}

/// Both of `CurveCurveTangent`'s selectors, set to the arc `end` slot.
///
/// The two-curve twin of `arc_line_tangent_at_the_arc_end_selects_the_other_endpoint`,
/// and the only fixture that drives `other2` off zero. The arcs meet at their
/// `end` points; under an inverted mapping the tangency would instead line up
/// their *start* points, leaving the touch point at its seed — well off the
/// centre line (seed cross product ≈ 0.79) and nowhere near the collinearity
/// asserted here.
#[test]
fn curve_curve_tangent_at_both_arc_ends_selects_the_other_endpoints() {
    let mut s = Sketch::new();
    let c1 = s.point(0.0, 0.0);
    let a_start = s.point(0.0, 0.005);
    // 5 mm from c1 already, so the dimension is satisfied at the seed and it is
    // the tangency alone that has to drive the touch point onto the centre line.
    let a_end = s.point(0.004, 0.003);
    let arc_a = s.arc(c1, a_start, a_end);

    let c2 = s.point(0.015, 0.0);
    let b_start = s.point(0.015, 0.010);
    // Seeded apart from arc A's end, so the coincidence has to be driven too.
    let b_end = s.point(0.0045, 0.0035);
    let arc_b = s.arc(c2, b_start, b_end);

    s.constrain(SketchConstraint::Fix(c1));
    s.constrain(SketchConstraint::Fix(c2));
    s.constrain(SketchConstraint::Coincident { a: a_end, b: b_end });
    s.constrain(SketchConstraint::Distance {
        a: c1,
        b: a_end,
        value: 0.005,
    });
    s.constrain(SketchConstraint::CurveCurveTangent {
        a: arc_a,
        a_at_end: true,
        b: arc_b,
        b_at_end: true,
    });

    let result = reify_constraints::solve_sketch(s.system());
    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "two arcs tangent at their end points are solvable, got {result:?}"
    );

    let centre_a = point_of(&result, c1);
    let centre_b = point_of(&result, c2);
    let touch = point_of(&result, a_end);
    assert_point_near(centre_a, (0.0, 0.0), "anchored centre A");
    assert_point_near(centre_b, (0.015, 0.0), "anchored centre B");
    assert_point_near(point_of(&result, b_end), touch, "the shared touch point");
    assert_near(dist(centre_a, touch), 0.005, "dimensioned arc A radius");

    assert_near(
        cross(unit(delta(centre_a, touch)), unit(delta(touch, centre_b))),
        0.0,
        "collinearity of centre A, the touch point at both arcs' ENDS, and centre B",
    );
}

// ---------------------------------------------------------------------------
// The remaining 2D relations
// ---------------------------------------------------------------------------

/// `PtOnLine` drops a point onto its line and leaves it free to slide.
///
/// Both facts are asserted, because each alone is satisfiable the wrong way:
/// `y == 0` without the DOF check is also what a mapping that anchored the
/// point would produce, and `dof == 1` without the position check is what
/// dropping the constraint produces. Together they say "one equation, and it is
/// the right one".
///
/// The remaining degree of freedom is the point's position *along* the line —
/// `PtOnLine` says the point is on the line's infinite extension, not where.
/// Where it actually lands is deliberately not asserted: libslvs resolves a
/// leftover DOF itself rather than leaving the free param at its seed (measured:
/// this fixture's point slides from x = 8 mm to x ≈ 3e-6 m), and pinning that
/// choice would be a test of solver internals, not of the mapping.
#[test]
fn point_on_line_lands_on_the_line_and_keeps_one_dof() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.020, 0.0);
    let rail = s.line(a, b);
    // Seeded off the line in y, and short of both endpoints in x.
    let p = s.point(0.008, 0.003);

    // `Fix` on a line anchors both of its endpoints — one declaration, two slvs
    // constraints — which is the expansion the attribution map has to survive.
    s.constrain(SketchConstraint::Fix(rail));
    s.constrain(SketchConstraint::PtOnLine { pt: p, line: rail });

    let result = reify_constraints::solve_sketch(s.system());

    let (start, end) = line_of(&result, rail);
    assert_point_near(start, (0.0, 0.0), "anchored line start");
    assert_point_near(end, (0.020, 0.0), "anchored line end");

    let (_, py) = point_of(&result, p);
    assert_near(py, 0.0, "constrained point's distance off the y = 0 rail");
    assert_eq!(
        dof_of(&result),
        1,
        "a point on a fixed line keeps exactly one DOF — where along it it sits"
    );
}

/// `AtMidpoint` puts a point at the middle of a fixed line, with nothing left
/// over.
///
/// `dof == 0` is the load-bearing half: a midpoint is two equations, so a
/// mapping that emitted only one would still land the point *somewhere*
/// plausible while leaving a degree of freedom behind.
#[test]
fn at_midpoint_centres_the_point_with_no_dof_left() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.020, 0.0);
    let span = s.line(a, b);
    // Seeded off centre in both axes.
    let m = s.point(0.006, 0.002);

    s.constrain(SketchConstraint::Fix(span));
    s.constrain(SketchConstraint::AtMidpoint { pt: m, line: span });

    let result = reify_constraints::solve_sketch(s.system());

    assert_point_near(point_of(&result, m), (0.010, 0.0), "the line's midpoint");
    assert_eq!(
        dof_of(&result),
        0,
        "a midpoint on a fixed line is fully determined"
    );
}

/// `SymmetricLine` mirrors a point across a line.
///
/// The mirror image is *derived*, not dimensioned: nothing in the fixture names
/// `(+4 mm, +4 mm)`, so reading it back is only possible if both halves of the
/// constraint — the pair straddles the centreline, and the segment joining them
/// crosses it square — actually reached libslvs.
#[test]
fn symmetric_line_mirrors_a_point_across_the_centreline() {
    let mut s = Sketch::new();
    // A vertical centreline through the origin.
    let c0 = s.point(0.0, 0.0);
    let c1 = s.point(0.0, 0.020);
    let centreline = s.line(c0, c1);

    let a = s.point(-0.004, 0.004);
    // Seeded on the wrong side of the centreline entirely.
    let b = s.point(0.001, -0.002);

    s.constrain(SketchConstraint::Fix(centreline));
    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::SymmetricLine {
        a,
        b,
        about: centreline,
    });

    let result = reify_constraints::solve_sketch(s.system());

    assert_point_near(point_of(&result, a), (-0.004, 0.004), "the anchored point");
    assert_point_near(point_of(&result, b), (0.004, 0.004), "its mirror image");
}

/// `EqualLengthLines` propagates one line's length to another.
///
/// Length only: the second line is free to point wherever the rest of the
/// fixture leaves it, which is why the assertion is on its length rather than on
/// its far endpoint. It is seeded at 3 mm, so 10 mm has to be driven.
#[test]
fn equal_length_lines_propagates_a_single_length() {
    let mut s = Sketch::new();
    let a0 = s.point(0.0, 0.0);
    let a1 = s.point(0.010, 0.0);
    let driver = s.line(a0, a1);

    let b0 = s.point(0.020, 0.0);
    let b1 = s.point(0.020, 0.003);
    let follower = s.line(b0, b1);

    s.constrain(SketchConstraint::Fix(driver));
    s.constrain(SketchConstraint::Fix(b0));
    s.constrain(SketchConstraint::Vertical(follower));
    s.constrain(SketchConstraint::EqualLengthLines {
        a: driver,
        b: follower,
    });

    let result = reify_constraints::solve_sketch(s.system());

    let (d_start, d_end) = line_of(&result, driver);
    let (f_start, f_end) = line_of(&result, follower);
    assert_near(dist(d_start, d_end), 0.010, "the anchored driving length");
    assert_near(dist(f_start, f_end), 0.010, "the length equated to it");
}

// ---------------------------------------------------------------------------
// Failing-constraint attribution
// ---------------------------------------------------------------------------

/// The failing set, or a descriptive panic naming the actual arm.
fn failing(result: &SketchSolveResult) -> &Vec<(SketchConstraintId, SourceSpan)> {
    match result {
        SketchSolveResult::Inconsistent { failing } => failing,
        other => panic!("expected SketchSolveResult::Inconsistent, got {other:?}"),
    }
}

/// An anchored vertical segment told to be both 10 mm and 20 mm long.
///
/// Returns the sketch plus the ids of the two constraints that contradict each
/// other, so a test can name the culprits it expects without re-deriving them.
/// The two anchoring constraints are *not* in conflict: they are there to make
/// the contradiction reachable, and a failing set that named them would be
/// attributing the failure to the wrong declarations.
fn contradictory_dimensions() -> (Sketch, SketchConstraintId, SketchConstraintId) {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.0, 0.010);
    let segment = s.line(a, b);

    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::Vertical(segment));
    let ten = s.constrain(SketchConstraint::Distance { a, b, value: 0.010 });
    let twenty = s.constrain(SketchConstraint::Distance { a, b, value: 0.020 });

    (s, ten, twenty)
}

/// An over-constrained sketch names the declarations that contradict, each with
/// its own span.
///
/// This is the difference between a diagnostic that can point at source and one
/// that can only say "something is over-constrained": a bare count is not
/// attributable, and neither is a set of synthetic ids. Every pair that comes
/// back has to resolve to a constraint the caller actually declared, carrying
/// the span that caller wrote it at.
///
/// The span check is what makes this more than a plausibility test — the fixture
/// gives every constraint a distinct span, so a failing set that paired the
/// right ids with the wrong spans (an off-by-one in the reverse map, say) fails
/// here rather than reading as correct.
#[test]
fn contradictory_dimensions_resolve_to_their_own_constraints() {
    let (s, ten, twenty) = contradictory_dimensions();

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Inconsistent { .. }),
        "10 mm and 20 mm cannot both hold, got {result:?}"
    );

    let failing = failing(&result);
    assert!(
        !failing.is_empty(),
        "an inconsistent solve must name what failed"
    );

    // Every pair resolves to a constraint that is really in the input, with
    // really that constraint's span.
    for (id, span) in failing {
        let def = s
            .system()
            .constraints
            .iter()
            .find(|d| d.id == *id)
            .unwrap_or_else(|| {
                panic!("failing set names {id:?}, which is not a constraint in the input sketch")
            });
        assert_eq!(
            *span, def.span,
            "{id:?} came back paired with a span that is not its own"
        );
    }

    let named: Vec<SketchConstraintId> = failing.iter().map(|(id, _)| *id).collect();
    assert!(
        named.contains(&ten) && named.contains(&twenty),
        "both contradictory dimensions must be named, got {named:?} \
         (expected to contain {ten:?} and {twenty:?})"
    );

    // Distinct ids carry distinct spans, so the two culprits cannot have been
    // handed the same one.
    assert_ne!(
        span_for(ten),
        span_for(twenty),
        "the fixture itself must give the two culprits different spans"
    );
}

/// The failing set comes back sorted by constraint id, identically every time.
///
/// Ordering is part of the contract, not an accident of iteration: a diagnostic
/// whose constraint list reshuffles between runs makes its own output
/// unreviewable and any test over it flaky.
///
/// Run-to-run equality alone does *not* pin that contract — libslvs is itself
/// deterministic, so two runs of one input agree whatever order the resolver
/// emits, and this test would pass unchanged with the sort deleted. The
/// ascending-by-id assertion is the half that fails if it is: it pins the
/// documented order rather than mere reproducibility.
#[test]
fn the_failing_set_is_ordered_deterministically() {
    let (s, _, _) = contradictory_dimensions();

    let first = reify_constraints::solve_sketch(s.system());
    let second = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        failing(&first),
        failing(&second),
        "the failing set must be identical across identical solves"
    );

    let failing = failing(&first);
    assert!(
        failing.windows(2).all(|w| w[0].0 < w[1].0),
        "the failing set must be sorted ascending by constraint id — and strictly, \
         since it is deduplicated — got {failing:?}"
    );
}

/// A failing `Fix` on a *line* resolves to that one declaration, named once.
///
/// `Fix` is the only declaration that expands into more than one slvs
/// constraint: on a line it anchors both endpoints, so two `SLVS_C_WHERE_DRAGGED`
/// handles both map back to a single `fix(...)`. The other over-constrained
/// fixture in this file anchors a bare *point*, which expands to exactly one
/// handle — so until here, nothing checked that a handle belonging to a
/// multi-handle declaration is attributed to the declaration rather than to the
/// anonymous anchor libslvs actually reported.
///
/// The contradiction: the segment is seeded 10 mm long and off-horizontal, then
/// told to be horizontal, 20 mm long, and pinned at its seed all at once.
///
/// On multiplicity, one measured fact bounds what this fixture can prove: across
/// every arrangement probed against the linked libslvs, its failing report names
/// **at most one handle per declaration** — with two point anchors in place of
/// the line `Fix`, only the second is reported. So the raw set here never
/// contains the same declaration twice, and `resolve_failing`'s dedup is not
/// exercised end to end. That collapse is pinned directly instead, by
/// `resolve_failing_collapses_one_declarations_many_handles` in `sketch.rs`.
/// The "exactly once" assertion below is kept as the regression guard it can
/// honestly be: it fails if the emit path ever starts attributing both anchors
/// and the dedup is dropped.
#[test]
fn a_failing_fix_on_a_line_resolves_to_that_declaration_once() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    // Off-horizontal and 10 mm-ish, so the horizontal and the dimension both
    // have to move something the anchor refuses to let move.
    let b = s.point(0.010, 0.004);
    let segment = s.line(a, b);

    s.constrain(SketchConstraint::Distance { a, b, value: 0.020 });
    s.constrain(SketchConstraint::Horizontal(segment));
    // Anchors BOTH endpoints: one declaration, two SLVS_C_WHERE_DRAGGED.
    let anchor = s.constrain(SketchConstraint::Fix(segment));

    let result = reify_constraints::solve_sketch(s.system());
    assert!(
        matches!(result, SketchSolveResult::Inconsistent { .. }),
        "a segment cannot be pinned at its seed and also horizontal and 20 mm, \
         got {result:?}"
    );

    let failing = failing(&result);
    let named: Vec<SketchConstraintId> = failing.iter().map(|(id, _)| *id).collect();
    assert!(
        named.contains(&anchor),
        "the line anchor is one of the contradicting declarations — its handle \
         must resolve to the `fix(...)` the author wrote, got {named:?}"
    );

    assert_eq!(
        named.iter().filter(|id| **id == anchor).count(),
        1,
        "a Fix on a line is one declaration however many anchors it lowered to, \
         got {named:?}"
    );

    // Not just the anchor: no declaration may repeat, since each is one thing the
    // author wrote.
    let mut unique = named.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        named.len(),
        "the failing set must name each declaration at most once, got {named:?}"
    );

    // Every id still carries its own span — the multi-handle declaration
    // included, which is the half an off-by-one in the reverse map would break.
    for (id, span) in failing {
        assert_eq!(
            *span,
            span_for(*id),
            "{id:?} came back paired with a span that is not its own"
        );
    }
}

// ---------------------------------------------------------------------------
// Malformed input: typed rejection instead of fabricated geometry
// ---------------------------------------------------------------------------
//
// `reify-constraints` is a crate boundary, so it defends itself: a malformed
// `SketchSystem` is rejected here regardless of what the language front end does
// or does not reject at compile time (INV-SF-1, INV-SF-5).
//
// What every fixture below produced before this contract existed was measured,
// not assumed, and it was the same failure mode each time — a *successful*
// `Solved`, with the offending declaration silently dropped:
//
// - a constraint on an undeclared entity  => `Solved`, dof 2, constraint gone
// - `Diameter` on a line / `Horizontal` on a point => `Solved`, dof 4, both gone
// - a duplicate entity id => `Solved` listing that id twice, both entries
//   carrying the *second* declaration's coordinates — geometry the caller never
//   wrote
// - a line whose endpoint names a circle => `Solved`, dof 5, the line itself
//   absent from the readback
//
// A caller cannot tell any of those apart from a real answer, which is why the
// assertion here is on a typed error and not merely on "did not converge".

/// The build error, or a descriptive panic naming the actual arm.
fn build_error(result: &SketchSolveResult) -> &SketchBuildError {
    match result {
        SketchSolveResult::Malformed(err) => err,
        other => panic!("expected SketchSolveResult::Malformed, got {other:?}"),
    }
}

/// A constraint naming an entity the sketch never declares is rejected, and the
/// error names both the dangling id and the constraint that reached for it.
#[test]
fn a_constraint_on_an_undeclared_entity_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    // Never declared: `Sketch` hands out ids from 1, so 999 cannot collide.
    let ghost = SketchEntityId(999);
    let cid = s.constrain(SketchConstraint::Coincident { a, b: ghost });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::UnknownEntity {
            referenced_by: cid,
            entity: ghost,
            span: span_for(cid),
        },
        "a dangling constraint operand must name the id and the declaration \
         that referenced it, with that declaration's own span"
    );
}

/// A dimension aimed at a line is rejected: `Diameter` is the radius family, and
/// a line has no radius.
///
/// The `expected`/`found` pair is what makes this renderable as a diagnostic
/// without re-deriving the rule at the consumer.
#[test]
fn a_diameter_on_a_line_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.010, 0.0);
    let segment = s.line(a, b);
    let cid = s.constrain(SketchConstraint::Diameter {
        circle: segment,
        value: 0.010,
    });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::WrongEntityKind {
            constraint: cid,
            entity: segment,
            expected: SketchSlotKind::Curve,
            found: SketchEntityKind::Line,
            span: span_for(cid),
        },
        "the radius family takes a circle or an arc, never a line"
    );
}

/// An orientation constraint aimed at a point is rejected: `Horizontal`
/// constrains a direction, and a point has none.
#[test]
fn a_horizontal_on_a_point_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let cid = s.constrain(SketchConstraint::Horizontal(a));

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::WrongEntityKind {
            constraint: cid,
            entity: a,
            expected: SketchSlotKind::Line,
            found: SketchEntityKind::Point,
            span: span_for(cid),
        },
        "`Horizontal` orients a line, so a point in that slot is a kind error"
    );
}

/// A point-to-point relation aimed at a circle is rejected.
///
/// The mirror image of the two above — the slot wants a point and was handed a
/// curve — which is what pins that the check reads the *slot's* requirement
/// rather than just noticing any mismatch.
#[test]
fn a_coincidence_on_a_circle_is_rejected() {
    let mut s = Sketch::new();
    let center = s.point(0.0, 0.0);
    let circle = s.circle(center, 0.004);
    let free = s.point(0.010, 0.0);
    let cid = s.constrain(SketchConstraint::Coincident { a: free, b: circle });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::WrongEntityKind {
            constraint: cid,
            entity: circle,
            expected: SketchSlotKind::Point,
            found: SketchEntityKind::Circle,
            span: span_for(cid),
        },
        "`Coincident` relates two points; a circle in either slot is a kind error"
    );
}

/// Declaring the same entity id twice is rejected.
///
/// Not a pedantic uniqueness rule: ids are how everything else in the system
/// refers to geometry, so a repeated id makes every reference to it ambiguous.
/// The measured pre-contract behaviour was the concrete harm — a readback that
/// listed the id twice with the *second* declaration's coordinates in both
/// entries, silently discarding the first.
#[test]
fn a_duplicate_entity_id_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    s.entity_with_id(a, SketchEntity::Point { x: 0.005, y: 0.005 });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::DuplicateEntity { entity: a },
        "an id declared twice makes every reference to it ambiguous"
    );
}

/// A duplicate *constraint* id is rejected, for the mirror-image reason a
/// duplicate entity id is.
///
/// Not a symmetry-for-its-own-sake check: a constraint id is what the failing
/// set is keyed and deduplicated by. Two declarations sharing one id both emit
/// and both land in the attribution map, so an inconsistent solve naming both
/// collapses them into a single entry carrying whichever span survived the
/// dedup — one real culprit vanishes from the diagnostic, and the survivor can
/// be reported against the other declaration's span. That is the constraint-side
/// twin of what `a_duplicate_entity_id_is_rejected` prevents.
#[test]
fn a_duplicate_constraint_id_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.0, 0.010);
    let first = s.constrain(SketchConstraint::Distance { a, b, value: 0.010 });
    // Same id, different relation and — via `span_for` — a span the first
    // declaration would be mis-attributed to.
    s.constrain_with_id(first, SketchConstraint::Distance { a, b, value: 0.020 });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::DuplicateConstraint { constraint: first },
        "an id declared twice makes the failing set ambiguous"
    );
}

/// A non-finite seed coordinate is rejected rather than handed to the solver.
///
/// Measured before this contract existed: `Slvs_Solve` accepts NaN params and
/// returns `SLVS_RESULT_OKAY` with NaN still in them, which reads back as a
/// fully-formed `Solved`. A consumer cannot tell that apart from real geometry,
/// which is the same failure mode as a silently dropped declaration.
#[test]
fn a_non_finite_point_coordinate_is_rejected() {
    let mut s = Sketch::new();
    s.point(0.0, 0.0);
    let bad = s.point(0.005, f64::NAN);

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::NonFiniteEntityValue {
            entity: bad,
            field: SketchValueField::Y,
        },
        "a NaN seed must be refused, and the error must name which coordinate"
    );
}

/// Infinity is refused on the same footing as NaN, and on a circle's radius as
/// well as a point's coordinates.
///
/// Two axes in one fixture on purpose: the check is `is_finite`, not `is_nan`,
/// and it covers every literal in the entity vocabulary rather than just the one
/// that happened to be tested first.
#[test]
fn an_infinite_circle_radius_is_rejected() {
    let mut s = Sketch::new();
    let center = s.point(0.0, 0.0);
    let bad = s.circle(center, f64::INFINITY);

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::NonFiniteEntityValue {
            entity: bad,
            field: SketchValueField::Radius,
        },
        "an infinite radius is as unsolvable as a NaN one"
    );
}

/// A non-finite *dimension* is rejected, and the error carries the declaring
/// constraint's span.
///
/// The span is what makes this renderable: unlike an entity seed, a dimension is
/// something the author wrote at a place, and the whole point of the
/// constraint-side variant is that it can point there.
#[test]
fn a_non_finite_dimension_is_rejected_with_its_span() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.010, 0.0);
    let bad = s.constrain(SketchConstraint::Distance {
        a,
        b,
        value: f64::NAN,
    });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::NonFiniteConstraintValue {
            constraint: bad,
            field: SketchValueField::Value,
            span: span_for(bad),
        },
        "a NaN dimension must be refused, with the span it was declared at"
    );
}

/// A line whose two endpoint slots name the same point is rejected.
///
/// Structurally degenerate, not merely badly seeded: the two slots resolve to one
/// pair of params, so the segment has no direction for `Horizontal` to constrain
/// and no length for a dimension to drive — and no constraint can ever separate
/// them, because there is nothing to separate.
///
/// `Fix` on such a line compounds it, emitting two identical
/// `SLVS_C_WHERE_DRAGGED` on the same point.
#[test]
fn a_line_between_one_point_and_itself_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let segment = s.entity(SketchEntity::Line { start: a, end: a });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::DegenerateEntity {
            owner: segment,
            repeated: a,
        },
        "a line needs two distinct endpoints to have a direction at all"
    );
}

/// An arc whose centre is also one of its endpoints is rejected: zero radius,
/// and libslvs' implicit `|c-s| = |c-e|` becomes `0 = |c-e|`.
#[test]
fn an_arc_centred_on_its_own_endpoint_is_rejected() {
    let mut s = Sketch::new();
    let c = s.point(0.0, 0.0);
    let end = s.point(0.005, 0.0);
    let arc = s.entity(SketchEntity::Arc {
        center: c,
        start: c,
        end,
    });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::DegenerateEntity {
            owner: arc,
            repeated: c,
        },
        "an arc whose centre is its own start point has no radius"
    );
}

/// Two *distinct* points seeded at the same coordinates are NOT degenerate.
///
/// The guard against over-correcting the structural check above into a numeric
/// one. Seeds are provisional by contract — the solver moves them — so
/// coincident starting coordinates are an ordinary starting state and a
/// constraint drives the two points apart from there. Only structural
/// degeneracy, where one declaration names the same entity in two slots and no
/// constraint could ever separate them, is refused.
///
/// The separating constraint is a `Coincident` onto an anchored third point
/// rather than a `Distance` between the pair: a distance equation's gradient is
/// `(b - a) / |b - a|`, which at coincident seeds is a division by zero and
/// leaves libslvs unable to converge for numerical reasons that have nothing to
/// do with validation (measured: `Inconsistent`). A coincidence is linear in the
/// params, so it drives the pair apart from a standing start and keeps this test
/// about what it claims to be about.
#[test]
fn distinct_points_seeded_at_the_same_place_are_not_degenerate() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.0, 0.0);
    let target = s.point(0.010, 0.0);
    let segment = s.line(a, b);

    s.constrain(SketchConstraint::Fix(a));
    s.constrain(SketchConstraint::Fix(target));
    s.constrain(SketchConstraint::Coincident { a: b, b: target });

    let result = reify_constraints::solve_sketch(s.system());

    assert!(
        matches!(result, SketchSolveResult::Solved { .. }),
        "coincident seeds are a starting state, not a malformed declaration, \
         got {result:?}"
    );
    assert_near(
        dist(point_of(&result, a), point_of(&result, b)),
        0.010,
        "the coincidence drives the two coincident seeds apart",
    );
    // And the line built on them is real geometry once they separate.
    let (start, end) = line_of(&result, segment);
    assert_point_near(start, (0.0, 0.0), "anchored line start");
    assert_point_near(end, (0.010, 0.0), "driven line end");
}

/// A line whose endpoint slot names a circle is rejected.
///
/// This is the entity-level twin of the constraint kind check: a composite's
/// defining slots are as typed as a constraint's, and `owner`/`referenced` name
/// both halves so the diagnostic can point at the line, not just at the circle.
#[test]
fn a_line_endpoint_naming_a_circle_is_rejected() {
    let mut s = Sketch::new();
    let center = s.point(0.0, 0.0);
    let circle = s.circle(center, 0.004);
    let far = s.point(0.010, 0.0);
    let segment = s.line(circle, far);

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::BadEntityRef {
            owner: segment,
            referenced: circle,
            expected: SketchSlotKind::Point,
            found: Some(SketchEntityKind::Circle),
        },
        "a line is defined by two points; a circle in an endpoint slot is a kind error"
    );
}

/// A line whose endpoint slot names nothing at all is rejected too.
///
/// The dangling case reports `found: None` rather than being folded into
/// `UnknownEntity`: that variant names the *constraint* that reached for the
/// missing id, and an entity's defining slot has no constraint to name.
#[test]
fn a_line_endpoint_naming_an_undeclared_entity_is_rejected() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let ghost = SketchEntityId(999);
    let segment = s.line(a, ghost);

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::BadEntityRef {
            owner: segment,
            referenced: ghost,
            expected: SketchSlotKind::Point,
            found: None,
        },
        "an endpoint slot naming an undeclared id must say so, not be skipped"
    );
}

/// With more than one malformed declaration, the one reported is the first in
/// declaration order.
///
/// Which error surfaces is part of the contract, not an accident of iteration:
/// the same input has to produce the same diagnostic every run (O9), and a
/// consumer that fixes the reported error and re-solves needs the sequence to
/// converge rather than to shuffle.
#[test]
fn the_first_malformed_declaration_in_order_is_reported() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let b = s.point(0.010, 0.0);
    let segment = s.line(a, b);
    let first = s.constrain(SketchConstraint::Horizontal(a));
    s.constrain(SketchConstraint::Diameter {
        circle: segment,
        value: 0.010,
    });

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::WrongEntityKind {
            constraint: first,
            entity: a,
            expected: SketchSlotKind::Line,
            found: SketchEntityKind::Point,
            span: span_for(first),
        },
        "the earlier-declared error must be the reported one"
    );

    let repeat = reify_constraints::solve_sketch(s.system());
    assert_eq!(
        build_error(&result),
        build_error(&repeat),
        "the reported error must be identical across identical solves"
    );
}

/// Entity-level errors are reported before constraint-level ones.
///
/// Ordering across the two passes is deliberate: a constraint naming an entity
/// whose own declaration is malformed would otherwise produce a second,
/// derivative error about a slot that was never well-defined to begin with.
#[test]
fn a_malformed_entity_is_reported_before_a_malformed_constraint() {
    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let ghost = SketchEntityId(999);
    // Declared second, so a purely positional rule would report the constraint.
    s.constrain(SketchConstraint::Horizontal(a));
    let segment = s.line(a, ghost);

    let result = reify_constraints::solve_sketch(s.system());

    assert_eq!(
        build_error(&result),
        &SketchBuildError::BadEntityRef {
            owner: segment,
            referenced: ghost,
            expected: SketchSlotKind::Point,
            found: None,
        },
        "the entity pass runs first, so a broken entity outranks a broken constraint"
    );
}

/// `SketchBuildError` is a real `std::error::Error` whose rendering names the
/// ids involved.
///
/// The structured fields are the contract — a consumer renders its own
/// span-bearing diagnostic and never scrapes this string — but the error still
/// has to be usable in a plain `Box<dyn Error>` log line without printing
/// something opaque.
///
/// Both ids are deliberately distinctive three-digit numbers rather than the
/// allocator's 1, 2, 3…: `contains("1")` is satisfied by almost any sentence
/// mentioning almost any number, so an assertion on a small id would still pass
/// with the id dropped from the message entirely. `771` and `999` appear in the
/// rendering only if they were really put there.
#[test]
fn build_errors_are_std_errors_that_name_their_ids() {
    fn assert_is_std_error<E: std::error::Error>(_: &E) {}

    let mut s = Sketch::new();
    let a = s.point(0.0, 0.0);
    let ghost = SketchEntityId(999);
    let cid = s.constrain_with_id(
        SketchConstraintId(771),
        SketchConstraint::Coincident { a, b: ghost },
    );

    let result = reify_constraints::solve_sketch(s.system());
    let err = build_error(&result);
    assert_is_std_error(err);

    let rendered = err.to_string();
    assert!(
        rendered.contains("999"),
        "the rendering must name the offending entity id, got: {rendered}"
    );
    assert!(
        rendered.contains("771"),
        "the rendering must name the constraint that referenced it, got: {rendered}"
    );
    assert_eq!(cid, SketchConstraintId(771));
}
