//! Per-op trait inference for geometry expressions.
//!
//! Implements `docs/prds/geometry-traits.md` task 2: derive a
//! `Bounded`/`Connected`/`Convex` set for every geometry-typed expression so
//! that the conformance walker can validate `param g : Bounded`-style call
//! sites at compile time.
//!
//! # Design
//!
//! Inference is a **pure function** over `&CompiledExpr` rather than a
//! cached field on each `CompiledGeometryOp` variant. This is a deliberate
//! departure from the PRD's wording (see plan's design decision §1):
//! caching the set would require a 7-variant constructor refactor across
//! `geometry.rs`/`geometry_boolean.rs`/.../test fixtures, and the
//! conformance walker — currently the only consumer — recomputes cheaply
//! per call site. The conformance walker has a `&CompiledExpr` in hand
//! before any `CompiledGeometryOp` array is built, so dispatching by
//! function-call name is also strictly more useful than walking the op
//! array. If a future consumer needs an op-array walk or a cached set on
//! the IR (e.g. for serialization), it can be added additively without
//! breaking this module's public surface.
//!
//! # Public surface
//!
//! - [`InferredTraits`] — three-flag value type plus named constructors
//!   (`all`, `none`, `bounded_only`, `bounded_connected`).
//! - [`GeometryTrait`] — enum used by [`InferredTraits::has`] for diagnostic
//!   checks (`Bounded` / `Connected` / `Convex`).
//! - [`infer_primitive`] — per-`PrimitiveKind` lookup.
//! - `combine_union` / `combine_difference` / `combine_intersection` /
//!   `combine_transform` / `combine_modify` / `combine_pattern` /
//!   `combine_sweep` — pure pairwise/unary propagation rules.
//! - [`try_infer_traits_for_function_call`] — returns `Some(InferredTraits)`
//!   for every explicitly-dispatched function name, or `None` for the
//!   unknown-name fallback. Consumed by the coverage test in
//!   `crates/reify-compiler/tests/geometry_traits_inference_tests.rs`.
//! - [`infer_traits_for_expr`] — walks a `CompiledExpr` tree by FunctionCall
//!   name. This is the **primary** consumer-facing entry point: the conformance
//!   walker calls it from `crates/reify-compiler/src/conformance/mod.rs`.
//!
//! # TODO(geometry-traits-followup) / TODO(geometry-traits-task-4-or-later)
//!
//! The inference table only covers the primitives, combinators, and curve
//! constructors that exist on this branch. The PRD anticipates additional
//! Unbounded sources that have not yet been introduced; when they land, the
//! changes required here are localised.
//!
//! ## Unimplemented Unbounded primitives
//!
//! | Future construct        | Where it slots in                                   | Expected `InferredTraits`             |
//! |-------------------------|-----------------------------------------------------|---------------------------------------|
//! | `half_space(...)`       | `PrimitiveKind::HalfSpace` arm in [`infer_primitive`] / `"half_space"` arm in `infer_traits_for_function_call` | `InferredTraits { bounded: false, connected: true, convex: true }` |
//! | `extrude_infinite(...)` | `"extrude_infinite"` name routed to [`combine_sweep`] with an Unbounded profile, or a dedicated arm | `InferredTraits::none()`              |
//! | (parametric ray curve)  | New `"ray"`-style arm in `infer_traits_for_function_call` | `InferredTraits::none()` (or tuned)   |
//!
//! When an Unbounded source lands, double-check that every routing path the
//! conformance walker uses is updated. In particular, both the **direct**
//! arms (`box`, `cylinder`, `union`, `intersection`, `difference`, …) and
//! the **variadic** arms (`union_all`, `intersection_all`) share the same
//! `_ => all()` fallback, so an unknown name is silently treated as
//! Bounded — adding a new Unbounded primitive without an explicit arm
//! defeats the Bounded check.
//!
//! After the inference table is updated, add an end-to-end negative test
//! in `geometry_traits_inference_tests.rs` exercising the
//! `E_GEOMETRY_UNBOUNDED` emission path in
//! `crates/reify-compiler/src/conformance/mod.rs` against real source —
//! e.g. `Foo(g: half_space(...))` with `param g : Bounded`. The
//! conformance walker hook (`emit_leaf_conformance_for_arg_type` for
//! `Type::Geometry` + required-trait `"Bounded"`) is already in place and
//! will fire automatically once the inference reports the missing
//! `bounded` flag.

use crate::types::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_types::{CompiledExpr, CompiledExprKind, ValueCellId};

/// The closed v0.1 set of stdlib geometry-conformance marker trait names.
///
/// These are the seven pure marker traits declared in
/// `crates/reify-compiler/stdlib/geometry_traits.ri`; the set is fixed by
/// the stdlib's `§3.10 trait-decl surface`. When a structure explicitly
/// declares one of these as a trait bound, the compiler emits a
/// `W_TRAIT_USER_ASSERTED` warning (see `DiagnosticCode::TraitUserAsserted`).
///
/// Order is stable — matches the `EXPECTED_GEOMETRY_TRAITS` fixture in
/// `crates/reify-test-support/src/fixtures.rs` so parametric tests can
/// iterate both in the same order. Case-sensitive: Reify trait names are
/// PascalCase by convention.
pub const GEOMETRY_MARKER_TRAITS: &[&str] = &[
    "Bounded",
    "Closed",
    "Manifold",
    "Orientable",
    "Convex",
    "Connected",
    "Watertight",
];

/// Returns `true` iff `name` is one of the seven stdlib geometry-conformance
/// marker trait names (case-sensitive).
///
/// This is the detection predicate used by the `entity.rs` trait_bound
/// iteration to decide whether to emit a `W_TRAIT_USER_ASSERTED` warning.
/// Detection is name-based (not qualified-trait-resolution-based) — see
/// task 2321's design decision §1 for the rationale.
pub fn is_geometry_marker_trait(name: &str) -> bool {
    GEOMETRY_MARKER_TRAITS.contains(&name)
}

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

/// An environment that maps `ValueCellId`s to pre-resolved `InferredTraits`.
///
/// Implemented by the conformance walker to resolve geometry-typed `let`
/// bindings through the realization op-array. Returns `None` when the id is
/// not a geometry-typed let (or when the env has no information about the id),
/// in which case the caller falls back to `InferredTraits::all()` (safe-default).
pub trait LetBindingEnv {
    /// Look up the inferred traits for a value cell.
    ///
    /// Returns `Some(traits)` if the id is a known geometry-typed let binding
    /// whose traits can be resolved, or `None` to signal "unknown / use the
    /// safe default".
    fn lookup(&self, id: &ValueCellId) -> Option<InferredTraits>;
}

/// An empty `LetBindingEnv` that always returns `None`.
///
/// Used by the legacy `infer_traits_for_expr` wrapper and by any call site that
/// does not need let-binding resolution (e.g. the coverage unit tests).
pub struct EmptyLetEnv;

impl LetBindingEnv for EmptyLetEnv {
    fn lookup(&self, _id: &ValueCellId) -> Option<InferredTraits> {
        None
    }
}

/// Walk the inference table over a `CompiledExpr`.
///
/// This is the call-site form used by the conformance walker, which has a
/// `&CompiledExpr` argument in hand. The dispatch is **by function name**:
/// each stdlib geometry constructor (`box`, `cylinder`, ...) and each
/// combinator (`union`, `intersection`, `difference`, `union_all`,
/// `intersection_all`, `translate`, `rotate`, `scale`, `rotate_around`,
/// `fillet`, `chamfer`, `shell`, `draft`, `thicken`, `linear_pattern`,
/// `circular_pattern`, `mirror`, `linear_pattern_2d`, `arbitrary_pattern`,
/// `extrude`, `extrude_symmetric`, `revolve`, `revolve_full`, `sweep`,
/// `sweep_guided`, `loft`, `loft_guided`, `pipe`, plus curve constructors
/// `line_segment`, `arc`, `helix`, `interp`, `bezier`, `nurbs`) maps to
/// the matching primitive or `combine_*` helper.
///
/// # Geometry-arg recursion
///
/// For combinators, recurse on every argument that has `result_type ==
/// Type::Geometry` (boolean ops take two geometry args, transforms/modify/
/// pattern take one as the first geometry-typed arg, sweeps take a list
/// of profiles plus a path). Non-geometry args are skipped — their
/// inferred traits don't participate in the combine.
///
/// # Default-Bounded fallback
///
/// Any expression kind we don't recognise (`ValueRef`, `Literal` of a
/// geometry handle, `UserFunctionCall`, `MethodCall` returning geometry,
/// `Conditional`, `Match`, etc.) defaults to [`InferredTraits::all()`].
/// This is a deliberate **safe-default-Bounded** fallback: the conformance
/// walker only emits `E_GEOMETRY_UNBOUNDED` when the inferred set
/// **lacks** Bounded, so an opaque expression at a Bounded slot is
/// assumed to satisfy the bound. The alternative — defaulting to `none()`
/// — would produce spurious diagnostics on every `let g = box(...)`
/// indirected through a value-ref. Future work that reasons about
/// non-FunctionCall expressions (e.g. inferring traits through a
/// `let g : Solid = ...` binding) can extend the match arms additively.
pub fn infer_traits_for_expr(expr: &CompiledExpr) -> InferredTraits {
    infer_traits_for_expr_in_env(expr, &EmptyLetEnv)
}

/// Walk the inference table over a `CompiledExpr`, consulting `env` to resolve
/// `ValueRef` ids to pre-resolved trait sets.
///
/// The `ValueRef(id)` arm calls `env.lookup(id)` and falls back to
/// `InferredTraits::all()` when the env returns `None` — preserving the
/// existing safe-default behaviour for unbound refs.
///
/// All other arms behave identically to [`infer_traits_for_expr`]: the
/// dispatch table lives here and the env-less wrapper is a thin alias over
/// `EmptyLetEnv`.
pub fn infer_traits_for_expr_in_env(
    expr: &CompiledExpr,
    env: &dyn LetBindingEnv,
) -> InferredTraits {
    match &expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            infer_traits_for_function_call_in_env(function.name.as_str(), args, env)
        }
        CompiledExprKind::ValueRef(id) => env.lookup(id).unwrap_or(InferredTraits::all()),
        // Default-Bounded for every other expression kind. See the doc-comment
        // on `infer_traits_for_expr` for the rationale.
        _ => InferredTraits::all(),
    }
}

/// Walk the inference table over a compiled geometry op-array.
///
/// This is the **inverse** of the `infer_traits_for_expr` FunctionCall arm —
/// same dispatch table (same `combine_*` helpers and `infer_primitive` lookup)
/// but keyed by `CompiledGeometryOp` variant instead of function-call name.
///
/// # Root convention
///
/// The op-array represents a geometry construction tree where `ops.last()` is
/// the **root** (the final result). Recursion follows `GeomRef::Step(idx)` back
/// into earlier ops in the array.
///
/// # Safe defaults
///
/// - Empty array → `InferredTraits::all()` (defensive; compiler never emits an
///   empty realization).
/// - `GeomRef::Sub(_)` → `InferredTraits::all()` (safe-default: sub-component
///   geometry is not chased through the call stack; doing so would require
///   resolving cross-entity references that are out of scope here).
/// - Out-of-range `GeomRef::Step(idx)` → `InferredTraits::all()` (defensive;
///   well-formed IR never produces an out-of-range index).
/// - Self-referential or forward-chain `GeomRef::Step(idx)` (`idx >=
///   current_position`) → `InferredTraits::all()` (defensive; well-formed
///   SSA-style construction guarantees `idx < current_position`, so any
///   violation is malformed IR that would otherwise infinite-recurse).
pub fn infer_traits_for_op(ops: &[CompiledGeometryOp]) -> InferredTraits {
    match ops.last() {
        None => InferredTraits::all(),
        Some(op) => infer_op(op, ops, ops.len() - 1),
    }
}

/// Dispatch on a single `CompiledGeometryOp`, recursing back into `ops` for
/// `GeomRef::Step` references. Doc: see [`infer_traits_for_op`].
///
/// `current_position` is the index of `op` within `ops`. It is threaded
/// through all recursive calls so that `infer_geom_ref` can enforce the
/// `idx < current_position` SSA-style invariant.
fn infer_op(
    op: &CompiledGeometryOp,
    ops: &[CompiledGeometryOp],
    current_position: usize,
) -> InferredTraits {
    match op {
        CompiledGeometryOp::Primitive { kind, .. } => infer_primitive(*kind),

        CompiledGeometryOp::Boolean { op: bool_op, left, right } => {
            let a = infer_geom_ref(left, ops, current_position);
            let b = infer_geom_ref(right, ops, current_position);
            match bool_op {
                BooleanOp::Union => combine_union(a, b),
                BooleanOp::Difference => combine_difference(a, b),
                BooleanOp::Intersection => combine_intersection(a, b),
            }
        }

        CompiledGeometryOp::Modify { target, .. } => {
            combine_modify(infer_geom_ref(target, ops, current_position))
        }

        CompiledGeometryOp::Transform { target, .. } => {
            combine_transform(infer_geom_ref(target, ops, current_position))
        }

        CompiledGeometryOp::Pattern { target, .. } => {
            combine_pattern(infer_geom_ref(target, ops, current_position))
        }

        CompiledGeometryOp::Sweep { profiles, .. } => {
            // Sweep inherits traits from the first profile (bounded, connected
            // geometry swept along a finite path stays bounded and connected).
            let profile = profiles
                .first()
                .map(|r| infer_geom_ref(r, ops, current_position))
                .unwrap_or(InferredTraits::all());
            combine_sweep(profile)
        }

        // Curve constructors are 1-D primitives; return safe all() default.
        CompiledGeometryOp::Curve { .. } => InferredTraits::all(),
    }
}

/// Resolve a `GeomRef` within an op-array, returning the inferred traits for
/// the referenced op. `GeomRef::Sub(_)` returns the safe all() default.
///
/// # Cycle guard — `idx < current_position` invariant
///
/// Well-formed SSA-style op-arrays are constructed bottom-up: every
/// `GeomRef::Step(idx)` in a valid array refers to a strictly earlier op
/// (`idx < current_position`). A violation (`idx >= current_position`) means
/// the IR is malformed — either a self-reference (`idx == current_position`)
/// or a forward-chain that loops back. Both cases would infinite-recurse
/// without this guard, so we short-circuit to `InferredTraits::all()` to
/// match the safe-default convention used by every other defensive arm in
/// this module.
///
/// When recursing into a well-formed reference, `idx` becomes the new
/// `current_position`, tightening the invariant monotonically.
fn infer_geom_ref(
    geom_ref: &GeomRef,
    ops: &[CompiledGeometryOp],
    current_position: usize,
) -> InferredTraits {
    match geom_ref {
        GeomRef::Step(idx) => {
            // Cycle guard: reject self-referential or forward-chain Step.
            // Well-formed IR guarantees idx < current_position (SSA-style
            // bottom-up construction). Violation → safe default all().
            if *idx >= current_position {
                return InferredTraits::all();
            }
            // Defensive: if idx is out of range (malformed IR), default to all().
            ops.get(*idx)
                .map(|op| infer_op(op, ops, *idx))
                .unwrap_or(InferredTraits::all())
        }
        // Sub-component geometry is not chased through the call stack.
        GeomRef::Sub(_) => InferredTraits::all(),
    }
}

/// Dispatch on the function-call name. Returns `Some(InferredTraits)` for
/// every explicitly-dispatched name, or `None` for the unknown-name fallback.
///
/// Each known arm either returns a constant trait set for a primitive, or
/// recurses on the geometry-typed arguments and folds the matching
/// `combine_*` rule.
///
/// # Coverage contract
///
/// This function is consumed by the coverage test
/// `every_geometry_function_name_has_explicit_dispatch_arm` in
/// `crates/reify-compiler/tests/geometry_traits_inference_tests.rs`.
/// That test iterates `crate::GEOMETRY_FUNCTION_NAMES` and asserts `Some(_)`
/// for each name. Adding a name to `GEOMETRY_FUNCTION_NAMES` without a
/// corresponding arm here causes the test to fail loudly.
///
/// # `None` arm
///
/// `None` is the return value of the `_ =>` arm — the single, audited place
/// where an unknown name would fall back to Bounded. The private wrapper
/// [`infer_traits_for_function_call_in_env`] maps `None` to `InferredTraits::all()`.
pub fn try_infer_traits_for_function_call(
    name: &str,
    args: &[CompiledExpr],
) -> Option<InferredTraits> {
    try_infer_traits_for_function_call_in_env(name, args, &EmptyLetEnv)
}

/// Env-aware dispatch on the function-call name. Returns `Some(InferredTraits)`
/// for every explicitly-dispatched name, or `None` for the unknown-name fallback.
///
/// The dispatch table lives here; the env-less
/// [`try_infer_traits_for_function_call`] is a thin alias over `EmptyLetEnv`.
/// The `env` is threaded into every geometry-arg recursive call so that
/// `ValueRef` ids inside arg positions resolve via the env.
pub fn try_infer_traits_for_function_call_in_env(
    name: &str,
    args: &[CompiledExpr],
    env: &dyn LetBindingEnv,
) -> Option<InferredTraits> {
    match name {
        // ─── Primitive constructors → all() ─────────────────────────────
        "box" | "cylinder" | "sphere" | "tube" => Some(InferredTraits::all()),

        // ─── Boolean combinators → recurse + combine_* ──────────────────
        "union" => {
            let (a, b) = first_two_geometry_args_in_env(args, env);
            Some(combine_union(a, b))
        }
        "difference" => {
            let (a, b) = first_two_geometry_args_in_env(args, env);
            Some(combine_difference(a, b))
        }
        "intersection" => {
            let (a, b) = first_two_geometry_args_in_env(args, env);
            Some(combine_intersection(a, b))
        }

        // ─── Variadic Boolean combinators → fold combine_* across args ──
        //
        // `union_all` / `intersection_all` are recognised by
        // [`crate::units::is_geometry_function`] and routed here from the
        // conformance walker. Without explicit arms they would silently take
        // the unknown-name `_ => None` fallback below, which is harmless
        // today (every primitive is Bounded) but defeats the Bounded check
        // the moment `half_space` / `extrude_infinite` lands and is fed
        // through `union_all`. We fold the matching pairwise rule across
        // every geometry-typed argument; an empty geometry-arg list defaults
        // to `all()` (defensive — well-formed source always supplies at
        // least one argument).
        "union_all" => Some(fold_geometry_args_in_env(args, combine_union, env)),
        "intersection_all" => {
            Some(fold_geometry_args_in_env(args, combine_intersection, env))
        }

        // ─── Transform combinators → recurse + combine_transform ────────
        "translate" | "rotate" | "scale" | "rotate_around" => {
            let t = first_geometry_arg_in_env(args, env);
            Some(combine_transform(t))
        }

        // ─── Modify combinators → recurse + combine_modify ──────────────
        "fillet" | "chamfer" | "shell" | "draft" | "thicken" => {
            let t = first_geometry_arg_in_env(args, env);
            Some(combine_modify(t))
        }

        // ─── Pattern combinators → recurse + combine_pattern ────────────
        "linear_pattern"
        | "circular_pattern"
        | "mirror"
        | "linear_pattern_2d"
        | "arbitrary_pattern" => {
            let t = first_geometry_arg_in_env(args, env);
            Some(combine_pattern(t))
        }

        // ─── Sweep combinators → recurse + combine_sweep ────────────────
        "extrude" | "extrude_symmetric" | "revolve" | "revolve_full" | "sweep"
        | "sweep_guided" | "loft" | "loft_guided" | "pipe" => {
            let t = first_geometry_arg_in_env(args, env);
            Some(combine_sweep(t))
        }

        // ─── Curve constructors → all() (1-D primitives) ────────────────
        "line_segment" | "arc" | "helix" | "interp" | "bezier" | "nurbs" => {
            Some(InferredTraits::all())
        }

        // Unknown function name → None. The private wrapper maps this to
        // `InferredTraits::all()` (default-Bounded). This is the single
        // audited place where an unrecognised name falls back to Bounded —
        // see the "Default-Bounded fallback" section in the `infer_traits_for_expr`
        // doc-comment above.
        _ => None,
    }
}

/// Thin private wrapper: dispatch via [`try_infer_traits_for_function_call_in_env`]
/// and collapse `None` to `InferredTraits::all()`.
///
/// This is the single, audited place where unknown geometry function names
/// fall back to fully-Bounded. The `try_*` companion returns `None` precisely
/// for the `_ =>` arm, so the coverage test can detect the gap without any
/// `#[cfg(test)]` branches in production code.
fn infer_traits_for_function_call_in_env(
    name: &str,
    args: &[CompiledExpr],
    env: &dyn LetBindingEnv,
) -> InferredTraits {
    try_infer_traits_for_function_call_in_env(name, args, env).unwrap_or(InferredTraits::all())
}

/// Find the first geometry-typed argument and recurse with the env, defaulting
/// to `InferredTraits::all()` if no geometry arg is present (defensive — a
/// well-formed call site always has one).
fn first_geometry_arg_in_env(args: &[CompiledExpr], env: &dyn LetBindingEnv) -> InferredTraits {
    args.iter()
        .find(|a| a.result_type == reify_types::Type::Geometry)
        .map(|a| infer_traits_for_expr_in_env(a, env))
        .unwrap_or(InferredTraits::all())
}

/// Fold `combine` across every geometry-typed argument with env threading
/// (used by the variadic `union_all` / `intersection_all` dispatch arms).
/// Returns `InferredTraits::all()` when no geometry arg is present, matching
/// the defensive default used by the unary/binary helpers above.
fn fold_geometry_args_in_env(
    args: &[CompiledExpr],
    combine: fn(InferredTraits, InferredTraits) -> InferredTraits,
    env: &dyn LetBindingEnv,
) -> InferredTraits {
    args.iter()
        .filter(|a| a.result_type == reify_types::Type::Geometry)
        .map(|a| infer_traits_for_expr_in_env(a, env))
        .reduce(combine)
        .unwrap_or(InferredTraits::all())
}

/// Find the first two geometry-typed arguments and recurse on each with the
/// env, defaulting to `InferredTraits::all()` if either is missing.
fn first_two_geometry_args_in_env(
    args: &[CompiledExpr],
    env: &dyn LetBindingEnv,
) -> (InferredTraits, InferredTraits) {
    let mut iter = args
        .iter()
        .filter(|a| a.result_type == reify_types::Type::Geometry);
    let a = iter
        .next()
        .map(|a| infer_traits_for_expr_in_env(a, env))
        .unwrap_or(InferredTraits::all());
    let b = iter
        .next()
        .map(|a| infer_traits_for_expr_in_env(a, env))
        .unwrap_or(InferredTraits::all());
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::{is_geometry_marker_trait, GEOMETRY_MARKER_TRAITS};

    /// `GEOMETRY_MARKER_TRAITS` must agree with the shared test-fixture
    /// `EXPECTED_GEOMETRY_TRAITS` and `is_geometry_marker_trait` must accept
    /// every name in that fixture.  Driving the assertion off
    /// `EXPECTED_GEOMETRY_TRAITS` rather than a third inline copy means any
    /// divergence between the two independently-maintained lists surfaces here
    /// rather than silently passing.
    #[test]
    fn is_geometry_marker_trait_recognises_each_of_the_seven_stdlib_names() {
        let expected = reify_test_support::EXPECTED_GEOMETRY_TRAITS;
        assert_eq!(
            GEOMETRY_MARKER_TRAITS.len(),
            expected.len(),
            "GEOMETRY_MARKER_TRAITS length mismatch against EXPECTED_GEOMETRY_TRAITS: {:?}",
            GEOMETRY_MARKER_TRAITS
        );
        for name in expected {
            assert!(
                is_geometry_marker_trait(name),
                "expected is_geometry_marker_trait({name:?}) == true, but got false"
            );
        }
    }

    /// Non-marker names — including lowercase variants — must return `false`.
    /// Case-sensitivity is by design: Reify trait names are PascalCase.
    #[test]
    fn is_geometry_marker_trait_rejects_non_marker_names() {
        let non_markers = ["Container", "Material", "Elastic", "watertight", ""];
        for name in &non_markers {
            assert!(
                !is_geometry_marker_trait(name),
                "expected is_geometry_marker_trait({name:?}) == false, but got true"
            );
        }
    }
}
