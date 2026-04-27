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

use crate::types::PrimitiveKind;

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
