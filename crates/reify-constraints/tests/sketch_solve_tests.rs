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
