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

use std::collections::HashMap;
use std::fmt;

use reify_core::SourceSpan;

use crate::slvs_sys::{Slvs_hConstraint, Slvs_hParam};

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
    Circle { center: SketchEntityId, radius: f64 },
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

/// Which kind of entity a declaration is.
///
/// The `found` half of a kind mismatch: what the slot was actually handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchEntityKind {
    Point,
    Line,
    Circle,
    Arc,
}

/// What a slot in an entity or constraint declaration will accept.
///
/// Deliberately a different type from [`SketchEntityKind`] rather than the same
/// one reused: several slots accept more than one kind — the radius family takes
/// a circle *or* an arc, [`SketchConstraint::Fix`] takes a point *or* a line —
/// and collapsing those to a single kind would report a rule narrower than the
/// one actually enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchSlotKind {
    Point,
    Line,
    /// A circle or an arc: libslvs treats an arc as a circle with two ends for
    /// the whole radius family and for point-on-circle.
    Curve,
    /// An arc specifically — narrower than [`SketchSlotKind::Curve`].
    ///
    /// The tangency constraints read the curve's *endpoints*, and a full circle
    /// has none, so accepting one here would hand libslvs point handles the
    /// entity does not carry.
    Arc,
    /// A point or a line.
    PointOrLine,
}

impl SketchSlotKind {
    /// Whether an entity of kind `found` satisfies this slot.
    fn accepts(self, found: SketchEntityKind) -> bool {
        matches!(
            (self, found),
            (SketchSlotKind::Point, SketchEntityKind::Point)
                | (SketchSlotKind::Line, SketchEntityKind::Line)
                | (
                    SketchSlotKind::Curve,
                    SketchEntityKind::Circle | SketchEntityKind::Arc
                )
                | (SketchSlotKind::Arc, SketchEntityKind::Arc)
                | (
                    SketchSlotKind::PointOrLine,
                    SketchEntityKind::Point | SketchEntityKind::Line
                )
        )
    }
}

impl fmt::Display for SketchEntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SketchEntityKind::Point => "a point",
            SketchEntityKind::Line => "a line",
            SketchEntityKind::Circle => "a circle",
            SketchEntityKind::Arc => "an arc",
        })
    }
}

impl fmt::Display for SketchSlotKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SketchSlotKind::Point => "a point",
            SketchSlotKind::Line => "a line",
            SketchSlotKind::Curve => "a circle or an arc",
            SketchSlotKind::Arc => "an arc",
            SketchSlotKind::PointOrLine => "a point or a line",
        })
    }
}

impl SketchEntity {
    /// Which kind this entity is.
    pub fn kind(&self) -> SketchEntityKind {
        match self {
            SketchEntity::Point { .. } => SketchEntityKind::Point,
            SketchEntity::Line { .. } => SketchEntityKind::Line,
            SketchEntity::Circle { .. } => SketchEntityKind::Circle,
            SketchEntity::Arc { .. } => SketchEntityKind::Arc,
        }
    }

    /// The entities this one is defined by, each paired with what its slot
    /// requires.
    ///
    /// Every composite is defined by points and nothing else, so every pair here
    /// is [`SketchSlotKind::Point`] — but the slot kind is returned rather than
    /// assumed, so a future composite with a non-point slot cannot slip past the
    /// validator by inheriting an assumption made here.
    fn defining_refs(&self) -> Vec<(SketchEntityId, SketchSlotKind)> {
        let point = SketchSlotKind::Point;
        match *self {
            SketchEntity::Point { .. } => Vec::new(),
            SketchEntity::Line { start, end } => vec![(start, point), (end, point)],
            SketchEntity::Circle { center, .. } => vec![(center, point)],
            SketchEntity::Arc { center, start, end } => {
                vec![(center, point), (start, point), (end, point)]
            }
        }
    }
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
    Diameter { circle: SketchEntityId, value: f64 },
    /// A circle or arc has radius `value` metres.
    Radius { circle: SketchEntityId, value: f64 },
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

impl SketchConstraint {
    /// The entities this constraint names, each paired with what its slot
    /// requires.
    ///
    /// This is the *single* statement of which slot takes which kind. The emit
    /// path in `solvespace.rs` resolves the same operands through its own
    /// per-kind lookups, and the two have to agree — so this table is what the
    /// validator reads, and the emit path's own kind checks became a guard
    /// against the two drifting apart rather than a check on caller input.
    ///
    /// Order matters: the validator reports the first offending operand, so
    /// operands are listed in the order they are written in the declaration.
    fn operands(&self) -> Vec<(SketchEntityId, SketchSlotKind)> {
        use SketchSlotKind::{Arc, Curve, Line, Point, PointOrLine};
        match *self {
            SketchConstraint::Coincident { a, b } => vec![(a, Point), (b, Point)],
            SketchConstraint::PtOnLine { pt, line } => vec![(pt, Point), (line, Line)],
            SketchConstraint::PtOnCircle { pt, circle } => vec![(pt, Point), (circle, Curve)],
            SketchConstraint::Distance { a, b, .. } => vec![(a, Point), (b, Point)],
            SketchConstraint::Angle { a, b, .. } => vec![(a, Line), (b, Line)],
            SketchConstraint::Parallel { a, b } => vec![(a, Line), (b, Line)],
            SketchConstraint::Perpendicular { a, b } => vec![(a, Line), (b, Line)],
            SketchConstraint::Diameter { circle, .. } => vec![(circle, Curve)],
            SketchConstraint::Radius { circle, .. } => vec![(circle, Curve)],
            SketchConstraint::EqualRadius { a, b } => vec![(a, Curve), (b, Curve)],
            SketchConstraint::ArcLineTangent { arc, line, .. } => vec![(arc, Arc), (line, Line)],
            SketchConstraint::CurveCurveTangent { a, b, .. } => vec![(a, Arc), (b, Arc)],
            SketchConstraint::Horizontal(line) => vec![(line, Line)],
            SketchConstraint::Vertical(line) => vec![(line, Line)],
            SketchConstraint::SymmetricLine { a, b, about } => {
                vec![(a, Point), (b, Point), (about, Line)]
            }
            SketchConstraint::AtMidpoint { pt, line } => vec![(pt, Point), (line, Line)],
            SketchConstraint::EqualLengthLines { a, b } => vec![(a, Line), (b, Line)],
            SketchConstraint::Fix(id) => vec![(id, PointOrLine)],
        }
    }
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

impl SketchSystem {
    /// Check that every id is declared once and every slot holds the kind of
    /// entity it requires.
    ///
    /// Run before anything is emitted, so a malformed system is refused whole
    /// rather than lowered with the offending declaration quietly dropped. That
    /// distinction is the entire point: a partial lowering still *solves*, and
    /// what comes back is a plausible answer to a question the caller did not
    /// ask.
    ///
    /// Reports the first error in declaration order, entities before
    /// constraints. Deterministic by construction (O9), which is what lets a
    /// consumer fix what it is told about and re-solve without the diagnostic
    /// shuffling underneath it. Entities are checked first because a constraint
    /// naming an entity whose own declaration is broken would otherwise produce
    /// a derivative complaint about a slot that was never well-defined.
    ///
    /// # Errors
    ///
    /// Returns the first [`SketchBuildError`] found, or `Ok(())` for a
    /// well-formed system.
    pub(crate) fn validate(&self) -> Result<(), SketchBuildError> {
        // Pass 1: ids are unique — and the index everything else resolves
        // against.
        let mut kinds: HashMap<SketchEntityId, SketchEntityKind> =
            HashMap::with_capacity(self.entities.len());
        for def in &self.entities {
            if kinds.insert(def.id, def.entity.kind()).is_some() {
                return Err(SketchBuildError::DuplicateEntity { entity: def.id });
            }
        }

        // Pass 2: each composite's defining slots.
        //
        // A second pass rather than folded into the first: entities may name one
        // another in any order — emission creates every point before any
        // composite regardless of declaration order — so a forward-only check
        // would reject a line written above its own endpoints.
        for def in &self.entities {
            for (referenced, expected) in def.entity.defining_refs() {
                match kinds.get(&referenced) {
                    Some(found) if expected.accepts(*found) => {}
                    found => {
                        return Err(SketchBuildError::BadEntityRef {
                            owner: def.id,
                            referenced,
                            expected,
                            found: found.copied(),
                        });
                    }
                }
            }
        }

        // Pass 3: constraint operands.
        for def in &self.constraints {
            for (entity, expected) in def.constraint.operands() {
                match kinds.get(&entity) {
                    Some(found) if expected.accepts(*found) => {}
                    Some(found) => {
                        return Err(SketchBuildError::WrongEntityKind {
                            constraint: def.id,
                            entity,
                            expected,
                            found: *found,
                            span: def.span,
                        });
                    }
                    None => {
                        return Err(SketchBuildError::UnknownEntity {
                            referenced_by: def.id,
                            entity,
                            span: def.span,
                        });
                    }
                }
            }
        }

        Ok(())
    }
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
    /// The system was malformed and was never handed to libslvs at all.
    ///
    /// The one arm that is not a solver report: it says the question was
    /// ill-posed, not that the solver could not answer it. Kept here rather than
    /// wrapping the whole return in a `Result` so callers match one exhaustive
    /// enum — a malformed sketch is one more way not to get geometry back, and
    /// the readback arms stay in a single place.
    Malformed(SketchBuildError),
}

/// Reverse map from [`SketchSystem`] ids to the params that carry their solved
/// geometry.
///
/// Crate-internal by design: it carries raw `Slvs_*` handles, which must not
/// cross the crate boundary (see the `solve_sketch`-as-the-only-seam decision).
///
/// This is the *readback* index specifically — the emit side keeps its own
/// index of slvs entity handles, because the two want different things (emission
/// resolves operands by entity handle and kind; readback resolves geometry by
/// param).  Entries are appended in [`SketchSystem::entities`] declaration order
/// and read back in that same order, which is what makes the solved output order
/// a function of the input rather than of allocation timing (O9).
pub(crate) struct SketchHandleMap {
    entries: Vec<SketchEntityHandles>,
    /// Reverse map from an emitted slvs constraint back to the declaration that
    /// produced it.
    ///
    /// Not one-to-one in the emit direction: one declaration can expand into
    /// several slvs constraints (`Fix` on a line anchors both endpoints), and
    /// each of those handles maps back to the same declaration.  That is the
    /// whole point — libslvs reports failures by slvs handle, and a diagnostic
    /// has to name the line the author wrote, not the second of two anonymous
    /// anchors.
    attribution: HashMap<Slvs_hConstraint, (SketchConstraintId, SourceSpan)>,
}

/// One sketch entity and how to reconstruct it from solved params.
struct SketchEntityHandles {
    id: SketchEntityId,
    readback: SketchReadback,
}

/// Where a solved entity's geometry comes from once `Slvs_Solve` returns.
///
/// Points own their params; composite entities own none, and are reconstructed
/// from the params of the points they are defined by.  That indirection is why
/// a composite records its operands' params rather than params of its own.
enum SketchReadback {
    Point {
        x: Slvs_hParam,
        y: Slvs_hParam,
    },
    Line {
        start: (Slvs_hParam, Slvs_hParam),
        end: (Slvs_hParam, Slvs_hParam),
    },
    /// A circle is the one composite that *does* own a param — its radius,
    /// which libslvs holds in a separate distance-entity carrier.
    Circle {
        center: (Slvs_hParam, Slvs_hParam),
        radius: Slvs_hParam,
    },
    /// An arc owns no radius param: its radius is implied by centre→start.
    Arc {
        center: (Slvs_hParam, Slvs_hParam),
        start: (Slvs_hParam, Slvs_hParam),
        end: (Slvs_hParam, Slvs_hParam),
    },
}

impl SketchHandleMap {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            attribution: HashMap::new(),
        }
    }

    /// Carry the emit side's handle→declaration index out with the map.
    pub(crate) fn set_attribution(
        &mut self,
        attribution: HashMap<Slvs_hConstraint, (SketchConstraintId, SourceSpan)>,
    ) {
        self.attribution = attribution;
    }

    /// Resolve the slvs handles libslvs reported as failing back to the
    /// declarations that produced them.
    ///
    /// Deduplicated by [`SketchConstraintId`] — a `Fix` that expanded into two
    /// anchors and failed at both is one failing declaration, not two — and
    /// sorted by id, so the set is a function of the input rather than of
    /// libslvs' internal ordering.  Sorting by *id* rather than by handle also
    /// keeps the order stable if the emit order ever changes.
    ///
    /// A handle with no entry cannot happen: every constraint in a sketch
    /// system is emitted through the attributing path, so the map covers all of
    /// them.  It is therefore reported rather than quietly skipped — a
    /// diagnostic that silently names fewer culprits than libslvs found is
    /// worse than one that admits it lost track.
    pub(crate) fn resolve_failing(
        &self,
        failed: &[Slvs_hConstraint],
    ) -> Vec<(SketchConstraintId, SourceSpan)> {
        let mut out: Vec<(SketchConstraintId, SourceSpan)> = Vec::with_capacity(failed.len());
        for handle in failed {
            match self.attribution.get(handle) {
                Some(origin) => out.push(*origin),
                None => {
                    tracing::error!(
                        constraint_handle = handle.0,
                        "libslvs reported a failing constraint this sketch never emitted; \
                         it cannot be attributed to a declaration"
                    );
                    debug_assert!(
                        false,
                        "unattributable failing constraint handle {}",
                        handle.0
                    );
                }
            }
        }
        out.sort_by_key(|(id, _)| *id);
        out.dedup_by_key(|(id, _)| *id);
        out
    }

    fn push(&mut self, id: SketchEntityId, readback: SketchReadback) {
        self.entries.push(SketchEntityHandles { id, readback });
    }

    /// Record a point and the two params carrying its coordinates.
    pub(crate) fn push_point(&mut self, id: SketchEntityId, x: Slvs_hParam, y: Slvs_hParam) {
        self.push(id, SketchReadback::Point { x, y });
    }

    /// Record a line and the params of the two points defining it.
    pub(crate) fn push_line(
        &mut self,
        id: SketchEntityId,
        start: (Slvs_hParam, Slvs_hParam),
        end: (Slvs_hParam, Slvs_hParam),
    ) {
        self.push(id, SketchReadback::Line { start, end });
    }

    /// Record a circle: its centre point's params and its own radius param.
    pub(crate) fn push_circle(
        &mut self,
        id: SketchEntityId,
        center: (Slvs_hParam, Slvs_hParam),
        radius: Slvs_hParam,
    ) {
        self.push(id, SketchReadback::Circle { center, radius });
    }

    /// Record an arc and the params of the three points defining it.
    pub(crate) fn push_arc(
        &mut self,
        id: SketchEntityId,
        center: (Slvs_hParam, Slvs_hParam),
        start: (Slvs_hParam, Slvs_hParam),
        end: (Slvs_hParam, Slvs_hParam),
    ) {
        self.push(id, SketchReadback::Arc { center, start, end });
    }

    /// Resolve every recorded entity against the solved param values.
    ///
    /// `values` maps param handle to solved value.  A param missing from
    /// `values` is impossible by construction — every param this map references
    /// was pushed into the same system that produced them — so it is reported
    /// rather than defaulted to zero, which would fabricate geometry.
    pub(crate) fn read_back(
        &self,
        values: &HashMap<Slvs_hParam, f64>,
    ) -> Result<Vec<(SketchEntityId, SolvedSketchEntity)>, Slvs_hParam> {
        let at = |p: &Slvs_hParam| values.get(p).copied().ok_or(*p);
        let xy = |p: &(Slvs_hParam, Slvs_hParam)| Ok((at(&p.0)?, at(&p.1)?));

        let mut out = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let solved = match &entry.readback {
                SketchReadback::Point { x, y } => SolvedSketchEntity::Point {
                    x: at(x)?,
                    y: at(y)?,
                },
                SketchReadback::Line { start, end } => SolvedSketchEntity::Line {
                    start: xy(start)?,
                    end: xy(end)?,
                },
                SketchReadback::Circle { center, radius } => SolvedSketchEntity::Circle {
                    center: xy(center)?,
                    radius: at(radius)?,
                },
                SketchReadback::Arc { center, start, end } => SolvedSketchEntity::Arc {
                    center: xy(center)?,
                    start: xy(start)?,
                    end: xy(end)?,
                },
            };
            out.push((entry.id, solved));
        }
        Ok(out)
    }
}

/// Why a [`SketchSystem`] could not be lowered into a slvs system at all.
///
/// Distinct from [`SketchSolveResult`]: these are malformed *inputs* — a
/// dangling entity reference, a duplicate id, a constraint slot handed the
/// wrong kind of entity — caught before any solving is attempted.  Every
/// variant carries the offending ids as structured fields so the consumer can
/// render a diagnostic without scraping a message string.
///
/// Reported by [`SketchSystem::validate`], which runs before any slvs entity or
/// constraint is emitted — so a system that trips one of these is refused
/// whole, never partially lowered and solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchBuildError {
    /// A constraint names an entity id the system never declares.
    UnknownEntity {
        referenced_by: SketchConstraintId,
        entity: SketchEntityId,
        /// The referencing constraint's span — the place a diagnostic points at,
        /// since the missing entity has no declaration to point at.
        span: SourceSpan,
    },
    /// The same entity id is declared more than once, which makes every
    /// reference to it ambiguous.
    DuplicateEntity { entity: SketchEntityId },
    /// A constraint slot was handed an entity of a kind it does not accept.
    WrongEntityKind {
        constraint: SketchConstraintId,
        entity: SketchEntityId,
        expected: SketchSlotKind,
        found: SketchEntityKind,
        span: SourceSpan,
    },
    /// A composite entity's defining slot was handed the wrong kind of entity,
    /// or one that does not exist.
    ///
    /// `found` is `None` for a reference to an undeclared id. That case is not
    /// folded into [`SketchBuildError::UnknownEntity`] because that variant
    /// names the *constraint* that reached for the missing id, and an entity's
    /// defining slot has no constraint to name — `owner` is the entity itself.
    BadEntityRef {
        owner: SketchEntityId,
        referenced: SketchEntityId,
        expected: SketchSlotKind,
        found: Option<SketchEntityKind>,
    },
}

impl fmt::Display for SketchBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SketchBuildError::UnknownEntity {
                referenced_by,
                entity,
                ..
            } => write!(
                f,
                "constraint {} references entity {}, which the sketch does not declare",
                referenced_by.0, entity.0
            ),
            SketchBuildError::DuplicateEntity { entity } => {
                write!(f, "entity {} is declared more than once", entity.0)
            }
            SketchBuildError::WrongEntityKind {
                constraint,
                entity,
                expected,
                found,
                ..
            } => write!(
                f,
                "constraint {} expects {expected} but entity {} is {found}",
                constraint.0, entity.0
            ),
            SketchBuildError::BadEntityRef {
                owner,
                referenced,
                expected,
                found,
            } => match found {
                Some(found) => write!(
                    f,
                    "entity {} is defined by entity {}, which must be {expected} but is {found}",
                    owner.0, referenced.0
                ),
                None => write!(
                    f,
                    "entity {} is defined by entity {}, which must be {expected} \
                     but is not declared",
                    owner.0, referenced.0
                ),
            },
        }
    }
}

impl std::error::Error for SketchBuildError {}
