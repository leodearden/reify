//! Characterization tests for the sketch slot table: which kind of entity each
//! declaration slot accepts, the order in which a malformed declaration's
//! problems are reported, and the routing of every operand to its own entity.
//!
//! These pin behaviour that `sketch_solve_tests.rs` leaves implicit: a
//! composite declared before its own points, a curve slot handed an arc, the
//! tangency slots refusing a full circle, and the premise order within one
//! declaration (literals, then reference kinds, then repeats). Everything goes
//! through the public `solve_sketch` seam against a real `Slvs_Solve`.

use reify_constraints::{
    SketchBuildError, SketchConstraint, SketchConstraintDef, SketchConstraintId, SketchEntity,
    SketchEntityDef, SketchEntityId, SketchEntityKind, SketchSlotKind, SketchSolveResult,
    SketchSystem, SketchValueField, SolvedSketchEntity,
};
use reify_core::SourceSpan;

/// Positional and radial tolerance in metres, the precedent
/// `sketch_solve_tests.rs` measured for this solver and workplane path.
const TOL: f64 = 1e-6;

/// A [`SketchSystem`] built with caller-chosen ids.
///
/// Explicit ids are what let a composite be declared before the points that
/// define it, and let an id differ from its declaration position.
#[derive(Default)]
struct Fixture {
    system: SketchSystem,
}

/// The span stamped on constraint `id`: distinct ids get disjoint spans.
fn span_for(id: SketchConstraintId) -> SourceSpan {
    let start = id.0 * 10;
    SourceSpan::new(start, start + 4)
}

impl Fixture {
    fn entity(&mut self, id: u32, entity: SketchEntity) -> SketchEntityId {
        let id = SketchEntityId(id);
        self.system.entities.push(SketchEntityDef {
            id,
            entity,
            aux: false,
        });
        id
    }

    fn point(&mut self, id: u32, x: f64, y: f64) -> SketchEntityId {
        self.entity(id, SketchEntity::Point { x, y })
    }

    fn line(&mut self, id: u32, start: SketchEntityId, end: SketchEntityId) -> SketchEntityId {
        self.entity(id, SketchEntity::Line { start, end })
    }

    fn constrain(&mut self, id: u32, constraint: SketchConstraint) -> SketchConstraintId {
        let id = SketchConstraintId(id);
        self.system.constraints.push(SketchConstraintDef {
            id,
            constraint,
            span: span_for(id),
        });
        id
    }

    fn solve(&self) -> SketchSolveResult {
        reify_constraints::solve_sketch(&self.system)
    }
}

/// The build error, or a descriptive panic naming the arm actually returned.
fn build_error(result: &SketchSolveResult) -> &SketchBuildError {
    match result {
        SketchSolveResult::Malformed(err) => err,
        other => panic!("expected SketchSolveResult::Malformed, got {other:?}"),
    }
}

fn solved(result: &SketchSolveResult) -> &[(SketchEntityId, SolvedSketchEntity)] {
    match result {
        SketchSolveResult::Solved { entities, .. } => entities,
        other => panic!("expected SketchSolveResult::Solved, got {other:?}"),
    }
}

fn dof_of(result: &SketchSolveResult) -> i32 {
    match result {
        SketchSolveResult::Solved { dof, .. } => *dof,
        other => panic!("expected SketchSolveResult::Solved to read dof, got {other:?}"),
    }
}

fn lookup(result: &SketchSolveResult, id: SketchEntityId) -> &SolvedSketchEntity {
    let entities = solved(result);
    match entities.iter().find(|(eid, _)| *eid == id) {
        Some((_, entity)) => entity,
        None => panic!("entity {id:?} is absent from the solved readback {entities:?}"),
    }
}

fn point_of(result: &SketchSolveResult, id: SketchEntityId) -> (f64, f64) {
    match lookup(result, id) {
        SolvedSketchEntity::Point { x, y } => (*x, *y),
        other => panic!("entity {id:?} is not a Point in the solved readback: {other:?}"),
    }
}

fn line_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), (f64, f64)) {
    match lookup(result, id) {
        SolvedSketchEntity::Line { start, end } => (*start, *end),
        other => panic!("entity {id:?} is not a Line in the solved readback: {other:?}"),
    }
}

fn circle_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), f64) {
    match lookup(result, id) {
        SolvedSketchEntity::Circle { center, radius } => (*center, *radius),
        other => panic!("entity {id:?} is not a Circle in the solved readback: {other:?}"),
    }
}

fn arc_of(result: &SketchSolveResult, id: SketchEntityId) -> ((f64, f64), (f64, f64), (f64, f64)) {
    match lookup(result, id) {
        SolvedSketchEntity::Arc { center, start, end } => (*center, *start, *end),
        other => panic!("entity {id:?} is not an Arc in the solved readback: {other:?}"),
    }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn unit_from(from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let n = dist(from, to);
    assert!(n > TOL, "degenerate direction from {from:?} to {to:?}");
    ((to.0 - from.0) / n, (to.1 - from.1) / n)
}

fn dot(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < TOL,
        "{what}: expected {expected}, got {actual}"
    );
}

fn assert_point_near(actual: (f64, f64), expected: (f64, f64), what: &str) {
    assert!(
        dist(actual, expected) < TOL,
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

// ---------------------------------------------------------------------------
// Premise order within one declaration, and between the passes
// ---------------------------------------------------------------------------

/// An arc naming its centre twice AND an undeclared endpoint is reported for
/// the undeclared endpoint: every slot's kind is settled before repeats are.
#[test]
fn an_entitys_reference_kinds_are_checked_before_its_repeats() {
    let mut f = Fixture::default();
    let c = f.point(1, 0.0, 0.0);
    let arc = f.entity(
        2,
        SketchEntity::Arc {
            center: c,
            start: c,
            end: SketchEntityId(999),
        },
    );

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::BadEntityRef {
            owner: arc,
            referenced: SketchEntityId(999),
            expected: SketchSlotKind::Point,
            found: None,
        }
    );
}

#[test]
fn an_entitys_literals_are_checked_before_its_references() {
    let mut f = Fixture::default();
    let circle = f.entity(
        1,
        SketchEntity::Circle {
            center: SketchEntityId(999),
            radius: f64::NAN,
        },
    );

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::NonFiniteEntityValue {
            entity: circle,
            field: SketchValueField::Radius,
        }
    );
}

#[test]
fn a_constraints_literals_are_checked_before_its_operands() {
    let mut f = Fixture::default();
    let p = f.point(1, 0.0, 0.0);
    let distance = f.constrain(
        7,
        SketchConstraint::Distance {
            a: SketchEntityId(999),
            b: p,
            value: f64::INFINITY,
        },
    );

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::NonFiniteConstraintValue {
            constraint: distance,
            field: SketchValueField::Value,
            span: span_for(distance),
        }
    );
}

/// Two wrong operands: the one written first (`b`) is the one reported, not the
/// centreline after it.
#[test]
fn a_constraints_operands_are_checked_in_written_order() {
    let mut f = Fixture::default();
    let p1 = f.point(1, 0.0, 0.0);
    let p2 = f.point(2, 0.010, 0.0);
    let centre = f.point(3, 0.0, 0.010);
    let circle = f.entity(
        4,
        SketchEntity::Circle {
            center: centre,
            radius: 0.003,
        },
    );
    let symmetric = f.constrain(
        5,
        SketchConstraint::SymmetricLine {
            a: p1,
            b: circle,
            about: p2,
        },
    );

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::WrongEntityKind {
            constraint: symmetric,
            entity: circle,
            expected: SketchSlotKind::Point,
            found: SketchEntityKind::Circle,
            span: span_for(symmetric),
        }
    );
}

/// Pass 1 (unique entity ids) runs over every entity before pass 2 screens any
/// entity's literals.
#[test]
fn a_duplicate_entity_id_outranks_an_earlier_entitys_literal_error() {
    let mut f = Fixture::default();
    f.point(1, f64::NAN, 0.0);
    f.point(1, 0.0, 0.0);

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::DuplicateEntity {
            entity: SketchEntityId(1),
        }
    );
}

/// Pass 3 (unique constraint ids) runs over every constraint before pass 4
/// checks any constraint's operands.
#[test]
fn a_duplicate_constraint_id_outranks_an_earlier_constraints_kind_error() {
    let mut f = Fixture::default();
    let p = f.point(1, 0.0, 0.0);
    f.constrain(1, SketchConstraint::Horizontal(p));
    f.constrain(1, SketchConstraint::Fix(p));

    assert_eq!(
        *build_error(&f.solve()),
        SketchBuildError::DuplicateConstraint {
            constraint: SketchConstraintId(1),
        }
    );
}

/// Naming one entity in two slots is refused for a composite *entity* only; a
/// constraint doing so is lowered. What libslvs then makes of it is not part of
/// this contract.
#[test]
fn a_constraint_naming_one_entity_twice_is_not_malformed() {
    let mut f = Fixture::default();
    let p = f.point(1, 0.0, 0.0);
    f.constrain(1, SketchConstraint::Coincident { a: p, b: p });

    let result = f.solve();
    assert!(
        !matches!(result, SketchSolveResult::Malformed(_)),
        "a constraint repeating an operand must reach the solver, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Slot widths
// ---------------------------------------------------------------------------

/// Every slot narrower than "any entity", each handed a kind just outside it.
#[test]
fn each_slot_refuses_the_kinds_outside_it() {
    struct Case {
        name: &'static str,
        constraint: fn(&Kinds) -> SketchConstraint,
        entity: fn(&Kinds) -> SketchEntityId,
        expected: SketchSlotKind,
        found: SketchEntityKind,
    }

    struct Kinds {
        point: SketchEntityId,
        line: SketchEntityId,
        circle: SketchEntityId,
        arc: SketchEntityId,
    }

    let cases = [
        Case {
            name: "Fix(circle)",
            constraint: |k| SketchConstraint::Fix(k.circle),
            entity: |k| k.circle,
            expected: SketchSlotKind::PointOrLine,
            found: SketchEntityKind::Circle,
        },
        Case {
            name: "ArcLineTangent { arc: circle }",
            constraint: |k| SketchConstraint::ArcLineTangent {
                arc: k.circle,
                line: k.line,
                at_end: false,
            },
            entity: |k| k.circle,
            expected: SketchSlotKind::Arc,
            found: SketchEntityKind::Circle,
        },
        Case {
            name: "CurveCurveTangent { a: arc, b: circle }",
            constraint: |k| SketchConstraint::CurveCurveTangent {
                a: k.arc,
                a_at_end: false,
                b: k.circle,
                b_at_end: false,
            },
            entity: |k| k.circle,
            expected: SketchSlotKind::Arc,
            found: SketchEntityKind::Circle,
        },
        Case {
            name: "PtOnCircle { circle: line }",
            constraint: |k| SketchConstraint::PtOnCircle {
                pt: k.point,
                circle: k.line,
            },
            entity: |k| k.line,
            expected: SketchSlotKind::Curve,
            found: SketchEntityKind::Line,
        },
        Case {
            name: "Angle { b: point }",
            constraint: |k| SketchConstraint::Angle {
                a: k.line,
                b: k.point,
                degrees: 30.0,
            },
            entity: |k| k.point,
            expected: SketchSlotKind::Line,
            found: SketchEntityKind::Point,
        },
    ];

    for case in &cases {
        let mut f = Fixture::default();
        let point = f.point(1, 0.0, 0.0);
        let far = f.point(2, 0.010, 0.0);
        let line = f.line(3, point, far);
        let centre = f.point(4, 0.0, 0.020);
        let circle = f.entity(
            5,
            SketchEntity::Circle {
                center: centre,
                radius: 0.003,
            },
        );
        let arc_centre = f.point(6, 0.020, 0.020);
        let arc_start = f.point(7, 0.025, 0.020);
        let arc_end = f.point(8, 0.020, 0.025);
        let arc = f.entity(
            9,
            SketchEntity::Arc {
                center: arc_centre,
                start: arc_start,
                end: arc_end,
            },
        );
        let kinds = Kinds {
            point,
            line,
            circle,
            arc,
        };
        let constraint = f.constrain(100, (case.constraint)(&kinds));

        assert_eq!(
            *build_error(&f.solve()),
            SketchBuildError::WrongEntityKind {
                constraint,
                entity: (case.entity)(&kinds),
                expected: case.expected,
                found: case.found,
                span: span_for(constraint),
            },
            "case {}",
            case.name
        );
    }
}

/// An arc whose centre is anchored and whose start and end are held to the
/// horizontal and vertical through it: everything but its radius is fixed.
struct SpokedArc {
    centre: SketchEntityId,
    arc: SketchEntityId,
}

/// Declares a [`SpokedArc`] centred at the origin, its endpoints seeded off
/// both spokes and off any radius the fixtures constrain.
fn spoked_arc(f: &mut Fixture) -> SpokedArc {
    let centre = f.point(1, 0.0, 0.0);
    let start = f.point(2, 0.003, 0.0005);
    let end = f.point(3, 0.0005, 0.007);
    let arc = f.entity(
        4,
        SketchEntity::Arc {
            center: centre,
            start,
            end,
        },
    );
    let horizontal = f.line(5, centre, start);
    let vertical = f.line(6, centre, end);
    f.constrain(1, SketchConstraint::Fix(centre));
    f.constrain(2, SketchConstraint::Horizontal(horizontal));
    f.constrain(3, SketchConstraint::Vertical(vertical));
    SpokedArc { centre, arc }
}

/// Asserts the spoked arc solved to radius `r`, start on +u and end on +v.
fn assert_spoked_arc_radius(result: &SketchSolveResult, spoked: &SpokedArc, r: f64) {
    let (centre, start, end) = arc_of(result, spoked.arc);
    assert_point_near(centre, point_of(result, spoked.centre), "arc centre");
    assert_point_near(centre, (0.0, 0.0), "anchored arc centre");
    assert_point_near(start, (r, 0.0), "arc start on the horizontal spoke");
    assert_point_near(end, (0.0, r), "arc end on the vertical spoke");
    assert_near(dist(centre, start), r, "|centre - start|");
}

#[test]
fn the_radius_slot_takes_an_arc() {
    let mut f = Fixture::default();
    let spoked = spoked_arc(&mut f);
    f.constrain(
        10,
        SketchConstraint::Radius {
            circle: spoked.arc,
            value: 0.005,
        },
    );

    let result = f.solve();

    assert_spoked_arc_radius(&result, &spoked, 0.005);
    assert_eq!(dof_of(&result), 0);
}

#[test]
fn the_point_on_circle_slot_takes_an_arc() {
    let mut f = Fixture::default();
    let spoked = spoked_arc(&mut f);
    f.constrain(
        10,
        SketchConstraint::Radius {
            circle: spoked.arc,
            value: 0.005,
        },
    );
    // Held to the horizontal through the centre, on the far side from the
    // arc's start, so the circumference is the only thing placing it.
    let p = f.point(20, -0.002, 0.0003);
    let to_p = f.line(21, spoked.centre, p);
    f.constrain(11, SketchConstraint::Horizontal(to_p));
    f.constrain(
        12,
        SketchConstraint::PtOnCircle {
            pt: p,
            circle: spoked.arc,
        },
    );

    let result = f.solve();

    assert_spoked_arc_radius(&result, &spoked, 0.005);
    assert_point_near(
        point_of(&result, p),
        (-0.005, 0.0),
        "point on the arc's circle",
    );
    assert_eq!(dof_of(&result), 0);
}

#[test]
fn the_equal_radius_slot_takes_an_arc() {
    let mut f = Fixture::default();
    let spoked = spoked_arc(&mut f);
    let k_centre = f.point(20, 0.030, 0.0);
    let circle = f.entity(
        21,
        SketchEntity::Circle {
            center: k_centre,
            radius: 0.003,
        },
    );
    f.constrain(10, SketchConstraint::Fix(k_centre));
    f.constrain(
        11,
        SketchConstraint::Radius {
            circle,
            value: 0.004,
        },
    );
    f.constrain(
        12,
        SketchConstraint::EqualRadius {
            a: circle,
            b: spoked.arc,
        },
    );

    let result = f.solve();

    assert_near(circle_of(&result, circle).1, 0.004, "dimensioned circle");
    assert_spoked_arc_radius(&result, &spoked, 0.004);
    assert_eq!(dof_of(&result), 0);
}

// ---------------------------------------------------------------------------
// Routing through per-kind tables
// ---------------------------------------------------------------------------

/// Every operand reaches its own entity when composites are declared before
/// their points and kinds are interleaved.
///
/// Declaration order is circle K, line L1, arc A, line L2, then the eight
/// points in an order unrelated to their use, with ids unrelated to either.
/// So no entity's position among its kind matches its position among the
/// curves, its declaration position, or its id. Each constraint's residual is
/// asserted on its own entity, and the two radii differ, so an operand routed
/// to a neighbour of the same or another kind shows up in the readback.
#[test]
fn every_operand_routes_to_its_own_entity_under_forward_declaration() {
    let k = SketchEntityId(50);
    let l1 = SketchEntityId(20);
    let a = SketchEntityId(70);
    let l2 = SketchEntityId(10);

    let l2_end = SketchEntityId(8);
    let a_centre = SketchEntityId(3);
    let k_centre = SketchEntityId(6);
    let l1_start = SketchEntityId(1);
    let a_end = SketchEntityId(5);
    let l2_start = SketchEntityId(2);
    let a_start = SketchEntityId(7);
    let l1_end = SketchEntityId(4);

    let mut f = Fixture::default();
    f.entity(
        k.0,
        SketchEntity::Circle {
            center: k_centre,
            radius: 0.003,
        },
    );
    f.line(l1.0, l1_start, l1_end);
    f.entity(
        a.0,
        SketchEntity::Arc {
            center: a_centre,
            start: a_start,
            end: a_end,
        },
    );
    f.line(l2.0, l2_start, l2_end);
    f.point(l2_end.0, 0.012, -0.028);
    f.point(a_centre.0, 0.040, 0.0);
    f.point(k_centre.0, -0.030, 0.0);
    f.point(l1_start.0, 0.0, 0.0);
    f.point(a_end.0, 0.036, 0.003);
    f.point(l2_start.0, 0.0, -0.030);
    f.point(a_start.0, 0.041, 0.006);
    f.point(l1_end.0, 0.010, 0.020);

    // L1: anchored whole, which must resolve both of its endpoints.
    f.constrain(31, SketchConstraint::Fix(l1));
    // L2: anchored start, horizontal, 10 mm long.
    f.constrain(32, SketchConstraint::Horizontal(l2));
    f.constrain(33, SketchConstraint::Fix(l2_start));
    f.constrain(
        34,
        SketchConstraint::Distance {
            a: l2_start,
            b: l2_end,
            value: 0.010,
        },
    );
    // K: anchored centre, 4 mm radius.
    f.constrain(35, SketchConstraint::Fix(k_centre));
    f.constrain(
        36,
        SketchConstraint::Radius {
            circle: k,
            value: 0.004,
        },
    );
    // A: anchored centre, 5 mm radius, its start square to L2 and its end
    // square to L1.
    f.constrain(37, SketchConstraint::Fix(a_centre));
    f.constrain(
        38,
        SketchConstraint::Radius {
            circle: a,
            value: 0.005,
        },
    );
    f.constrain(
        39,
        SketchConstraint::ArcLineTangent {
            arc: a,
            line: l2,
            at_end: false,
        },
    );
    f.constrain(
        40,
        SketchConstraint::ArcLineTangent {
            arc: a,
            line: l1,
            at_end: true,
        },
    );

    let result = f.solve();

    let readback_ids: Vec<SketchEntityId> = solved(&result).iter().map(|(id, _)| *id).collect();
    let declared_ids: Vec<SketchEntityId> = f.system.entities.iter().map(|def| def.id).collect();
    assert_eq!(
        readback_ids, declared_ids,
        "readback is in declaration order"
    );

    let (l1_s, l1_e) = line_of(&result, l1);
    assert_point_near(l1_s, (0.0, 0.0), "L1 start held at its seed");
    assert_point_near(l1_e, (0.010, 0.020), "L1 end held at its seed");
    assert_point_near(point_of(&result, l1_end), l1_e, "L1 end point");

    let (l2_s, l2_e) = line_of(&result, l2);
    assert_point_near(l2_s, (0.0, -0.030), "L2 anchored start");
    assert_near(l2_e.1 - l2_s.1, 0.0, "L2 is horizontal");
    assert_near(dist(l2_s, l2_e), 0.010, "L2 length");
    assert_point_near(l2_e, (0.010, -0.030), "L2 end");

    let (k_c, k_r) = circle_of(&result, k);
    assert_point_near(k_c, (-0.030, 0.0), "K anchored centre");
    assert_near(k_r, 0.004, "K radius");

    let (a_c, a_s, a_e) = arc_of(&result, a);
    assert_point_near(a_c, (0.040, 0.0), "A anchored centre");
    assert_near(dist(a_c, a_s), 0.005, "A radius at its start");
    assert_near(dist(a_c, a_e), 0.005, "A radius at its end");
    assert_near(
        dot(unit_from(a_c, a_s), unit_from(l2_s, l2_e)),
        0.0,
        "A's radius at its start is square to L2",
    );
    assert_near(
        dot(unit_from(a_c, a_e), unit_from(l1_s, l1_e)),
        0.0,
        "A's radius at its end is square to L1",
    );

    assert_eq!(dof_of(&result), 0, "every table is fully anchored");
}
