//! Per-op trait inference for geometry expressions.
//!
//! Implements `docs/prds/geometry-traits.md` task 2: derive a
//! `Bounded`/`Connected`/`Convex` set for every geometry-typed expression so
//! that the conformance walker can validate `param g : Bounded`-style call
//! sites at compile time.
//!
//! # Design
//!
//! Inference is a **pure function** over `&[CompiledGeometryOp]` and
//! `&CompiledExpr` rather than a cached field on each `CompiledGeometryOp`
//! variant. This is a deliberate departure from the PRD's wording (see plan's
//! design decision §1): caching the set would require a 7-variant constructor
//! refactor across `geometry.rs`/`geometry_boolean.rs`/.../test fixtures, and
//! the conformance walker — currently the only consumer — recomputes cheaply
//! per call site. If a future consumer needs the cached set on the IR (e.g.
//! for serialization), it can be added additively without breaking this
//! module's public surface.
//!
//! # Public surface
//!
//! - [`InferredTraits`] — three-flag value type plus named constructors
//!   (`all`, `none`, `bounded_only`, `bounded_connected`).
//! - [`GeometryTrait`] — enum used by [`InferredTraits::has`] for diagnostic
//!   checks (`Bounded` / `Connected` / `Convex`).
//!
//! Subsequent steps in this task add the lookup helpers
//! (`infer_primitive`, `combine_*`, `infer_traits_for_op`,
//! `infer_traits_for_expr`).
//!
//! # TODO(geometry-traits-task-4-or-later)
//!
//! When `half_space` / `extrude_infinite` (or any other Unbounded primitive)
//! land, extend `infer_primitive` to return an `InferredTraits` lacking
//! `bounded` for those `PrimitiveKind` variants and add the corresponding
//! end-to-end negative test exercising
//! `crates/reify-compiler/src/conformance/mod.rs`'s
//! `E_GEOMETRY_UNBOUNDED` emission for real source.

use crate::types::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PatternKind, PrimitiveKind,
    SweepKind, TransformKind,
};

/// The three compile-time-inferred geometry traits.
///
/// Names mirror the PRD; only these three are tracked because the remaining
/// stdlib geometry traits (`Closed`, `Manifold`, `Watertight`) are
/// runtime/topology properties that the compiler cannot determine from the
/// IR shape alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryTrait {
    /// Finite extent — every coordinate is bounded.
    Bounded,
    /// Single connected component (no disjoint pieces).
    Connected,
    /// Convex point-set (every line segment between two points stays inside).
    Convex,
}

/// Compile-inferred trait set for a geometry expression.
///
/// The three flags are independent — any subset is reachable. Use the named
/// constructors below for the common subsets; bespoke combinations can use
/// struct-literal construction directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InferredTraits {
    /// Whether the geometry has finite extent.
    pub bounded: bool,
    /// Whether the geometry is a single connected component.
    pub connected: bool,
    /// Whether the geometry is convex.
    pub convex: bool,
}

impl InferredTraits {
    /// All three flags set — the safe-default for primitives whose semantics
    /// satisfy every compile-inferred trait (`box`, `cylinder`, `sphere`,
    /// `tube`).
    pub const fn all() -> Self {
        Self {
            bounded: true,
            connected: true,
            convex: true,
        }
    }

    /// All three flags cleared — used for sources that fail every check
    /// (e.g. a future `half_space` primitive).
    pub const fn none() -> Self {
        Self {
            bounded: false,
            connected: false,
            convex: false,
        }
    }

    /// Only `bounded` set — typical Boolean-result shape (Union/Intersection
    /// of bounded inputs preserves Bounded but cannot guarantee Connected or
    /// Convex from the IR alone).
    pub const fn bounded_only() -> Self {
        Self {
            bounded: true,
            connected: false,
            convex: false,
        }
    }

    /// `bounded` and `connected` set — typical Modify-result shape (Fillet,
    /// Chamfer, Shell, Draft, Thicken preserve Bounded+Connected but not
    /// Convex).
    pub const fn bounded_connected() -> Self {
        Self {
            bounded: true,
            connected: true,
            convex: false,
        }
    }

    /// Look up the flag for a [`GeometryTrait`] kind. Used by the
    /// conformance walker's diagnostic emit path so the same enum kind drives
    /// both the inference table and the call-site check.
    pub const fn has(&self, kind: GeometryTrait) -> bool {
        match kind {
            GeometryTrait::Bounded => self.bounded,
            GeometryTrait::Connected => self.connected,
            GeometryTrait::Convex => self.convex,
        }
    }
}

/// Look up the inferred traits for a primitive geometry kind.
///
/// All four current variants (`Box`, `Cylinder`, `Sphere`, `Tube`) are
/// fully Bounded+Connected+Convex.
///
/// # Future variants
///
/// When PRD `geometry-traits.md` adds `half_space` and `extrude_infinite`,
/// extend this match to return `InferredTraits::none()` (or a tuned subset
/// such as `convex`-only) for those kinds. The exhaustive `match` will
/// fail to compile against the un-updated arm, so the maintenance is
/// localised.
pub const fn infer_primitive(kind: PrimitiveKind) -> InferredTraits {
    match kind {
        PrimitiveKind::Box
        | PrimitiveKind::Cylinder
        | PrimitiveKind::Sphere
        | PrimitiveKind::Tube => InferredTraits::all(),
    }
}

/// Boolean union propagation rule.
///
/// `bounded` is preserved iff **both** operands are bounded — an unbounded
/// operand contributes its unboundedness to the union. `connected` and
/// `convex` are always dropped: the union of two disjoint connected
/// pieces is disconnected, and the union of two convex sets is generally
/// not convex (and the IR cannot tell whether they overlap).
pub const fn combine_union(a: InferredTraits, b: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: a.bounded && b.bounded,
        connected: false,
        convex: false,
    }
}

/// Boolean difference propagation rule.
///
/// `bounded` is inherited from the **left** (cuttee) operand: subtracting
/// any cutter from a bounded body stays bounded. `connected` and `convex`
/// are dropped: cutting can produce disjoint or non-convex remainders.
pub const fn combine_difference(left: InferredTraits, _right: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: left.bounded,
        connected: false,
        convex: false,
    }
}

/// Boolean intersection propagation rule.
///
/// `bounded` is preserved if **either** operand is bounded (the bounded
/// one bounds the intersection from the outside). `convex` is preserved
/// iff **both** operands are convex (the intersection of two convex sets
/// is convex). `connected` is dropped: intersection can produce disjoint
/// pieces.
pub const fn combine_intersection(a: InferredTraits, b: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: a.bounded || b.bounded,
        connected: false,
        convex: a.convex && b.convex,
    }
}

/// Transform propagation rule (translate/rotate/scale/rotate_around).
///
/// All three traits are preserved: rigid motions and uniform scaling are
/// bijective continuous maps (and convexity-preserving). The IR-level
/// inference does not distinguish between transform variants — the rule
/// is a single all-preserving identity.
pub const fn combine_transform(input: InferredTraits) -> InferredTraits {
    input
}

/// Modify propagation rule (fillet/chamfer/shell/draft/thicken).
///
/// `bounded` and `connected` are preserved (modify ops are local
/// single-body operations on a single solid). `convex` is dropped:
/// shelling, drafting, and even filleting can produce non-convex
/// remainders.
pub const fn combine_modify(input: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: input.bounded,
        connected: input.connected,
        convex: false,
    }
}

/// Pattern propagation rule (linear/circular/mirror/linear_2d/arbitrary).
///
/// `bounded` is preserved (a finite pattern of bounded inputs stays
/// bounded). `connected` is always dropped (multiple disjoint copies).
/// `convex` is dropped (multiple convex pieces ≠ one convex set).
pub const fn combine_pattern(input: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: input.bounded,
        connected: false,
        convex: false,
    }
}

/// Sweep propagation rule (loft/extrude/revolve/sweep/extrude_symmetric/
/// sweep_guided/loft_guided/pipe).
///
/// `bounded` and `connected` are inherited from the **profile** (a
/// bounded, connected profile swept along a finite path stays bounded
/// and connected). `convex` is always dropped: even a convex profile
/// swept along a curved path produces a non-convex solid in general.
pub const fn combine_sweep(profile: InferredTraits) -> InferredTraits {
    InferredTraits {
        bounded: profile.bounded,
        connected: profile.connected,
        convex: false,
    }
}

/// Look up the inferred traits for a curve constructor.
///
/// Curves are 1-D primitives consumed as sweep inputs. All current
/// `CurveKind` variants (line_segment, arc, helix, interp, bezier, nurbs)
/// are treated as Bounded+Connected+Convex: the propagation through
/// `combine_sweep` will drop Convex anyway, so encoding all curves as
/// `all()` keeps the table uniform and lets `combine_sweep` remain the
/// single decision point for sweep-output convexity. (A future infinite
/// curve, e.g. a parametric ray, would slot in here as `none()` or a
/// tuned subset.)
pub const fn infer_curve(kind: CurveKind) -> InferredTraits {
    match kind {
        CurveKind::LineSegment
        | CurveKind::Arc
        | CurveKind::Helix
        | CurveKind::InterpCurve
        | CurveKind::BezierCurve
        | CurveKind::NurbsCurve => InferredTraits::all(),
    }
}

/// Resolve a single `GeomRef` (Step or Sub) within an op array to its
/// inferred trait set.
///
/// `GeomRef::Step(i)` recurses into [`infer_traits_for_op`] with index
/// `i`. `GeomRef::Sub(_)` returns [`InferredTraits::all()`] — the safe
/// v0.1 default for cross-component references.
///
/// # Design decision (plan §3, pinned by test)
///
/// Cross-component inference (resolving the sub's own realization to a
/// real trait set) is intentionally deferred: structure instances built
/// from primitives are typically bounded, so defaulting to "all three"
/// avoids spurious `E_GEOMETRY_UNBOUNDED` at every cross-component
/// boundary. The integration test
/// `infer_traits_for_op_treats_geomref_sub_as_bounded` pins this
/// behaviour so a future change to cross-component inference becomes a
/// deliberate, observable diff rather than silent drift.
fn resolve_geom_ref(geom_ref: &GeomRef, ops: &[CompiledGeometryOp]) -> InferredTraits {
    match geom_ref {
        GeomRef::Step(idx) => infer_traits_for_op(*idx, ops),
        GeomRef::Sub(_) => InferredTraits::all(),
    }
}

/// Walk the inference table over a `CompiledGeometryOp` array.
///
/// Returns the [`InferredTraits`] for the operation at index `op_idx` by
/// recursively resolving its inputs through [`resolve_geom_ref`] and
/// applying the matching `combine_*` rule. Suppress visited tracking
/// (and the resulting recursion guard) is unnecessary because compiled
/// op arrays are acyclic by construction — a `GeomRef::Step(j)` always
/// satisfies `j < op_idx` (op compilation is forward-only).
///
/// # Panics
///
/// Panics if `op_idx >= ops.len()` (use a debug-only bounds check via
/// indexing). Callers are responsible for passing a valid index.
pub fn infer_traits_for_op(op_idx: usize, ops: &[CompiledGeometryOp]) -> InferredTraits {
    match &ops[op_idx] {
        CompiledGeometryOp::Primitive { kind, .. } => infer_primitive(*kind),
        CompiledGeometryOp::Boolean { op, left, right } => {
            let l = resolve_geom_ref(left, ops);
            let r = resolve_geom_ref(right, ops);
            match op {
                BooleanOp::Union => combine_union(l, r),
                BooleanOp::Difference => combine_difference(l, r),
                BooleanOp::Intersection => combine_intersection(l, r),
            }
        }
        CompiledGeometryOp::Modify { kind, target, .. } => {
            let t = resolve_geom_ref(target, ops);
            // ModifyKind variants share the same propagation rule today;
            // a future variant with different semantics (e.g. a Modify
            // that re-introduces convexity) can branch here.
            let _: ModifyKind = *kind;
            combine_modify(t)
        }
        CompiledGeometryOp::Transform { kind, target, .. } => {
            let t = resolve_geom_ref(target, ops);
            let _: TransformKind = *kind;
            combine_transform(t)
        }
        CompiledGeometryOp::Pattern { kind, target, .. } => {
            let t = resolve_geom_ref(target, ops);
            let _: PatternKind = *kind;
            combine_pattern(t)
        }
        CompiledGeometryOp::Sweep {
            kind, profiles, ..
        } => {
            let _: SweepKind = *kind;
            // Sweep takes Bounded+Connected from the **profile**. When
            // multiple profiles are supplied (loft), conservatively
            // intersect the trait sets — a profile lacking Bounded
            // poisons the whole sweep, and Connected requires every
            // profile to be connected. A missing profile defaults to
            // `all()` so empty `profiles` arrays do not under-bound the
            // result.
            let profile_traits = profiles
                .iter()
                .map(|p| resolve_geom_ref(p, ops))
                .reduce(|acc, next| InferredTraits {
                    bounded: acc.bounded && next.bounded,
                    connected: acc.connected && next.connected,
                    convex: acc.convex && next.convex,
                })
                .unwrap_or(InferredTraits::all());
            combine_sweep(profile_traits)
        }
        CompiledGeometryOp::Curve { kind, .. } => infer_curve(*kind),
    }
}
