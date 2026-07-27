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

use std::collections::{HashMap, HashSet};
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

/// A fixed-capacity list of `(entity, required kind)` slot pairs.
///
/// Three is the widest declaration in either slot table — an [`SketchEntity::Arc`]'s
/// centre plus two endpoints, a [`SketchConstraint::SymmetricLine`]'s two points
/// plus its centreline — and both tables are closed enums, so the capacity is a
/// property of the vocabulary rather than a guess that a future variant could
/// outgrow unnoticed: adding a wider variant fails to compile against `CAP`.
///
/// Exists so [`SketchSystem::validate`]'s linear scan does not heap-allocate a
/// throwaway `Vec` per declaration purely to iterate a fixed-arity slot list,
/// while keeping the single-source-of-truth slot tables exactly where they are.
#[derive(Debug, Clone, Copy)]
struct SlotList {
    slots: [(SketchEntityId, SketchSlotKind); SlotList::CAP],
    len: usize,
}

impl SlotList {
    const CAP: usize = 3;

    /// The padding a shorter list's unused tail carries. Never read: `iter`
    /// stops at `len`.
    const PAD: (SketchEntityId, SketchSlotKind) = (SketchEntityId(0), SketchSlotKind::Point);

    const EMPTY: SlotList = SlotList {
        slots: [SlotList::PAD; SlotList::CAP],
        len: 0,
    };

    /// The occupied slots, in declaration order.
    fn iter(&self) -> impl Iterator<Item = (SketchEntityId, SketchSlotKind)> + '_ {
        self.slots[..self.len].iter().copied()
    }
}

impl From<[(SketchEntityId, SketchSlotKind); 1]> for SlotList {
    fn from([a]: [(SketchEntityId, SketchSlotKind); 1]) -> Self {
        SlotList {
            slots: [a, SlotList::PAD, SlotList::PAD],
            len: 1,
        }
    }
}

impl From<[(SketchEntityId, SketchSlotKind); 2]> for SlotList {
    fn from([a, b]: [(SketchEntityId, SketchSlotKind); 2]) -> Self {
        SlotList {
            slots: [a, b, SlotList::PAD],
            len: 2,
        }
    }
}

impl From<[(SketchEntityId, SketchSlotKind); 3]> for SlotList {
    fn from(slots: [(SketchEntityId, SketchSlotKind); 3]) -> Self {
        SlotList {
            slots,
            len: SlotList::CAP,
        }
    }
}

/// Which literal field of a declaration a numeric complaint is about.
///
/// Named rather than positional so a consumer can say *which* number was bad
/// without re-deriving the declaration's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchValueField {
    /// A point's `x` seed coordinate.
    X,
    /// A point's `y` seed coordinate.
    Y,
    /// A circle's seed radius.
    Radius,
    /// A dimensional constraint's value: a length, a diameter or a radius.
    Value,
    /// An angle constraint's value, in degrees.
    Degrees,
}

impl fmt::Display for SketchValueField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SketchValueField::X => "its x coordinate",
            SketchValueField::Y => "its y coordinate",
            SketchValueField::Radius => "its radius",
            SketchValueField::Value => "its value",
            SketchValueField::Degrees => "its angle in degrees",
        })
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
    fn defining_refs(&self) -> SlotList {
        let point = SketchSlotKind::Point;
        match *self {
            SketchEntity::Point { .. } => SlotList::EMPTY,
            SketchEntity::Line { start, end } => [(start, point), (end, point)].into(),
            SketchEntity::Circle { center, .. } => [(center, point)].into(),
            SketchEntity::Arc { center, start, end } => {
                [(center, point), (start, point), (end, point)].into()
            }
        }
    }

    /// This entity's own literal seed values, each paired with the field it was
    /// written in.
    ///
    /// Composites hold no literals of their own — their geometry is entirely a
    /// function of the points they name — so they yield nothing here and are
    /// validated numerically by way of those points.
    fn literal_values(&self) -> SlotValues {
        match *self {
            SketchEntity::Point { x, y } => {
                [(SketchValueField::X, x), (SketchValueField::Y, y)].into()
            }
            SketchEntity::Circle { radius, .. } => [(SketchValueField::Radius, radius)].into(),
            SketchEntity::Line { .. } | SketchEntity::Arc { .. } => SlotValues::EMPTY,
        }
    }
}

/// A fixed-capacity list of `(field, literal)` pairs — the numeric twin of
/// [`SlotList`], and allocation-free for the same reason.
///
/// Two is the widest literal set in either vocabulary (a point's `x` and `y`);
/// every dimensional constraint carries exactly one.
#[derive(Debug, Clone, Copy)]
struct SlotValues {
    values: [(SketchValueField, f64); SlotValues::CAP],
    len: usize,
}

impl SlotValues {
    const CAP: usize = 2;

    /// Padding for a shorter list's unused tail. Never read: `iter` stops at
    /// `len`. Zero rather than NaN so a padding slot that somehow *were* read
    /// would not itself trip the non-finite check it pads out.
    const PAD: (SketchValueField, f64) = (SketchValueField::Value, 0.0);

    const EMPTY: SlotValues = SlotValues {
        values: [SlotValues::PAD; SlotValues::CAP],
        len: 0,
    };

    /// The occupied values, in declaration order.
    fn iter(&self) -> impl Iterator<Item = (SketchValueField, f64)> + '_ {
        self.values[..self.len].iter().copied()
    }
}

impl From<[(SketchValueField, f64); 1]> for SlotValues {
    fn from([a]: [(SketchValueField, f64); 1]) -> Self {
        SlotValues {
            values: [a, SlotValues::PAD],
            len: 1,
        }
    }
}

impl From<[(SketchValueField, f64); 2]> for SlotValues {
    fn from(values: [(SketchValueField, f64); 2]) -> Self {
        SlotValues {
            values,
            len: SlotValues::CAP,
        }
    }
}

/// One entity declaration: its id, its geometry, and whether it is construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchEntityDef {
    pub id: SketchEntityId,
    pub entity: SketchEntity,
    /// Auxiliary (construction) geometry — participates in solving but is not
    /// part of the sketch's output profile (PRD D12).
    ///
    /// **This layer does not act on it.** Aux and non-aux entities are emitted
    /// identically — that is the point: construction geometry is exactly the
    /// geometry that constrains without being drawn, so excluding it from the
    /// solve would change the answer. It is also read back identically, so a
    /// consumer assembling the output profile must join
    /// [`SketchSolveResult::Solved`]'s entity list back to this `SketchSystem`
    /// by id and drop the aux ones itself. Filtering here would mean the solved
    /// readback no longer covered every entity the caller declared, which is the
    /// wrong default for a layer whose job is to report what the solver did.
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
    fn operands(&self) -> SlotList {
        use SketchSlotKind::{Arc, Curve, Line, Point, PointOrLine};
        match *self {
            SketchConstraint::Coincident { a, b } => [(a, Point), (b, Point)].into(),
            SketchConstraint::PtOnLine { pt, line } => [(pt, Point), (line, Line)].into(),
            SketchConstraint::PtOnCircle { pt, circle } => [(pt, Point), (circle, Curve)].into(),
            SketchConstraint::Distance { a, b, .. } => [(a, Point), (b, Point)].into(),
            SketchConstraint::Angle { a, b, .. } => [(a, Line), (b, Line)].into(),
            SketchConstraint::Parallel { a, b } => [(a, Line), (b, Line)].into(),
            SketchConstraint::Perpendicular { a, b } => [(a, Line), (b, Line)].into(),
            SketchConstraint::Diameter { circle, .. } => [(circle, Curve)].into(),
            SketchConstraint::Radius { circle, .. } => [(circle, Curve)].into(),
            SketchConstraint::EqualRadius { a, b } => [(a, Curve), (b, Curve)].into(),
            SketchConstraint::ArcLineTangent { arc, line, .. } => [(arc, Arc), (line, Line)].into(),
            SketchConstraint::CurveCurveTangent { a, b, .. } => [(a, Arc), (b, Arc)].into(),
            SketchConstraint::Horizontal(line) => [(line, Line)].into(),
            SketchConstraint::Vertical(line) => [(line, Line)].into(),
            SketchConstraint::SymmetricLine { a, b, about } => {
                [(a, Point), (b, Point), (about, Line)].into()
            }
            SketchConstraint::AtMidpoint { pt, line } => [(pt, Point), (line, Line)].into(),
            SketchConstraint::EqualLengthLines { a, b } => [(a, Line), (b, Line)].into(),
            SketchConstraint::Fix(id) => [(id, PointOrLine)].into(),
        }
    }

    /// This constraint's dimensional value, if it carries one.
    ///
    /// Exhaustive over the vocabulary rather than a catch-all, so a future
    /// dimensional variant cannot be added without deciding — at the compiler's
    /// insistence — whether its value is one the validator must screen.
    fn literal_values(&self) -> SlotValues {
        use SketchValueField::{Degrees, Value};
        match *self {
            SketchConstraint::Distance { value, .. } => [(Value, value)].into(),
            SketchConstraint::Diameter { value, .. } => [(Value, value)].into(),
            SketchConstraint::Radius { value, .. } => [(Value, value)].into(),
            SketchConstraint::Angle { degrees, .. } => [(Degrees, degrees)].into(),
            SketchConstraint::Coincident { .. }
            | SketchConstraint::PtOnLine { .. }
            | SketchConstraint::PtOnCircle { .. }
            | SketchConstraint::Parallel { .. }
            | SketchConstraint::Perpendicular { .. }
            | SketchConstraint::EqualRadius { .. }
            | SketchConstraint::ArcLineTangent { .. }
            | SketchConstraint::CurveCurveTangent { .. }
            | SketchConstraint::Horizontal(_)
            | SketchConstraint::Vertical(_)
            | SketchConstraint::SymmetricLine { .. }
            | SketchConstraint::AtMidpoint { .. }
            | SketchConstraint::EqualLengthLines { .. }
            | SketchConstraint::Fix(_) => SlotValues::EMPTY,
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
    /// Within a single declaration the checks run premise-first: a declaration's
    /// own literals are screened before its references (they depend on nothing
    /// else), references' kinds before whether those references repeat (naming
    /// "the same point twice" is only a meaningful complaint once both slots are
    /// known to hold points at all).
    ///
    /// What is deliberately *not* checked: whether two distinct points happen to
    /// be seeded at the same coordinates. Seeds are provisional by contract — the
    /// solver is free to move them — so a coincident pair of seeds is an ordinary
    /// starting state, not a malformed declaration. Only structural degeneracy,
    /// where one declaration names the same entity in two slots and no constraint
    /// could ever separate them, is refused.
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

        // Pass 2: each entity's own literals, then its defining slots.
        //
        // A second pass rather than folded into the first: entities may name one
        // another in any order — emission creates every point before any
        // composite regardless of declaration order — so a forward-only check
        // would reject a line written above its own endpoints.
        for def in &self.entities {
            // A non-finite seed is not caught downstream: `Slvs_Solve` accepts
            // NaN params and can return SLVS_RESULT_OKAY with NaN still in them,
            // which reads back as a fully-formed `Solved` carrying coordinates
            // that answer nothing.
            for (field, value) in def.entity.literal_values().iter() {
                if !value.is_finite() {
                    return Err(SketchBuildError::NonFiniteEntityValue {
                        entity: def.id,
                        field,
                    });
                }
            }

            let refs = def.entity.defining_refs();
            for (referenced, expected) in refs.iter() {
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

            // A composite naming one point in two slots is singular by
            // construction, not merely badly seeded: a zero-length line has no
            // direction for `Horizontal` to constrain, and an arc whose centre is
            // its own endpoint has no radius. No constraint can separate two
            // slots that are literally the same param pair, so this cannot be
            // solved out of — unlike a coincident *seed*, which can.
            for (i, (referenced, _)) in refs.iter().enumerate() {
                if refs
                    .iter()
                    .take(i)
                    .any(|(earlier, _)| earlier == referenced)
                {
                    return Err(SketchBuildError::DegenerateEntity {
                        owner: def.id,
                        repeated: referenced,
                    });
                }
            }
        }

        // Pass 3: constraint ids are unique.
        //
        // Its own scan, mirroring pass 1, and ahead of the operand check for the
        // same reason: a duplicated id makes attribution ambiguous. Both
        // declarations emit, both land in the attribution map, and the failing
        // set — deduplicated by id — collapses them into one entry carrying
        // whichever span survived. One genuine culprit would vanish from the
        // diagnostic and the other could be reported against the wrong span.
        let mut seen: HashSet<SketchConstraintId> = HashSet::with_capacity(self.constraints.len());
        for def in &self.constraints {
            if !seen.insert(def.id) {
                return Err(SketchBuildError::DuplicateConstraint { constraint: def.id });
            }
        }

        // Pass 4: each constraint's own literals, then its operands.
        for def in &self.constraints {
            for (field, value) in def.constraint.literal_values().iter() {
                if !value.is_finite() {
                    return Err(SketchBuildError::NonFiniteConstraintValue {
                        constraint: def.id,
                        field,
                        span: def.span,
                    });
                }
            }

            for (entity, expected) in def.constraint.operands().iter() {
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
    ///
    /// Covers **every** declared entity, including the ones marked
    /// [`SketchEntityDef::aux`]: this layer neither filters construction
    /// geometry nor marks it in the readback, so a consumer building an output
    /// profile joins back to its own `SketchSystem` by id to exclude it.
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
    /// libslvs converged, but a param the readback needed was absent from the
    /// solved system, so no geometry could be reconstructed.
    ///
    /// Impossible by construction — every param the handle map references was
    /// pushed into the very system that produced the solved values — and kept as
    /// its own arm precisely so it never has to be reported as something it is
    /// not. Folding it into [`SketchSolveResult::UnknownError`] would render as
    /// "libslvs returned unknown result code 0", which is false twice over:
    /// libslvs returned OKAY, and the failure was entirely on this side of the
    /// FFI. `param` is the raw slvs param handle, for a bug report rather than
    /// for the author of the sketch.
    ReadbackFailed { param: u32 },
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
    /// The same constraint id is declared more than once, which makes the
    /// failing set ambiguous.
    ///
    /// The constraint-side twin of [`SketchBuildError::DuplicateEntity`], and
    /// refused for the mirror-image reason: both declarations would emit and both
    /// would land in the attribution map, so an inconsistent solve that named
    /// them both would report a single entry carrying one declaration's span —
    /// losing one culprit and possibly mis-siting the other.
    ///
    /// Carries no span: two declarations share the id, so there is no single
    /// place to point at, and naming either one would suggest that one is the
    /// offender.
    DuplicateConstraint { constraint: SketchConstraintId },
    /// An entity's literal seed value is not a finite number.
    ///
    /// `Slvs_Solve` does not reject NaN or ±∞ — it can return
    /// `SLVS_RESULT_OKAY` with them still in the params, which reads back as a
    /// perfectly well-formed `Solved`. Screening here is what keeps that from
    /// being indistinguishable from a real answer.
    NonFiniteEntityValue {
        entity: SketchEntityId,
        field: SketchValueField,
    },
    /// A constraint's dimensional value is not a finite number.
    ///
    /// Split from [`SketchBuildError::NonFiniteEntityValue`] rather than folded
    /// into one variant for the same reason [`SketchBuildError::UnknownEntity`]
    /// is split from [`SketchBuildError::BadEntityRef`]: a constraint has a span
    /// to point at and an entity declaration does not, and a variant whose span
    /// were sometimes absent would push that check onto every consumer.
    NonFiniteConstraintValue {
        constraint: SketchConstraintId,
        field: SketchValueField,
        span: SourceSpan,
    },
    /// A composite entity names the same entity in two of its defining slots.
    ///
    /// Distinct from two *separate* points seeded at the same coordinates, which
    /// is legal: seeds are provisional and a constraint can drive them apart.
    /// Here the two slots are the same params, so nothing can ever separate them
    /// — the line has no direction and the arc has no radius, and the equations
    /// libslvs derives from them are singular.
    DegenerateEntity {
        owner: SketchEntityId,
        repeated: SketchEntityId,
    },
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
            SketchBuildError::DuplicateConstraint { constraint } => {
                write!(f, "constraint {} is declared more than once", constraint.0)
            }
            SketchBuildError::NonFiniteEntityValue { entity, field } => write!(
                f,
                "entity {} was declared with {field} not a finite number",
                entity.0
            ),
            SketchBuildError::NonFiniteConstraintValue {
                constraint, field, ..
            } => write!(
                f,
                "constraint {} was declared with {field} not a finite number",
                constraint.0
            ),
            SketchBuildError::DegenerateEntity { owner, repeated } => write!(
                f,
                "entity {} names entity {} in more than one of its defining slots, \
                 which leaves it with no shape to solve for",
                owner.0, repeated.0
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The two properties `resolve_failing` promises — one entry per failing
    /// *declaration*, ascending by id — driven directly.
    ///
    /// Driven here rather than through `solve_sketch` because the linked libslvs
    /// will not produce the input that exercises them. Measured across every
    /// over-constrained arrangement probed in `tests/sketch_solve_tests.rs`, its
    /// failing report names at most one handle per declaration and returns them
    /// already ascending, so an integration fixture cannot distinguish this
    /// function from one with neither the dedup nor the sort. The collapse is
    /// still a real contract: `Fix` on a line emits two handles for one
    /// declaration, and an emit-path change that made libslvs name both would
    /// otherwise start printing the same `fix(...)` twice.
    ///
    /// The unattributable-handle arm is deliberately not exercised: it is a
    /// `debug_assert!(false, ..)`, so reaching it is a panic under `cfg(debug)`
    /// and a silent skip otherwise — a test would pin the profile, not the
    /// behaviour.
    fn handle_map_with(attribution: &[(u32, u32, u32)]) -> SketchHandleMap {
        let mut map = SketchHandleMap::new();
        map.set_attribution(
            attribution
                .iter()
                .map(|&(handle, id, span_start)| {
                    (
                        Slvs_hConstraint(handle),
                        (
                            SketchConstraintId(id),
                            SourceSpan::new(span_start, span_start + 4),
                        ),
                    )
                })
                .collect(),
        );
        map
    }

    #[test]
    fn resolve_failing_collapses_one_declarations_many_handles() {
        // Handles 10 and 11 are the two anchors a `Fix` on a line lowers to:
        // one declaration, id 7, one span.
        let map = handle_map_with(&[(10, 7, 70), (11, 7, 70), (12, 9, 90)]);

        let resolved = map.resolve_failing(&[
            Slvs_hConstraint(10),
            Slvs_hConstraint(11),
            Slvs_hConstraint(12),
        ]);

        assert_eq!(
            resolved,
            vec![
                (SketchConstraintId(7), SourceSpan::new(70, 74)),
                (SketchConstraintId(9), SourceSpan::new(90, 94)),
            ],
            "three failing handles over two declarations are two failing declarations"
        );
    }

    #[test]
    fn resolve_failing_sorts_ascending_by_declaration_id() {
        // Handle order deliberately the reverse of declaration order, which is
        // what the sort exists to normalize.
        let map = handle_map_with(&[(10, 9, 90), (11, 3, 30), (12, 6, 60)]);

        let resolved = map.resolve_failing(&[
            Slvs_hConstraint(10),
            Slvs_hConstraint(11),
            Slvs_hConstraint(12),
        ]);

        let ids: Vec<SketchConstraintId> = resolved.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            vec![
                SketchConstraintId(3),
                SketchConstraintId(6),
                SketchConstraintId(9)
            ],
            "the failing set is ordered by declaration id, not by slvs handle"
        );

        // Each id kept its own span through the sort.
        for (id, span) in &resolved {
            assert_eq!(*span, SourceSpan::new(id.0 * 10, id.0 * 10 + 4));
        }
    }
}
