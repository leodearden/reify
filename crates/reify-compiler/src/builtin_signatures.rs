//! Per-argument type signatures for geometry topology-selector builtins
//! (task 4493, type-hygiene ζ).
//!
//! Hosts the checkable argument-slot table ([`builtin_arg_slots`]) and the
//! call-site checker ([`check_builtin_arg_types`]) for the geometry
//! topology-selector family.  The mechanism is generic (name-keyed and, since
//! task 5652, arity-keyed), so a few non-selector keys are hosted here too
//! rather than in a parallel checker — they are enumerated and justified in
//! `tests::NON_SELECTOR_ARG_SLOT_KEYS`.  Math args (polymorphic, no fixed dimension)
//! and geometry-handle arg0 (ε=4358's territory, PRD §4 out-of-scope) are
//! intentionally absent.
//!
//! # Design: sibling of `math_signatures.rs`
//!
//! Placed beside `math_signatures.rs` per PRD open-question-3 (implementer's
//! choice): the arg-slot table covers the geometry family, not the math-linalg
//! §3 family, so folding it into the frozen `math_signatures` contract would be
//! a misnomer.  The module structure mirrors `math_signatures.rs`: a public-to-
//! crate name-keyed match function + a small set of supporting types.
//!
//! # What the table covers, and the rules that decide it (decision-4 gradualism)
//!
//! The AUTHORITATIVE per-slot statement — index, argument name, dimension, and
//! why each neighbouring position is or is not gated — lives on the match arms
//! of [`builtin_arg_slots`], beside the code it describes. This section states
//! only the RULES that decide membership: restating the slots here as well
//! would be a second copy that nothing machine-checks, and it would drift the
//! first time a leaf adds or renames one. Read the arms for the slots.
//!
//! COVERED families:
//! - The geometry TOPOLOGY SELECTORS this table was built for (task 4493) —
//!   the mass-properties `density`, and the directional / height / extremal
//!   `tol` arguments.
//! - `generate`'s Int count (task 3994) — the lone non-geometry, non-`Scalar`
//!   slot. It hosts here only because the mechanism is generic (name-keyed),
//!   which avoids standing up a parallel checker for one slot.
//! - Pattern SPACING (task 5652) — `linear_pattern` / `linear_pattern_2d`.
//! - The pattern ORIGIN triple (task 5662) — `circular_pattern`'s 9-arg scalar
//!   form, `ox`/`oy`/`oz`. The row task 5652 deferred; it mirrors the
//!   eval-layer gate on the circular-pattern axis origin (task 5350). `mirror`'s
//!   7-arg triple was its pair until direction-typing ι-a retired that form;
//!   see the NOT-covered list below.
//! - The Contract C LENGTH families of PRD
//!   `docs/prds/v0_6/units-length-gate-completion.md` leaf η (task 5750):
//!   PRIMITIVE and PROFILE producers, MODIFY, SWEEP, and the TRANSFORM row
//!   that contract C6 forced in alongside them. These mirror, at the compile
//!   layer, the positions the eval-layer LENGTH chokepoint
//!   (`crates/reify-ir/src/arg_acceptance.rs`) rejects a bare number at. That
//!   set is no longer enumerated in prose: the Contract C closure guard,
//!   `crates/reify-eval/tests/harness_geometry/units_length_closure_guard.rs`,
//!   sweeps every builtin in `GEOMETRY_FUNCTION_NAMES` and asserts each numeric
//!   position is either rejected there or carries a justification row.
//! - The ANGLE producer positions of PRD
//!   `docs/prds/v0_6/angle-units-surface-convergence.md` leaf ζ (task 5782):
//!   the transform, sweep, curve, modify and pattern producers whose angle
//!   occupies a real positional index at the call site, all built by
//!   [`angle_arg`].
//! - The Euler builtins' `convention` slot (task #6082) — `orient_euler` and
//!   `orient_to_euler`. SO(3) builtins, not geometry; like `generate` they host
//!   here only because the mechanism is name-keyed and generic. They are the
//!   table's only `ExpectedArg::Enum` slots.
//! - The `ExpectedArg::Direction` slots, all built by [`direction_arg`]:
//!   `frame_at`'s `x`/`z` (direction-typing σ), the axis of the four axial
//!   joint ctors and the 13 PRB flexure ctors (direction-typing ζ-a), and the
//!   direction of the four directional selectors plus arity-2 `angle`'s
//!   `a`/`b` (direction-typing θ-b), the axes of `orient_axis_angle`,
//!   `orient_look_at` and `orient_basis` (direction-typing θ-a), rotate's and
//!   rotate_around's axis and mirror's normal (direction-typing ι-a), and the
//!   direction of `revolve` / `revolve_full` / `extrude_infinite` /
//!   `extrude_infinite_symmetric` (direction-typing ι-b), and `half_space`'s
//!   and `arc`'s normal (direction-typing ι-c); later leaves add rows on the
//!   same constructor.
//!
//! The RULES that decide whether a position inside a covered family gets a
//! slot — each one is why some argument sitting right next to a slotted one is
//! deliberately bare:
//!
//! - **arg0 of a geometry builtin is never checked.** A geometry handle is
//!   ε=4358's territory. A builtin whose arg0 is not a handle (`orient_euler`,
//!   the joint ctors' axis) may slot it.
//! - **ORIGIN vs DIRECTION.** A position in space written as scalar
//!   components (`translate`'s displacement) IS a LENGTH — a bare component is
//!   silently read as SI metres, the 1000×-too-big hazard this whole gate
//!   exists to close. A point passed as one composite `Point3` VALUE
//!   (`rotate_around`'s pivot, `mirror`'s and `half_space`'s origin) has no
//!   slot kind and is gated at build instead; see the NOT-covered list below. A
//!   unit-vector DIRECTION written as scalar components (`ax`/`ay`/`az`, a
//!   pattern's `dx`/`dy`/`dz`) is dimensionless and legitimately bare in
//!   correct `.ri`, so a slot there would reject valid code. (`revolve`,
//!   `extrude_infinite` and `half_space` no longer take scalar triples: since
//!   direction-typing ι-b and ι-c an origin is a Point3 value, gated at build,
//!   and a direction or normal a `Direction` slot.) Stated
//!   binding by the `UnitVectorComponent` rows of the closure guard's
//!   `ALLOWLIST`. The builtins whose arguments STRADDLE this line carry the
//!   split on their own arm — deliberately UNCOUNTED here, because a tally
//!   stated away from the arms is exactly the second copy nothing
//!   machine-checks that this section opens by warning against. It read
//!   "three" until task 5662 added the fourth and fifth. The rule governs bare
//!   scalar COMPONENT triples; a composite `Direction`-typed argument is
//!   slotted by `ExpectedArg::Direction`.
//! - **COUNTS, face INDICES and dimensionless RATIOS are never slots.** A
//!   wrong `count`, `face_{i}` or `scale` factor is an arity or semantic
//!   error, not a dimension one, and a LENGTH slot on a ratio would reject
//!   correct `.ri` outright.
//! - **ANGLE positions ARE gated here, on one hint-carrying template.** PRD
//!   `docs/prds/v0_6/angle-units-surface-convergence.md` leaf ζ (task 5782)
//!   executed the seam decree this rule used to DEFER to: every angle that
//!   occupies a real positional index at a call site is slotted, built by
//!   [`angle_arg`] so the wording cannot fork (decision D11), and the
//!   pre-existing selector `tol` slots moved onto that same template in the
//!   same change. Two angles are still absent, and neither is a gap: an axis
//!   DIRECTION component is dimensionless (the ORIGIN-vs-DIRECTION rule above),
//!   and `revolve_full(profile, origin, direction)`'s 2π is SYNTHESIZED at
//!   lowering rather than written by an author, so there is no positional
//!   index to key on.
//! - **Polymorphic and coercing slots stay out.** Math args (no fixed
//!   dimension) and Range slots (`edges_by_length` / `faces_by_area`).
//! - **Indirect gating is not gating.** `rounded_box` / `rounded_rect` /
//!   `zone_cylinder` / `zone_annulus` / `zone_profile` DESUGAR into
//!   Box/Cylinder/Rectangle/Circle/Pipe/Thicken ops, so their own surface
//!   arguments never reach a gated position under these names. A slot here
//!   would be NEW coverage whose arg name disagrees with whatever the eval
//!   layer reports for the desugared op — breaking the same-wording premise
//!   the migration hint depends on (PRD decision D9). (`zone_annulus`' arg3
//!   `length` is additionally DEAD: `geometry.rs` binds it to `_l`.)
//! - **Names with no checkable argument have no arm at all**
//!   (`split`, `face`, `edge`, `solid_body`, `volume`, `edges`, `faces`, …).
//!
//! # Contract C positions this table deliberately does NOT cover (task 5750)
//!
//! Task 5750 (units-length η) closed the four families its PRD leaf names —
//! primitive, profile, modify, sweep — plus the transform row that contract C6
//! forced into it. What remains OUTSIDE the table is recorded here rather than
//! left to be rediscovered, and each entry states WHY, so a reader can tell a
//! decision from an oversight. These are PROSE, not `TODO`s, on purpose: under
//! the repo's PTODO grammar a `TODO` must cite a live non-terminal task, and
//! NONE of these has one. (The `mirror`/`circular_pattern` row was the last
//! that did; task 5662 landed those slots, so that row moved UP into the
//! COVERED list above and its `TODO` retired with it — this table now carries
//! no live `TODO` at all.) Task 5752 (leaf ι, the Contract C closure guard) is the
//! backstop that will surface any of these if they are ever forgotten.
//!
//! - The task-5623 CURVE row — `line_segment`'s endpoints `x1`…`z2`, `arc`'s
//!   `radius`, and `helix`'s `radius`/`pitch`/`height`. These ARE
//!   index-addressable and ARE Contract C gated at eval, so they are eligible
//!   in the mechanical sense; they are simply not among the families task
//!   5750's leaf scoped, and no fixture forced the decision. A later leaf can
//!   add them by following the arms above verbatim. `arc`'s two ANGLES are the
//!   exception: PRD 3 leaf ζ (task 5782) slotted them, which leaves its radius
//!   — and `line_segment` and `helix` — outside the table, unchanged. `arc`'s
//!   centre is a Point3 (direction-typing ι-c), listed with the other Point3
//!   origins below.
//!
//! - The ARITY-OPEN variadic route — `polygon`'s 2-D vertex pairs, `interp`
//!   and `bezier`'s coordinate triples, and `nurbs`' `2 .. 2 + 3·n_points`
//!   pole span. Gated at eval since tasks 5658/5661 through
//!   `accept_variadic_length_args`, but NOT expressible as an index-keyed
//!   [`CheckableArg`] at all: the gated span is computed from an ARGUMENT
//!   (`n_points`), and the compile-layer names for these positions are the
//!   inert `c0`…`cN` that `geometry.rs` synthesises — so even a hand-written
//!   slot would word the mistake differently from the eval layer. This one is
//!   a structural exclusion, not a deferral.
//!
//! - The task-5745 DECODED-VALUE route — `decode_plane` / `decode_axis`
//!   origins and the `nurbs_surface` control-point grid. Also structural: a
//!   position on this route arrives already assembled into a composite `Value`
//!   by a stdlib producer (`plane_yz(10mm)` → `Value::Plane`), so it never
//!   passes through a positional argument index for this table to key on.
//!
//! - The `Point3` origin of `mirror(target, origin, normal)` and pivot of
//!   `rotate_around(target, pivot, axis, angle)` (direction-typing ι-a), and
//!   the origin of `half_space(origin, normal)` and centre of
//!   `arc(center, radius, start_angle, end_angle, normal)` (direction-typing
//!   ι-c).
//!   Structural: [`ExpectedArg`] has no Point kind, so `accept_length_point3`
//!   gates the point at build. An accepted narrowing, recorded in the decompose
//!   amendments of `docs/prds/v0_6/direction-typing.md` §8.
//!
//! # Known: a NESTED geometry argument is WALKED twice (task 5750)
//!
//! MEASURED with `target/debug/reify check`, task 5750, BEFORE the duplicate
//! drop described below:
//!
//! ```text
//! extrude(circle(4), 12mm)                      → 2 × "circle: radius …"
//! let c = circle(4); extrude(c, 12mm)           → 1 ×
//! let c = circle(4)                             → 1 ×
//! linear_pattern(box(10,10,10), 1,0,0,3,20mm)   → 2 × each box axis
//! linear_pattern(box(10mm,10mm,10mm), …, 20)    → 1 × "linear_pattern: spacing …"
//! ```
//!
//! So a call whose slots fire duplicates its diagnostics when it appears as a
//! NESTED geometry ARGUMENT, and reports once when let-bound or top-level. The
//! OUTER call of a nested expression is unaffected — which is what
//! `nested_linear_pattern_bare_spacing_emits_exactly_one_diagnostic` in
//! `tests/builtin_arg_signature_tests.rs` already pins.
//!
//! This PRE-DATES task 5750 and is not caused by these slots. The same shape is
//! measurable on a diagnostic family that involves no slot at all:
//! `extrude(circle(nope), 12mm)` reports `unresolved name: nope` THREE times,
//! and `box(missing_thing, 20mm, 10mm)` reports it twice. What task 5750
//! changed is only the VISIBILITY — before it, no primitive or profile had a
//! slot, so no nested inner call could emit an `ArgTypeMismatch` at all.
//!
//! Hypothesis, not measured: a nested geometry argument is walked by
//! `compile_expr` more than once (once as an argument expression, once through
//! the nested-geometry hoisting path), so every diagnostic emitted from the
//! type-inference walk is duplicated while diagnostics emitted from the
//! LOWERING path are not — consistent with `extrude(circle(4mm, 9mm), 12mm)`
//! reporting its arity error exactly once.
//!
//! CONTAINED, not fixed: [`emit_mismatch`] drops a pushed diagnostic whose
//! (code, span, message) triple is already present, so THIS family reports
//! once per position again. `extrude(circle(4), 12mm)` — the dominant shape a
//! nested profile takes in real `.ri` — was the case that made containment
//! worth doing here rather than waiting. The MEASURED probe table above is
//! kept verbatim because it is the evidence the underlying walk is still
//! doubled: `unresolved name` and every other diagnostic family emitted from
//! that walk is untouched by a drop scoped to `ArgTypeMismatch`. De-duplicating
//! the walk itself belongs in `expr.rs`, outside this file, and is filed as
//! follow-up work (task #6627).
//!
//! # Arity awareness (task 5652)
//!
//! [`builtin_arg_slots`] is keyed on `(name, arity)`, not `name` alone, and
//! guards only genuinely overloaded names.  The rule, its rationale and the
//! `relation_signatures::relation_operand_datum` precedent are documented there;
//! the concrete false positive it prevents is on that function's
//! `linear_pattern_2d` arm.
//!
//! Most keys here are topology selectors; the exceptions are enumerated and
//! justified in `tests::NON_SELECTOR_ARG_SLOT_KEYS`, which the coverage invariant
//! `tests::arg_slot_keys_are_registered_builtin_names` enforces.  That list lives
//! under `#[cfg(test)]` because the match arms are the source of truth for the
//! table's domain; it is the reviewed statement of which of them are
//! non-selectors, consumed only by the invariant that enforces it.
//!
//! # Two RULES task 6862 added
//!
//! Both are stated here as rules only; the arms and the tests that enforce them
//! are the source of truth, so there is no second copy to drift.
//!
//! 1. **`Type::ScalarParam` DEFERS at a dimension slot and REJECTS at an `Int`
//!    slot.** `Scalar<Q>` — what a dim-kinded generic PARAMETER
//!    (`fn beam<Q: Dimension>(l: Scalar<Q>)`) resolves to — has a KNOWN family
//!    and an OPEN dimension, and the asymmetry falls out of that. Why each half
//!    is right, what the defer COSTS, the cases bounding that cost and the two
//!    compiler precedents it follows are stated ONCE, on the two arms that make
//!    the decision — [`ExpectedArg::Scalar`] and [`ExpectedArg::Int`] in
//!    [`check_builtin_arg_types`]; the enforcement is
//!    `tests::scalar_param_defers_at_length_slot` and
//!    `tests::scalar_param_still_rejected_at_int_slot`.
//!
//! 2. **A slotted name that accepts more than one arity needs a guard or an
//!    argued exemption.** An arity-agnostic arm serving an overloaded name is
//!    how a slot index comes to fire on the wrong argument. The mechanism, the
//!    worked example, what the guard buys and its MEASURED blind spot are
//!    stated ONCE, on the HAZARD block above the primitive arms; the
//!    enforcement is
//!    `tests::lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`.
//!
//! # Relationship to the eval-layer units gate (task 5214)
//!
//! This check COMPLEMENTS `required_length_value` — it never replaces it.  It
//! upgrades the diagnostic to a compile-time `Error` with a call-site span
//! where a definite static type is available, but decision-6 gradualism means a
//! dynamically-typed spacing (typing as `Type::TypeParam` or `Type::Error`) is
//! skipped here and still relies on the eval-layer gate.  Both layers are load-
//! bearing; removing either leaves a hole.

use reify_core::direction::DIRECTION_MIGRATION_HINT;
use reify_core::units::{ANGLE_MIGRATION_HINT, DENSITY_MIGRATION_HINT, LENGTH_MIGRATION_HINT};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, DimensionVector, SourceSpan, Type};
use reify_ir::CompiledExpr;

/// A single checkable argument slot: the zero-based index, human-readable
/// parameter name, and expected type for a builtin argument.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CheckableArg {
    /// Zero-based index of this argument in the call argument list.
    pub index: usize,
    /// Human-readable parameter name used in diagnostic messages
    /// (e.g., `"density"`, `"tol"`, `"h"`).
    pub name: &'static str,
    /// Expected type for this slot.
    pub expected: ExpectedArg,
}

/// The expected type for a checkable builtin argument slot.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ExpectedArg {
    /// A dimensioned scalar with a specific dimension.
    Scalar {
        /// The required physical dimension (e.g., `DimensionVector::MASS_DENSITY`).
        dimension: DimensionVector,
        /// Human-readable type name for diagnostic messages
        /// (e.g., `"Density"`, `"Angle"`, `"Length"`).
        type_name: &'static str,
        /// Migration hint appended to the rejection message.
        ///
        /// Mirrors `ArgSpec::migration_hint` in
        /// `crates/reify-ir/src/arg_acceptance.rs`, and carries the SAME
        /// `&'static str` — both sides read
        /// [`reify_core::units::LENGTH_MIGRATION_HINT`] /
        /// [`reify_core::units::ANGLE_MIGRATION_HINT`] /
        /// [`reify_core::units::DENSITY_MIGRATION_HINT`] rather than repeating
        /// the literal, so the compile-time and runtime diagnostics for one
        /// authoring mistake cannot drift apart (PRD
        /// `docs/prds/v0_6/units-length-gate-completion.md` decision D9).
        ///
        /// Non-optional: a dimensioned slot always has a literal spelling to
        /// migrate TO, so a hint-less `Scalar` slot is unrepresentable.
        /// [`ExpectedArg::Int`] and [`ExpectedArg::Enum`] render un-hinted; an
        /// [`ExpectedArg::Direction`] slot is hinted only for a `Vector3` (see
        /// [`direction_mismatch_hint`]).
        ///
        /// NOT [`crate::conformance::dimensioned_scalar_migration_hint`], and
        /// deliberately so. That generator serves the DIMENSIONED struct-ctor /
        /// fn-param slots (task 5627, decisions D4-6) and is COMPUTED from the
        /// dimension: for LENGTH it renders "pass a dimensioned Length literal
        /// such as `1m`" — capital L, the word "literal", and `1m` not `5mm`.
        /// This path must instead reproduce the eval-layer C1 text VERBATIM,
        /// and rewording the ctor path to match would silently change
        /// already-shipped diagnostics that
        /// `tests/struct_ctor_field_conformance_tests.rs` guards. The
        /// divergence is pinned by
        /// `builtin_slot_and_ctor_conformance_length_hints_are_deliberately_different`.
        migration_hint: &'static str,
    },
    /// The integer type `Type::Int` (e.g. `generate`'s count argument, task 3994).
    ///
    /// Distinct from `Scalar { DIMENSIONLESS }` (a dimensionless `Real`): a count
    /// must be a true `Int`, so `generate(2.5, …)` and `generate(3mm, …)` are both
    /// rejected while `generate(3, …)` passes.
    ///
    /// Carries NO counterpart to the `Scalar` variant's `migration_hint`, and
    /// deliberately so: a wrong count is an arity / semantic error, not a
    /// dimension migration, so there is nothing to migrate TO. The two arms
    /// still share one message shape without a per-variant field —
    /// [`emit_mismatch`] takes `Option<&'static str>` and this arm passes
    /// `None` literally.
    Int {
        /// Human-readable type name for diagnostic messages (always `"Int"`).
        type_name: &'static str,
    },
    /// A specific declared enum type, matched by name against `Type::Enum(_)`
    /// (the Euler builtins' `convention` slot, task #6082).
    ///
    /// No `enum_defs` plumbing is needed to make this sound: `expr.rs`'s
    /// `EnumAccess` arm validates the variant against
    /// `reify_ir::EnumDef::contains_variant` before it ever produces a
    /// `Type::Enum(_)`, so a value of this type is already guaranteed to name a
    /// DECLARED enum with an EXISTING variant. This slot only has to pin WHICH
    /// enum — an `OutputFormat` value in a convention slot is the mismatch it
    /// exists to catch, alongside the `String` that motivated it.
    ///
    /// Like `Int`, carries no `migration_hint`: there is no dimensioned-literal
    /// spelling to migrate to, so [`emit_mismatch`] is passed `None`.
    Enum {
        /// The required enum type name (e.g. `"EulerConvention"`).
        enum_name: &'static str,
        /// Human-readable type name for diagnostic messages (normally the same
        /// as `enum_name`).
        type_name: &'static str,
    },
    /// Exactly `Type::Direction` (direction-typing PRD D8 / C7), with no
    /// coercion from `Vector3`, `Axis` or `Plane` (D2).
    ///
    /// Fieldless: the displayed name is `Type::Direction`'s own, so it cannot
    /// drift from what the arm accepts.
    Direction,
}

/// Build one LENGTH slot — the shape every Contract C length position shares.
///
/// Hoisted out of the arms below because task 5750 lands 26 of them across the
/// primitive and profile families at once, and spelling the
/// `ExpectedArg::Scalar { dimension, type_name, migration_hint }` literal out
/// longhand 26 times would bury the only two things that actually vary per
/// slot: the index and the argument NAME.
///
/// The seven pre-5750 LENGTH slots above keep their longhand form deliberately
/// — rewriting them would churn already-reviewed arms for no behavioural gain,
/// and this function is byte-for-byte the same construction they perform.
const fn length_arg(index: usize, name: &'static str) -> CheckableArg {
    CheckableArg {
        index,
        name,
        expected: ExpectedArg::Scalar {
            dimension: DimensionVector::LENGTH,
            type_name: "Length",
            migration_hint: LENGTH_MIGRATION_HINT,
        },
    }
}

/// Build one ANGLE slot — the shape every gated angle position shares.
///
/// Hoisted for the same reason [`length_arg`] is, and with a sharper
/// obligation behind it: PRD `docs/prds/v0_6/angle-units-surface-convergence.md`
/// leaf ζ lands its angle slots at once — the four directional selectors'
/// `tol`, and every producer angle position across `rotate`, `rotate_around`,
/// `revolve`, `arc`, `draft` and `circular_pattern` — and decision D11 requires
/// every ANGLE rejection to read with ONE wording. A longhand literal per slot
/// would be one more place per slot for a `migration_hint` to go missing,
/// which is exactly the drift this leaf exists to end: eval and compile
/// already spent one PRD apart on this field.
///
/// Callers need the inline `const { &[…] }` block — a const-fn CALL is not
/// promotable, as the "Shape" section on [`builtin_arg_slots`] explains for
/// [`length_arg`].
const fn angle_arg(index: usize, name: &'static str) -> CheckableArg {
    CheckableArg {
        index,
        name,
        expected: ExpectedArg::Scalar {
            dimension: DimensionVector::ANGLE,
            type_name: "Angle",
            migration_hint: ANGLE_MIGRATION_HINT,
        },
    }
}

/// Build one DIRECTION slot; callers need the `const { &[…] }` block, as for
/// [`angle_arg`].
const fn direction_arg(index: usize, name: &'static str) -> CheckableArg {
    CheckableArg {
        index,
        name,
        expected: ExpectedArg::Direction,
    }
}

/// Return the checkable argument slots for a named builtin call of a given
/// arity.
///
/// Returns an empty slice for:
/// - Unrecognized names.
/// - Names with no checked argument (e.g. `split`, `face`, `edge`,
///   `solid_body`, `volume`, `edges`, `faces`, …).
/// - A recognized *overloaded* name called at an arity whose positional layout
///   has no checked slot (see "Arity awareness" below).
///
/// The arms below are the AUTHORITATIVE per-slot statement — index, argument
/// name, dimension, and why each neighbouring position is or is not gated. The
/// module docs carry the rules only, so the two cannot drift.  Mirrors the
/// name-keyed structure of `math_fn_result_type` (task 4182 result-type
/// precedent).
///
/// # Shape: a borrowed constant table, not `vec![…]`
///
/// Every arm is a compile-time constant table returned by reference, so a call
/// costs a match and a pointer rather than a heap allocation.  That matters
/// because [`check_builtin_arg_types`] runs on EVERY builtin call in a module,
/// and since task 5750 the common producers (`box`, `circle`, `translate`,
/// `extrude`, `fillet`, …) all have slots — before it they fell through to the
/// non-allocating empty arm.
///
/// Two spellings appear below, and the difference is mechanical rather than
/// stylistic.  An arm written out longhand is a plain `&[…]`: a struct literal
/// over constants promotes to `'static` on its own.  An arm built from
/// [`length_arg`] needs the inline `const { &[…] }` block, because a const-fn
/// CALL is not promotable — the const block supplies the const context that
/// gives the borrowed array `'static`.  Both PROBED before being committed to.
///
/// # Arity awareness (task 5652)
///
/// `arg_count` is the number of arguments at the call site.  The table is keyed
/// on `(name, arity)` rather than `name` alone because a builtin's positional
/// layout is stable only *within* one overload: for an overloaded name, index
/// `i` denotes a different parameter in each form.  Precedent:
/// [`crate::relation_signatures::relation_operand_datum`], which discriminates
/// `"offset" if args.len() == 3` / `"angle" if args.len() == 3` for the same
/// reason.  The concrete false positive this prevents is documented on the
/// `linear_pattern_2d` arm below.
///
/// **Rule: guard only genuinely overloaded names.**  Non-overloaded arms stay
/// unguarded (arity-agnostic) so a short or long call still has its present
/// slots checked.  Gating a single-form builtin on its canonical arity would
/// weaken existing coverage silently — e.g. an `arg_count == 3` guard on
/// `edges_at_height` would make a 2-arg call slot-free instead of checking the
/// `h` that IS present.  Short-arg calls are already handled downstream by
/// `check_builtin_arg_types`'s `compiled_args.get(index)` bounds check; arity
/// errors are a separate diagnostic family.
///
/// **The standing obligation that rule creates** (task 6862 FINDING 2): a name
/// is only safely unguarded while it stays single-form.  When a lowering gains
/// a VALUE-FORM overload, the matching guard must land here in the same change,
/// or the existing indices start firing on the wrong arguments.  The hazard, why
/// the `compiled_args.get(index)` bounds check does not shield it, and the guard
/// that now enforces the coupling
/// ([`tests::lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`])
/// are all stated in the HAZARD block immediately above the primitive arms.
pub(crate) fn builtin_arg_slots(name: &str, arg_count: usize) -> &'static [CheckableArg] {
    match name {
        // ── Mass-properties topology selectors ───────────────────────────────
        // arg0: geometry handle (unchecked — ε=4358's territory)
        // arg1: density → MASS_DENSITY ("Density")
        "center_of_mass" | "moment_of_inertia" => &[CheckableArg {
            index: 1,
            name: "density",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
                migration_hint: DENSITY_MIGRATION_HINT,
            },
        }],

        // ── Directional topology selectors ───────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: the direction → Direction (direction-typing θ-b). Named `dir` /
        //       `axis` to match the eval reader's `resolve_direction_arg` labels,
        //       so both layers name the same argument.
        // arg2: tol → ANGLE ("Angle")
        // Task 3523 — faces_perpendicular_to/edges_perpendicular_to share the
        // directional (solid, axis, tol) shape, so arg2 tol is likewise ANGLE.
        //
        // Built with [`angle_arg`] like the producer angle positions below, so
        // every ANGLE rejection reads with one wording (decision D11). Pinned
        // by `angle_slot_rejection_carries_the_migration_hint`.
        "faces_by_normal" => const { &[direction_arg(1, "dir"), angle_arg(2, "tol")] },
        "edges_parallel_to" | "faces_perpendicular_to" | "edges_perpendicular_to" => {
            const { &[direction_arg(1, "axis"), angle_arg(2, "tol")] }
        }

        // ── angle(a, b): the angle between two Directions (θ-b) ─────────────
        // Arity 3 is the relation verb `angle(x, y, metric)`, policed by
        // `relation_signatures`, and stays slot-free — hence the guard.
        "angle" if arg_count == 2 => const { &[direction_arg(0, "a"), direction_arg(1, "b")] },

        // ── Height-based topology selectors ──────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: h → LENGTH ("Length")
        // arg2: tol → LENGTH ("Length")
        "edges_at_height" => &[
            CheckableArg {
                index: 1,
                name: "h",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: LENGTH_MIGRATION_HINT,
                },
            },
            CheckableArg {
                index: 2,
                name: "tol",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: LENGTH_MIGRATION_HINT,
                },
            },
        ],

        // ── Extremal topology selectors (task 3523) ──────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: axis string ("X"/"Y"/"Z" — unchecked; parsed at eval)
        // arg2: sense string ("Max"/"Min" — unchecked; parsed at eval)
        // arg3: tol → LENGTH ("Length"). A distance tolerance, like edges_at_height's
        // LENGTH tol — so an ANGLE tol (e.g. faces_by_normal's 1deg) is rejected at
        // compile time rather than only warned-about at eval (resolve_length_scalar_arg).
        "extremal_by_bbox" | "extremal_by_centroid" => &[CheckableArg {
            index: 3,
            name: "tol",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
                migration_hint: LENGTH_MIGRATION_HINT,
            },
        }],

        // ── List combinator: generate(n, |i| …) (task 3994, structural-query ζ) ──
        // arg0: n → Int (count). NOT a geometry selector — the only non-geometry
        // entry in this otherwise selector-focused table; the mechanism is generic
        // (name-keyed), so hosting `generate`'s lone Int slot here avoids a parallel
        // checker. A dimensionless `Real` (2.5) or dimensioned scalar (3mm) count is
        // rejected; a negative literal (`-1`) types as Int and PASSES here — it is
        // caught at eval (DiagnosticCode::GenerateNegativeCount, task 3994 step-10).
        // arg1: |i| … lambda (unchecked — typed via the list-helper return ladder).
        "generate" => &[CheckableArg {
            index: 0,
            name: "n",
            expected: ExpectedArg::Int { type_name: "Int" },
        }],

        // ── Pattern CSG producers: spacing is a Length (task 5652) ───────────
        // linear_pattern(target, dx, dy, dz, count, spacing)
        //   arg0:   target geometry handle (unchecked — ε=4358's territory)
        //   args1-3: dx/dy/dz — a DIMENSIONLESS direction vector, deliberately
        //            unchecked. Task 5214 established the direction is a unit
        //            vector, so a bare component carries no silent-metres
        //            hazard (unlike spacing, where a bare `10` was read as 10
        //            SI metres — 1000× a plausible 10 mm pitch).
        //   arg4:   count — an Int, deliberately unchecked here (a wrong count
        //           is an arity/semantic error, not a dimension error).
        //   arg5:   spacing → LENGTH ("Length").
        //
        // The `arg_count == 6` guard is load-bearing forward-compat, not
        // decoration, even though 6 is currently the only accepted arity
        // (geometry.rs `check_arg_count_exact(..., 6, ...)`): task 5351 lands a
        // 4-arg `(target, direction, count, spacing)` value form in which
        // `spacing` moves to index 3. Without the guard, index 5 would denote
        // nothing there — the slot would be semantically lying, and the only
        // thing keeping it quiet would be the downstream `compiled_args.get(5)`
        // bounds check, i.e. luck rather than intent.
        "linear_pattern" if arg_count == 6 => &[CheckableArg {
            index: 5,
            name: "spacing",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
                migration_hint: LENGTH_MIGRATION_HINT,
            },
        }],

        // linear_pattern_2d(target, dx1, dy1, dz1, count1, spacing1,
        //                           dx2, dy2, dz2, count2, spacing2)
        //   arg5:  spacing1 → LENGTH ("Length")
        //   arg10: spacing2 → LENGTH ("Length")
        // Direction components and counts are unchecked for the same reasons
        // as `linear_pattern` above.
        //
        // Here the `arg_count == 11` guard is not merely forward-compat — it is
        // the case that makes the whole arity dimension necessary. Task 5351's
        // 7-arg form is `(target, dir1, count1, spacing1, dir2, count2,
        // spacing2)`, in which index 5 is `count2`, an `Int`. An arity-agnostic
        // `spacing1@5 LENGTH` slot would emit a FALSE `ArgTypeMismatch` on
        // valid code there. Unlike `linear_pattern`'s 4-arg form (which has no
        // index 5 at all, so the `compiled_args.get(index)` bounds check
        // happens to shield it), a 7-arg call DOES have an index 5 holding a
        // different parameter — only this guard prevents the false positive.
        "linear_pattern_2d" if arg_count == 11 => &[
            CheckableArg {
                index: 5,
                name: "spacing1",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: LENGTH_MIGRATION_HINT,
                },
            },
            CheckableArg {
                index: 10,
                name: "spacing2",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: LENGTH_MIGRATION_HINT,
                },
            },
        ],

        // ── Pattern CSG producers: the ORIGIN triple is a Length (task 5662) ──
        //
        // The pattern-origin row task 5652 deferred (it turned six then-valid
        // call sites into hard compile errors, a separable breaking-surface
        // migration) and task 5662 closed. It mirrors, at the compile layer, the
        // eval-layer gate on the circular-pattern axis origin (task 5350). Arg
        // names are COPIED from `geometry.rs`'s lowering sites — the
        // `("ox".to_string(), …)` triple in its n==9 arm — never invented, so the
        // two layers word one authoring mistake identically (PRD decision D9).
        //
        // `mirror`'s 7-arg origin triple held slots here too until
        // direction-typing ι-a retired that form for
        // `mirror(target, origin, normal)`, whose Point3 origin no slot kind
        // describes (see the "Contract C positions this table deliberately does
        // NOT cover" section of the module doc).
        //
        // mirror(target, origin, normal)  — 3-arg Direction form
        // mirror(target, plane)           — 2-arg value form
        //   arg1:    the `origin`, a Point3<Length>: UNSLOTTED, gated at build by
        //            `accept_length_point3`, as `rotate_around`'s pivot is.
        //   arg2:    `normal` → Direction.
        //
        // The `arg_count == 3` guard does two jobs. It satisfies the coupling
        // rule: `mirror` accepts arities 2 and 3, so the two must expose
        // different slot sets. And it keeps the retired 7-arg scalar call down
        // to its arity error, with no spurious `normal@2` mismatch on its `oy`.
        // (The plane form is safe either way: the checker skips a slot past the
        // call's arity, so `normal@2` cannot reach a 2-arg call.) Pinned by
        // `mirror_slots_are_arity_3_only` and
        // `mirror_slots_the_normal_but_never_the_point3_origin`.
        "mirror" if arg_count == 3 => const { &[direction_arg(2, "normal")] },

        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)  — 9-arg
        // circular_pattern(target, axis, count, angle)                    — 4-arg
        //   args1-3: the rotation-axis ORIGIN → LENGTH ("Length").
        //   args4-6: the axis DIRECTION `ax`/`ay`/`az` — a dimensionless unit
        //            vector, UNSLOTTED for the same reason as `mirror`'s normal.
        //            The FIFTH and widest straddle case.
        //   arg7:    `count` — an Int. A wrong count is an arity/semantic error,
        //            not a dimension error.
        //   arg8:    `angle` → ANGLE ("Angle"), gated since PRD 3 leaf ζ
        //            (task 5782).
        //   Pinned by `circular_pattern_forms_are_keyed_independently` and
        //   `circular_pattern_slots_the_origin_and_the_angle_but_never_the_axis_or_count`.
        //
        // The `arg_count == 9` guard is load-bearing: index 1 is `ox` at
        // arity 9 but `axis` at arity 4.
        //
        // PREFIX AGREEMENT. This layer is keyed on the CALL, so it reports the
        // SURFACE name. The eval layer's `kind_label` (`PatternKind`'s
        // `Display`) renders that same name, so for `circular_pattern:` the two
        // layers agree byte-for-byte.
        // Only the `_centered` aliases still diverge; see
        // `check_builtin_arg_types`' "# Message format". This layer's half is
        // pinned by `circular_pattern_bare_origin_is_rejected_naming_the_origin_components`
        // in `tests/harness_type_checking/builtin_arg_signature_tests.rs`; the eval
        // half by reify-eval's `expected_length_rejection("circular_pattern", …)`
        // assertions in `src/geometry_ops/tests.rs`, e.g.
        // `compile_geometry_op_circular_pattern_origin_rejection_label_agrees_across_forms`.
        //
        // This arm makes `reify check` a REAL gate for these positions, not
        // merely `reify eval`: `cmd_check` returns `ExitCode::FAILURE` on any
        // compile `Severity::Error` and short-circuits BEFORE constraint checking
        // and before `build()`. `mirror`'s 7-arg arm was the one pinned at the
        // CLI seam; its `Point3`-origin successor is gated at build, and its row
        // in `reify-cli/tests/harness_cli_check/cli_check_bare_length_exit.rs`'s
        // `ROWS` is a `Layer::Eval` row.
        //
        // STRUCTURAL EXCLUSION OF THE DECODED-VALUE ROUTE — the AUTHORITATIVE
        // statement, kept here beside the code it describes per this module's
        // own header rule. Four other sites need it and POINT here rather than
        // restate it: `reify-cli/tests/fixtures/mirror_bare_origin{,_purpose}.ri`,
        // `cli_check.rs`' two build-diagnostic tests, and
        // `reify-eval/tests/mirror_circular_value_forms_e2e.rs`.
        //
        // A bare origin reaching `mirror` / `circular_pattern` through the value
        // form — `mirror(g, plane_yz(0))`, `circular_pattern(g, axis_z(…), n, a)`
        // — arrives already assembled into a composite `Value` by a stdlib
        // producer, so it never passes through a positional argument index for
        // this name-and-arity-keyed table to key on. NO arm can gate it: the
        // exclusion is structural, not a gap someone forgot to fill, so it stays
        // a BUILD-only diagnostic for as long as the table stays index-keyed.
        // That is why task 5748's two CLI fixtures had to move onto that route
        // when these arms landed — the short-circuit above would otherwise have
        // destroyed every assertion they carry.
        "circular_pattern" if arg_count == 9 => const { &[
            length_arg(1, "ox"),
            length_arg(2, "oy"),
            length_arg(3, "oz"),
            angle_arg(8, "angle"),
        ] },

        // circular_pattern(target, axis, count, angle) — 4-arg value form
        //   arg1: `axis`, a decoded Axis VALUE — never an origin slot (the
        //         STRUCTURAL EXCLUSION above is about the ORIGIN, which this
        //         form never exposes positionally).
        //   arg2: `count` — an Int, never a slot.
        //   arg3: `angle` → ANGLE ("Angle"), since PRD 3 leaf ζ (task 5782).
        // The guard is load-bearing: index 3 is the `oz` LENGTH at arity 9.
        "circular_pattern" if arg_count == 4 => const { &[angle_arg(3, "angle")] },

        // ── HAZARD: an arity-agnostic arm meets a future value-form overload ─
        //
        // Everything from here down (the task-5750 primitive / profile / modify
        // / sweep / transform blocks) is arity-AGNOSTIC except where an arm
        // spells an explicit `if arg_count ==` guard. That is correct while each
        // name has ONE form, and it is what the "guard only genuinely overloaded
        // names" rule above requires. The hazard is what happens when that stops
        // being true.
        //
        // If a slotted name later gains a VALUE-FORM overload in the lowering
        // WITHOUT a matching edit here, the slot INDICES below start denoting
        // different parameters — and fire on the wrong ones. Worked example
        // (task 6862 FINDING 2), realised by direction-typing ι-b:
        // `revolve(profile, axis, angle)` arrived alongside
        // `revolve(profile, origin, direction, angle)`, so index 2 is the
        // `Direction` in one form and the `angle` in the other. One
        // arity-agnostic arm would demand a Direction of an Angle on correct
        // code, which is why `revolve`'s arms below carry `if arg_count ==`
        // guards.
        //
        // `check_builtin_arg_types`' `compiled_args.get(index)` bounds check does
        // NOT shield that case. It shields `linear_pattern`'s 4-arg form only
        // because the slot indices there are ABSENT at the short arity; in
        // `revolve`'s case index 2 IS populated at both arities, just by
        // different parameters. A bounds check cannot tell those two situations
        // apart.
        //
        // Coupling: task 5351 owns the value forms, and `linear_pattern` /
        // `linear_pattern_2d` are already forward-compat-guarded for it.
        //
        // # What now enforces this
        //
        // `tests::lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`.
        // It compiles real calls at every arity, derives each lowering's ACCEPTED
        // arity set from the arg-count diagnostics, pins it, and requires that any
        // name accepting MORE THAN ONE arity either carry an `if arg_count ==`
        // guard or appear in `tests::MULTI_ARITY_AGNOSTIC_SAFE` with the layout
        // that proves its indices denote the same parameters at every form.
        //
        // Task 6862 chose that ENFORCEABLE-TEST route over the comment-only
        // route its brief also allowed, because the ledger is derived
        // BEHAVIOURALLY (from diagnostics, not from reading these arms) and so
        // cannot drift out of step with the lowering the way a comment can.
        //
        // Honest limit, MEASURED rather than assumed: the guard is blind to the
        // nine geometry TOPOLOGY SELECTORS and to `generate`, which emit no
        // arg-count diagnostic at all at any probed arity (see
        // `tests::ARITY_UNOBSERVABLE_SLOT_KEYS`). It therefore covers exactly
        // FINDING 2's subject — the geometry-lowering families task 5750 added —
        // and not task 4493/3994's selector family.

        // ── Primitive CSG producers: every dimension is a Length (task 5750) ──
        //
        // PRD `docs/prds/v0_6/units-length-gate-completion.md` leaf η, work
        // item 1. These mirror, at the compile layer, the primitive fields task
        // 5743 gated at eval: the positions `geometry_ops`'
        // `required_length_values` already gates. The hazard they close is the
        // silent one — `Value::as_f64` reads a bare `20` as 20 SI METRES, so
        // `box(20, 20, 10)` built a 20-metre part with no diagnostic at all
        // before task 5743.
        //
        // Arg NAMES are copied from the lowering sites in `geometry.rs` (the
        // `("width".to_string(), …)` pairs each arm builds), because the eval
        // layer renders its rejection from those same strings. A name invented
        // here would make the two layers word one authoring mistake
        // differently, which is exactly what decision D9 exists to prevent.
        //
        // The ARG names match; the BUILTIN prefix deliberately does not, for
        // the two `_centered` aliases. See `check_builtin_arg_types`'
        // "# Message format" section for the measured difference and why this
        // layer reports the surface name.
        //
        // ARITY-AGNOSTIC by the rule stated above: none of these names is
        // overloaded (each lowering arm calls `check_arg_count_exact`), so
        // guarding them on their canonical arity would silently weaken
        // coverage for short calls without buying anything. Pinned by the
        // `assert_slots_at_every_arity` sweeps in `tests`.
        //
        // NOT slotted here, each for a stated reason rather than by omission:
        // - `rounded_box` / `zone_cylinder` / `zone_annulus` / `zone_profile` —
        //   gated only INDIRECTLY. They DESUGAR into Box/Cylinder/Rectangle/
        //   Circle/Pipe/Thicken ops, so their own surface arguments never reach
        //   a gated position under these names. A slot here would therefore be
        //   NEW coverage whose arg name disagrees with whatever the eval layer
        //   reports for the desugared op — breaking D9's same-wording premise.
        //   (`zone_annulus`' arg3 `length` is additionally DEAD: `geometry.rs`
        //   binds it to `_l`.)
        // - `polygon` — Contract-C gated at eval via the task-5661 VARIADIC
        //   route, but its positions are arity-OPEN and its compile-layer names
        //   are the inert `c0`…`cN` that `geometry.rs` synthesises. There is no
        //   index-keyed `CheckableArg` to write.
        //
        // box(width, depth, height) / box_centered(…) — X/Y/Z extents, slot
        //   names size_x/size_y/size_z
        "box" | "box_centered" => const { &[
            length_arg(0, "size_x"),
            length_arg(1, "size_y"),
            length_arg(2, "size_z"),
        ] },

        // cylinder(radius, height) / cylinder_centered(radius, height)
        //
        // `cylinder_centered` lowers to TWO ops — the Cylinder plus a
        // compensating `Translate(dz = -height/2)` — but the table is keyed on
        // the CALL, whose two arguments are the same radius/height pair. The
        // desugaring's `× -0.5` multiplier is a synthesised `CompiledExpr` that
        // is never a call-site argument, so this check never sees it. That
        // matters: the multiplier MUST stay a bare dimensionless `Real` (see
        // the INVARIANT note on `geometry.rs`'s `cylinder_centered` arm), and a
        // slot cannot reach it to break that.
        "cylinder" | "cylinder_centered" => {
            const { &[length_arg(0, "radius"), length_arg(1, "height")] }
        }

        // sphere(radius)
        "sphere" => const { &[length_arg(0, "radius")] },

        // tube(outer_r, inner_r, height)
        "tube" => const { &[
            length_arg(0, "outer_r"),
            length_arg(1, "inner_r"),
            length_arg(2, "height"),
        ] },

        // cone(bottom_radius, top_radius, height)
        "cone" => const { &[
            length_arg(0, "bottom_radius"),
            length_arg(1, "top_radius"),
            length_arg(2, "height"),
        ] },

        // wedge(width, depth, height, top_width)
        "wedge" => const { &[
            length_arg(0, "width"),
            length_arg(1, "depth"),
            length_arg(2, "height"),
            length_arg(3, "top_width"),
        ] },

        // torus(major_radius, minor_radius)
        "torus" => const { &[
            length_arg(0, "major_radius"),
            length_arg(1, "minor_radius"),
        ] },

        // half_space(origin, normal)
        //   arg0:    the `origin`, a Point3<Length> on the boundary plane:
        //            UNSLOTTED (no `ExpectedArg` kind describes a point), gated
        //            at build by `accept_length_point3`, as `rotate_around`'s
        //            pivot is.
        //   arg1:    the outward `normal` → Direction (direction-typing ι-c).
        //   Single-form, so the arm is arity-agnostic. Pinned by
        //   `half_space_slots_its_normal_as_a_direction_but_never_its_point3_origin`.
        "half_space" => const { &[direction_arg(1, "normal")] },

        // ── Transform producers (task 5750) ─────────────────────────────────
        //
        // The compile-layer half of task 5623's eval-layer gate on
        // `translate`'s displacement. Arg names from `geometry_transform.rs`'s
        // `compile_transform_op`.
        //
        // `translate` and `rotate_around` are single-form
        // (`check_arg_count_exact` / `check_arg_count_exact_form`), so both arms
        // stay arity-agnostic per the rule stated on this function — and are
        // therefore subject to the HAZARD block above the primitives.
        //
        // translate(target, dx, dy, dz)
        //   arg0:    the geometry handle — permanently unchecked (ε=4358's
        //            territory), like every other arg0 in this table.
        //   args1-3: `dx`/`dy`/`dz` → LENGTH ("Length"). A DISPLACEMENT, not a
        //            direction: its magnitude is the whole point, so a bare
        //            component is silently read as SI metres.
        "translate" => const { &[
            length_arg(1, "dx"),
            length_arg(2, "dy"),
            length_arg(3, "dz"),
        ] },

        // rotate(target, axis, angle)            — 3-arg Direction form
        // rotate(target, orientation)            — 2-arg, task 4166's overload
        //   arg0:    the geometry handle (unchecked — ε=4358's territory).
        //   arg1:    `axis` → Direction (direction-typing ι-a).
        //   arg2:    `angle` → ANGLE ("Angle"). PRD 3 leaf ζ, boundary row B4.
        //
        // The arity guard is load-bearing: `rotate` is overloaded, and at arity
        // 2 index 1 is the Orientation VALUE (see the HAZARD block below).
        // Pinned by `rotate_slots_are_arity_3_only` and
        // `rotate_orientation_overload_yields_no_arg_type_mismatch`.
        "rotate" if arg_count == 3 => const { &[direction_arg(1, "axis"), angle_arg(2, "angle")] },

        // rotate_around(target, pivot, axis, angle)
        //   arg1:    the `pivot`, a Point3<Length>. No `ExpectedArg` kind
        //            describes a point, so it is UNSLOTTED here and gated at
        //            build by `accept_length_point3`.
        //   arg2:    `axis` → Direction (direction-typing ι-a).
        //   arg3:    `angle` → ANGLE ("Angle"), per
        //            `docs/prds/v0_6/angle-units-surface-convergence.md`.
        //   `scale` is deliberately absent from this block entirely: its
        //   `factor` (and the `factors` vec3 of its non-uniform form) is a
        //   dimensionless RATIO, so a LENGTH slot would reject correct code.
        //   Pinned by `scale_stays_slot_free_because_its_factor_is_dimensionless`.
        "rotate_around" => const { &[direction_arg(2, "axis"), angle_arg(3, "angle")] },

        // ── Modify producers (task 5750) ─────────────────────────────────────
        //
        // The compile-layer half of task 5744's eval-layer gates on the modify
        // magnitudes. Arg names are copied from `geometry_modify.rs`'s
        // `compile_modify_op` arms and its shared `compile_modify_2arg` helper.
        //
        // The arity-agnostic arms here are subject to the HAZARD block above the
        // primitives. Two names in this block ALREADY accept more than one arity
        // while staying agnostic — `offset_curve` and `shell` — and both are
        // recorded, with the layout that makes them safe, in
        // `tests::MULTI_ARITY_AGNOSTIC_SAFE`.
        //
        // This is where the table's first genuinely OVERLOADED names appear.
        // `fillet` and `chamfer` each accept a 2-arg all-edges form and a 3-arg
        // curated-edges form, and the MAGNITUDE MOVES between them:
        //   fillet(target, radius)          — radius at index 1
        //   fillet(target, edges, radius)   — radius at index 2, index 1 is the
        //                                     edge SELECTOR
        // An arity-agnostic `radius@1` slot would therefore demand a Length of
        // a Selector on correct code — the same class of false positive the
        // `linear_pattern_2d` arm's guard exists to prevent, and the reason
        // every modify arm whose magnitude moves — `fillet`, `chamfer`, `draft`
        // — carries an `arg_count` guard. Every OTHER name below holds its
        // magnitude at a STABLE index across every arity it accepts, so per the
        // rule stated on this function they stay unguarded.
        //
        // NOT slotted here, each for a stated reason:
        // - `offset_curve`'s 3rd argument (`"third"`) — a reference Surface
        //   handle OR a direction vec3, disambiguated at eval on the Value
        //   variant. A direction's components are legitimately bare, and the
        //   closure guard's `ALLOWLIST` (`units_length_closure_guard.rs`) names
        //   this position explicitly among the deliberately-not-gated set.
        // - `shell`'s args 2.. (`face_{i}`) — face INDICES, not lengths.
        // - `chamfer` / `fillet` arg1 in the CURATED form — the edge selector.

        // fillet(target, radius) / fillet(target, edges, radius)
        "fillet" if arg_count == 2 => const { &[length_arg(1, "radius")] },
        "fillet" if arg_count == 3 => const { &[length_arg(2, "radius")] },

        // chamfer(target, distance) / chamfer(target, edges, distance)
        "chamfer" if arg_count == 2 => const { &[length_arg(1, "distance")] },
        "chamfer" if arg_count == 3 => const { &[length_arg(2, "distance")] },

        // draft(target, angle, plane) / draft(target, faces, angle, plane)
        //   arg0: the geometry handle. 3-arg: `angle`@1 → ANGLE, the neutral
        //   `plane`@2 unslotted. 4-arg: the face SELECTOR@1 unslotted,
        //   `angle`@2 → ANGLE, `plane`@3 unslotted. Since PRD 3 leaf ζ
        //   (task 5782); pinned by `draft_angle_slot_moves_with_its_arity`.
        "draft" if arg_count == 3 => const { &[angle_arg(1, "angle")] },
        "draft" if arg_count == 4 => const { &[angle_arg(2, "angle")] },

        // chamfer_asymmetric(target, edges, d1, d2)
        //   Single-form (`check_arg_count_exact(4)`). BOTH setbacks are
        //   slotted: the eval layer reads the pair in one grouped call so an
        //   author fixes the line in a single edit, and reporting only `d1`
        //   here would degrade that to two edit-build cycles.
        "chamfer_asymmetric" => const { &[length_arg(2, "d1"), length_arg(3, "d2")] },

        // The single-magnitude modify family — magnitude at index 1, stable.
        //   shell(target, thickness, face_0, …) — `check_arg_count_at_least(2)`,
        //     so args 2.. are face indices and index 1 is `thickness` at EVERY
        //     accepted arity. Guarding it on one arity would silently hollow
        //     out the curated forms.
        //   offset_curve(curve, distance) / (curve, distance, third) — overloaded
        //     in ARITY but not in LAYOUT: `distance` stays at index 1 in both
        //     forms, which is precisely the case the guard rule says NOT to
        //     guard.
        //   The rest are single-form; a guard would deny them nothing and buy
        //     nothing.
        "shell" | "shell_open" => const { &[length_arg(1, "thickness")] },
        "thicken" => const { &[length_arg(1, "offset")] },
        "fillet_all" => const { &[length_arg(1, "radius")] },
        "zone_slab" => const { &[length_arg(1, "width")] },
        "offset_solid" | "offset_curve" => const { &[length_arg(1, "distance")] },

        // ── Sweep producers (task 5750) ──────────────────────────────────────
        //
        // The task-5744 `sweep` row, joined by the `Direction` of `revolve` /
        // `revolve_full` / `extrude_infinite` / `extrude_infinite_symmetric`
        // (direction-typing ι-b). Arg names from `geometry.rs`'s Sweep arms.
        //
        // Arity-agnostic except `revolve`, whose two forms are the HAZARD
        // block's worked example.
        //
        // extrude(profile, distance) / extrude_symmetric(profile, distance)
        // pipe(path, radius)
        //   All exact-2 single-form. Index 0 is the profile / path — a geometry
        //   handle, permanently unchecked (ε=4358's territory).
        "extrude" | "extrude_symmetric" => const { &[length_arg(1, "distance")] },
        "pipe" => const { &[length_arg(1, "radius")] },

        // revolve(profile, origin, direction, angle)   — 4 call args
        // revolve(profile, axis, angle)                — 3 call args
        // revolve_full(profile, origin, direction)     — 3 call args
        //   `direction` → DIRECTION; `angle` → ANGLE ("Angle", PRD 3 leaf ζ).
        //   The Point3 `origin` and the `Axis` datum are NOT slotted: no
        //   `ExpectedArg` kind holds either, so both are gated at build
        //   (`sweep_revolve`: the origin under the `ox`/`oy`/`oz` names, the
        //   Axis through `decode_axis`).
        //
        // `revolve`'s guards are load-bearing: index 2 is the Direction at
        // arity 4 and the angle at arity 3 (the HAZARD block). `revolve_full`
        // has no angle slot because its `angle` op-arg is a compiler-SYNTHESIZED
        // `Value::angle(TAU)` typed `Type::angle()` (`geometry.rs`, the type PRD
        // D10 ratified): no author writes it, so there is no user-written
        // position for this index-keyed table to gate. Pinned by
        // `revolve_slots_one_layout_per_form` and
        // `revolve_full_slots_the_direction_only_because_its_2pi_is_synthesized`.
        "revolve" if arg_count == 4 => {
            const { &[direction_arg(2, "direction"), angle_arg(3, "angle")] }
        }
        "revolve" if arg_count == 3 => const { &[angle_arg(2, "angle")] },
        "revolve_full" => const { &[direction_arg(2, "direction")] },

        // extrude_infinite(profile, direction)
        // extrude_infinite_symmetric(profile, direction)
        //   Single-form; the sense is the builtin, not an argument. Pinned by
        //   `extrude_infinite_pair_slots_the_direction`.
        "extrude_infinite" | "extrude_infinite_symmetric" => {
            const { &[direction_arg(1, "direction")] }
        }

        // ── 2-D profile producers (task 5750) ────────────────────────────────
        //
        // The compile-layer half of task 5743's eval-layer gates on the profile
        // fields. Same rules as the primitives above — including the HAZARD
        // block; `polygon`'s variadic vertex stream is excluded for the reason
        // recorded there.
        //
        // rectangle(width, height)
        "rectangle" => const { &[length_arg(0, "width"), length_arg(1, "height")] },

        // circle(radius)
        "circle" => const { &[length_arg(0, "radius")] },

        // ellipse(semi_major, semi_minor)
        "ellipse" => const { &[
            length_arg(0, "semi_major"),
            length_arg(1, "semi_minor"),
        ] },

        // ── Curve producers (PRD 3 leaf ζ, task 5782) ────────────────────────
        //
        // Arg names copied from `geometry_curve.rs`'s `arc` lowering arm (D9).
        //
        // arc(center, radius, start_angle, end_angle, normal)
        //   arg0:    the `center`, a Point3<Length>: UNSLOTTED (no `ExpectedArg`
        //            kind describes a point), gated at build by
        //            `accept_length_point3`.
        //   arg1:    `radius` — Length-semantic, but deliberately UNSLOTTED
        //            here: curve producers sit outside task 5750's families,
        //            recorded under the module doc's "Contract C positions this
        //            table deliberately does NOT cover" CURVE bullet. PRD 1's
        //            territory, not this leaf's; the build gates it.
        //   args2-3: `start_angle` / `end_angle` → ANGLE ("Angle").
        //   arg4:    the `normal` → Direction (direction-typing ι-c).
        //
        // Single-form (`check_arg_count_exact_form(.., 5, ..)`), so arity-agnostic
        // per the "guard only genuinely overloaded names" rule. Pinned by
        // `arc_slots_both_angles_and_its_normal_but_neither_its_centre_nor_its_radius`
        // and, at the message layer, by `arc_bare_angles_are_both_rejected_in_one_pass`.
        //
        // The constrained-sketch `arc/3` (#5508) shares this name. When it
        // reaches `check_builtin_arg_types`, its index 2 is a point, so #5508
        // must land an `arg_count == 5` guard on this arm, per the HAZARD block.
        "arc" => const { &[
            angle_arg(2, "start_angle"),
            angle_arg(3, "end_angle"),
            direction_arg(4, "normal"),
        ] },

        // ── Euler builtins: the convention is an enum, not a String (#6082) ──
        // orient_euler(convention, a, b, c) — a CONSTRUCTOR, so the convention
        // comes FIRST and selects the meaning of the three angles that follow
        // (R_xyz(a, b, c) notation).
        //
        // arg0: convention → EulerConvention.
        // args1-3: the three angles — deliberately UNCHECKED. They are
        //   semantically ANGLE, but an ANGLE slot would break a live call site
        //   today: `kinematic_stdlib_smoke.rs` passes bare Reals (0.1, 0.2,
        //   0.3), not rad-suffixed literals, so the slot would turn a valid
        //   call into a hard compile error and drag a separable
        //   dimensioned-literal migration into #6082. Same decision-4
        //   gradualism as the rest of the table; adding the ANGLE slots means
        //   migrating those call sites first.
        //
        // The arity guard is forward-compat, matching `linear_pattern`'s: 4 is
        // the only accepted arity today (the eval arm returns Undef otherwise),
        // so without it a future overload would silently inherit a slot at an
        // index that denotes a different parameter.
        "orient_euler" if arg_count == 4 => &[CheckableArg {
            index: 0,
            name: "convention",
            expected: ExpectedArg::Enum {
                enum_name: "EulerConvention",
                type_name: "EulerConvention",
            },
        }],

        // orient_to_euler(q, convention) — a DECOMPOSER, so it is SUBJECT-first
        // like its siblings orient_log(q) / orient_to_axis_angle(q) /
        // orient_inverse(q), which puts the convention at arg 1. The asymmetry
        // with the constructor above is deliberate; see #6082 and the comment
        // on the `EulerConvention` declaration in `stdlib/geometry_traits.ri`.
        //
        // arg0: q — the Orientation subject, unchecked (arg0 is never a slot).
        // arg1: convention → EulerConvention.
        "orient_to_euler" if arg_count == 2 => &[CheckableArg {
            index: 1,
            name: "convention",
            expected: ExpectedArg::Enum {
                enum_name: "EulerConvention",
                type_name: "EulerConvention",
            },
        }],

        // ── SO(3) constructors' axes → Direction (direction-typing θ-a) ──────
        // Names per reify-stdlib orientation.rs's DirectionParams consts. Each
        // has exactly one accepted arity (the eval arm returns Undef
        // otherwise), so each arm carries the guard, like `orient_euler`'s.
        "orient_axis_angle" if arg_count == 2 => const { &[direction_arg(0, "axis")] },
        "orient_look_at" if arg_count == 2 => {
            const { &[direction_arg(0, "forward"), direction_arg(1, "up")] }
        }
        "orient_basis" if arg_count == 3 => {
            const {
                &[
                    direction_arg(0, "x_axis"),
                    direction_arg(1, "y_axis"),
                    direction_arg(2, "z_axis"),
                ]
            }
        }

        // ── frame_at(o, x: Direction, z: Direction) ──────────────────────────
        // arg0: the origin, unchecked (arg0 is never a slot).
        // args1-2: x / z → Direction, names per units.rs's frame_at row.
        // Single-form, so arity-agnostic.
        "frame_at" => const { &[direction_arg(1, "x"), direction_arg(2, "z")] },

        // ── joint ctors: axis → Direction (direction-typing ζ-a) ─────────────
        // arg0: the joint axis. prismatic/revolute accept 2 or 3 args (the 3rd
        // is the pivot) and cylindrical 3; the axis is arg0 at every one, so
        // the arm is arity-agnostic. The ranges and pivot stay unslotted.
        "prismatic" | "revolute" | "cylindrical" => const { &[direction_arg(0, "axis")] },
        // planar(axis_x, axis_y, range_x, range_y, range_theta): single-form.
        "planar" => const { &[direction_arg(0, "axis_x"), direction_arg(1, "axis_y")] },

        // ── PRB flexure ctors: axis → Direction (direction-typing ζ-a) ───────
        // Indices per the parse fns in reify-stdlib/src/flexures/*. Each
        // ctor's axis sits at the same index at every accepted arity (the
        // trailing neutral / declared-range args are optional), so the arms
        // are arity-agnostic.
        // (length|r, width|t, thickness|b, material, pivot, axis[, neutral[, range]])
        "prb_cantilever_beam"
        | "prb_fixed_fixed_beam"
        | "prb_notch_circular"
        | "prb_notch_elliptical"
        | "prb_notch_right_circular"
        | "prb_living_hinge"
        | "prb_cross_spring_pivot"
        | "prb_prismatic_blade"
        | "prb_two_axis_pivot" => const { &[direction_arg(5, "axis")] },
        // (length, width, thickness, blade_spacing, material, motion_axis, pivot[, range])
        "prb_parallelogram_flexure" | "prb_double_parallelogram_flexure" => {
            const { &[direction_arg(5, "motion_axis")] }
        }
        // prb_let_joint: (length, width, thickness, n_blades, material, pivot, axis, ..);
        // prb_cartwheel_flexure: (blade_count, length, width, thickness, material, pivot, axis, ..).
        "prb_let_joint" | "prb_cartwheel_flexure" => const { &[direction_arg(6, "axis")] },

        // All other names: empty (no argument to check).
        _ => &[],
    }
}

/// Check the compiled arguments of a builtin call against its known type
/// signatures, pushing [`DiagnosticCode::ArgTypeMismatch`] errors for
/// DEFINITE static mismatches only.
///
/// # Gradualism (PRD decision 6)
///
/// The check fires only when a definite concrete type is available:
/// - `Type::Error` — poison sentinel; silently skipped at EVERY arm (avoids
///   cascading diagnostics off an unrelated root-cause error).
/// - `Type::TypeParam(_)` — unresolved type variable; silently skipped at EVERY
///   arm (constraint-aware / auto-type-param resolution is out of scope for ζ).
/// - `Type::ScalarParam(_)` — unresolved DIMENSION placeholder (`Scalar<Q>`);
///   silently skipped at the [`ExpectedArg::Scalar`] arm ONLY, and still
///   REJECTED at the [`ExpectedArg::Int`] arm (task 6862) and every other.
///   The scope is per-arm, not global, because the defer is a
///   DIMENSION-comparison argument and an `Int` count slot is not a dimension
///   slot; each arm states its own half of the reasoning below.
/// - Any other variant — a concrete known type; compared against the slot's
///   expected dimension.
///
/// # Anti-cascade
///
/// This function is a pure side-effect on `diagnostics`: it does NOT change
/// `result_type` inference or the emitted `FunctionCall` IR node.  Wiring it
/// immediately after `coerce_list_helper_args` (before the result-type ladder)
/// keeps type-inference side-effect-free.
///
/// # Message format
///
/// Mirrors γ's runtime `ArgRejection::message` wording so compile-time (ζ) and
/// runtime (γ) diagnostics read consistently per PRD §7.3:
/// `"{builtin}: {arg_name} argument expects {type_name}, got {actual}"`
///
/// A `Scalar` slot appends its [`ExpectedArg::Scalar::migration_hint`] with
/// `"; "`, matching `ArgRejection::message`'s own shape (task 5750, PRD
/// `docs/prds/v0_6/units-length-gate-completion.md` decision D9):
/// `"{builtin}: {arg_name} argument expects {type_name}, got {actual}; {hint}"`
///
/// Concretely, for a LENGTH slot:
/// `"box: size_x argument expects Length, got Int; pass a dimensioned length such as `5mm`"`
///
/// Every dimension renders this hinted form — LENGTH, ANGLE and DENSITY alike —
/// because the field is non-optional. [`ExpectedArg::Int`] and
/// [`ExpectedArg::Enum`] render un-hinted, for there is nothing to migrate TO;
/// an [`ExpectedArg::Direction`] slot is hinted only for a `Vector3` (see
/// [`direction_mismatch_hint`]).
///
/// `{builtin}` is the SURFACE call name — the identifier the author actually
/// typed. The eval layer instead renders its prefix from the LOWERED kind
/// (`geometry_ops`' `prim_box` passes `&PrimitiveKind` as its `kind_label`), so
/// for a name that is an ALIAS of another op the two prefixes differ:
/// `box_centered(20, 20, 10)` reads `box_centered: …` here and `box: …` at
/// eval. That is deliberate — reporting the name in the source beats reporting
/// a lowering detail the author never wrote — and it is the one respect in
/// which the two renderings are not byte-identical. D9's substance, the C1
/// template and the shared migration hint, is unaffected. MEASURED and pinned
/// by `centered_alias_slots_name_the_surface_builtin_not_the_lowered_kind` in
/// `tests/builtin_arg_signature_tests.rs`.
pub(crate) fn check_builtin_arg_types(
    name: &str,
    compiled_args: &[CompiledExpr],
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Arity-keyed lookup (task 5652): overloaded builtins expose different
    // slots per call arity, so the checker must tell the table how many args
    // this call site actually passed.
    let slots = builtin_arg_slots(name, compiled_args.len());
    for slot in slots {
        let Some(arg) = compiled_args.get(slot.index) else {
            // Arg absent (call is short) — skip. Arity errors are handled
            // elsewhere; a short-arg call is not a type-mismatch.
            continue;
        };

        match &slot.expected {
            ExpectedArg::Scalar {
                dimension: expected_dim,
                type_name,
                migration_hint,
            } => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently.
                //
                // `Type::ScalarParam(Q)` (task 6862) is the third member, and
                // the only one whose membership is arm-SPECIFIC. It is the
                // unresolved-DIMENSION placeholder a dim-kinded generic
                // produces: `fn beam<Q: Dimension>(l: Scalar<Q>)` resolves `l`
                // to `ScalarParam("Q")` (`type_resolution.rs`'s
                // `classify_dim_slot`), which displays as `Scalar<Q>`. Its
                // FAMILY is known — it IS a scalar, so it belongs at a
                // dimension slot — and only its DIMENSION is open. There is
                // therefore nothing here to compare: judging the UNINSTANTIATED
                // body under the strict `DimensionVector` equality the next arm
                // applies can only ever REJECT, so every such site is a false
                // positive.
                //
                // WHAT THE DEFER COSTS — MEASURED, not assumed (reviewer
                // amendment). It does NOT hand the wrong-dimension case on to a
                // per-instantiation recheck: no such recheck exists. A generic
                // fn body is compiled ONCE, generically — which is precisely why
                // the false positive existed — and the call-site unify arm
                // accepts `ScalarParam(Q)` against any `Scalar { .. }`. So
                // `beam(10kg)` emits NO compile Error at all. That silence is
                // the gradualism trade this arm makes, and it is pinned as such
                // by `wrong_dimension_through_a_dim_kinded_generic_is_undiagnosed_at_compile`
                // in `tests/builtin_arg_signature_tests.rs`.
                //
                // What the cost does NOT extend to, each MEASURED and pinned in
                // the same file:
                //   - the same wrong dimension written INLINE
                //     (`extrude(circle(10kg), 10kg)`) is rejected HERE at
                //     compile (`ArgTypeMismatch`) and again at build by the
                //     eval-layer LENGTH gate (`DimensionedArgRejected`) —
                //     `wrong_dimension_written_inline_is_rejected_at_both_layers`;
                //   - through a NON-generic fn (`fn beam(l: Length)`) it is
                //     rejected by overload resolution at the call site —
                //     `wrong_dimension_through_a_non_generic_fn_is_rejected_at_the_call_site`.
                // Only the dim-kinded GENERIC form is undiagnosed, which is the
                // narrowest shape this defer could have.
                //
                // Two precedents make the same call elsewhere in the compiler,
                // and this arm aligns with both rather than inventing a rule:
                // `type_compat::is_mul_div_gradualism_skip` (type_compat.rs,
                // cited from expr.rs:2037) already lists `ScalarParam`
                // alongside `TypeParam` as a DEFER kind; and
                // `type_acceptance::scalar_param_arg_defers_at_scalar_slot`
                // (task 5627 γ D4-5 / PRD invariant I5)
                // makes the identical call at the struct-ctor layer.
                //
                // MEASURED symptom this closes: before task 6862,
                // `fn beam<Q: Dimension>(l: Scalar<Q>) -> Solid { extrude(circle(l), l) }`
                // instantiated at `beam(10mm)` hard-errored at `reify check`
                // (exit 1) with two `ArgTypeMismatch`es — `circle: radius
                // argument expects Length, got Scalar<Q>; …` and `extrude:
                // distance argument expects Length, got Scalar<Q>; …` — on code
                // that is correct at Q = LENGTH. Fixture:
                // `tests/fixtures/dim_kinded_length_slot.ri`; pinned by
                // `scalar_param_defers_at_length_slot` below and by
                // `dim_kinded_generic_param_at_length_slot_compiles_clean` in
                // `tests/builtin_arg_signature_tests.rs`. The defer is fenced
                // on the other side by `bare_int_at_length_slot_still_rejected`
                // and `wrong_dimension_scalar_at_length_slot_still_rejected`,
                // so it cannot widen into a blanket disable of the slot.
                //
                // Reach: this table was introduced by task 4493 over 7 LENGTH
                // slots and widened to 33 slotted names by task 5750, so the
                // gap was pre-existing but its exposed surface is now large.
                Type::Error | Type::TypeParam(_) | Type::ScalarParam(_) => continue,

                // Dimensioned scalar: mismatch only when the dimension differs.
                Type::Scalar { dimension } => {
                    if dimension == expected_dim {
                        continue; // correct — no diagnostic
                    }
                    let actual = &arg.result_type;
                    emit_mismatch(
                        name,
                        slot.name,
                        type_name,
                        actual,
                        Some(*migration_hint),
                        call_span,
                        diagnostics,
                    );
                }

                // Any other concrete type (Bool, Geometry, Vector, …): definite
                // kind mismatch where a dimensioned scalar is required.
                other => {
                    emit_mismatch(
                        name,
                        slot.name,
                        type_name,
                        other,
                        Some(*migration_hint),
                        call_span,
                        diagnostics,
                    );
                }
            },

            ExpectedArg::Enum {
                enum_name,
                type_name,
            } => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently.
                Type::Error | Type::TypeParam(_) => continue,

                // Correct — a value of the required enum type. The variant is
                // already validated upstream (see `ExpectedArg::Enum`).
                Type::Enum(actual) if actual == enum_name => continue,

                // Any other concrete type — a `String` (the form task #6082
                // removed), or an enum of the wrong type — is a definite
                // mismatch.
                other => {
                    emit_mismatch(
                        name,
                        slot.name,
                        type_name,
                        other,
                        None,
                        call_span,
                        diagnostics,
                    );
                }
            },

            ExpectedArg::Int { type_name } => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently.
                //
                // `Type::ScalarParam(_)` is DELIBERATELY absent here, and the
                // asymmetry with the `Scalar` arm above is the point (task
                // 6862). That defer is a DIMENSION-comparison argument — it
                // holds because at a dimension slot there is nothing to compare
                // until `Q` binds. An `Int` count slot is not a dimension slot:
                // `Scalar<Q>` is a dimensioned scalar for EVERY binding of `Q`,
                // and at Q = DIMENSIONLESS it is precisely the `Real` this
                // arm's own comment below already names as a definite mismatch.
                // The FAMILY mismatch is therefore decidable without
                // instantiating, so rejecting is correct and loses nothing.
                //
                // The precedent is narrow in exactly the same way:
                // `type_acceptance::scalar_param_arg_defers_at_scalar_slot` gates
                // on `matches!(param_type, Type::Scalar { .. })`, so it too
                // defers only at a scalar slot and nowhere else.
                //
                // Pinned by `scalar_param_still_rejected_at_int_slot` below,
                // which is what makes this asymmetry ENFORCEABLE rather than
                // merely conventional.
                Type::Error | Type::TypeParam(_) => continue,

                // Correct — a true `Int` count.
                Type::Int => continue,

                // Any other concrete type — including a dimensionless `Real`
                // (`Type::Scalar { DIMENSIONLESS }`, e.g. `2.5`) or a dimensioned
                // scalar (`3mm`) — is a definite mismatch where an `Int` is required.
                other => {
                    emit_mismatch(
                        name,
                        slot.name,
                        type_name,
                        other,
                        // No migration hint, always: a count is not a
                        // dimension migration, so there is nothing to point
                        // the author at. See `ExpectedArg::Int`.
                        None,
                        call_span,
                        diagnostics,
                    );
                }
            },

            ExpectedArg::Direction => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently. `ScalarParam`
                // is rejected, as at the Int arm: a `Scalar<Q>` is never a
                // Direction for any `Q`.
                Type::Error | Type::TypeParam(_) => continue,
                Type::Direction => continue,
                other => emit_mismatch(
                    name,
                    slot.name,
                    &Type::Direction.to_string(),
                    other,
                    direction_mismatch_hint(other),
                    call_span,
                    diagnostics,
                ),
            },
        }
    }
}

/// Emit a single `ArgTypeMismatch` error diagnostic.
///
/// `migration_hint`, when `Some`, is appended as `"; {hint}"` — matching
/// `ArgRejection::message`'s shape in `crates/reify-ir/src/arg_acceptance.rs`
/// exactly, so the compile-time and runtime renderings of one authoring mistake
/// are byte-identical (PRD decision D9). The LABEL is deliberately left
/// un-hinted: it is the short inline caret annotation, and the hint belongs on
/// the message where the eval layer puts it.
///
/// # Idempotent per (code, span, message) — the nested-argument double-walk
///
/// An EXACT duplicate — same [`DiagnosticCode::ArgTypeMismatch`], same call
/// span, same rendered message — is dropped rather than pushed a second time.
/// This is not defensive decoration: a call appearing as a NESTED geometry
/// ARGUMENT is walked by `compile_expr` more than once, so without this guard
/// `extrude(circle(4), 12mm)` renders `circle: radius …` twice. See the
/// module docs' "a NESTED geometry argument" section for the measured probe
/// table and why the fix belongs here rather than being pinned as intended.
///
/// The triple is the whole key, and each third of it is load-bearing: the three
/// axes of `box(1, 2, 3)` share one call span and one code, and differ only in
/// their message, so a span- or code-keyed drop would silently collapse three
/// real errors into one. Pinned by
/// `nested_primitive_keeps_one_diagnostic_per_axis` in
/// `tests/builtin_arg_signature_tests.rs`.
///
/// Cost is a linear scan of `diagnostics` per emitted mismatch — paid only on
/// the ERROR path, where the module is already failing to compile and the
/// diagnostic count is small. The happy path never reaches this function.
fn emit_mismatch(
    builtin: &str,
    arg_name: &str,
    type_name: &str,
    actual: &Type,
    migration_hint: Option<&'static str>,
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let base = format!("{builtin}: {arg_name} argument expects {type_name}, got {actual}");
    let msg = match migration_hint {
        Some(hint) => format!("{base}; {hint}"),
        None => base,
    };

    // Already reported for this exact position, by an earlier walk of the same
    // nested argument expression. Two diagnostics identical in code, span AND
    // message are indistinguishable in any renderer, so the second carries no
    // information — dropping it is a pure UX win.
    let already_reported = diagnostics.iter().any(|existing| {
        existing.code == Some(DiagnosticCode::ArgTypeMismatch)
            && existing.message == msg
            && existing.labels.first().map(|label| label.span) == Some(call_span)
    });
    if already_reported {
        return;
    }

    let label_msg = format!("expected {type_name}, got {actual}");
    diagnostics.push(
        Diagnostic::error(msg)
            .with_code(DiagnosticCode::ArgTypeMismatch)
            .with_label(DiagnosticLabel::new(call_span, label_msg)),
    );
}

/// A `Vector3` where a Direction is required gets the `dir(…)` fix hint; any
/// other kind renders un-hinted (direction-typing C7).
fn direction_mismatch_hint(actual: &Type) -> Option<&'static str> {
    matches!(actual, Type::Vector { n: 3, .. }).then_some(DIRECTION_MIGRATION_HINT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{GEOMETRY_FUNCTION_NAMES, GEOMETRY_TOPOLOGY_SELECTOR_NAMES};
    use reify_core::direction::DIRECTION_MIGRATION_HINT;
    use reify_core::{DimensionVector, Severity, SourceSpan, Type, identity::ValueCellId};
    use reify_ir::CompiledExpr;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::OnceLock;

    /// Inclusive upper bound for every arity sweep in this module.
    ///
    /// The largest arity any arm currently guards on is 11 (`linear_pattern_2d`),
    /// so this leaves headroom above every guard in the table — an arity-guarded
    /// arm cannot hide from a sweep by sitting just above the bound. Raise it if a
    /// future arm guards on a higher arity.
    const MAX_PROBED_ARITY: usize = 14;

    /// The curated exemption list for [`builtin_arg_slots`] keys that are NOT
    /// members of `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` (task 5652).
    ///
    /// The table is overwhelmingly a geometry topology-selector table, and
    /// [`arg_slot_keys_are_registered_builtin_names`] asserts exactly that — every
    /// key yielding slots is a topology selector *or* is named here. Membership is
    /// a deliberate, reviewed decision, not a convenience escape hatch; the same
    /// invariant rejects dead entries, so a key removed from the table cannot
    /// leave a stale name behind.
    ///
    /// Per entry, why it is exempt:
    ///
    /// - `generate` — the task-3994 list combinator. Its slot is an `Int` count,
    ///   the lone non-geometry, non-`Scalar` slot in the table; it hosts here only
    ///   because the checking mechanism is generic (name-keyed), which avoids a
    ///   parallel one-entry checker. It is a list helper, never a selector.
    ///
    /// - `linear_pattern`, `linear_pattern_2d` — CSG producers registered in
    ///   `GEOMETRY_FUNCTION_NAMES` (units.rs), gaining LENGTH `spacing` slots in
    ///   task 5652. They must **not** be moved into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` merely to satisfy that invariant:
    ///   `expr.rs::infer_type`'s `NoUserFunctions` ladder consults
    ///   `is_geometry_topology_selector` *ahead of* `is_geometry_function`, and
    ///   the selector arm resolves its
    ///   result type with
    ///   `topology_selector_result_type(name).expect("is_geometry_topology_selector implies result type")`.
    ///   Neither pattern name has an entry in `topology_selector_result_type`, so
    ///   slice membership alone would turn every `linear_pattern(…)` call into a
    ///   panic on that `expect` — the names would never reach the CSG arm at all.
    ///   `unresolved_function::tests::builtin_name_families_are_pairwise_disjoint`
    ///   pins that `GEOMETRY_FUNCTION_NAMES` and
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` share no name, and assertion (b) of
    ///   [`arg_slot_keys_are_registered_builtin_names`] pins the producers'
    ///   `GEOMETRY_FUNCTION_NAMES` membership.
    /// - The task-5750 PRIMITIVE and PROFILE producers — `box`, `box_centered`,
    ///   `cylinder`, `cylinder_centered`, `sphere`, `tube`, `cone`, `wedge`,
    ///   `torus`, `half_space`, `rectangle`, `circle`, `ellipse`. Every one is a
    ///   CSG/profile producer registered in `GEOMETRY_FUNCTION_NAMES`, gaining
    ///   its LENGTH dimension slots in task 5750 (PRD
    ///   `docs/prds/v0_6/units-length-gate-completion.md` leaf η). They are
    ///   exempted here for EXACTLY the reason `linear_pattern` is, and the
    ///   consequence of getting it wrong is identical: moving any of them into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset assertion
    ///   would route every `box(…)` call through `expr.rs`'s selector arm, whose
    ///   `topology_selector_result_type(name).expect(…)` has no entry for them
    ///   — turning the most common call in the language into a panic. The
    ///   exemption list is the correct lever; the slice is not.
    ///
    /// - The task-5750 MODIFY and SWEEP producers — `fillet`, `fillet_all`,
    ///   `chamfer`, `chamfer_asymmetric`, `shell`, `shell_open`, `thicken`,
    ///   `zone_slab`, `offset_solid`, `offset_curve`, `extrude`,
    ///   `extrude_symmetric`, `pipe`, `revolve`, `revolve_full`. Same story as
    ///   the primitives above: all fifteen are registered in
    ///   `GEOMETRY_FUNCTION_NAMES`, none is a topology selector, and none may
    ///   be moved into `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset
    ///   assertion. Since direction-typing ι-b, `revolve` and `revolve_full`
    ///   hold a DIRECTION slot (and `revolve` an ANGLE slot from leaf ζ) rather
    ///   than task 5750's origin LENGTH slots; `extrude_infinite` and
    ///   `extrude_infinite_symmetric` join them on the same footing, slotting
    ///   only their Direction.
    ///
    /// - The task-5750 TRANSFORM producers — `translate` and `rotate_around`.
    ///   Same story again: both are registered in `GEOMETRY_FUNCTION_NAMES`,
    ///   neither is a topology selector, and neither may be moved into the
    ///   selector slice. `rotate_around` holds no LENGTH slot any more: since
    ///   direction-typing ι-a its slots are a Direction axis and leaf ζ's
    ///   ANGLE, so its listing is no longer task 5750's at all.
    ///
    /// - The PRD 3 leaf ζ ANGLE producers — `rotate`, `arc` and `draft`. The
    ///   same story once more: all three are CSG/curve producers registered in
    ///   `GEOMETRY_FUNCTION_NAMES` (`crates/reify-compiler/src/units.rs`), none
    ///   is a topology selector, and none may be moved into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset assertion —
    ///   `topology_selector_result_type` (`units.rs`) has no entry for any of
    ///   them, so slice membership would turn every `rotate(…)` / `arc(…)` /
    ///   `draft(…)` call into a panic on `expr.rs`'s
    ///   `topology_selector_result_type(name).expect(…)`. The other ANGLE-slotted
    ///   names — `revolve`, `rotate_around`, `circular_pattern` — are listed
    ///   under their LENGTH-slot families.
    ///
    /// - The task-5662 PATTERN ORIGIN producers — `mirror` and
    ///   `circular_pattern`. Same story a third time: both are CSG producers
    ///   registered in `GEOMETRY_FUNCTION_NAMES`, neither is a topology
    ///   selector, and neither may be moved into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset assertion —
    ///   doing so would route every `mirror(...)` call through `expr.rs`'s
    ///   selector arm, whose
    ///   `topology_selector_result_type(name).expect(...)` has no entry for
    ///   them and would panic. `circular_pattern` ALSO holds an ANGLE slot at
    ///   both of its forms from PRD 3 leaf ζ. `mirror`'s only slot is now its
    ///   Direction normal (direction-typing ι-a); the listing reason is
    ///   unchanged.
    ///
    /// - The direction-typing σ datum constructor `frame_at`, a member of
    ///   `units.rs`'s `DATUM_CONSTRUCTOR_NAMES` typed by
    ///   `datum_constructor_result_type`, not a topology selector. Moving it
    ///   into `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` would route it through
    ///   `expr.rs`'s `topology_selector_result_type(name).expect(…)`, which has
    ///   no entry for it.
    ///
    /// - direction-typing θ-b's arity-2 `angle(a, b)`, a `units.rs`
    ///   `GEOMETRY_QUERY_NAMES` member typed by `geometry_query_result_type`,
    ///   not a topology selector. Moving it into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` would route it through `expr.rs`'s
    ///   `topology_selector_result_type(name).expect(…)`, which has no entry
    ///   for it.
    pub(crate) const NON_SELECTOR_ARG_SLOT_KEYS: &[&str] = &[
        "generate",
        "linear_pattern",
        "linear_pattern_2d",
        // Task #6082 — SO(3) builtins, not topology selectors; their
        // `convention` slot is the table's only ExpectedArg::Enum.
        "orient_euler",
        "orient_to_euler",
        // Task 5750 — primitives.
        "box",
        "box_centered",
        "cylinder",
        "cylinder_centered",
        "sphere",
        "tube",
        "cone",
        "wedge",
        "torus",
        "half_space",
        // Task 5750 — profiles.
        "rectangle",
        "circle",
        "ellipse",
        // Task 5750 — modify.
        "fillet",
        "fillet_all",
        "chamfer",
        "chamfer_asymmetric",
        "shell",
        "shell_open",
        "thicken",
        "zone_slab",
        "offset_solid",
        "offset_curve",
        // Task 5750 — sweep.
        "extrude",
        "extrude_symmetric",
        "pipe",
        "revolve",
        "revolve_full",
        // direction-typing ι-b (task 9647) — sweep producers whose only slot
        // is their Direction.
        "extrude_infinite",
        "extrude_infinite_symmetric",
        // Task 5750 — transforms.
        "translate",
        "rotate_around",
        // Task 5662 — pattern origin triples.
        "mirror",
        "circular_pattern",
        // PRD 3 leaf ζ (task 5782) — ANGLE producers.
        "rotate",
        "arc",
        "draft",
        // direction-typing σ (#9636) — datum constructor, Direction slots.
        "frame_at",
        // direction-typing ζ-a — joint and PRB flexure ctors, Direction axis
        // slots. JOINT_TYPED_FN_NAMES / FLEXURE_CTOR_FN_NAMES members, not
        // topology selectors.
        "prismatic",
        "revolute",
        "cylindrical",
        "planar",
        "prb_cantilever_beam",
        "prb_fixed_fixed_beam",
        "prb_notch_circular",
        "prb_notch_elliptical",
        "prb_notch_right_circular",
        "prb_living_hinge",
        "prb_cross_spring_pivot",
        "prb_prismatic_blade",
        "prb_two_axis_pivot",
        "prb_parallelogram_flexure",
        "prb_double_parallelogram_flexure",
        "prb_let_joint",
        "prb_cartwheel_flexure",
        // direction-typing θ-b — arity-2 `angle(a, b)`, Direction slots. A
        // GEOMETRY_QUERY_NAMES member, not a topology selector.
        "angle",
        // direction-typing θ-a — SO(3) constructors typed by
        // orientation_signatures.rs's ORIENTATION_TYPED_FN_NAMES, not topology
        // selectors; their axes are Direction slots.
        "orient_axis_angle",
        "orient_look_at",
        "orient_basis",
    ];

    // ── builtin_arg_slots table contract (step-1) ────────────────────────────

    fn mass_density_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
                migration_hint: DENSITY_MIGRATION_HINT,
            },
        }
    }

    fn angle_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::ANGLE,
                type_name: "Angle",
                migration_hint: ANGLE_MIGRATION_HINT,
            },
        }
    }

    fn length_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
                migration_hint: LENGTH_MIGRATION_HINT,
            },
        }
    }

    /// moment_of_inertia → arg1 density (MASS_DENSITY).
    #[test]
    fn moment_of_inertia_has_density_slot() {
        let slots = builtin_arg_slots("moment_of_inertia", 2);
        assert_eq!(
            slots.len(),
            1,
            "moment_of_inertia should have 1 slot, got: {:?}",
            slots
        );
        assert_eq!(slots[0], mass_density_slot(1, "density"));
    }

    /// center_of_mass → arg1 density (MASS_DENSITY).
    #[test]
    fn center_of_mass_has_density_slot() {
        let slots = builtin_arg_slots("center_of_mass", 2);
        assert_eq!(
            slots.len(),
            1,
            "center_of_mass should have 1 slot, got: {:?}",
            slots
        );
        assert_eq!(slots[0], mass_density_slot(1, "density"));
    }

    /// faces_by_normal → arg1 dir (Direction, θ-b) AND arg2 tol (ANGLE).
    #[test]
    fn faces_by_normal_has_direction_and_angle_slots() {
        assert_eq!(
            builtin_arg_slots("faces_by_normal", 3),
            [direction_slot(1, "dir"), angle_slot(2, "tol")]
        );
    }

    /// edges_parallel_to → arg1 axis (Direction, θ-b) AND arg2 tol (ANGLE).
    #[test]
    fn edges_parallel_to_has_direction_and_angle_slots() {
        assert_eq!(
            builtin_arg_slots("edges_parallel_to", 3),
            [direction_slot(1, "axis"), angle_slot(2, "tol")]
        );
    }

    /// Task 3523. The perpendicular selectors mirror
    /// faces_by_normal/edges_parallel_to: arg1 `axis` is a Direction
    /// (direction-typing θ-b) and arg2 `tol` is an ANGLE.
    /// (faces_by_surface_kind/edges_by_curve_kind and the extremal ctors take
    /// string/length args — no ANGLE slot.)
    #[test]
    fn perpendicular_selectors_have_direction_and_angle_slots() {
        for name in ["faces_perpendicular_to", "edges_perpendicular_to"] {
            // Canonical call arity: (solid, axis, tol).
            assert_eq!(
                builtin_arg_slots(name, 3),
                [direction_slot(1, "axis"), angle_slot(2, "tol")],
                "{name}: arg1 axis must be Direction, arg2 tol ANGLE"
            );
        }
    }

    /// Arity-2 `angle(a, b)` slots both operands as Direction (θ-b); arity 3 is
    /// the relation verb `angle(x, y, metric)`, policed by
    /// `relation_signatures`, and stays slot-free.
    #[test]
    fn angle_slots_its_two_directions_only_at_arity_two() {
        assert_eq!(
            builtin_arg_slots("angle", 2),
            [direction_slot(0, "a"), direction_slot(1, "b")]
        );
        for arg_count in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 2) {
            assert!(
                builtin_arg_slots("angle", arg_count).is_empty(),
                "angle at arity {arg_count} must carry no slot"
            );
        }
    }

    /// The three SO(3) constructors slot their axes as Direction (θ-a), each
    /// only at its one accepted arity.
    #[test]
    fn orientation_constructors_slot_their_directions_only_at_their_arity() {
        let expected: [(&str, usize, Vec<CheckableArg>); 3] = [
            ("orient_axis_angle", 2, vec![direction_slot(0, "axis")]),
            (
                "orient_look_at",
                2,
                vec![direction_slot(0, "forward"), direction_slot(1, "up")],
            ),
            (
                "orient_basis",
                3,
                vec![
                    direction_slot(0, "x_axis"),
                    direction_slot(1, "y_axis"),
                    direction_slot(2, "z_axis"),
                ],
            ),
        ];
        for (name, arity, slots) in expected {
            assert_eq!(
                builtin_arg_slots(name, arity),
                slots,
                "{name} at arity {arity}"
            );
            for arg_count in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != arity) {
                assert!(
                    builtin_arg_slots(name, arg_count).is_empty(),
                    "{name} at arity {arg_count} must carry no slot"
                );
            }
        }
    }

    /// edges_at_height → arg1 h (LENGTH) AND arg2 tol (LENGTH).
    #[test]
    fn edges_at_height_has_h_and_tol_slots() {
        let slots = builtin_arg_slots("edges_at_height", 3);
        assert_eq!(
            slots.len(),
            2,
            "edges_at_height should have 2 slots, got: {:?}",
            slots
        );
        assert_eq!(slots[0], length_slot(1, "h"));
        assert_eq!(slots[1], length_slot(2, "tol"));
    }

    /// Task 3523 (amendment). The extremal selectors take a distance `tol` at
    /// arg index 3 (after the geometry handle + axis/sense strings), so each must
    /// expose exactly one checkable LENGTH slot at index 3 — mirroring
    /// edges_at_height's LENGTH tol. This keeps a wrong-dimension tol (e.g. an
    /// ANGLE `1deg`) a compile-time error rather than an eval-time-only warning.
    #[test]
    fn extremal_selectors_have_length_tol_slot() {
        for name in ["extremal_by_bbox", "extremal_by_centroid"] {
            // Canonical call arity: (solid, axis, sense, tol).
            let slots = builtin_arg_slots(name, 4);
            assert_eq!(
                slots.len(),
                1,
                "{name} should have 1 slot, got: {:?}",
                slots
            );
            assert_eq!(
                slots[0],
                length_slot(3, "tol"),
                "{name} arg3 tol must be LENGTH"
            );
        }
    }

    // ── Task 5750 (units-length η): PRIMITIVE + PROFILE LENGTH slots ─────────
    //
    // PRD `docs/prds/v0_6/units-length-gate-completion.md` leaf η, work item 1
    // (boundary row 9). These pin the compile-layer half of task 5743's
    // eval-layer gates on the 21 primitive fields and the 5 profile fields.
    //
    // The arg NAMES are copied from the lowering sites in `geometry.rs` (the
    // `("width".to_string(), …)` pairs), not invented here: the eval layer
    // renders its rejection from those same strings, so a name that drifted
    // would silently break decision D9's premise that both layers word one
    // authoring mistake identically.
    //
    // None of these names is overloaded, so per [`builtin_arg_slots`]' stated
    // "guard only genuinely overloaded names" rule every arm stays
    // ARITY-AGNOSTIC. The helper below therefore sweeps `0..=MAX_PROBED_ARITY`
    // rather than probing the canonical arity alone: an arm that later grew a
    // stray `if arg_count == N` guard would still satisfy a single-arity probe
    // while having quietly dropped coverage for short and long calls.

    /// Assert `name` exposes exactly `expected` at EVERY arity in
    /// `0..=MAX_PROBED_ARITY`.
    ///
    /// Bundles the two halves of a non-overloaded arm's contract: WHICH slots
    /// it exposes (the table contract) and that the answer does not vary with
    /// arity (the no-stray-guard contract).
    fn assert_slots_at_every_arity(name: &str, expected: &[CheckableArg]) {
        for arg_count in 0usize..=MAX_PROBED_ARITY {
            assert_eq!(
                builtin_arg_slots(name, arg_count),
                expected,
                "builtin_arg_slots({name:?}, {arg_count}) must expose exactly the \
                 expected slots; {name} is not an overloaded name, so its \
                 arm must carry no `if arg_count ==` guard"
            );
        }
    }

    /// box / box_centered → [size_x@0, size_y@1, size_z@2].
    ///
    /// The positional `box(width, depth, height)` binds to the X/Y/Z extents,
    /// so the slots are named for their axis. A transposed pair here would
    /// still type-check but would misname the offending argument in every
    /// diagnostic the slot emits.
    #[test]
    fn box_primitives_have_three_length_slots() {
        for name in ["box", "box_centered"] {
            assert_slots_at_every_arity(
                name,
                &[
                    length_slot(0, "size_x"),
                    length_slot(1, "size_y"),
                    length_slot(2, "size_z"),
                ],
            );
        }
    }

    /// cylinder / cylinder_centered → [radius@0, height@1].
    ///
    /// `cylinder_centered` lowers to TWO ops (a Cylinder plus a compensating
    /// Translate), but the slot table is keyed on the CALL, whose two args are
    /// the same radius/height pair — the desugaring's internal `× -0.5`
    /// multiplier is a synthesised `CompiledExpr`, never a call-site argument,
    /// so `check_builtin_arg_types` never sees it.
    #[test]
    fn cylinder_primitives_have_radius_and_height_slots() {
        for name in ["cylinder", "cylinder_centered"] {
            assert_slots_at_every_arity(name, &[length_slot(0, "radius"), length_slot(1, "height")]);
        }
    }

    /// sphere → [radius@0].
    #[test]
    fn sphere_has_radius_slot() {
        assert_slots_at_every_arity("sphere", &[length_slot(0, "radius")]);
    }

    /// tube → [outer_r@0, inner_r@1, height@2].
    #[test]
    fn tube_has_three_length_slots() {
        assert_slots_at_every_arity(
            "tube",
            &[
                length_slot(0, "outer_r"),
                length_slot(1, "inner_r"),
                length_slot(2, "height"),
            ],
        );
    }

    /// cone → [bottom_radius@0, top_radius@1, height@2].
    #[test]
    fn cone_has_three_length_slots() {
        assert_slots_at_every_arity(
            "cone",
            &[
                length_slot(0, "bottom_radius"),
                length_slot(1, "top_radius"),
                length_slot(2, "height"),
            ],
        );
    }

    /// wedge → [width@0, depth@1, height@2, top_width@3].
    #[test]
    fn wedge_has_four_length_slots() {
        assert_slots_at_every_arity(
            "wedge",
            &[
                length_slot(0, "width"),
                length_slot(1, "depth"),
                length_slot(2, "height"),
                length_slot(3, "top_width"),
            ],
        );
    }

    /// torus → [major_radius@0, minor_radius@1].
    #[test]
    fn torus_has_two_radius_slots() {
        assert_slots_at_every_arity(
            "torus",
            &[
                length_slot(0, "major_radius"),
                length_slot(1, "minor_radius"),
            ],
        );
    }

    /// half_space(origin, normal) → the Direction `normal` only, at EVERY arity
    /// (direction-typing ι-c).
    ///
    /// The `origin` at index 0 stays unslotted: it is a Point3, which no
    /// `ExpectedArg` kind describes, and `accept_length_point3` gates it at
    /// build, as `rotate_around`'s pivot is. `half_space` is single-form, so its
    /// arm carries no `if arg_count ==` guard.
    #[test]
    fn half_space_slots_its_normal_as_a_direction_but_never_its_point3_origin() {
        assert_slots_at_every_arity("half_space", &[direction_slot(1, "normal")]);
    }

    /// rectangle → [width@0, height@1].
    #[test]
    fn rectangle_has_width_and_height_slots() {
        assert_slots_at_every_arity(
            "rectangle",
            &[length_slot(0, "width"), length_slot(1, "height")],
        );
    }

    /// circle → [radius@0].
    #[test]
    fn circle_has_radius_slot() {
        assert_slots_at_every_arity("circle", &[length_slot(0, "radius")]);
    }

    /// ellipse → [semi_major@0, semi_minor@1].
    #[test]
    fn ellipse_has_two_semi_axis_slots() {
        assert_slots_at_every_arity(
            "ellipse",
            &[
                length_slot(0, "semi_major"),
                length_slot(1, "semi_minor"),
            ],
        );
    }

    /// The PROFILE family's variadic sibling stays wholly slot-free.
    ///
    /// `polygon(x1, y1, x2, y2, …)` IS Contract-C gated at eval (task 5661, via
    /// the variadic route), but its positions are arity-OPEN and its compile-
    /// layer arg names are the inert `c0`…`cN` that `geometry.rs`'s arm
    /// synthesises — so there is no index-keyed `CheckableArg` to write, and a
    /// `c0`-named diagnostic would not word the mistake the way the eval layer
    /// does. Pinned so the omission reads as a decision rather than an oversight.
    #[test]
    fn polygon_stays_slot_free_because_its_positions_are_arity_open() {
        assert_slots_at_every_arity("polygon", &[]);
    }

    // ── Task 5750 (units-length η): MODIFY + SWEEP LENGTH slots ──────────────
    //
    // The compile-layer half of task 5744's eval-layer gates on the modify and
    // sweep magnitudes. Same name-copied-from-the-lowering-site discipline as
    // the primitive block above; sources are `geometry_modify.rs` and
    // `geometry.rs`'s Sweep arms.
    //
    // UNLIKE the primitives, this family contains the table's first genuinely
    // OVERLOADED names. `fillet` and `chamfer` each accept a 2-arg all-edges
    // form and a 3-arg curated-edges form, and the magnitude MOVES between
    // them: `fillet(target, radius)` vs `fillet(target, edges, radius)`. An
    // arity-agnostic `radius@1` slot would therefore fire on the 3-arg form's
    // `edges` SELECTOR — a false positive on correct code, which is exactly the
    // failure `linear_pattern_2d`'s guard was introduced to prevent. Both arms
    // must be arity-keyed, and the tests below assert the negative half (no
    // slot at the other form's index) as well as the positive.

    /// fillet / chamfer are OVERLOADED: the magnitude moves with the arity.
    ///
    /// Asserts both forms AND that neither form leaks a slot at the other's
    /// index — the arity-keying is only load-bearing if the wrong index is
    /// genuinely empty, and an `assert_eq!` on the expected vec alone would
    /// pass for an arm that returned both slots at every arity.
    #[test]
    fn fillet_and_chamfer_slots_are_arity_keyed() {
        for (name, arg) in [("fillet", "radius"), ("chamfer", "distance")] {
            // 2-arg all-edges form: (target, magnitude).
            assert_eq!(
                builtin_arg_slots(name, 2),
                vec![length_slot(1, arg)],
                "{name}(target, {arg}) must expose its magnitude at index 1"
            );
            // 3-arg curated form: (target, edges, magnitude).
            assert_eq!(
                builtin_arg_slots(name, 3),
                vec![length_slot(2, arg)],
                "{name}(target, edges, {arg}) must expose its magnitude at \
                 index 2 — index 1 holds the edge SELECTOR there, and slotting \
                 it would emit a false ArgTypeMismatch on correct code"
            );
            // Every other arity is an arity ERROR at lowering, not an overload,
            // so the table must not invent a layout for it.
            for arg_count in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 2 && *n != 3) {
                assert!(
                    builtin_arg_slots(name, arg_count).is_empty(),
                    "{name} accepts only 2 or 3 args; arity {arg_count} must \
                     yield no slots, got {:?}",
                    builtin_arg_slots(name, arg_count)
                );
            }
        }
    }

    /// chamfer_asymmetric(target, edges, d1, d2) → [d1@2, d2@3].
    ///
    /// Single-form (`check_arg_count_exact(4)`), so arity-agnostic. BOTH
    /// magnitudes are slotted: the eval layer diagnoses `d1` and `d2` in one
    /// build, and a compile layer that reported only the first would degrade
    /// that to two edit-build cycles for one line.
    #[test]
    fn chamfer_asymmetric_has_both_distance_slots() {
        assert_slots_at_every_arity(
            "chamfer_asymmetric",
            &[length_slot(2, "d1"), length_slot(3, "d2")],
        );
    }

    /// The single-magnitude modify family → the magnitude at index 1.
    ///
    /// Every one of these holds its magnitude at index 1 REGARDLESS of arity,
    /// so all stay arity-agnostic per the table's rule:
    /// * `shell` uses `check_arg_count_at_least(2)` — args 2.. are face
    ///   indices, so index 1 is `thickness` at every accepted arity. Guarding
    ///   it on one arity would hollow out coverage for the curated forms.
    /// * `shell_open` is exact-3 and `fillet_all` / `thicken` / `zone_slab` /
    ///   `offset_solid` are exact-2, but a guard would buy nothing: index 1
    ///   denotes the same parameter in the only form each accepts.
    /// * `offset_curve` accepts 2 OR 3 args and is the interesting case — it is
    ///   overloaded in ARITY but not in LAYOUT, because `distance` stays at
    ///   index 1 in both forms and only the optional third argument differs.
    ///   That is precisely the situation the table's rule says NOT to guard.
    #[test]
    fn single_magnitude_modify_slots_are_arity_agnostic() {
        for (name, arg) in [
            ("shell", "thickness"),
            ("shell_open", "thickness"),
            ("thicken", "offset"),
            ("fillet_all", "radius"),
            ("zone_slab", "width"),
            ("offset_solid", "distance"),
            ("offset_curve", "distance"),
        ] {
            assert_slots_at_every_arity(name, &[length_slot(1, arg)]);
        }
    }

    /// offset_curve's THIRD argument is never slotted.
    ///
    /// At arity 3 it is either a reference Surface handle or a direction vec3 —
    /// `geometry_modify.rs` stashes it under the neutral name `"third"` and
    /// defers the disambiguation to eval. A direction's components are
    /// legitimately bare, and the closure guard's `ALLOWLIST`
    /// (`units_length_closure_guard.rs`) names this position explicitly as
    /// deliberately-not-gated ("its own production diagnostic calls it a
    /// direction vec3"), so a LENGTH slot here would reject correct `.ri`.
    /// Stated positively rather than left to the vec assertion above, because
    /// it is a DECISION, not an omission.
    #[test]
    fn offset_curve_third_argument_is_never_slotted() {
        let slotted: Vec<usize> = builtin_arg_slots("offset_curve", 3)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert!(
            !slotted.contains(&2),
            "offset_curve's 3rd argument is a reference Surface OR a direction \
             vec3 and must stay slot-free; got slots at {slotted:?}"
        );
    }

    /// extrude / extrude_symmetric / pipe → the magnitude at index 1.
    ///
    /// All exact-2 single-form, so arity-agnostic. `pipe`'s index 0 is its
    /// PATH (a geometry handle resolved through `profiles[0]`), and `extrude`'s
    /// is its profile — both arg0, both permanently unchecked.
    #[test]
    fn two_arg_sweep_slots_are_arity_agnostic() {
        for (name, arg) in [
            ("extrude", "distance"),
            ("extrude_symmetric", "distance"),
            ("pipe", "radius"),
        ] {
            assert_slots_at_every_arity(name, &[length_slot(1, arg)]);
        }
    }

    /// revolve → one GUARDED layout per form (direction-typing ι-b, task 9647).
    ///
    /// `revolve(profile, origin, direction, angle)` slots the Direction at 2
    /// and the angle at 3; `revolve(profile, axis, angle)` slots only the
    /// angle, at 2. Index 2 names a different parameter in each form, which is
    /// the HAZARD block's worked example, so the arms carry `if arg_count ==`
    /// guards and every other arity slots nothing. The Point3 origin and the
    /// Axis datum are not slotted: no `ExpectedArg` kind exists for either, so
    /// both are gated at build.
    #[test]
    fn revolve_slots_one_layout_per_form() {
        assert_eq!(
            builtin_arg_slots("revolve", 4),
            [direction_slot(2, "direction"), angle_slot(3, "angle")]
        );
        assert_eq!(builtin_arg_slots("revolve", 3), [angle_slot(2, "angle")]);
        for arg_count in (0usize..=MAX_PROBED_ARITY).filter(|n| ![3, 4].contains(n)) {
            assert!(
                builtin_arg_slots("revolve", arg_count).is_empty(),
                "revolve/{arg_count} is not a revolve form and must slot nothing"
            );
        }
    }

    /// CONTROL — `revolve_full(profile, origin, direction)` slots the Direction
    /// and NO angle.
    ///
    /// The `angle` in its LOWERED op-args is a compiler-SYNTHESIZED
    /// `Value::angle(TAU)` typed `Type::angle()` (`geometry.rs`, the type PRD
    /// D10 ratified). No author ever writes it, so there is no user-written
    /// position for an index-keyed table to gate.
    #[test]
    fn revolve_full_slots_the_direction_only_because_its_2pi_is_synthesized() {
        assert_slots_at_every_arity("revolve_full", &[direction_slot(2, "direction")]);
    }

    /// extrude_infinite / extrude_infinite_symmetric → the Direction at 1.
    #[test]
    fn extrude_infinite_pair_slots_the_direction() {
        for name in ["extrude_infinite", "extrude_infinite_symmetric"] {
            assert_slots_at_every_arity(name, &[direction_slot(1, "direction")]);
        }
    }

    /// arc(center, radius, start_angle, end_angle, normal) → BOTH angles and
    /// the Direction `normal`, at EVERY arity (direction-typing ι-c); neither
    /// its centre nor its radius.
    ///
    /// The table's first CURVE producer. Single-form at exactly 5 args, so
    /// arity-agnostic per the "guard only genuinely overloaded names" rule — a
    /// gratuitous `arg_count == 5` guard here would silently hollow out short
    /// and long calls for no gain.
    ///
    /// The two unslotted positions are excluded for DIFFERENT reasons:
    /// * `center`@0 is a Point3, which no `ExpectedArg` kind describes;
    ///   `accept_length_point3` gates it at build.
    /// * `radius`@1 IS Length-semantic, and its absence is a SCOPE boundary
    ///   rather than a judgement that it is dimensionless. Curve producers were
    ///   outside the four Contract C families of
    ///   `docs/prds/v0_6/units-length-gate-completion.md` leaf η, so its compile
    ///   slot remains PRD 1's territory; the build gates it.
    #[test]
    fn arc_slots_both_angles_and_its_normal_but_neither_its_centre_nor_its_radius() {
        assert_slots_at_every_arity(
            "arc",
            &[
                angle_slot(2, "start_angle"),
                angle_slot(3, "end_angle"),
                direction_slot(4, "normal"),
            ],
        );
    }

    /// draft(target, angle, plane) / draft(target, faces, angle, plane) → the
    /// ANGLE, keyed per arity because it MOVES between the two forms.
    ///
    /// The same moving-magnitude shape as `fillet`/`chamfer`
    /// (`geometry_modify.rs`' `draft` arm): angle@1 at arity 3, angle@2 at
    /// arity 4, where index 1 is the face SELECTOR. Either arity-agnostic
    /// spelling fires on correct code at the other form — an agnostic angle@1
    /// would demand an Angle of the face selector at arity 4, and an agnostic
    /// angle@2 would demand an Angle of the neutral PLANE at arity 3.
    #[test]
    fn draft_angle_slot_moves_with_its_arity() {
        assert_eq!(
            builtin_arg_slots("draft", 3),
            vec![angle_slot(1, "angle")],
            "draft(target, angle, plane) — the angle is arg1; the neutral plane \
             at arg2 is not a scalar"
        );
        assert_eq!(
            builtin_arg_slots("draft", 4),
            vec![angle_slot(2, "angle")],
            "draft(target, faces, angle, plane) — arg1 is the face SELECTOR, the \
             angle is arg2"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 3 && *n != 4) {
            assert!(
                builtin_arg_slots("draft", arity).is_empty(),
                "draft at arity {arity} is neither accepted form, so it must \
                 expose NO slots"
            );
        }
    }

    // ── Task 5750 (units-length η): TRANSFORM LENGTH slots ───────────────────
    //
    // Task 5623's eval-layer gates on `translate`'s displacement and
    // `rotate_around`'s pivot; the pivot became a `Point3` value gated at build
    // when direction-typing ι-a retired the 8-arg form, so only `translate`
    // keeps LENGTH slots here. Not one of the four families this leaf's task
    // text names — it is pulled in by contract C6, whose named fixtures
    // (`translate_non_geometry_target_uses_fallback`,
    // `rotate_around_let_bound_target_ops`) force the decision either way, so
    // the row is completed rather than left half-open. Arg names from
    // `geometry_transform.rs`.

    /// translate(target, dx, dy, dz) → the DISPLACEMENT triple only.
    ///
    /// Index 0 is the geometry handle: permanently unchecked, ε=4358's
    /// territory, and already pinned in general by `arg0_never_fires`. Asserted
    /// here too, positively, because `translate`'s displacement starts at index
    /// 1 and an off-by-one in the arm would land exactly there.
    #[test]
    fn translate_slots_the_displacement_but_never_the_handle() {
        assert_slots_at_every_arity(
            "translate",
            &[
                length_slot(1, "dx"),
                length_slot(2, "dy"),
                length_slot(3, "dz"),
            ],
        );

        let slotted: Vec<usize> = builtin_arg_slots("translate", 4)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert!(
            !slotted.contains(&0),
            "translate arg0 is the geometry handle and must stay slot-free; \
             got slots at {slotted:?}"
        );
    }

    /// rotate(target, axis, angle) → the Direction `axis` and the ANGLE, at
    /// arity 3 ONLY (direction-typing ι-a, task 9646; the angle is PRD
    /// `docs/prds/v0_6/angle-units-surface-convergence.md` boundary row B4).
    ///
    /// The guard is load-bearing because `rotate` stays overloaded: at arity 2,
    /// task 4166's `rotate(target, orientation)`, index 1 holds an Orientation
    /// VALUE, so an arity-agnostic `axis@1` Direction slot would fire on correct
    /// code. The retired scalar arity 5 exposes no slots either: such a call
    /// draws only its arity error.
    #[test]
    fn rotate_slots_are_arity_3_only() {
        assert_eq!(
            builtin_arg_slots("rotate", 3),
            vec![direction_slot(1, "axis"), angle_slot(2, "angle")],
            "rotate(target, axis, angle) — arg0 is the geometry handle, arg1 the \
             Direction axis and arg2 the ANGLE"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 3) {
            assert!(
                builtin_arg_slots("rotate", arity).is_empty(),
                "rotate at arity {arity} is not the 3-arg Direction form, so it must \
                 expose NO slots — at arity 2 index 1 is an Orientation VALUE, and \
                 arity 5 is the retired scalar form; got {:?}",
                builtin_arg_slots("rotate", arity)
            );
        }
    }

    /// rotate_around(target, pivot, axis, angle) → the Direction `axis` and the
    /// ANGLE, at EVERY arity (direction-typing ι-a, task 9646).
    ///
    /// `rotate_around` is single-form again once the scalar
    /// `rotate_around(target, px, py, pz, ax, ay, az, angle)` retires, so its arm
    /// carries no `if arg_count ==` guard (the guard-only-overloaded-names rule).
    /// The `pivot` at index 1 stays unslotted: it is a Point3, which no
    /// `ExpectedArg` kind describes, and `accept_length_point3` gates it at build.
    #[test]
    fn rotate_around_slots_the_axis_and_the_angle_but_never_the_pivot() {
        assert_slots_at_every_arity(
            "rotate_around",
            &[direction_slot(2, "axis"), angle_slot(3, "angle")],
        );
    }

    /// A retired 8-arg `rotate_around` call is an arity error that names the
    /// Direction form (PRD B13). Its arm is arity-agnostic, so the call may also
    /// draw slot mismatches at `axis@2` and `angle@3`: accepted migration noise,
    /// since the arity error is what teaches the form.
    #[test]
    fn rotate_around_retired_8_arg_call_names_the_direction_form() {
        let module = compile_module(
            "module test\n\nstructure def S {\n    let b = box(1mm, 1mm, 1mm)\n    \
             let r = rotate_around(b, 0mm, 0mm, 0mm, 0, 0, 1, 90deg)\n}\n",
        );
        let expected = "rotate_around() expects 4 arguments: \
                        rotate_around(geo, point3(x, y, z), dir(x, y, z), angle), got 8";
        assert!(
            module
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.message == expected),
            "expected the Error {expected:?}; got: {:#?}",
            module.diagnostics
        );
    }

    /// `scale` stays wholly slot-free — its factor is DIMENSIONLESS.
    ///
    /// A scale factor is a ratio, so demanding a Length of it would reject
    /// correct `.ri` outright. The closure guard's `ALLOWLIST`
    /// (`units_length_closure_guard.rs`) names the dimensionless scale FACTOR
    /// explicitly among the deliberately-not-gated set. Pinned as a seam
    /// control: `scale` sits in the same `compile_transform_op` match as
    /// `translate` and `rotate_around`, so "the transform family is gated" must
    /// not be read as "every transform argument is gated".
    ///
    /// Covers BOTH of its lowered forms — the uniform `factor` and the
    /// `ScaleNonUniform` `factors` vec3 — since the sweep probes every arity
    /// and the arm is keyed on the name alone.
    #[test]
    fn scale_stays_slot_free_because_its_factor_is_dimensionless() {
        assert_slots_at_every_arity("scale", &[]);
    }

    /// Task 5652 (step-1 RED). Pins the guard-only-overloaded-names half of
    /// [`builtin_arg_slots`]'s contract (rule and rationale documented there):
    /// the `arg_count` parameter is *available* to every arm but *used* only by
    /// genuinely overloaded names, so adding the dimension changed no existing
    /// behaviour — and (b) never conjures slots for an unrecognized name.
    ///
    /// RED for a compile reason when written: `builtin_arg_slots` took one
    /// parameter, so the crate's test build failed. That failure IS the pinned
    /// deliverable — the signature change itself.
    #[test]
    fn arg_slots_are_arity_aware() {
        // (a) Non-overloaded selector arms are arity-agnostic: the same slots
        // come back regardless of how many args the call site actually passed.
        for arity in [2usize, 3] {
            assert_eq!(
                builtin_arg_slots("moment_of_inertia", arity),
                vec![mass_density_slot(1, "density")],
                "moment_of_inertia must expose its density slot at every arity \
                 (probed {arity}) — it is not an overloaded builtin, so adding \
                 the arity dimension must not gate it"
            );
        }
        for arity in [2usize, 3] {
            assert_eq!(
                builtin_arg_slots("edges_at_height", arity),
                vec![length_slot(1, "h"), length_slot(2, "tol")],
                "edges_at_height must expose both LENGTH slots at every arity \
                 (probed {arity}); gating it on arity 3 would hollow out \
                 edges_at_height_short_args_no_panic, which passes 2 args"
            );
        }

        // (b) An unrecognized name returns empty at every arity probed — the
        // arity parameter must never conjure slots for a name not in the table.
        for arity in 0usize..=MAX_PROBED_ARITY {
            assert!(
                builtin_arg_slots("definitely_not_a_builtin", arity).is_empty(),
                "unrecognized name must return empty slots at arity {arity}"
            );
        }
    }

    /// Names with no dimensioned-scalar arg or unrecognized names return empty.
    #[test]
    fn empty_for_unchecked_names() {
        let unchecked = [
            "edges",
            "faces",
            "adjacent_faces",
            "shared_edges",
            "split",
            "face",
            "edge",
            "solid_body",
            "volume",
            // `"box"` used to sit here. Task 5750 (units-length η) gave it
            // three LENGTH slots, so it is no longer an unchecked
            // name; `box_primitives_have_three_length_slots` is now its pin.
            // `union` replaces it as a still-unslotted CSG producer so this
            // list keeps covering that shape.
            "union",
            "",
            "closest_point",
            "is_on",
            "angle_between_surfaces",
            "edges_by_length",
            "faces_by_area",
        ];
        for name in unchecked {
            // Swept across arities: an unchecked name must stay slot-free at
            // EVERY call arity, so no arity guard can accidentally admit one.
            for arg_count in 0usize..=MAX_PROBED_ARITY {
                let slots = builtin_arg_slots(name, arg_count);
                assert!(
                    slots.is_empty(),
                    "builtin_arg_slots({:?}, {}) should be empty, got {:?}",
                    name,
                    arg_count,
                    slots
                );
            }
        }
    }

    /// Coverage invariant (amended by task 5652): every key in the table's
    /// domain is either a topology selector (`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`)
    /// or a deliberately curated exemption ([`NON_SELECTOR_ARG_SLOT_KEYS`]) —
    /// catching typos and keeping the arg-slot table consistent with the
    /// recognized families even as new keys land.
    ///
    /// # Why this replaces `arg_slot_keys_are_subset_of_topology_selector_names`
    ///
    /// The predecessor asserted the same subset but only ever *probed* names
    /// drawn from that same slice plus a hardcoded typo list, so a non-selector
    /// key was never probed and the assertion was vacuous for it — `generate` has
    /// been one since task 3994.  The fix is to CLOSE that hole (probe a
    /// genuinely wider name set, name the exemptions explicitly) rather than to
    /// widen the selector slice; why widening would be actively wrong is on
    /// [`NON_SELECTOR_ARG_SLOT_KEYS`].
    ///
    /// The probe set is the closed-world family table (via
    /// [`slot_probe_names`]) + the typo/non-geometry list, swept across arities
    /// up to [`MAX_PROBED_ARITY`] so neither a new key from an unrelated family
    /// (say a MASS_DENSITY slot on `mass_properties`) nor an arity-guarded arm
    /// can hide from the invariant.
    #[test]
    fn arg_slot_keys_are_registered_builtin_names() {
        // Extra non-selector names that must never map to non-empty slots.
        // `"box"` and `"cylinder"` used to head this list. Task 5750
        // (units-length η) gave both LENGTH slots, so they must move out of a
        // must-stay-empty list; they are exempted via NON_SELECTOR_ARG_SLOT_KEYS
        // instead. `union` / `difference` replace them so the list still NAMES a
        // registered CSG producer that carries no dimensioned arg.
        let extra_non_selector: &[&str] = &[
            "union",
            "difference",
            "vec3",
            "cross",
            "dot",
            "sqrt",
            "abs",
            "",
            "volume",
            "body",
            // Deliberate typos of actual checked names — must all return empty.
            "moment_of_inertta",
            "center_of_mas",
            "faces_by_norml",
            // Typos of the new task-5652 pattern keys.
            "linear_patern",
            "linear_pattern_3d",
            // Typos / near-misses of the new task-5750 primitive + profile keys.
            // Unlike the family-slice names above, these are reachable ONLY by
            // being listed here, and they are the failure mode the arms invite:
            // a `"box" | "boxx" =>` fat-finger would hand slots to a name that
            // does not exist.
            "boxes",
            "cylindar",
            "rectangel",
            "elipse",
            "half_spce",
        ];

        let mut any_nonempty = false;
        for name in slot_probe_names()
            .into_iter()
            .chain(extra_non_selector.iter().copied())
        {
            // Swept across arities so an arity-guarded arm cannot hide from
            // the invariant by being empty at the one arity probed.
            for arg_count in 0usize..=MAX_PROBED_ARITY {
                let slots = builtin_arg_slots(name, arg_count);
                if !slots.is_empty() {
                    any_nonempty = true;
                    assert!(
                        GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
                            || NON_SELECTOR_ARG_SLOT_KEYS.contains(&name),
                        "builtin_arg_slots({:?}, {}) returned non-empty slots, but {:?} \
                         is in neither GEOMETRY_TOPOLOGY_SELECTOR_NAMES nor \
                         NON_SELECTOR_ARG_SLOT_KEYS; fix the name, or — if the key is \
                         intentional — record it in NON_SELECTOR_ARG_SLOT_KEYS with a \
                         rationale",
                        name,
                        arg_count,
                        name
                    );
                }
            }
        }

        // Sanity: the table must not be empty.
        assert!(
            any_nonempty,
            "no names returned non-empty slots — the builtin_arg_slots table \
             appears to be empty or unreachable"
        );

        // (a) No dead allow-list entries: every exemption must actually earn its
        // place by yielding slots at SOME arity. Without this, a key removed
        // from the table would leave a stale name in the exemption list, and the
        // invariant above would silently stop covering it.
        for &name in NON_SELECTOR_ARG_SLOT_KEYS {
            let yields_slots = (0usize..=MAX_PROBED_ARITY)
                .any(|arg_count| !builtin_arg_slots(name, arg_count).is_empty());
            assert!(
                yields_slots,
                "NON_SELECTOR_ARG_SLOT_KEYS contains {:?}, but builtin_arg_slots({:?}, n) \
                 is empty for every n in 0..={} — the exemption is dead; \
                 remove it from the list",
                name, name, MAX_PROBED_ARITY
            );
        }

        // (b) The pattern keys are deliberately NOT topology selectors, and ARE
        // registered CSG producers. Pinning this is the point of the exemption
        // list: adding them to GEOMETRY_TOPOLOGY_SELECTOR_NAMES would satisfy the
        // subset assertion above while actively breaking dispatch — see
        // NON_SELECTOR_ARG_SLOT_KEYS for the mechanism.
        for &name in &[
            "linear_pattern",
            "linear_pattern_2d",
            // Task 5662 — same mechanism, and the one where a mis-move would
            // panic rather than merely misreport: `expr.rs`' selector arm calls
            // `topology_selector_result_type(name).expect(...)`.
            "mirror",
            "circular_pattern",
        ] {
            assert!(
                !GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name),
                "{:?} must NOT be in GEOMETRY_TOPOLOGY_SELECTOR_NAMES — it is a CSG \
                 producer in GEOMETRY_FUNCTION_NAMES; it is exempted via \
                 NON_SELECTOR_ARG_SLOT_KEYS instead",
                name
            );
            assert!(
                GEOMETRY_FUNCTION_NAMES.contains(&name),
                "{:?} is expected to be a registered CSG producer in \
                 GEOMETRY_FUNCTION_NAMES",
                name
            );
        }

        // (c) Smoke: the five canonical checked selector names must individually
        // return non-empty at their canonical call arity.
        for &(name, arg_count) in &[
            ("center_of_mass", 2usize),
            ("moment_of_inertia", 2),
            ("faces_by_normal", 3),
            ("edges_parallel_to", 3),
            ("edges_at_height", 3),
        ] {
            assert!(
                !builtin_arg_slots(name, arg_count).is_empty(),
                "expected non-empty slots for {:?}; \
                 has the name been removed from the table?",
                name
            );
        }
    }

    // ── check_builtin_arg_types unit tests (step-3) ──────────────────────────

    fn dummy_cell_id() -> ValueCellId {
        ValueCellId {
            entity: "test_entity".to_string(),
            member: "x".to_string(),
        }
    }

    fn dummy_span() -> SourceSpan {
        SourceSpan::new(0, 10)
    }

    fn arg_expr(ty: Type) -> CompiledExpr {
        CompiledExpr::value_ref(dummy_cell_id(), ty)
    }

    /// (a) DEFINITE mismatch: moment_of_inertia arg1 = Scalar{DIMENSIONLESS}
    /// → exactly 1 Error diagnostic with code ArgTypeMismatch naming key parts.
    ///
    /// Also pins the `{actual}` rendering: `Type::dimensionless_scalar()` renders
    /// as `"Real"` via `Type::Display` (ty.rs:432-433 — dimensionless scalars are
    /// rendered as "Real", not "Scalar[dimensionless]").  The compile-time message
    /// must say "got Real" so it reads consistently with the runtime
    /// `ArgRejection::message` wording (PRD §7.3).
    #[test]
    fn moment_of_inertia_dimensionless_arg1_gives_error() {
        let args = vec![
            arg_expr(Type::Geometry),               // arg0 — unchecked
            arg_expr(Type::dimensionless_scalar()), // arg1 — bare Real
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("moment_of_inertia", &args, dummy_span(), &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "expected exactly 1 diagnostic, got: {:?}",
            diags
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            d.message.contains("moment_of_inertia"),
            "message missing builtin name: {}",
            d.message
        );
        assert!(
            d.message.contains("density"),
            "message missing arg name: {}",
            d.message
        );
        assert!(
            d.message.contains("Density"),
            "message missing type name: {}",
            d.message
        );
        assert!(
            d.message.contains("expects"),
            "message missing 'expects': {}",
            d.message
        );
        // Pin {actual} rendering: Type::dimensionless_scalar() Display = "Real"
        // (ty.rs — dimensionless scalars write "Real", not "Scalar[dimensionless]").
        // This ensures compile-time and runtime wordings stay in sync (PRD §7.3).
        assert!(
            d.message.contains("got Real"),
            "message should say 'got Real' for a bare dimensionless scalar; \
             Type::dimensionless_scalar() Display must render as \"Real\": {}",
            d.message
        );
    }

    /// (b) CORRECT: moment_of_inertia arg1 = Scalar{MASS_DENSITY} → 0 diagnostics.
    #[test]
    fn moment_of_inertia_correct_density_gives_no_error() {
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            }),
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("moment_of_inertia", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "expected no diagnostics, got: {:?}",
            diags
        );
    }

    /// (c) GRADUALISM: arg1 = Type::Error → 0 diagnostics (poison sentinel skipped).
    #[test]
    fn gradualism_error_type_passes_silently() {
        let args = vec![arg_expr(Type::Geometry), arg_expr(Type::Error)];
        let mut diags = Vec::new();
        check_builtin_arg_types("moment_of_inertia", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "Type::Error should be silently skipped, got: {:?}",
            diags
        );
    }

    /// (c) GRADUALISM: arg1 = Type::TypeParam("T") → 0 diagnostics (unresolved variable).
    #[test]
    fn gradualism_type_param_passes_silently() {
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(Type::TypeParam("T".to_string())),
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("moment_of_inertia", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "Type::TypeParam should be silently skipped, got: {:?}",
            diags
        );
    }

    /// (d) KIND mismatch: faces_by_normal arg2 = Type::Bool (where ANGLE expected)
    /// → 1 Error diagnostic naming "Angle".
    #[test]
    fn faces_by_normal_bool_arg2_gives_error_naming_angle() {
        let dir_type = Type::Direction;
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(dir_type),
            arg_expr(Type::Bool), // wrong kind — Bool, not a dimensioned scalar
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("faces_by_normal", &args, dummy_span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("Angle"),
            "message missing 'Angle': {}",
            diags[0].message
        );
    }

    /// (e) WRONG-DIM scalar: faces_by_normal arg2 = Scalar{LENGTH} (not ANGLE) → 1 Error.
    #[test]
    fn faces_by_normal_length_tol_gives_error() {
        let dir_type = Type::Direction;
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(dir_type),
            arg_expr(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("faces_by_normal", &args, dummy_span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
    }

    /// (f) CORRECT: faces_by_normal arg2 = Scalar{ANGLE} → 0 diagnostics.
    #[test]
    fn faces_by_normal_correct_angle_gives_no_error() {
        let dir_type = Type::Direction;
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(dir_type),
            arg_expr(Type::Scalar {
                dimension: DimensionVector::ANGLE,
            }),
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("faces_by_normal", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "correct Angle arg should give no diagnostics, got: {:?}",
            diags
        );
    }

    /// Task 3523 (amendment). An ANGLE `tol` passed to an extremal ctor (where a
    /// LENGTH distance tolerance is expected) is a DEFINITE compile-time mismatch
    /// → 1 ArgTypeMismatch naming "Length" and the "tol" arg; a correct LENGTH tol
    /// passes silently. Pins the new LENGTH slot end-to-end through
    /// check_builtin_arg_types for both extremal names. (Without the slot this
    /// wrong-dimension tol would be caught only later at eval by
    /// resolve_length_scalar_arg's warning.)
    #[test]
    fn extremal_angle_tol_gives_error_naming_length() {
        // arg0 geometry, arg1 axis string, arg2 sense string, arg3 tol.
        let with_tol = |tol: Type| {
            vec![
                arg_expr(Type::Geometry),
                arg_expr(Type::String), // axis — unchecked
                arg_expr(Type::String), // sense — unchecked
                arg_expr(tol),
            ]
        };
        for name in ["extremal_by_bbox", "extremal_by_centroid"] {
            // (a) wrong dimension: ANGLE tol → 1 ArgTypeMismatch naming "Length".
            let mut diags = Vec::new();
            check_builtin_arg_types(
                name,
                &with_tol(Type::Scalar {
                    dimension: DimensionVector::ANGLE,
                }),
                dummy_span(),
                &mut diags,
            );
            assert_eq!(
                diags.len(),
                1,
                "{name}: expected 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
            assert!(
                diags[0].message.contains("Length"),
                "{name}: message must name expected 'Length': {}",
                diags[0].message
            );
            assert!(
                diags[0].message.contains("tol"),
                "{name}: message must name the 'tol' arg: {}",
                diags[0].message
            );

            // (b) correct dimension: LENGTH tol → 0 diagnostics.
            let mut diags = Vec::new();
            check_builtin_arg_types(
                name,
                &with_tol(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
                dummy_span(),
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "{name}: a correct LENGTH tol must give no diagnostics, got: {:?}",
                diags
            );
        }
    }

    /// (g) SHORT args: edges_at_height with only 1 arg (h correct, tol absent)
    /// → no panic, checks only the present slot.
    #[test]
    fn edges_at_height_short_args_no_panic() {
        let args = vec![
            arg_expr(Type::Geometry),
            arg_expr(Type::length()), // arg1 h — correct LENGTH
                                      // arg2 tol absent
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("edges_at_height", &args, dummy_span(), &mut diags);
        // h is correct → no diagnostic; tol absent → skipped (no panic)
        assert!(
            diags.is_empty(),
            "correct h + absent tol → no diagnostics, got: {:?}",
            diags
        );
    }

    /// (h) UNCHECKED slot: arg0 (any type, e.g. Scalar{DIMENSIONLESS}) never fires.
    #[test]
    fn arg0_never_fires() {
        // Only arg0 present — the density slot is at index 1 which is absent
        let args = vec![arg_expr(Type::dimensionless_scalar())];
        let mut diags = Vec::new();
        check_builtin_arg_types("moment_of_inertia", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "arg0 should never be checked, got: {:?}",
            diags
        );
    }

    /// (i) Unrecognized name (e.g., "volume") → 0 diagnostics.
    #[test]
    fn unrecognized_name_gives_no_diagnostics() {
        let args = vec![arg_expr(Type::Bool)];
        let mut diags = Vec::new();
        check_builtin_arg_types("volume", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "unrecognized name should give no diagnostics, got: {:?}",
            diags
        );
    }

    // ── generate Int-count slot (task 3994, structural-query ζ) ───────────────

    fn int_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Int { type_name: "Int" },
        }
    }

    /// generate → arg0 n (Int).
    #[test]
    fn generate_has_int_count_slot() {
        let slots = builtin_arg_slots("generate", 2);
        assert_eq!(
            slots.len(),
            1,
            "generate should have 1 slot, got: {:?}",
            slots
        );
        assert_eq!(slots[0], int_slot(0, "n"));
    }

    /// CORRECT: generate arg0 = Type::Int → 0 diagnostics.
    #[test]
    fn generate_int_count_gives_no_error() {
        let args = vec![arg_expr(Type::Int)];
        let mut diags = Vec::new();
        check_builtin_arg_types("generate", &args, dummy_span(), &mut diags);
        assert!(diags.is_empty(), "Int count must pass, got: {:?}", diags);
    }

    /// MISMATCH: generate arg0 = Scalar{LENGTH} (`3mm`) → 1 ArgTypeMismatch naming Int.
    #[test]
    fn generate_length_count_gives_error_naming_int() {
        let args = vec![arg_expr(Type::length())];
        let mut diags = Vec::new();
        check_builtin_arg_types("generate", &args, dummy_span(), &mut diags);
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("generate:"),
            "message missing builtin name: {}",
            diags[0].message
        );
        assert!(
            diags[0].message.contains("expects Int"),
            "message should pin the expected Int type: {}",
            diags[0].message
        );
    }

    /// MISMATCH: generate arg0 = dimensionless Real (`2.5`) → 1 ArgTypeMismatch.
    /// A dimensionless scalar is NOT an `Int` — a count must be a true integer.
    #[test]
    fn generate_real_count_gives_error() {
        let args = vec![arg_expr(Type::dimensionless_scalar())];
        let mut diags = Vec::new();
        check_builtin_arg_types("generate", &args, dummy_span(), &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "dimensionless Real count must be rejected, got: {:?}",
            diags
        );
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
    }

    /// GRADUALISM: generate arg0 = Type::Error / Type::TypeParam → 0 diagnostics.
    #[test]
    fn generate_count_gradualism_passes_silently() {
        for ty in [Type::Error, Type::TypeParam("T".to_string())] {
            let args = vec![arg_expr(ty)];
            let mut diags = Vec::new();
            check_builtin_arg_types("generate", &args, dummy_span(), &mut diags);
            assert!(
                diags.is_empty(),
                "poison/unresolved count must pass silently, got: {:?}",
                diags
            );
        }
    }

    /// SHORT args: generate with no args → no panic, no diagnostic (slot absent).
    #[test]
    fn generate_short_args_no_panic() {
        let args: Vec<CompiledExpr> = vec![];
        let mut diags = Vec::new();
        check_builtin_arg_types("generate", &args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "absent count arg → no diagnostic, got: {:?}",
            diags
        );
    }

    // ── frame_at Direction slots (direction-typing σ) ────────────────────────

    fn direction_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Direction,
        }
    }

    fn frame_at_args(x: Type, z: Type) -> Vec<CompiledExpr> {
        let origin = Type::Point {
            n: 3,
            quantity: Box::new(Type::length()),
        };
        vec![arg_expr(origin), arg_expr(x), arg_expr(z)]
    }

    fn frame_at_diagnostics(x: Type, z: Type) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        check_builtin_arg_types("frame_at", &frame_at_args(x, z), dummy_span(), &mut diags);
        diags
    }

    #[test]
    fn frame_at_slots_x_and_z_as_direction_but_never_the_origin() {
        assert_slots_at_every_arity(
            "frame_at",
            &[direction_slot(1, "x"), direction_slot(2, "z")],
        );
    }

    #[test]
    fn direction_slot_accepts_direction_and_defers_error_and_type_param() {
        for (x, z) in [
            (Type::Direction, Type::Direction),
            (Type::Error, Type::TypeParam("T".to_string())),
        ] {
            let diags = frame_at_diagnostics(x.clone(), z.clone());
            assert!(
                diags.is_empty(),
                "({x}, {z}) must pass silently, got: {diags:?}"
            );
        }
    }

    #[test]
    fn direction_slot_rejects_every_non_vector3_kind_without_hint() {
        for ty in [
            Type::String,
            Type::Int,
            Type::length(),
            Type::ScalarParam("Q".into()),
            Type::Axis,
            Type::Plane,
            Type::Vector {
                n: 2,
                quantity: Box::new(Type::Int),
            },
            Type::Point {
                n: 3,
                quantity: Box::new(Type::length()),
            },
        ] {
            let diags = frame_at_diagnostics(ty.clone(), Type::Direction);
            assert_eq!(
                diags.len(),
                1,
                "{ty} at x: expected 1 diagnostic, got: {diags:?}"
            );
            assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
            assert_eq!(diags[0].severity, Severity::Error);
            assert_eq!(
                diags[0].message,
                format!("frame_at: x argument expects Direction, got {ty}")
            );
        }
    }

    #[test]
    fn direction_slot_rejects_vector3_with_dir_hint() {
        let int_vector = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Int),
        };
        let length_vector = Type::Vector {
            n: 3,
            quantity: Box::new(Type::length()),
        };
        let diags = frame_at_diagnostics(int_vector, length_vector.clone());
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                format!(
                    "frame_at: x argument expects Direction, got Vector3<Int>; \
                     {DIRECTION_MIGRATION_HINT}"
                ),
                format!(
                    "frame_at: z argument expects Direction, got {length_vector}; \
                     {DIRECTION_MIGRATION_HINT}"
                ),
            ]
        );
    }

    // ── linear_pattern direction + spacing slots (task 5652, direction-typing ι-c) ──

    /// Build a 4-arg `linear_pattern(target, direction, count, spacing)` arg
    /// list with the given `direction` and `spacing` types. `count` is an Int
    /// the table leaves unchecked — see the `linear_pattern` arm of
    /// [`builtin_arg_slots`].
    fn linear_pattern_args_with(direction: Type, spacing: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry), // 0 target
            arg_expr(direction),      // 1 direction
            arg_expr(Type::Int),      // 2 count
            arg_expr(spacing),        // 3 spacing
        ]
    }

    /// [`linear_pattern_args_with`] with a correct `Direction`, so a diagnostic
    /// is a statement about `spacing` alone.
    fn linear_pattern_args(spacing: Type) -> Vec<CompiledExpr> {
        linear_pattern_args_with(Type::Direction, spacing)
    }

    /// linear_pattern → direction@1 (Direction) and spacing@3 (LENGTH) at EVERY
    /// arity: it is a single-form name, so its arm carries no arity guard.
    #[test]
    fn linear_pattern_slots_its_direction_and_spacing_at_every_arity() {
        assert_slots_at_every_arity(
            "linear_pattern",
            &[direction_slot(1, "direction"), length_slot(3, "spacing")],
        );
    }

    /// CORRECT: a dimensioned `Length` spacing → 0 diagnostics.
    #[test]
    fn linear_pattern_length_spacing_gives_no_error() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern",
            &linear_pattern_args(Type::length()),
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "a dimensioned Length spacing must pass, got: {:?}",
            diags
        );
    }

    /// MISMATCH: a bare `Int` spacing (`10`) → exactly 1 ArgTypeMismatch Error
    /// naming the builtin, the `spacing` arg and the expected `Length`.
    #[test]
    fn linear_pattern_bare_int_spacing_gives_error_naming_length() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern",
            &linear_pattern_args(Type::Int),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        for needle in ["linear_pattern", "spacing", "Length"] {
            assert!(
                diags[0].message.contains(needle),
                "message must contain {needle:?}: {}",
                diags[0].message
            );
        }
    }

    /// MISMATCH: a DIMENSIONED but WRONG-dimension spacing (`10deg`) → exactly 1
    /// ArgTypeMismatch naming the expected `Length` AND the offending unit.
    ///
    /// Distinct from the bare-`Int` case above at the code level, not just the
    /// source level: a bare count lands in `check_builtin_arg_types`'s catch-all
    /// `other =>` arm, whereas a wrong-dimension scalar goes through the
    /// `Type::Scalar { dimension } != expected_dim` arm. Without this case that
    /// arm is unexercised for both pattern slots — and it is the likelier user
    /// slip once the check lands, since the user who knows units are required can
    /// still reach for the wrong one.
    ///
    /// Pins the `{actual}` rendering, like `moment_of_inertia`'s `"got Real"`
    /// case: an ANGLE scalar renders as `Scalar[rad]` (its SI base-unit symbol)
    /// via `Type::Display`, NOT as the friendly slot type name `"Angle"` — only
    /// the *expected* side of the message uses those.
    #[test]
    fn linear_pattern_wrong_dimension_spacing_names_length_and_actual_unit() {
        for (name, args) in [
            (
                "linear_pattern",
                linear_pattern_args(Type::Scalar {
                    dimension: DimensionVector::ANGLE,
                }),
            ),
            (
                "linear_pattern_2d",
                linear_pattern_2d_args(
                    Type::Scalar {
                        dimension: DimensionVector::ANGLE,
                    },
                    Type::length(),
                ),
            ),
        ] {
            let mut diags = Vec::new();
            check_builtin_arg_types(name, &args, dummy_span(), &mut diags);
            assert_eq!(
                diags.len(),
                1,
                "{name}: expected 1 diagnostic, got: {:?}",
                diags
            );
            assert_eq!(diags[0].severity, Severity::Error);
            assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
            for needle in ["expects Length", "got Scalar[rad]"] {
                assert!(
                    diags[0].message.contains(needle),
                    "{name}: message must contain {needle:?} so the user can see WHICH \
                     unit was wrong, not merely that one was: {}",
                    diags[0].message
                );
            }
        }
    }

    /// GRADUALISM: an unresolved `TypeParam` / poison `Error` spacing → 0
    /// diagnostics. The executable proof of the two-layer relationship stated in
    /// this module's "Relationship to the eval-layer units gate" section.
    #[test]
    fn linear_pattern_unresolved_spacing_passes_silently() {
        for ty in [Type::TypeParam("T".to_string()), Type::Error] {
            let mut diags = Vec::new();
            check_builtin_arg_types(
                "linear_pattern",
                &linear_pattern_args(ty.clone()),
                dummy_span(),
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "{ty} spacing must pass silently (gradualism) — the eval-layer \
                 gate stays the backstop; got: {:?}",
                diags
            );
        }
    }

    /// CLEAN: a `Direction` at index 1 draws no diagnostic.
    #[test]
    fn linear_pattern_accepts_a_direction_at_index_1() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern",
            &linear_pattern_args_with(Type::Direction, Type::length()),
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "a Direction at index 1 must pass, got: {:?}",
            diags
        );
    }

    /// MISMATCH: a `Vector3` direction is one ArgTypeMismatch carrying the
    /// `dir(x, y, z)` hint.
    #[test]
    fn linear_pattern_vector_direction_gives_error_with_dir_hint() {
        let int_vector = Type::Vector {
            n: 3,
            quantity: Box::new(Type::Int),
        };
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern",
            &linear_pattern_args_with(int_vector, Type::length()),
            dummy_span(),
            &mut diags,
        );
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [format!(
                "linear_pattern: direction argument expects Direction, got \
                 Vector3<Int>; {DIRECTION_MIGRATION_HINT}"
            )]
        );
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert_eq!(diags[0].severity, Severity::Error);
    }

    // ── linear_pattern_2d spacing1/spacing2 LENGTH slots (task 5652) ─────────

    /// Build an 11-arg
    /// `linear_pattern_2d(target, dx1, dy1, dz1, count1, spacing1,
    ///                            dx2, dy2, dz2, count2, spacing2)`
    /// arg list with the two given spacing types.
    fn linear_pattern_2d_args(spacing1: Type, spacing2: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry),               // 0  target
            arg_expr(Type::dimensionless_scalar()), // 1  dx1
            arg_expr(Type::dimensionless_scalar()), // 2  dy1
            arg_expr(Type::dimensionless_scalar()), // 3  dz1
            arg_expr(Type::Int),                    // 4  count1
            arg_expr(spacing1),                     // 5  spacing1
            arg_expr(Type::dimensionless_scalar()), // 6  dx2
            arg_expr(Type::dimensionless_scalar()), // 7  dy2
            arg_expr(Type::dimensionless_scalar()), // 8  dz2
            arg_expr(Type::Int),                    // 9  count2
            arg_expr(spacing2),                     // 10 spacing2
        ]
    }

    /// linear_pattern_2d @ arity 11 → spacing1@5 + spacing2@10 (both LENGTH);
    /// every other arity → empty.
    ///
    /// Arity 7 gets its own assertion because it is the CONCRETE false positive
    /// the arity dimension prevents, not a hypothetical — the reason, and how it
    /// differs from `linear_pattern`'s 4-arg form, is on the `linear_pattern_2d`
    /// arm of [`builtin_arg_slots`] and restated in the assertion message below,
    /// where a failure is actually read.
    #[test]
    fn linear_pattern_2d_spacing_slots_are_arity_11_only() {
        assert_eq!(
            builtin_arg_slots("linear_pattern_2d", 11),
            vec![length_slot(5, "spacing1"), length_slot(10, "spacing2")],
            "linear_pattern_2d args 5 and 10 are spacing1/spacing2 and must be LENGTH"
        );
        assert!(
            builtin_arg_slots("linear_pattern_2d", 7).is_empty(),
            "at arity 7 (task 5351's future direction-value form) index 5 is \
             `count2`, an Int — a spacing1@5 LENGTH slot would emit a FALSE \
             ArgTypeMismatch on valid code, so arity 7 must expose NO slots"
        );
        for arity in [0usize, 1, 5, 6, 10, 12] {
            assert!(
                builtin_arg_slots("linear_pattern_2d", arity).is_empty(),
                "linear_pattern_2d at arity {arity} is not the 11-arg form, \
                 so it must expose no slots"
            );
        }
    }

    /// CORRECT: both spacings dimensioned → 0 diagnostics.
    #[test]
    fn linear_pattern_2d_length_spacings_give_no_error() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern_2d",
            &linear_pattern_2d_args(Type::length(), Type::length()),
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "two dimensioned Length spacings must pass, got: {:?}",
            diags
        );
    }

    /// Each spacing slot is independently wired: a bare `spacing1` names
    /// `spacing1`, a bare `spacing2` names `spacing2`, and both bare give 2.
    ///
    /// Naming the RIGHT one matters — a copy-paste slip duplicating the
    /// `spacing1` name onto index 10 would point the user at the wrong axis.
    #[test]
    fn linear_pattern_2d_bare_spacings_are_reported_independently() {
        // (a) only spacing1 bare → 1 diagnostic naming spacing1 (not spacing2).
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern_2d",
            &linear_pattern_2d_args(Type::Int, Type::length()),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("spacing1"),
            "must name spacing1: {}",
            diags[0].message
        );
        assert!(
            !diags[0].message.contains("spacing2"),
            "must NOT name spacing2: {}",
            diags[0].message
        );

        // (b) only spacing2 bare → 1 diagnostic naming spacing2.
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern_2d",
            &linear_pattern_2d_args(Type::length(), Type::Int),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(diags.len(), 1, "expected 1 diagnostic, got: {:?}", diags);
        assert!(
            diags[0].message.contains("spacing2"),
            "must name spacing2: {}",
            diags[0].message
        );
        assert!(
            !diags[0].message.contains("spacing1"),
            "must NOT name spacing1: {}",
            diags[0].message
        );

        // (c) both bare → 2 diagnostics.
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "linear_pattern_2d",
            &linear_pattern_2d_args(Type::Int, Type::Int),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(
            diags.len(),
            2,
            "both spacings bare → one diagnostic each, got: {:?}",
            diags
        );
    }

    // ── mirror / circular_pattern ORIGIN LENGTH slots (task 5662) ────────────

    /// Build a 3-arg `mirror(target, origin, normal)` arg list.
    fn mirror_args(origin: Type, normal: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry), // 0 target
            arg_expr(origin),         // 1 origin
            arg_expr(normal),         // 2 normal
        ]
    }

    /// Build a 9-arg
    /// `circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)` arg list
    /// with the given origin-component type.
    fn circular_pattern_args(origin: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry),               // 0 target
            arg_expr(origin.clone()),               // 1 ox
            arg_expr(origin.clone()),               // 2 oy
            arg_expr(origin),                       // 3 oz
            arg_expr(Type::dimensionless_scalar()), // 4 ax
            arg_expr(Type::dimensionless_scalar()), // 5 ay
            arg_expr(Type::dimensionless_scalar()), // 6 az
            arg_expr(Type::Int),                    // 7 count
            arg_expr(Type::Scalar {
                dimension: DimensionVector::ANGLE,
            }), // 8 angle
        ]
    }

    /// mirror @ arity 3 → normal@2 (Direction); every other arity → empty
    /// (direction-typing ι-a, task 9646).
    ///
    /// Arity 2 gets its own assertion because it is the other accepted form,
    /// `mirror(target, plane)`: the coupling rule needs its slot set to differ
    /// from arity 3's. The retired scalar arity 7 exposes no slots, so such a
    /// call draws only its arity error.
    #[test]
    fn mirror_slots_are_arity_3_only() {
        assert_eq!(
            builtin_arg_slots("mirror", 3),
            vec![direction_slot(2, "normal")],
            "mirror(target, origin, normal) — arg2 is the Direction normal"
        );
        assert!(
            builtin_arg_slots("mirror", 2).is_empty(),
            "arity 2 (`mirror(target, plane)`, the task-5745 decoded-value form) \
             must expose NO slots, so its slot set differs from arity 3's"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 3) {
            assert!(
                builtin_arg_slots("mirror", arity).is_empty(),
                "mirror at arity {arity} is not the 3-arg Direction form, so it must \
                 expose NO slots"
            );
        }
    }

    /// circular_pattern @ arity 9 → ox@1 / oy@2 / oz@3 (LENGTH) + angle@8
    /// (ANGLE); @ arity 4 → angle@3 (ANGLE); every other arity → empty.
    ///
    /// Neither form may share an arm with the other, because the same index
    /// denotes a different parameter in each: index 3 is the `oz` LENGTH at
    /// arity 9 but the `angle` at arity 4, and index 1 is `ox` at arity 9 but
    /// the decoded `Axis` VALUE at arity 4.
    #[test]
    fn circular_pattern_forms_are_keyed_independently() {
        assert_eq!(
            builtin_arg_slots("circular_pattern", 9),
            vec![
                length_slot(1, "ox"),
                length_slot(2, "oy"),
                length_slot(3, "oz"),
                angle_slot(8, "angle"),
            ],
            "circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle) — \
             args 1-3 are the rotation-axis ORIGIN (LENGTH) and arg8 is the ANGLE"
        );
        assert_eq!(
            builtin_arg_slots("circular_pattern", 4),
            vec![angle_slot(3, "angle")],
            "circular_pattern(target, axis, count, angle) — index 1 is the decoded \
             Axis VALUE (never an origin slot), index 2 the Int count, and only \
             index 3 is the ANGLE"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 9 && *n != 4) {
            assert!(
                builtin_arg_slots("circular_pattern", arity).is_empty(),
                "circular_pattern at arity {arity} is neither accepted form, so it \
                 must expose NO slots"
            );
        }
    }

    /// `mirror`'s 3-arg form slots its Direction normal and NEVER its Point3
    /// origin (direction-typing ι-a, task 9646).
    ///
    /// Stated POSITIVELY as a decision rather than left to be inferred from an
    /// absence: no `ExpectedArg` kind describes a point, so the origin is gated
    /// at build by `accept_length_point3` — the accepted narrowing of the
    /// direction-typing PRD's §8 decompose amendments.
    #[test]
    fn mirror_slots_the_normal_but_never_the_point3_origin() {
        let slotted: Vec<usize> = builtin_arg_slots("mirror", 3)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert_eq!(
            slotted,
            vec![2],
            "mirror's slotted index set at arity 3 must be exactly the normal {{2}}; \
             the Point3 origin at index 1 stays slot-free; got {slotted:?}"
        );
    }

    /// The accepted narrowing, executed: whatever the 3-arg `mirror`'s origin
    /// is typed, the compile layer reports nothing for it.
    #[test]
    fn mirror_point3_origin_draws_no_slot_diagnostic_whatever_its_type() {
        let point3 = |quantity: Type| Type::Point {
            n: 3,
            quantity: Box::new(quantity),
        };
        for origin in [point3(Type::length()), point3(Type::Int), Type::Int] {
            let mut diags = Vec::new();
            check_builtin_arg_types(
                "mirror",
                &mirror_args(origin.clone(), Type::Direction),
                dummy_span(),
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "mirror: a {origin} origin has no compile-time slot, got: {diags:?}"
            );
        }
    }

    /// The STRADDLE case, fifth of five, and the widest: `circular_pattern`'s
    /// 9-arg form carries a gated ORIGIN, an un-gated axis DIRECTION, an Int
    /// `count` and a gated `angle`.
    ///
    /// The slot set, position by position:
    ///
    /// * `ox`/`oy`/`oz` are a point in space — bare components silently read as
    ///   SI metres, so they are slotted LENGTH;
    /// * `angle` is slotted ANGLE since PRD 3 leaf ζ (task 5782);
    /// * `ax`/`ay`/`az` are a unit vector, legitimately bare;
    /// * `count` is an Int — a wrong count is an arity/semantic error, not a
    ///   dimension error.
    #[test]
    fn circular_pattern_slots_the_origin_and_the_angle_but_never_the_axis_or_count() {
        let slotted: Vec<usize> = builtin_arg_slots("circular_pattern", 9)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert_eq!(
            slotted,
            vec![1, 2, 3, 8],
            "circular_pattern's slotted index set at arity 9 must be exactly the \
             ORIGIN triple plus the angle {{1,2,3,8}}; got {slotted:?}"
        );
        for excluded in [4usize, 5, 6] {
            assert!(
                !slotted.contains(&excluded),
                "circular_pattern arg{excluded} is an axis DIRECTION component — a \
                 dimensionless unit vector, legitimately bare; got slots at \
                 {slotted:?}"
            );
        }
        assert!(
            !slotted.contains(&7),
            "circular_pattern arg7 is the Int `count` — a wrong count is an \
             arity/semantic error, never a dimension slot; got slots at {slotted:?}"
        );
    }

    /// CORRECT: a dimensioned `Length` origin → 0 diagnostics.
    #[test]
    fn pattern_origin_length_components_give_no_error() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "circular_pattern",
            &circular_pattern_args(Type::length()),
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "circular_pattern: a dimensioned Length origin must pass, got: {diags:?}"
        );
    }

    /// MISMATCH: a bare `Int` origin (`0`) → exactly 3 ArgTypeMismatch Errors,
    /// ONE PER COMPONENT, each naming the builtin, its own component, the
    /// expected `Length` and the migration hint.
    ///
    /// Three, not one: each component is an independent slot, so a copy-paste
    /// slip that duplicated a single name across all three would still yield
    /// three diagnostics — hence the per-component name assertion below.
    #[test]
    fn pattern_bare_int_origin_gives_three_errors_naming_each_component() {
        let name = "circular_pattern";
        let mut diags = Vec::new();
        check_builtin_arg_types(
            name,
            &circular_pattern_args(Type::Int),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(
            diags.len(),
            3,
            "{name}: a bare origin triple must produce one diagnostic per \
             component, got: {diags:?}"
        );
        for (diag, component) in diags.iter().zip(["ox", "oy", "oz"]) {
            assert_eq!(diag.severity, Severity::Error, "{name}/{component}");
            assert_eq!(
                diag.code,
                Some(DiagnosticCode::ArgTypeMismatch),
                "{name}/{component}"
            );
            for needle in [
                name,
                component,
                "expects Length",
                "pass a dimensioned length such as `5mm`",
            ] {
                assert!(
                    diag.message.contains(needle),
                    "{name}/{component}: message must contain {needle:?}: {}",
                    diag.message
                );
            }
        }
    }

    /// MISMATCH: a DIMENSIONED but WRONG-dimension origin (`0deg`) names the
    /// expected `Length` AND the offending unit.
    ///
    /// Distinct from the bare-`Int` case at the CODE level, not just the source
    /// level: a bare Int lands in `check_builtin_arg_types`' catch-all `other =>`
    /// arm, whereas a wrong-dimension scalar goes through the
    /// `Type::Scalar { dimension } != expected_dim` arm. Without this case that
    /// arm is unexercised for the origin slots.
    #[test]
    fn pattern_wrong_dimension_origin_names_length_and_actual_unit() {
        let angle = Type::Scalar {
            dimension: DimensionVector::ANGLE,
        };
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "circular_pattern",
            &circular_pattern_args(angle),
            dummy_span(),
            &mut diags,
        );
        assert_eq!(
            diags.len(),
            3,
            "circular_pattern: expected 3 diagnostics, got: {diags:?}"
        );
        for diag in &diags {
            assert_eq!(diag.severity, Severity::Error);
            assert_eq!(diag.code, Some(DiagnosticCode::ArgTypeMismatch));
            for needle in ["expects Length", "got Scalar[rad]"] {
                assert!(
                    diag.message.contains(needle),
                    "circular_pattern: message must contain {needle:?} so the user \
                     can see WHICH unit was wrong, not merely that one was: {}",
                    diag.message
                );
            }
        }
    }

    /// GRADUALISM: an unresolved `TypeParam` / poison `Error` origin → 0
    /// diagnostics. The executable proof of the two-layer relationship stated in
    /// this module's "Relationship to the eval-layer units gate" section.
    #[test]
    fn pattern_unresolved_origin_passes_silently() {
        for ty in [Type::TypeParam("T".to_string()), Type::Error] {
            let mut diags = Vec::new();
            check_builtin_arg_types(
                "circular_pattern",
                &circular_pattern_args(ty.clone()),
                dummy_span(),
                &mut diags,
            );
            assert!(
                diags.is_empty(),
                "circular_pattern: a {ty} origin must pass silently (gradualism) — \
                 the eval-layer gate stays the backstop; got: {diags:?}"
            );
        }
    }

    /// Neither decoded-VALUE form draws a diagnostic, even with a definitely-
    /// wrong type at index 1.
    ///
    /// For `circular_pattern` the ARITY GUARD, not the `compiled_args.get(index)`
    /// bounds check, is what keeps it quiet: its value form DOES have an index 1,
    /// holding an `Axis`, which the 9-arg form slots as `ox`. `mirror`'s only
    /// slot is `normal@2`, which the bounds check alone keeps off a 2-arg call.
    #[test]
    fn pattern_value_form_arities_give_no_diagnostics() {
        // mirror(target, plane) — the task-5745 decoded-value form.
        let mirror_value_form = vec![
            arg_expr(Type::Geometry), // 0 target
            arg_expr(Type::Int),      // 1 plane — definitely wrong, NOT checked here
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("mirror", &mirror_value_form, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "a 2-arg mirror exposes no slots even though index 1 EXISTS, got: {:?}",
            diags
        );

        // circular_pattern(target, axis, count, angle).
        let circular_value_form = vec![
            arg_expr(Type::Geometry), // 0 target
            arg_expr(Type::Int),      // 1 axis — definitely wrong, NOT checked here
            arg_expr(Type::Int),      // 2 count
            arg_expr(Type::Scalar {
                dimension: DimensionVector::ANGLE,
            }), // 3 angle
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "circular_pattern",
            &circular_value_form,
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "a 4-arg circular_pattern exposes no slot at index 1 even though \
             index 1 EXISTS, got: {:?}",
            diags
        );
    }

    // ── Task 6862: `Type::ScalarParam` at a dimension slot ────────────────────

    /// A dimension-kinded generic fn PARAMETER must DEFER at a LENGTH slot,
    /// never reject.
    ///
    /// `fn beam<Q: Dimension>(l: Scalar<Q>) -> Solid { extrude(circle(l), l) }`
    /// resolves `l` to `Type::ScalarParam("Q")` (`type_resolution.rs`'s
    /// `classify_dim_slot`), which displays as `Scalar<Q>`. Its FAMILY is known —
    /// it is a scalar — and only its DIMENSION is open, so at a dimension slot
    /// there is nothing to compare: judging the UNINSTANTIATED body under strict
    /// `DimensionVector` equality can only ever REJECT, so every such site is a
    /// false positive.
    ///
    /// The defer's COST — that `beam(10kg)` is then undiagnosed, because nothing
    /// rechecks a generic body per instantiation — is stated on the
    /// `ExpectedArg::Scalar` arm and pinned by the three MEASUREMENT tests named
    /// there.
    ///
    /// Three names, so the fix cannot be accidentally index-specific: `circle`
    /// (slot 0), `extrude` (slot 1) and `box` (slots 0/1/2).
    ///
    /// End-to-end sibling: `dim_kinded_generic_param_at_length_slot_compiles_clean`
    /// in `tests/builtin_arg_signature_tests.rs`, over the fixture
    /// `tests/fixtures/dim_kinded_length_slot.ri`.
    #[test]
    fn scalar_param_defers_at_length_slot() {
        let q = || arg_expr(Type::ScalarParam("Q".to_string()));

        // circle(radius) — the slot is index 0.
        let mut diags = Vec::new();
        check_builtin_arg_types("circle", &[q()], dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "ScalarParam at circle's LENGTH radius slot must defer, got: {:?}",
            diags
        );

        // extrude(profile, distance) — the slot is index 1, so a defer that only
        // covered index 0 would survive the `circle` case and die here.
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "extrude",
            &[arg_expr(Type::Geometry), q()],
            dummy_span(),
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "ScalarParam at extrude's LENGTH distance slot must defer, got: {:?}",
            diags
        );

        // box(w, d, h) — slots 0, 1 AND 2 at once.
        let mut diags = Vec::new();
        check_builtin_arg_types("box", &[q(), q(), q()], dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "ScalarParam at box's three LENGTH slots must defer, got: {:?}",
            diags
        );
    }

    /// NEGATIVE CONTROL (green before AND after task 6862) — a bare `Int` at a
    /// LENGTH slot is still a definite mismatch.
    ///
    /// The defer added for `ScalarParam` is a DIMENSION argument; it must not
    /// leak into the family check that catches `circle(5)`.
    #[test]
    fn bare_int_at_length_slot_still_rejected() {
        let mut diags = Vec::new();
        check_builtin_arg_types("circle", &[arg_expr(Type::Int)], dummy_span(), &mut diags);
        assert_eq!(
            diags.len(),
            1,
            "a bare Int radius must still be rejected exactly once, got: {:?}",
            diags
        );
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::ArgTypeMismatch));
        for needle in ["circle", "radius", "Length", "got Int"] {
            assert!(
                d.message.contains(needle),
                "message missing {:?}: {}",
                needle,
                d.message
            );
        }
    }

    /// NEGATIVE CONTROL (green before AND after task 6862) — a CONCRETE
    /// wrong-dimension scalar at a LENGTH slot is still rejected.
    ///
    /// This is the assertion that proves the `ScalarParam` defer does not blanket
    /// -disable the slot: a `Scalar{MASS}` has a known dimension, so the
    /// comparison is decidable and rejecting is correct.
    #[test]
    fn wrong_dimension_scalar_at_length_slot_still_rejected() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "circle",
            &[arg_expr(Type::Scalar {
                dimension: DimensionVector::MASS,
            })],
            dummy_span(),
            &mut diags,
        );
        assert_eq!(
            diags.len(),
            1,
            "a concrete MASS radius must still be rejected exactly once, got: {:?}",
            diags
        );
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("Length"),
            "message must still name the expected Length: {}",
            diags[0].message
        );
    }

    /// NARROWNESS PIN (green before AND after task 6862) — `ScalarParam` must
    /// STILL be rejected at an `ExpectedArg::Int` slot.
    ///
    /// This is what makes the asymmetry in `check_builtin_arg_types` ENFORCEABLE
    /// rather than merely conventional. The defer is a DIMENSION-comparison
    /// argument and an `Int` count slot is not a dimension slot: `Scalar<Q>` is a
    /// dimensioned scalar for EVERY binding of `Q` (at Q = DIMENSIONLESS it is
    /// `Real`, which the `Int` arm's own comment already names as a definite
    /// mismatch), so the family mismatch is decidable without instantiating.
    #[test]
    fn scalar_param_still_rejected_at_int_slot() {
        let mut diags = Vec::new();
        check_builtin_arg_types(
            "generate",
            &[arg_expr(Type::ScalarParam("Q".to_string()))],
            dummy_span(),
            &mut diags,
        );
        assert_eq!(
            diags.len(),
            1,
            "ScalarParam at generate's Int count slot must still be rejected, got: {:?}",
            diags
        );
        assert_eq!(diags[0].code, Some(DiagnosticCode::ArgTypeMismatch));
        assert!(
            diags[0].message.contains("expects Int"),
            "message should pin the expected Int type: {}",
            diags[0].message
        );
    }

    // ── Task 6862 FINDING 2: the lowering-arity ledger ───────────────────────
    //
    // FINDING 2's hazard — an arity-agnostic arm meeting a future value-form
    // overload — is stated once, with its worked example, on the HAZARD block
    // above the primitive arms in this file. What follows is its enforcement.
    //
    // The existing `assert_slots_at_every_arity` sweeps catch a stray GUARD
    // being ADDED here; they cannot catch a new OVERLOAD being added over in
    // the lowering. This ledger closes that direction: it derives each
    // lowering's ACCEPTED ARITY SET behaviourally (by compiling real calls and
    // reading back the arg-count diagnostics), pins it, and couples it to the
    // slot table.

    /// Compile `source` through the crate's OWN stdlib entry points.
    ///
    /// This must NOT go through `reify_test_support::compile_source_with_stdlib`:
    /// the crate's `[dev-dependencies]` self-pull (`reify-compiler { features =
    /// ["test-support"] }`) puts two `reify_compiler` instances in the unit-test
    /// graph, and that helper returns the *external* instance's `CompiledModule`,
    /// which would not unify with `crate::CompiledModule` here (E0308).
    ///
    /// `relation_signatures.rs`'s `compile_module` is the same three lines for
    /// the same reason — see its doc comment rather than a third telling of the
    /// argument here. `guards.rs`'s `guarded_param_default_tests::compile`
    /// shares only the RATIONALE, not the shape (MEASURED: it parses through
    /// `reify_syntax::parse` with NO stdlib prelude, asserts the parse is clean,
    /// and calls the module-local `compile`), so it is not a third copy of this
    /// helper. Hoisting the two that ARE identical into one crate-internal
    /// `#[cfg(test)]` module is the right move and is filed as follow-up work:
    /// it needs edits in `relation_signatures.rs` and a `mod` line in `lib.rs`,
    /// both outside task 6862's scope.
    fn compile_module(source: &str) -> crate::CompiledModule {
        let parsed = crate::parse_with_stdlib(source, reify_core::ModulePath::single("test"));
        crate::compile_with_stdlib(&parsed)
    }

    /// The name universe the slot-key sweeps probe: the closed-world family
    /// table flattened, plus [`NON_SELECTOR_ARG_SLOT_KEYS`], before the "does it
    /// yield slots" filter. Derived, never a hand copy:
    /// [`slot_probe_names_reach_every_closed_world_family_name`] pins that.
    fn slot_probe_names() -> BTreeSet<&'static str> {
        crate::unresolved_function::BUILTIN_NAME_FAMILIES
            .iter()
            .flat_map(|&(_, slice)| slice.iter())
            .chain(NON_SELECTOR_ARG_SLOT_KEYS)
            .copied()
            .collect()
    }

    /// Every name [`builtin_arg_slots`] actually serves, derived MECHANICALLY
    /// rather than listed by hand.
    ///
    /// Filters [`slot_probe_names`] — the closed-world family table plus
    /// [`NON_SELECTOR_ARG_SLOT_KEYS`] — to the names yielding a non-empty slot
    /// list at some arity in `0..=MAX_PROBED_ARITY`. Same technique
    /// [`arg_slot_keys_are_registered_builtin_names`] uses, and what makes the
    /// ledger guard self-maintaining: a name added to the table in a future leaf
    /// is picked up here automatically instead of quietly escaping the guard.
    ///
    /// This function is the sole input to
    /// [`every_slotted_name_is_ledgered_or_recorded_unobservable`], so a slot key
    /// it cannot NAME escapes the ledger and the coupling rule and leaves that
    /// guard GREEN.
    ///
    /// The [`NON_SELECTOR_ARG_SLOT_KEYS`] chain in [`slot_probe_names`] keeps a
    /// slot key swept if its family row is later discharged into the registry.
    /// The selectors have no chain of their own: they are swept only through the
    /// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` row, which
    /// [`slotted_name_sweep_reaches_every_curated_slot_registry`] asserts.
    fn slotted_builtin_names() -> BTreeSet<&'static str> {
        slot_probe_names()
            .into_iter()
            .filter(|name| {
                (0usize..=MAX_PROBED_ARITY).any(|k| !builtin_arg_slots(name, k).is_empty())
            })
            .collect()
    }

    /// The completeness sweep reaches a slot key through ANY curated registry,
    /// not just through the closed-world family table.
    ///
    /// Pins the two claims [`slotted_builtin_names`]'s doc makes, so neither can
    /// rot into a comment that is merely believed:
    ///
    /// 1. Every [`NON_SELECTOR_ARG_SLOT_KEYS`] entry comes back from the sweep.
    ///    (The list already rejects dead entries elsewhere, so each name here
    ///    does yield slots.)
    ///
    /// 2. `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` is a closed-world family row, which
    ///    is WHY it needs no chain of its own. If a registry migration
    ///    discharges that row, this fails loudly instead of silently shrinking
    ///    the sweep — the selectors are 9 of the ledger's
    ///    [`ARITY_UNOBSERVABLE_SLOT_KEYS`] entries, so losing them would hollow
    ///    out that arm rather than break it.
    #[test]
    fn slotted_name_sweep_reaches_every_curated_slot_registry() {
        let swept = slotted_builtin_names();

        for &name in NON_SELECTOR_ARG_SLOT_KEYS {
            assert!(
                swept.contains(name),
                "{name:?} is a curated slot-table key reachable through no registry \
                 that slot_probe_names() probes, so it would escape \
                 every_slotted_name_is_ledgered_or_recorded_unobservable entirely. \
                 Make slot_probe_names() reach it."
            );
        }

        assert!(
            crate::unresolved_function::BUILTIN_NAME_FAMILIES
                .iter()
                .any(|&(_, slice)| slice == GEOMETRY_TOPOLOGY_SELECTOR_NAMES),
            "GEOMETRY_TOPOLOGY_SELECTOR_NAMES is swept only through its row in \
             unresolved_function::BUILTIN_NAME_FAMILIES; that row is gone, so a \
             registry migration that discharged it must chain the selectors into \
             slot_probe_names() explicitly."
        );
    }

    /// Fork-guard on [`slot_probe_names`]: the universe reaches every name of
    /// every closed-world family row, so a family added to
    /// `unresolved_function::BUILTIN_NAME_FAMILIES` is swept for slot keys with
    /// no list edit in this module, and a hand-kept or filtered copy that drifts
    /// from the table reds here.
    ///
    /// Misses are collected per family label before asserting, so one failure
    /// names every family the universe drifted from.
    #[test]
    fn slot_probe_names_reach_every_closed_world_family_name() {
        let probed = slot_probe_names();
        let mut misses: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for &(family, slice) in crate::unresolved_function::BUILTIN_NAME_FAMILIES {
            for &name in slice {
                if !probed.contains(name) {
                    misses.entry(family).or_default().push(name);
                }
            }
        }

        let report: String = misses
            .iter()
            .map(|(family, names)| format!("\n  {family}: {}", names.join(", ")))
            .collect();
        assert!(
            misses.is_empty(),
            "slot_probe_names() misses closed-world family names, so the slot-key \
             sweeps cannot reach them. The sweeps must derive their universe from \
             unresolved_function::BUILTIN_NAME_FAMILIES, not from a hand-kept copy \
             of it. Missed, by family:{report}"
        );
    }

    /// Probe each slotted name's lowering for the arities it ACCEPTS, by
    /// compiling one synthetic module holding every (name, arity) call and
    /// reading back the arg-count diagnostics.
    ///
    /// # Anchored on the MESSAGE, not on the label
    ///
    /// Three different emit sites produce arg-count errors —
    /// [`crate::arg_check::check_arg_count_exact`],
    /// [`crate::arg_check::check_arg_count_at_least`], and bare custom pushes
    /// such as `geometry.rs`'s `extrude` arm, which carries NO `"wrong number of
    /// arguments"` label at all. The label is therefore NOT universal. The
    /// message shape `"{name}() expects … got {N}"` IS: it held for every
    /// observable name across all three emit sites when this ledger was
    /// measured. The trailing `", got {N}"` is matched with `ends_with` and not
    /// `contains`, because `", got 1"` is a prefix of `", got 12"`.
    ///
    /// # Failure polarity is deliberately safe
    ///
    /// An arity message this matcher does NOT recognise makes the probe read
    /// that arity as ACCEPTED, which BREAKS the pin below — a false RED that
    /// forces a human look. It can never produce a false GREEN.
    ///
    /// # Headroom
    ///
    /// Measured: a 660-call probe module (44 slotted names × 15 arities)
    /// produced 459 arity diagnostics with no truncation and no diagnostic cap,
    /// so ONE compile suffices for the whole sweep.
    ///
    /// # Memoised
    ///
    /// Both consumers — the pin/coupling guard and the completeness arm — need
    /// the identical probe, and the probe is a full `parse_with_stdlib` +
    /// `compile_with_stdlib` over a module whose size grows with both the slot
    /// table and [`MAX_PROBED_ARITY`]. It is pure and deterministic, so it runs
    /// ONCE per test binary behind a `OnceLock` and both callers share the
    /// result.
    fn lowering_accepted_arities() -> &'static BTreeMap<String, BTreeSet<usize>> {
        static PROBE: OnceLock<BTreeMap<String, BTreeSet<usize>>> = OnceLock::new();
        PROBE.get_or_init(probe_lowering_accepted_arities)
    }

    /// The uncached probe body behind [`lowering_accepted_arities`]. Pure and
    /// deterministic — it depends only on the slot table and the lowering, both
    /// compile-time constants of the test binary — which is what makes sharing
    /// one result across both consumers safe.
    fn probe_lowering_accepted_arities() -> BTreeMap<String, BTreeSet<usize>> {
        let names: Vec<&str> = slotted_builtin_names().into_iter().collect();

        let mut src = String::from("module test\n\nstructure def ArityProbe {\n");
        for (i, name) in names.iter().enumerate() {
            for k in 0usize..=MAX_PROBED_ARITY {
                let args = vec!["1mm"; k].join(", ");
                src.push_str(&format!("    let v{i}_{k} = {name}({args})\n"));
            }
        }
        src.push_str("}\n");

        let module = compile_module(&src);

        names
            .iter()
            .map(|name| {
                let prefix = format!("{name}() expects");
                let rejected: BTreeSet<usize> = (0usize..=MAX_PROBED_ARITY)
                    .filter(|k| {
                        let suffix = format!(", got {k}");
                        module.diagnostics.iter().any(|d| {
                            d.message.starts_with(&prefix) && d.message.ends_with(&suffix)
                        })
                    })
                    .collect();
                let accepted = (0usize..=MAX_PROBED_ARITY)
                    .filter(|k| !rejected.contains(k))
                    .collect();
                ((*name).to_string(), accepted)
            })
            .collect()
    }

    /// A ledger row's pinned accepted-arity set, held STRUCTURALLY so that a row
    /// backed by an OPEN lowering tail stays correct when [`MAX_PROBED_ARITY`]
    /// moves.
    ///
    /// [`MAX_PROBED_ARITY`] is shared by ~10 sweeps in this module, so raising it
    /// for an unrelated reason is a plausible future edit. Written as the
    /// enumerated window `[2, 3, …, 14]`, `shell`'s
    /// `check_arg_count_at_least(2)` tail would break the pin on that edit and
    /// report it as "accepted-arity set moved … value-form overload arriving" —
    /// pointing the reader at entirely the wrong cause. [`AtLeast`] expands
    /// against the CURRENT bound instead, so the row means what the lowering
    /// means.
    ///
    /// [`AtLeast`]: AcceptedArities::AtLeast
    #[derive(Debug, Clone, Copy)]
    enum AcceptedArities {
        /// A CLOSED set — one `check_arg_count_exact` form per member.
        Exactly(&'static [usize]),
        /// An OPEN tail — a `check_arg_count_at_least(n)` lowering, expanding to
        /// `n..=MAX_PROBED_ARITY`.
        AtLeast(usize),
    }

    impl AcceptedArities {
        /// The set to compare the probe against, resolved against the CURRENT
        /// [`MAX_PROBED_ARITY`].
        fn expand(self) -> BTreeSet<usize> {
            match self {
                AcceptedArities::Exactly(arities) => arities.iter().copied().collect(),
                AcceptedArities::AtLeast(min) => (min..=MAX_PROBED_ARITY).collect(),
            }
        }
    }

    /// The PINNED accepted-arity ledger: every slotted name whose lowering
    /// emits an observable arg-count diagnostic, and the arities each accepts.
    ///
    /// Derived by MEASUREMENT (see [`lowering_accepted_arities`]), not by
    /// reading the lowering arms, and held as an [`AcceptedArities`] so a row
    /// backed by an OPEN lowering tail does not silently become wrong when
    /// [`MAX_PROBED_ARITY`] moves. If an entry disagrees with the probe,
    /// INVESTIGATE — a name that GAINED an accepted arity is exactly FINDING 2's
    /// hazard arriving (task 5351's value forms are the named motivating case) —
    /// rather than blindly re-pinning the new value.
    ///
    /// The slotted names deliberately absent from this list are recorded, with
    /// the measurement that justifies their absence, in
    /// [`ARITY_UNOBSERVABLE_SLOT_KEYS`]; the completeness arm of
    /// [`lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`] asserts
    /// every slotted name sits in exactly one of the two lists.
    const LOWERING_ACCEPTED_ARITIES: &[(&str, AcceptedArities)] = &[
        // Primitive CSG producers.
        ("box", AcceptedArities::Exactly(&[3])),
        ("box_centered", AcceptedArities::Exactly(&[3])),
        ("cone", AcceptedArities::Exactly(&[3])),
        ("cylinder", AcceptedArities::Exactly(&[2])),
        ("cylinder_centered", AcceptedArities::Exactly(&[2])),
        ("half_space", AcceptedArities::Exactly(&[2])),
        ("sphere", AcceptedArities::Exactly(&[1])),
        ("torus", AcceptedArities::Exactly(&[2])),
        ("tube", AcceptedArities::Exactly(&[3])),
        ("wedge", AcceptedArities::Exactly(&[4])),
        // 2-D profile producers.
        ("circle", AcceptedArities::Exactly(&[1])),
        ("ellipse", AcceptedArities::Exactly(&[2])),
        ("rectangle", AcceptedArities::Exactly(&[2])),
        // Modify producers.
        ("chamfer", AcceptedArities::Exactly(&[2, 3])),
        ("chamfer_asymmetric", AcceptedArities::Exactly(&[4])),
        ("fillet", AcceptedArities::Exactly(&[2, 3])),
        ("fillet_all", AcceptedArities::Exactly(&[2])),
        ("offset_curve", AcceptedArities::Exactly(&[2, 3])),
        ("offset_solid", AcceptedArities::Exactly(&[2])),
        // `shell` is `check_arg_count_at_least(2)` — args 2.. are face indices,
        // so the accepted tail is OPEN. `AtLeast` says exactly that, and
        // expands against the CURRENT MAX_PROBED_ARITY, so raising that bound
        // no longer breaks this pin with a misattributed message.
        ("shell", AcceptedArities::AtLeast(2)),
        ("shell_open", AcceptedArities::Exactly(&[3])),
        ("thicken", AcceptedArities::Exactly(&[2])),
        ("zone_slab", AcceptedArities::Exactly(&[2])),
        // Sweep producers.
        ("extrude", AcceptedArities::Exactly(&[2])),
        ("extrude_symmetric", AcceptedArities::Exactly(&[2])),
        ("pipe", AcceptedArities::Exactly(&[2])),
        ("revolve", AcceptedArities::Exactly(&[3, 4])),
        ("revolve_full", AcceptedArities::Exactly(&[3])),
        ("extrude_infinite", AcceptedArities::Exactly(&[2])),
        ("extrude_infinite_symmetric", AcceptedArities::Exactly(&[2])),
        // Transform producers and patterns.
        ("linear_pattern", AcceptedArities::Exactly(&[4])),
        ("linear_pattern_2d", AcceptedArities::Exactly(&[11])),
        ("rotate_around", AcceptedArities::Exactly(&[4])),
        ("translate", AcceptedArities::Exactly(&[4])),
        // PRD 3 leaf ζ (task 5782). `rotate` is multi-arity because it is
        // genuinely OVERLOADED — task 4166's 2-arg Orientation form alongside
        // the 3-arg Direction one (direction-typing ι-a; any other arity reads
        // "rotate() expects 2 or 3 arguments: rotate(geo, orientation) or
        // rotate(geo, dir(x, y, z), angle), got {n}") — and is safe under the
        // coupling rule the way the pattern rows below are: its arm carries a
        // load-bearing `if arg_count ==` guard, so the two arities expose
        // DIFFERENT slot sets and no MULTI_ARITY_AGNOSTIC_SAFE exemption is
        // needed or permitted.
        ("rotate", AcceptedArities::Exactly(&[2, 3])),
        ("arc", AcceptedArities::Exactly(&[5])),
        // `draft` moves its angle between forms; its guarded pair discharges
        // the coupling rule exactly as `rotate`'s guard does.
        ("draft", AcceptedArities::Exactly(&[3, 4])),
        // Pattern ORIGIN triples (task 5662) — multi-arity because the name
        // is genuinely OVERLOADED: a component form (circular_pattern's scalar
        // one, mirror's point-direction one since direction-typing ι-a) and the
        // value form. Both are safe under the coupling rule because
        // their arms carry a load-bearing `if arg_count ==` guard, so no
        // MULTI_ARITY_AGNOSTIC_SAFE exemption is needed; pinned by
        // `pattern_origin_family_is_ledgered_and_guarded_not_exempted`.
        //
        // PROVENANCE, worth one line: these two rows exist because task 5662
        // landed two new slotted names on `main` while task 6862 was on a
        // branch, and `every_slotted_name_is_ledgered_or_recorded_unobservable`
        // FIRED on them unprompted at the merge. The completeness arm caught an
        // unrelated task's change — which is the evidence that this guard is
        // enforceable rather than decorative.
        ("circular_pattern", AcceptedArities::Exactly(&[4, 9])),
        ("mirror", AcceptedArities::Exactly(&[2, 3])),
    ];

    /// Slotted names whose lowering emits NO observable arg-count diagnostic, so
    /// [`lowering_accepted_arities`] cannot derive an accepted-arity set for them
    /// and a [`LOWERING_ACCEPTED_ARITIES`] row would be VACUOUS rather than
    /// merely absent.
    ///
    /// # The measurement, not an assumption
    ///
    /// Compiling every one of these names at every arity in `0..=MAX_PROBED_ARITY`
    /// produced NO arg-count diagnostic AT ALL — the probe therefore reads every
    /// arity as "accepted" for them, which is not a fact about the lowering but
    /// the absence of one. The geometry TOPOLOGY SELECTOR family is simply not
    /// routed through a lowering arity check the way the geometry PRODUCERS are.
    /// That measurement is re-run on every test run, not trusted: the
    /// no-dead-entries arm of
    /// [`every_slotted_name_is_ledgered_or_recorded_unobservable`] asserts each
    /// name here still yields a FULL accepted set, so if a selector ever gains an
    /// arity check this list turns RED and forces the name into the ledger, where
    /// the pin and the coupling rule can see it.
    ///
    /// # The consequence, stated honestly
    ///
    /// The FINDING-2 guard therefore covers the geometry-LOWERING families —
    /// task 5750's subject, and the 26 arity-agnostic arms that motivated the
    /// finding — and NOT task 4493/3994's selector family, nor task #6082's two
    /// Euler builtins, nor direction-typing σ's `frame_at`. Nobody should read
    /// the guard as broader than that. The nine selectors plus `generate` are
    /// also the names least exposed to the hazard: none is overloaded today,
    /// and a value-form overload is a producer-side notion (task 5351).
    ///
    /// The two Euler builtins and `frame_at` are unobservable for a DIFFERENT
    /// reason worth stating, because it is not the selectors': they have no
    /// lowering arm at all. All three are pure eval-builtins dispatched through
    /// `reify_stdlib::eval_builtin`, which answers a wrong-arity call with
    /// `Value::Undef` at EVAL time rather than a compile diagnostic — so there
    /// is no arity check for the probe to observe. The Euler pair is the one
    /// pair here that does NOT rely on this exemption for its safety: both arms
    /// carry an explicit `if arg_count ==` guard (4 and 2), which is the
    /// stronger fix the agnostic-arm rule asks for, so their slot indices cannot
    /// fire on the wrong argument even if a future overload appears. `frame_at`
    /// is the unguarded eval-only entry: single-form, so its arm carries no
    /// guard, and it DOES rely on this exemption — an overload must add the
    /// guard in the same change.
    const ARITY_UNOBSERVABLE_SLOT_KEYS: &[&str] = &[
        // The nine geometry topology selectors (task 4493).
        "center_of_mass",
        "moment_of_inertia",
        "faces_by_normal",
        "edges_parallel_to",
        "faces_perpendicular_to",
        "edges_perpendicular_to",
        "edges_at_height",
        "extremal_by_bbox",
        "extremal_by_centroid",
        // The task-3994 list combinator, the table's lone `Int` count slot.
        "generate",
        // The two Euler builtins (task #6082), the table's only
        // `ExpectedArg::Enum` slots. MEASURED, not assumed: the probe reports
        // {0..=14} for both — every arity accepted, i.e. no arity diagnostic at
        // any of them — the same full-set signature as the nine selectors above
        // and unlike a ledgered name such as `fillet` ({2, 3}).
        "orient_euler",
        "orient_to_euler",
        // direction-typing σ's `frame_at`: a pure eval builtin with no lowering
        // arm, so the probe sees the same full-window signature as the Euler
        // pair. Unlike that pair its arm is UNGUARDED (single-form); a future
        // overload must land an `if arg_count ==` guard in the same change,
        // since this ledger cannot observe one. Nothing pins the single form:
        // reify-stdlib geometry.rs's `frame_at_wrong_arity_undef` checks only
        // that a 1-arg call is Undef, and stays green if a 2- or 4-arg
        // overload lands.
        "frame_at",
        // direction-typing ζ-a's joint and PRB flexure ctors: pure eval
        // builtins with no lowering arity diagnostic, measured by the probe
        // (the full-window signature, like `frame_at`). Their arms are
        // UNGUARDED because the axis keeps one index at every accepted arity;
        // an overload that moves the axis must add an `if arg_count ==` guard
        // in the same change, since this ledger cannot observe one.
        "prismatic",
        "revolute",
        "cylindrical",
        "planar",
        "prb_cantilever_beam",
        "prb_fixed_fixed_beam",
        "prb_notch_circular",
        "prb_notch_elliptical",
        "prb_notch_right_circular",
        "prb_living_hinge",
        "prb_cross_spring_pivot",
        "prb_prismatic_blade",
        "prb_two_axis_pivot",
        "prb_parallelogram_flexure",
        "prb_double_parallelogram_flexure",
        "prb_let_joint",
        "prb_cartwheel_flexure",
        // direction-typing θ-b's arity-2 `angle(a, b)`. MEASURED: the probe
        // reports the full {0..=14} window — no lowering arity diagnostic at
        // any arity, like the Euler pair. Its arm carries an explicit
        // `if arg_count == 2` guard (arity 3 is the slot-free relation verb),
        // so it does not rely on this exemption for its safety.
        "angle",
        // direction-typing θ-a's three SO(3) constructors: pure eval builtins
        // with no lowering arm. MEASURED: the probe reports the full {0..=14}
        // window for each, like the Euler pair. Each arm carries an explicit
        // `if arg_count ==` guard (2, 2, 3), so none relies on this exemption
        // for its safety.
        "orient_axis_angle",
        "orient_look_at",
        "orient_basis",
    ];

    /// Names that legitimately accept MORE THAN ONE arity while being served by
    /// an arity-AGNOSTIC arm in [`builtin_arg_slots`].
    ///
    /// Adding a name here is a DELIBERATE CLAIM, not a way to quiet the guard:
    /// that the slot indices denote THE SAME PARAMETERS at every accepted arity,
    /// verified against the lowering. When they do NOT — the usual case — the
    /// correct fix is an `if arg_count ==` guard on the arm, as `fillet`,
    /// `chamfer`, `linear_pattern` and `linear_pattern_2d` already carry.
    ///
    /// The companion no-dead-entries assertion in
    /// [`lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`] requires
    /// every name here to still be multi-arity AND still be agnostic, so a stale
    /// entry cannot silently hollow out the rule.
    ///
    /// Per entry, why it is safe — each VERIFIED against the lowering, not
    /// assumed:
    ///
    /// - `offset_curve` — accepts 2 and 3. The 2-arg form goes through
    ///   `geometry_modify.rs`'s `compile_modify_2arg(…, "distance", …)`, which
    ///   builds `[("target", …), ("distance", …)]`; the 3-arg form builds
    ///   `[("target", …), ("distance", …), ("third", …)]` explicitly. Index 1 is
    ///   `distance` in BOTH, so the arm's `length_arg(1, "distance")` denotes the
    ///   same parameter either way. Note plainly what this is: FINDING 2's exact
    ///   hazard shape, ALREADY PRESENT in the table and benign only by
    ///   coincidence of the lowering's layout. It is listed rather than silently
    ///   tolerated precisely so that a future third case has to be argued.
    ///
    /// - `shell` — `check_arg_count_at_least("shell", …, 2, …)`, so it accepts an
    ///   OPEN tail. The lowering builds `[("target", …), ("thickness", …)]` and
    ///   then appends the remaining args as `face_{i}` indices, so index 1 is
    ///   `thickness` at every arity and the agnostic arm denotes the same
    ///   parameter throughout.
    const MULTI_ARITY_AGNOSTIC_SAFE: &[&str] = &["offset_curve", "shell"];

    /// FINDING 2's guard: the lowering-arity ledger is pinned, and a name that
    /// accepts MORE THAN ONE arity may not be served by an arity-AGNOSTIC arm
    /// unless it is an explicitly-argued [`MULTI_ARITY_AGNOSTIC_SAFE`] entry.
    ///
    /// Four assertions, and the second is the one that makes FINDING 2
    /// enforceable rather than conventional:
    ///
    /// 0. MATCHER SANITY — every name pinned to a BOUNDED set must have had at
    ///    least one rejection RECOGNISED by the probe. Asserted first, and with
    ///    its own wording, so that a reworded arity diagnostic is reported as a
    ///    probe failure rather than as every ledger row simultaneously
    ///    "gaining an overload" — two very different causes that the pin's
    ///    message alone cannot tell apart.
    ///
    /// 1. PIN — each ledger entry's measured accepted-arity set still equals the
    ///    pinned set. A slotted name that gains an accepted arity (i.e. gains a
    ///    value-form overload) breaks here, in this file, next to the slot table
    ///    that must be updated with it.
    ///
    /// 2. COUPLING — for every multi-arity name, `builtin_arg_slots(name, k)`
    ///    must NOT be identical across its accepted arities, i.e. the arm must
    ///    carry an `if arg_count ==` guard (as `fillet` / `chamfer` /
    ///    `linear_pattern` / `linear_pattern_2d` already do), UNLESS the name is
    ///    an argued [`MULTI_ARITY_AGNOSTIC_SAFE`] entry. An arity-agnostic arm
    ///    serving an overloaded name is EXACTLY how a slot comes to fire on the
    ///    wrong argument.
    ///
    /// 3. NO DEAD EXEMPTIONS — every [`MULTI_ARITY_AGNOSTIC_SAFE`] entry must
    ///    still be multi-arity AND still be agnostic. Without this, a name that
    ///    later gained a guard (or lost an overload) would sit in the exemption
    ///    list forever, hollowing rule 2 out for it. Mirrors assertion (a) of
    ///    [`arg_slot_keys_are_registered_builtin_names`].
    #[test]
    fn lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table() {
        let probed = lowering_accepted_arities();
        let mut agnostic_multi_arity: Vec<(&str, Vec<usize>)> = Vec::new();

        // (0) MATCHER SANITY, asserted BEFORE the pin so a reworded diagnostic
        // is reported as ITSELF instead of as 30-odd simultaneous "accepted-arity
        // set moved" failures, which name the wrong cause.
        //
        // Every name below is pinned to a BOUNDED set, so the probe must have
        // recognised at least one REJECTION for it. A name whose measured set is
        // the FULL probe window means the probe saw no rejection at all — which
        // is a fact about the MATCHER, not about the lowering gaining arities.
        let full_window: BTreeSet<usize> = (0usize..=MAX_PROBED_ARITY).collect();
        let matcher_blind: Vec<&str> = LOWERING_ACCEPTED_ARITIES
            .iter()
            .filter(|(name, pinned)| {
                pinned.expand() != full_window && probed.get(*name) == Some(&full_window)
            })
            .map(|(name, _)| *name)
            .collect();
        assert!(
            matcher_blind.is_empty(),
            "the probe saw NO recognised arg-count rejection for these ledgered \
             names, though each is pinned to a BOUNDED arity set: \
             {matcher_blind:?}.\n\
             Read this as a PROBE failure, NOT as task 6862 FINDING 2. Either \
             the arity diagnostic MESSAGE SHAPE changed out from under \
             `lowering_accepted_arities`'s matcher (`\"{{name}}() expects …, \
             got {{N}}\"` — there are THREE emit sites, so a reword at one of \
             them blinds only part of this list), or those lowerings dropped \
             their arity check entirely. Fix that first; the pin failures below \
             would otherwise all report the same thing under the wrong name."
        );

        for (name, pinned_arities) in LOWERING_ACCEPTED_ARITIES {
            let pinned: BTreeSet<usize> = pinned_arities.expand();
            let measured = probed.get(*name).unwrap_or_else(|| {
                panic!(
                    "ledger names {name:?}, but it is not a slotted builtin — the probe never \
                     saw it. Remove the stale row, or fix the name."
                )
            });

            // (1) PIN.
            assert_eq!(
                measured, &pinned,
                "{name}'s lowering accepted-arity set moved: pinned {pinned:?}, measured \
                 {measured:?}. If the lowering gained a VALUE-FORM overload, that is task \
                 6862 FINDING 2 arriving — check `builtin_arg_slots({name:?}, …)` denotes \
                 the same parameters at the NEW arity before re-pinning."
            );

            // (2) COUPLING. Collected rather than asserted in-loop, so ONE run
            // names every offender instead of only the alphabetically-first.
            if pinned.len() > 1 && !MULTI_ARITY_AGNOSTIC_SAFE.contains(name) {
                let arities: Vec<usize> = pinned.iter().copied().collect();
                let first = builtin_arg_slots(name, arities[0]);
                let arity_agnostic = arities.iter().all(|&k| builtin_arg_slots(name, k) == first);
                if arity_agnostic {
                    agnostic_multi_arity.push((*name, arities));
                }
            }
        }

        assert!(
            agnostic_multi_arity.is_empty(),
            "these names accept more than one arity, but `builtin_arg_slots` returns the \
             SAME slots at every one of them: {agnostic_multi_arity:?}. An arity-agnostic \
             arm serving an overloaded name makes the slot indices fire on the wrong \
             arguments (task 6862 FINDING 2). Add an `if arg_count ==` guard — or, if the \
             indices genuinely denote the same parameters at every accepted arity, record \
             the name in MULTI_ARITY_AGNOSTIC_SAFE with the layout that proves it."
        );

        // (3) NO DEAD EXEMPTIONS.
        for &name in MULTI_ARITY_AGNOSTIC_SAFE {
            let measured = probed.get(name).unwrap_or_else(|| {
                panic!(
                    "MULTI_ARITY_AGNOSTIC_SAFE names {name:?}, but it is not a slotted \
                     builtin — the probe never saw it. Remove the stale entry."
                )
            });
            assert!(
                measured.len() > 1,
                "MULTI_ARITY_AGNOSTIC_SAFE names {name:?}, but its lowering now accepts \
                 only {measured:?}. The exemption no longer applies — delete the entry so \
                 the coupling rule covers this name again."
            );
            let arities: Vec<usize> = measured.iter().copied().collect();
            let first = builtin_arg_slots(name, arities[0]);
            assert!(
                arities.iter().all(|&k| builtin_arg_slots(name, k) == first),
                "MULTI_ARITY_AGNOSTIC_SAFE names {name:?}, but its arm is no longer \
                 arity-agnostic — it now carries an `if arg_count ==` guard, which is the \
                 stronger fix. Delete the entry; the guard already satisfies the rule."
            );
        }
    }

    /// COMPLETENESS: every name the slot table serves is accounted for by the
    /// FINDING-2 guard — either it carries a measured [`LOWERING_ACCEPTED_ARITIES`]
    /// row, or it is a recorded [`ARITY_UNOBSERVABLE_SLOT_KEYS`] entry.
    ///
    /// Without this arm the ledger has a silent hole: a name newly added to
    /// [`builtin_arg_slots`] in a future leaf would simply not appear in the
    /// ledger, and
    /// [`lowering_arity_ledger_is_pinned_and_coupled_to_the_slot_table`] — which
    /// iterates the LEDGER, not the table — would never look at it. The new name
    /// would escape the coupling rule entirely, which is exactly the class of
    /// silent escape task 6862 FINDING 2 exists to close.
    ///
    /// The served-name set is derived MECHANICALLY by [`slotted_builtin_names`],
    /// so this is self-maintaining: nobody has to remember to update a list.
    ///
    /// The no-dead-entries companion holds the other direction — an
    /// [`ARITY_UNOBSERVABLE_SLOT_KEYS`] entry must still yield slots AND still be
    /// genuinely unobservable, so a name that later gains an arity check cannot
    /// sit in the wrong list forever.
    #[test]
    fn every_slotted_name_is_ledgered_or_recorded_unobservable() {
        let probed = lowering_accepted_arities();

        let unaccounted: Vec<&str> = slotted_builtin_names()
            .into_iter()
            .filter(|name| {
                !LOWERING_ACCEPTED_ARITIES.iter().any(|(n, _)| n == name)
                    && !ARITY_UNOBSERVABLE_SLOT_KEYS.contains(name)
            })
            .collect();
        assert!(
            unaccounted.is_empty(),
            "these names yield arg slots but are accounted for by neither list: \
             {unaccounted:?}. A new slotted name must be added to \
             LOWERING_ACCEPTED_ARITIES with its measured accepted arities — or, if its \
             lowering emits no arity diagnostic at all, recorded in \
             ARITY_UNOBSERVABLE_SLOT_KEYS with the measurement that shows it."
        );

        // No dead entries, direction 1: still in the table.
        for &name in ARITY_UNOBSERVABLE_SLOT_KEYS {
            assert!(
                (0usize..=MAX_PROBED_ARITY).any(|k| !builtin_arg_slots(name, k).is_empty()),
                "ARITY_UNOBSERVABLE_SLOT_KEYS names {name:?}, but it yields no slots at any \
                 arity — the key left the table. Remove the stale entry."
            );
        }

        // No dead entries, direction 2: still genuinely UNOBSERVABLE. A name
        // whose lowering has gained an arity check now has a real accepted-arity
        // set, so it belongs in the ledger where the pin and the coupling rule
        // can see it — not in the exemption list.
        for &name in ARITY_UNOBSERVABLE_SLOT_KEYS {
            let measured = probed.get(name).expect("slotted name must have been probed");
            let fully_accepted: BTreeSet<usize> = (0usize..=MAX_PROBED_ARITY).collect();
            assert_eq!(
                measured, &fully_accepted,
                "ARITY_UNOBSERVABLE_SLOT_KEYS names {name:?}, but the probe now sees it \
                 REJECT some arities ({measured:?} accepted of {fully_accepted:?}) — its \
                 lowering has gained an observable arity check. Move it to \
                 LOWERING_ACCEPTED_ARITIES with the measured set."
            );
        }
    }

    /// The task-5662 pattern-origin pair is LEDGERED, and its multi-arity row is
    /// discharged by a real `if arg_count ==` guard rather than by an exemption.
    ///
    /// # Provenance
    ///
    /// `mirror` and `circular_pattern` are not names task 6862 went looking for.
    /// They landed on `main` in task 5662 while this branch was away, and
    /// [`every_slotted_name_is_ledgered_or_recorded_unobservable`] FIRED on them
    /// unprompted the moment the two were merged, naming exactly
    /// `["circular_pattern", "mirror"]`. That is the completeness arm doing the
    /// one job it exists for, on an UNRELATED task's change — the strongest
    /// available evidence the FINDING-2 guard is enforceable rather than
    /// decorative.
    ///
    /// # What this pins that main's arity tests do not
    ///
    /// [`mirror_slots_are_arity_3_only`] and
    /// [`circular_pattern_forms_are_keyed_independently`] already pin the slot
    /// SHAPE against a hard-coded arity, and this test deliberately does not
    /// restate them. It pins the LEDGER-side fact they cannot see: each name
    /// carries a MEASURED [`LOWERING_ACCEPTED_ARITIES`] row, that row is
    /// MULTI-ARITY, and the coupling rule is therefore discharged the STRONG way
    /// — by a guard that discriminates the accepted arities — and not by a
    /// [`MULTI_ARITY_AGNOSTIC_SAFE`] exemption. Every arity it reasons about is
    /// read back from the probe; none is written by hand.
    ///
    /// Since direction-typing ι-a (task 9646) `mirror`'s second form is
    /// `mirror(target, origin, normal)`, whose Point3 origin no slot kind
    /// describes, so NO accepted `mirror` arity slots index 1. The guard still
    /// discharges the coupling rule, by giving arities 2 and 3 different slot
    /// sets.
    #[test]
    fn pattern_origin_family_is_ledgered_and_guarded_not_exempted() {
        let probed = lowering_accepted_arities();

        for (name, origin_slotted_forms) in [("mirror", 0usize), ("circular_pattern", 1)] {
            assert!(
                LOWERING_ACCEPTED_ARITIES.iter().any(|(n, _)| *n == name),
                "{name:?} yields origin slots (task 5662) but carries no \
                 LOWERING_ACCEPTED_ARITIES row, so nothing pins its accepted \
                 arities and the coupling rule never reaches it. Measure the set \
                 with `lowering_accepted_arities()` and land the row."
            );

            let measured = probed
                .get(name)
                .unwrap_or_else(|| panic!("{name:?} must be a slotted builtin"));

            // MULTI-ARITY is what puts these two under the coupling rule at all:
            // the lowering accepts BOTH a component form and the value form.
            assert!(
                measured.len() > 1,
                "{name} is expected to accept two forms (a value form and a \
                 component form), but the probe measured only {measured:?}. If the \
                 value form went away the coupling rule no longer bites here and \
                 this test should go with it."
            );

            // Discharged by a GUARD, never by an exemption.
            assert!(
                !MULTI_ARITY_AGNOSTIC_SAFE.contains(&name),
                "{name} must satisfy the coupling rule with its `if arg_count ==` \
                 guard, never with a MULTI_ARITY_AGNOSTIC_SAFE exemption: the \
                 value form hands its composite plane/axis an index the component \
                 form slots differently, so an arity-agnostic arm would fire on \
                 correct code."
            );
            // Keyed on INDEX 1 — the origin — not on "any slot": the invariant
            // is that the origin never reaches the value form's composite.
            // `circular_pattern`'s value form legitimately slots its angle@3.
            let origin_slotted: Vec<usize> = measured
                .iter()
                .copied()
                .filter(|&k| builtin_arg_slots(name, k).iter().any(|s| s.index == 1))
                .collect();
            assert_eq!(
                origin_slotted.len(),
                origin_slotted_forms,
                "{origin_slotted_forms} of {name}'s accepted arities {measured:?} may \
                 carry a slot at index 1 (the origin): circular_pattern's scalar \
                 form, and none of mirror's, whose origin is a Point3. A value form \
                 hands index 1 a composite built by a stdlib producer, so slotting \
                 it would fire on correct code. Index-1 slots found at arities: \
                 {origin_slotted:?}"
            );
        }
    }
}
