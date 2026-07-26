//! Crate-boundary data model for 2D constrained sketches.
//!
//! This is the typed input/output vocabulary that `solve_sketch` consumes and
//! produces — the seam described by `docs/prds/v0_6/constrained-2d-sketch.md`
//! §7 C5.  It is deliberately the *crate-boundary twin* of the language-level
//! sketch template: every expression has already been evaluated down to a
//! plain SI `f64`, so nothing in this module depends on the evaluator.
//!
//! Two design rules govern the shapes here:
//!
//! - **Typed handles only (INV-SF-5).**  Entities and constraints are named by
//!   [`SketchEntityId`] / [`SketchConstraintId`] newtypes, never by strings and
//!   never by a bare `f64` standing in for a handle.  A slot that wants a point
//!   cannot be handed a name that happens to spell one.
//! - **Declaration order is the canonical order (O9).**  [`SketchSystem`] holds
//!   `Vec`s, not maps, so slvs handle allocation — and therefore the solved
//!   readback and the failing-constraint set — is a deterministic function of
//!   the input.
//!
//! Note on scope: this module surfaces exactly what libslvs reported (a raw
//! `dof: i32`, the resolved failing set, the raw non-OK result codes).  The
//! classification of that readback into diagnostics — PRD C3's
//! `SketchSolveOutcome` and its DOF ledger — belongs to the consumer, which is
//! the only place that knows which DOFs were declared `auto`.

use reify_core::SourceSpan;

/// Identifies a sketch entity within one [`SketchSystem`].
///
/// Opaque by intent: the numeric value carries no geometric meaning and is not
/// an index into `SketchSystem::entities` (callers may number entities however
/// they like, as long as ids are unique within the system).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SketchEntityId(pub u32);

/// Identifies a sketch constraint within one [`SketchSystem`].
///
/// Returned verbatim in [`SketchSolveResult::Inconsistent`], so a failing
/// constraint resolves back to the declaration that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SketchConstraintId(pub u32);

/// A sketch entity with all of its literal geometry pre-evaluated to SI units.
///
/// Coordinates and radii are **seed values**: the solver is free to move them.
/// Composite entities (`Line`, `Circle`, `Arc`) reference their defining points
/// by id rather than embedding coordinates, so two entities can genuinely share
/// a point (which is how endpoint coincidence and tangency are expressed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SketchEntity {
    /// A point in the sketch plane. `x`/`y` are seed coordinates in metres.
    Point { x: f64, y: f64 },
    /// A straight segment between two [`SketchEntity::Point`] entities.
    Line {
        start: SketchEntityId,
        end: SketchEntityId,
    },
    /// A full circle. `radius` is a seed radius in metres.
    Circle {
        center: SketchEntityId,
        radius: f64,
    },
    /// An arc of a circle, defined by its center and two endpoints.
    ///
    /// The two endpoints are equidistant from the center by construction —
    /// libslvs contributes that equation itself, so DOF accounting must not
    /// count it twice.
    Arc {
        center: SketchEntityId,
        start: SketchEntityId,
        end: SketchEntityId,
    },
}

/// One entity declaration: its id, its geometry, and whether it is construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchEntityDef {
    pub id: SketchEntityId,
    pub entity: SketchEntity,
    /// Auxiliary (construction) geometry — participates in solving but is not
    /// part of the sketch's output profile.
    pub aux: bool,
}

/// The 2D constraint vocabulary, one variant per libslvs constraint mapping.
///
/// Dimensional values are SI (metres) except [`SketchConstraint::Angle`], which
/// is in degrees to match libslvs' own `SLVS_C_ANGLE` convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SketchConstraint {
    /// Two points occupy the same location.
    Coincident {
        a: SketchEntityId,
        b: SketchEntityId,
    },
    /// A point lies on a line's infinite extension.
    PtOnLine {
        pt: SketchEntityId,
        line: SketchEntityId,
    },
    /// A point lies on a circle's or arc's circumference.
    PtOnCircle {
        pt: SketchEntityId,
        circle: SketchEntityId,
    },
    /// Two points are `value` metres apart.
    Distance {
        a: SketchEntityId,
        b: SketchEntityId,
        value: f64,
    },
    /// Two lines meet at `degrees`.
    Angle {
        a: SketchEntityId,
        b: SketchEntityId,
        degrees: f64,
    },
    /// Two lines are parallel.
    Parallel {
        a: SketchEntityId,
        b: SketchEntityId,
    },
    /// Two lines are perpendicular.
    Perpendicular {
        a: SketchEntityId,
        b: SketchEntityId,
    },
    /// A circle or arc has diameter `value` metres.
    Diameter {
        circle: SketchEntityId,
        value: f64,
    },
    /// A circle or arc has radius `value` metres.
    Radius {
        circle: SketchEntityId,
        value: f64,
    },
    /// Two circles/arcs share a radius.
    EqualRadius {
        a: SketchEntityId,
        b: SketchEntityId,
    },
    /// A line is tangent to an arc at one of the arc's endpoints.
    ///
    /// `at_end` selects which endpoint of the arc is the tangent point
    /// (`false` = the arc's start, `true` = its end).
    ArcLineTangent {
        arc: SketchEntityId,
        line: SketchEntityId,
        at_end: bool,
    },
    /// Two curves (arcs) are tangent where they meet.
    ///
    /// `a_at_end` / `b_at_end` select which endpoint of each curve is the
    /// tangent point.
    CurveCurveTangent {
        a: SketchEntityId,
        a_at_end: bool,
        b: SketchEntityId,
        b_at_end: bool,
    },
    /// A line is parallel to the sketch plane's u axis.
    Horizontal(SketchEntityId),
    /// A line is parallel to the sketch plane's v axis.
    Vertical(SketchEntityId),
    /// Two points mirror one another across a line.
    SymmetricLine {
        a: SketchEntityId,
        b: SketchEntityId,
        about: SketchEntityId,
    },
    /// A point sits at a line's midpoint.
    AtMidpoint {
        pt: SketchEntityId,
        line: SketchEntityId,
    },
    /// Two lines have the same length.
    EqualLengthLines {
        a: SketchEntityId,
        b: SketchEntityId,
    },
    /// Anchor an entity where it was declared.
    ///
    /// Applied to a point this pins that point; applied to a line it pins both
    /// endpoints.  Deliberately expressed as a real constraint rather than by
    /// parking the params outside the solved group, so that an over-constrained
    /// anchor is attributable to its source span like any other constraint.
    Fix(SketchEntityId),
}

/// One constraint declaration: its id, the relation, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchConstraintDef {
    pub id: SketchConstraintId,
    pub constraint: SketchConstraint,
    /// The source span this constraint was written at, returned verbatim in the
    /// failing set so the consumer can render a span-bearing diagnostic.
    pub span: SourceSpan,
}

/// A complete sketch: entities and constraints in declaration order.
///
/// Order is load-bearing (O9): handle allocation walks these `Vec`s, so the
/// same `SketchSystem` always produces the same slvs system and therefore the
/// same solved output, bit for bit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SketchSystem {
    pub entities: Vec<SketchEntityDef>,
    pub constraints: Vec<SketchConstraintDef>,
}

/// A solved entity, with every reference resolved to concrete coordinates.
///
/// Composite entities carry their endpoints' solved positions inline: the
/// consumer wants geometry, not another round of id chasing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolvedSketchEntity {
    Point {
        x: f64,
        y: f64,
    },
    Line {
        start: (f64, f64),
        end: (f64, f64),
    },
    Circle {
        center: (f64, f64),
        radius: f64,
    },
    Arc {
        center: (f64, f64),
        start: (f64, f64),
        end: (f64, f64),
    },
}

/// The raw outcome of one `Slvs_Solve` over a [`SketchSystem`].
///
/// Every arm mirrors something libslvs actually reported; nothing here is a
/// classification.  In particular `dof` is libslvs' own degrees-of-freedom
/// count for the sketch group, not a judgement about whether the sketch is
/// "properly constrained".
#[derive(Debug, Clone, PartialEq)]
pub enum SketchSolveResult {
    /// libslvs converged. `entities` is in [`SketchSystem::entities`] order.
    Solved {
        entities: Vec<(SketchEntityId, SolvedSketchEntity)>,
        dof: i32,
    },
    /// The constraints contradict one another. `failing` names the constraints
    /// libslvs identified as mutually inconsistent, each with its source span.
    Inconsistent {
        failing: Vec<(SketchConstraintId, SourceSpan)>,
    },
    /// Newton iteration ran out of steps without converging.
    DidntConverge,
    /// The system has more unknowns than libslvs will accept.
    TooManyUnknowns,
    /// The system exceeded `i32::MAX` params/entities/constraints and could not
    /// be handed to the C API at all.
    TooLarge,
    /// The global libslvs mutex was poisoned by a prior panic; solving was
    /// refused rather than risking undefined behaviour in the C++ globals.
    LockPoisoned,
    /// libslvs returned a result code this binding does not know.
    UnknownError(i32),
}

/// Reverse map from [`SketchSystem`] ids to the slvs handles emitted for them.
///
/// Crate-internal by design: it carries raw `Slvs_*` handles, which must not
/// cross the crate boundary (see the `solve_sketch`-as-the-only-seam decision).
// Declared here alongside the rest of the sketch data model; populated and
// consumed by the direct-build spine in `solvespace.rs`.
#[allow(dead_code)]
pub(crate) struct SketchHandleMap {}

/// Why a [`SketchSystem`] could not be lowered into a slvs system at all.
///
/// Distinct from [`SketchSolveResult`]: these are malformed *inputs* — a
/// dangling entity reference, a duplicate id, a constraint slot handed the
/// wrong kind of entity — caught before any solving is attempted.  Every
/// variant carries the offending ids as structured fields so the consumer can
/// render a diagnostic without scraping a message string.
///
/// Uninhabited until the validation pass that produces it exists.
pub enum SketchBuildError {}
