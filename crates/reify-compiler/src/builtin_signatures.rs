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
//! - Pattern ORIGIN triples (task 5662) — `mirror`'s 7-arg and
//!   `circular_pattern`'s 9-arg scalar forms, `ox`/`oy`/`oz`. The row task 5652
//!   deferred; it mirrors the `pattern` rows of `arg_acceptance.rs`' family
//!   table (mirror plane, task 5214; circular axis, task 5350).
//! - The Contract C LENGTH families of PRD
//!   `docs/prds/v0_6/units-length-gate-completion.md` leaf η (task 5750):
//!   PRIMITIVE and PROFILE producers, MODIFY, SWEEP, and the TRANSFORM row
//!   that contract C6 forced in alongside them. These mirror, at the compile
//!   layer, the family table in `crates/reify-eval/src/arg_acceptance.rs` —
//!   the canonical enumeration of every position routed through the eval-layer
//!   LENGTH chokepoint.
//!
//! The RULES that decide whether a position inside a covered family gets a
//! slot — each one is why some argument sitting right next to a slotted one is
//! deliberately bare:
//!
//! - **arg0 is never checked.** A geometry handle is ε=4358's territory.
//! - **ORIGIN vs DIRECTION.** A point in space (`half_space`'s `px`/`py`/`pz`,
//!   `revolve`'s `ox`/`oy`/`oz`, `rotate_around`'s pivot, `translate`'s
//!   displacement) IS a LENGTH — a bare component is silently read as SI
//!   metres, the 1000×-too-big hazard this whole gate exists to close. A
//!   unit-vector DIRECTION (`nx`/`ny`/`nz`, `ax`/`ay`/`az`, a pattern's or
//!   `extrude_infinite`'s `dx`/`dy`/`dz`) is dimensionless and legitimately
//!   bare in correct `.ri`, so a slot there would reject valid code. Stated
//!   binding at `crates/reify-eval/src/arg_acceptance.rs`'s "unit-vector
//!   DIRECTIONS" paragraph. The builtins whose arguments STRADDLE this line
//!   carry the split on their own arm — deliberately UNCOUNTED here, because a
//!   tally stated away from the arms is exactly the second copy nothing
//!   machine-checks that this section opens by warning against. It read
//!   "three" until task 5662 added the fourth and fifth.
//! - **COUNTS, face INDICES and dimensionless RATIOS are never slots.** A
//!   wrong `count`, `face_{i}` or `scale` factor is an arity or semantic
//!   error, not a dimension one, and a LENGTH slot on a ratio would reject
//!   correct `.ri` outright.
//! - **Every ANGLE belongs to
//!   `docs/prds/v0_6/angle-units-surface-convergence.md`** by binding seam
//!   decree — `revolve`'s and `rotate_around`'s `angle`, `draft`'s `angle`.
//!   The pre-existing ANGLE `tol` slots are that PRD's inheritance, not a
//!   precedent to extend; adding a new one here would be a scope violation.
//!   It is also why those slots carry no migration hint: the eval layer has
//!   none to mirror, and PRD 3 owns closing both halves together.
//! - **Polymorphic and coercing slots stay out.** Math args (no fixed
//!   dimension), the `dir` Vec3 slot (accepts list literals like `[0,0,1]`
//!   that coerce), and Range slots (`edges_by_length` / `faces_by_area`).
//! - **Indirect gating is not gating.** `rounded_box` / `rounded_rect` /
//!   `zone_cylinder` / `zone_annulus` / `zone_profile` DESUGAR into
//!   Box/Cylinder/Rectangle/Circle/Pipe/Thicken ops, so their own surface
//!   arguments never reach a gated position under these names. A slot here
//!   would be NEW coverage whose arg name disagrees with whatever the eval
//!   layer reports for the desugared op — breaking the same-wording premise
//!   the migration hint depends on (PRD decision D9). (`zone_annulus`' arg3
//!   `length` is additionally DEAD: `geometry.rs` binds it to `_l`.)
//! - **Names with no dimensioned-scalar argument have no arm at all**
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
//!   centre `cx`/`cy`/`cz` plus its `radius`, and `helix`'s
//!   `radius`/`pitch`/`height`. These ARE index-addressable and ARE Contract C
//!   gated at eval, so they are eligible in the mechanical sense; they are
//!   simply not among the families task 5750's leaf scoped, and no fixture
//!   forced the decision. A later leaf can add them by following the arms
//!   above verbatim.
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
//! - `extrude_infinite` — a SWEEP-family producer with NO arm here at all,
//!   which is why it is easy to read as an oversight. It is not: its signature
//!   is `(profile, dx, dy, dz, direction)`, i.e. a geometry handle, a
//!   unit-vector DIRECTION triple, and a `"positive"`/`"negative"`/`"both"`
//!   string. Not one of its arguments is a magnitude, so there is no LENGTH
//!   position to gate — the eval layer likewise reads `dx`/`dy`/`dz` with a
//!   plain `f64_arg` rather than through the Contract C chokepoint
//!   (`crates/reify-eval/src/geometry_ops.rs`). Recorded because it is absent,
//!   not because it is deferred.
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
//! # Relationship to the eval-layer units gate (task 5214)
//!
//! This check COMPLEMENTS `required_length_value` — it never replaces it.  It
//! upgrades the diagnostic to a compile-time `Error` with a call-site span
//! where a definite static type is available, but decision-6 gradualism means a
//! dynamically-typed spacing (typing as `Type::TypeParam` or `Type::Error`) is
//! skipped here and still relies on the eval-layer gate.  Both layers are load-
//! bearing; removing either leaves a hole.

use reify_core::units::{DENSITY_MIGRATION_HINT, LENGTH_MIGRATION_HINT};
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
        /// Optional migration hint appended to the rejection message.
        ///
        /// Mirrors `ArgSpec::migration_hint` in
        /// `crates/reify-eval/src/arg_acceptance.rs`, and carries the SAME
        /// `&'static str` — both sides read
        /// [`reify_core::units::LENGTH_MIGRATION_HINT`] /
        /// [`reify_core::units::DENSITY_MIGRATION_HINT`] rather than repeating
        /// the literal, so the compile-time and runtime diagnostics for one
        /// authoring mistake cannot drift apart (PRD
        /// `docs/prds/v0_6/units-length-gate-completion.md` decision D9).
        ///
        /// `None` where the eval layer likewise offers no hint — see the ANGLE
        /// slots below.
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
        migration_hint: Option<&'static str>,
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
            migration_hint: Some(LENGTH_MIGRATION_HINT),
        },
    }
}

/// Return the checkable dimensioned-scalar argument slots for a named builtin
/// call of a given arity.
///
/// Returns an empty slice for:
/// - Unrecognized names.
/// - Names with no checked dimensioned-scalar arg (e.g. `split`, `face`, `edge`,
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
                migration_hint: Some(DENSITY_MIGRATION_HINT),
            },
        }],

        // ── Directional topology selectors ───────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: dir Vec3 (unchecked — accepts list literals like [0,0,1])
        // arg2: tol → ANGLE ("Angle")
        // Task 3523 — faces_perpendicular_to/edges_perpendicular_to share the
        // directional (solid, dir, tol) shape, so arg2 tol is likewise ANGLE.
        "faces_by_normal"
        | "edges_parallel_to"
        | "faces_perpendicular_to"
        | "edges_perpendicular_to" => &[CheckableArg {
            index: 2,
            name: "tol",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::ANGLE,
                type_name: "Angle",
                // No migration hint, deliberately: the eval layer has no
                // `angle_spec` hint either, and this check exists to MIRROR
                // that layer's wording (PRD decision D9), not to get ahead of
                // it. Closing the ANGLE gap is PRD 3's by binding seam decree,
                // and it owns BOTH halves together — adding one here alone
                // would make the layers disagree in the other direction.
                // Pinned by `angle_slot_rejection_carries_no_migration_hint`.
                // Prose rather than a TODO on purpose: a TODO must cite a live
                // task under the PTODO grammar, and PRD 3 has no task id yet.
                migration_hint: None,
            },
        }],

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
                    migration_hint: Some(LENGTH_MIGRATION_HINT),
                },
            },
            CheckableArg {
                index: 2,
                name: "tol",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: Some(LENGTH_MIGRATION_HINT),
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
                migration_hint: Some(LENGTH_MIGRATION_HINT),
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
                migration_hint: Some(LENGTH_MIGRATION_HINT),
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
                    migration_hint: Some(LENGTH_MIGRATION_HINT),
                },
            },
            CheckableArg {
                index: 10,
                name: "spacing2",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                    migration_hint: Some(LENGTH_MIGRATION_HINT),
                },
            },
        ],

        // ── Pattern CSG producers: the ORIGIN triple is a Length (task 5662) ──
        //
        // The pattern-origin row task 5652 deferred (it turned six then-valid
        // call sites into hard compile errors, a separable breaking-surface
        // migration) and this task closes. These mirror, at the compile layer,
        // the `pattern` rows of `crates/reify-eval/src/arg_acceptance.rs`'s
        // family table: the mirror-plane origin (task 5214) and the
        // circular-pattern axis origin (task 5350). Arg names are COPIED from
        // `geometry.rs`'s lowering sites — the `("ox".to_string(), …)` triples
        // in its n==7 and n==9 arms — never invented, so the two layers word one
        // authoring mistake identically (PRD decision D9).
        //
        // mirror(target, ox, oy, oz, nx, ny, nz)          — 7-arg scalar form
        // mirror(target, plane)                           — 2-arg value form
        //   args1-3: the mirror-plane ORIGIN → LENGTH ("Length"). A point in
        //            space, so a bare `10` is silently read as 10 SI METRES —
        //            1000× a plausible 10 mm offset.
        //   args4-6: the plane NORMAL `nx`/`ny`/`nz` — a DIMENSIONLESS unit
        //            vector, deliberately UNSLOTTED. Legitimately bare in
        //            correct `.ri` (a normal's scale is irrelevant to the plane
        //            it defines), so a LENGTH slot would reject valid code.
        //            This is the FOURTH builtin whose args STRADDLE the
        //            ORIGIN-vs-DIRECTION boundary, after `half_space`,
        //            `revolve` and `rotate_around` — the split is stated binding
        //            at `arg_acceptance.rs`' "unit-vector DIRECTIONS" paragraph.
        //            Pinned by `mirror_slots_the_origin_but_never_the_normal`.
        //
        // The `arg_count == 7` guard is SEMANTICALLY LOAD-BEARING, not
        // forward-compat: index 1 is `ox` at arity 7 but `plane` at arity 2, so
        // an arity-agnostic slot would demand a Length of a Plane on correct
        // code — the same class of false positive the `linear_pattern_2d` and
        // `fillet` guards exist to prevent. Note this is NOT justified by "the
        // eval layer covers the value form": that hole was task 5745's and is
        // separately closed. The justification is that index 1 denotes a
        // DIFFERENT PARAMETER in each overload.
        "mirror" if arg_count == 7 => const { &[
            length_arg(1, "ox"),
            length_arg(2, "oy"),
            length_arg(3, "oz"),
        ] },

        // circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle)  — 9-arg
        // circular_pattern(target, axis, count, angle)                    — 4-arg
        //   args1-3: the rotation-axis ORIGIN → LENGTH ("Length").
        //   args4-6: the axis DIRECTION `ax`/`ay`/`az` — a dimensionless unit
        //            vector, UNSLOTTED for the same reason as `mirror`'s normal.
        //            The FIFTH and widest straddle case.
        //   arg7:    `count` — an Int. A wrong count is an arity/semantic error,
        //            not a dimension error.
        //   arg8:    `angle` — owned by
        //            `docs/prds/v0_6/angle-units-surface-convergence.md` by
        //            binding seam decree; gating it here would be a scope
        //            violation.
        //   Pinned by
        //   `circular_pattern_slots_the_origin_but_never_the_axis_count_or_angle`.
        //
        // Same load-bearing guard: index 1 is `ox` at arity 9 but `axis` at
        // arity 4.
        //
        // MEASURED PREFIX DIVERGENCE. This layer is keyed on the CALL, so it
        // reports the SURFACE name `circular_pattern:`, while the eval layer
        // renders its `{builtin}` from `PatternKind::Circular`'s `Display` — its
        // `kind_label` — which is `"circular"` (`types.rs:1748`). That is the
        // same class as the `box_centered`-vs-`box` divergence already recorded
        // under `check_builtin_arg_types`' "# Message format", and it holds this
        // way deliberately: reporting the name the author actually typed beats
        // reporting a lowering detail they never wrote. `mirror` does NOT
        // diverge — `PatternKind::Mirror` displays as `"mirror"`
        // (`types.rs:1749`) — so the two layers agree byte-for-byte there. Both
        // halves are pinned by
        // `circular_pattern_slot_names_the_surface_builtin_not_the_lowered_kind`
        // in `tests/builtin_arg_signature_tests.rs`.
        //
        // These two arms make `reify check` a REAL gate for these positions, not
        // merely `reify eval`: `cmd_check` returns `ExitCode::FAILURE` on any
        // compile `Severity::Error` and short-circuits BEFORE constraint checking
        // and before `build()`. Pinned at the CLI seam by
        // `check_rejects_bare_scalar_mirror_origin_before_reaching_build`.
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
        ] },

        // ── Primitive CSG producers: every dimension is a Length (task 5750) ──
        //
        // PRD `docs/prds/v0_6/units-length-gate-completion.md` leaf η, work
        // item 1. These mirror, at the compile layer, the task-5743 `primitive`
        // row of `crates/reify-eval/src/arg_acceptance.rs`'s family table: the
        // positions `geometry_ops`' `required_length_values` already gates at
        // eval. The hazard they close is the silent one — `Value::as_f64` reads
        // a bare `20` as 20 SI METRES, so `box(20, 20, 10)` built a 20-metre
        // part with no diagnostic at all before task 5743.
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
        // box(width, height, depth) / box_centered(width, height, depth)
        //   NOTE index 1 is `height` — the Y extent — NOT `depth`.
        "box" | "box_centered" => const { &[
            length_arg(0, "width"),
            length_arg(1, "height"),
            length_arg(2, "depth"),
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
        //   NOTE this orders its triple width/DEPTH/height — the opposite of
        //   `box`'s width/height/depth. Both orders are copied from their own
        //   lowering site rather than shared, which is why they can differ.
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

        // half_space(px, py, pz, nx, ny, nz)
        //   args0-2: the boundary-plane POINT → LENGTH ("Length").
        //   args3-5: the outward NORMAL `nx`/`ny`/`nz` — a DIMENSIONLESS unit
        //            vector, deliberately UNSLOTTED. Its components are
        //            legitimately bare in correct `.ri` (a normal's scale is
        //            irrelevant to the plane it defines), so a LENGTH slot here
        //            would reject valid code. This is the one builtin whose
        //            args STRADDLE the boundary, and it draws the same
        //            ORIGIN-vs-DIRECTION split the circular pattern already
        //            draws for `ox`/`oy`/`oz` vs `ax`/`ay`/`az` — stated
        //            binding at `crates/reify-eval/src/arg_acceptance.rs`'s
        //            "unit-vector DIRECTIONS" paragraph. Pinned by
        //            `half_space_slots_the_point_but_never_the_normal`.
        "half_space" => const { &[
            length_arg(0, "px"),
            length_arg(1, "py"),
            length_arg(2, "pz"),
        ] },

        // ── Transform producers (task 5750) ─────────────────────────────────
        //
        // The task-5623 `transform` row of
        // `crates/reify-eval/src/arg_acceptance.rs`'s family table. Arg names
        // from `geometry_transform.rs`'s `compile_transform_op`.
        //
        // Both names are single-form (`check_arg_count_exact`), so both arms
        // stay arity-agnostic per the rule stated on this function.
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

        // rotate_around(target, px, py, pz, ax, ay, az, angle)
        //   The third STRADDLE case, structurally identical to `revolve`'s:
        //   args1-3: the PIVOT `px`/`py`/`pz` → LENGTH ("Length") — a point in
        //            space.
        //   args4-6: the axis DIRECTION `ax`/`ay`/`az` — a dimensionless unit
        //            vector, legitimately bare in correct `.ri`. UNSLOTTED.
        //   arg7:    `angle` — owned by
        //            `docs/prds/v0_6/angle-units-surface-convergence.md` by
        //            binding seam decree; gating it here would be a scope
        //            violation.
        //   `scale` is deliberately absent from this block entirely: its
        //   `factor` (and the `factors` vec3 of its non-uniform form) is a
        //   dimensionless RATIO, so a LENGTH slot would reject correct code.
        //   Pinned by `scale_stays_slot_free_because_its_factor_is_dimensionless`.
        "rotate_around" => const { &[
            length_arg(1, "px"),
            length_arg(2, "py"),
            length_arg(3, "pz"),
        ] },

        // ── Modify producers (task 5750) ─────────────────────────────────────
        //
        // The compile-layer half of the task-5744 `modify` row of
        // `crates/reify-eval/src/arg_acceptance.rs`'s family table. Arg names
        // are copied from `geometry_modify.rs`'s `compile_modify_op` arms and
        // its shared `compile_modify_2arg` helper.
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
        // these two are the only modify arms that carry an `arg_count` guard.
        // Every OTHER name below holds its magnitude at a STABLE index across
        // every arity it accepts, so per the rule stated on this function they
        // stay unguarded.
        //
        // NOT slotted here, each for a stated reason:
        // - `draft`'s `angle` — `docs/prds/v0_6/angle-units-surface-convergence.md`
        //   owns every ANGLE by binding seam decree; gating one here would be a
        //   scope violation, not an improvement.
        // - `offset_curve`'s 3rd argument (`"third"`) — a reference Surface
        //   handle OR a direction vec3, disambiguated at eval on the Value
        //   variant. A direction's components are legitimately bare, and
        //   `arg_acceptance.rs` names this position explicitly among the
        //   deliberately-not-gated set.
        // - `shell`'s args 2.. (`face_{i}`) — face INDICES, not lengths.
        // - `chamfer` / `fillet` arg1 in the CURATED form — the edge selector.

        // fillet(target, radius) / fillet(target, edges, radius)
        "fillet" if arg_count == 2 => const { &[length_arg(1, "radius")] },
        "fillet" if arg_count == 3 => const { &[length_arg(2, "radius")] },

        // chamfer(target, distance) / chamfer(target, edges, distance)
        "chamfer" if arg_count == 2 => const { &[length_arg(1, "distance")] },
        "chamfer" if arg_count == 3 => const { &[length_arg(2, "distance")] },

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
        // The task-5744 `sweep` row, joined by the axis ORIGIN of `revolve` /
        // `revolve_full` — a task-5623 position that contract C6 forces into
        // this leaf through its named fixtures. Arg names from `geometry.rs`'s
        // Sweep arms.
        //
        // extrude(profile, distance) / extrude_symmetric(profile, distance)
        // pipe(path, radius)
        //   All exact-2 single-form. Index 0 is the profile / path — a geometry
        //   handle, permanently unchecked (ε=4358's territory).
        "extrude" | "extrude_symmetric" => const { &[length_arg(1, "distance")] },
        "pipe" => const { &[length_arg(1, "radius")] },

        // revolve(profile, ox, oy, oz, ax, ay, az, angle)
        // revolve_full(profile, ox, oy, oz, ax, ay, az)
        //   The second STRADDLE case, after `half_space` — three kinds of
        //   argument in one list, and only one of them gated:
        //   args1-3: the axis ORIGIN `ox`/`oy`/`oz` → LENGTH ("Length"). A
        //            point in space; a bare component is read as SI metres.
        //   args4-6: the axis DIRECTION `ax`/`ay`/`az` — a dimensionless unit
        //            vector, legitimately bare in correct `.ri`. UNSLOTTED, for
        //            the same reason `half_space`'s normal is.
        //   arg7:    `angle` (present only on `revolve`; `revolve_full` injects
        //            a literal 2π at lowering) — owned by
        //            `docs/prds/v0_6/angle-units-surface-convergence.md` by
        //            binding seam decree. Gating it here would be a scope
        //            violation AND would make the two layers disagree in the
        //            direction that PRD has to close together.
        //   Both are single-form, so both stay arity-agnostic. Pinned by
        //   `revolve_slots_the_origin_but_never_the_axis_or_the_angle`.
        "revolve" | "revolve_full" => const { &[
            length_arg(1, "ox"),
            length_arg(2, "oy"),
            length_arg(3, "oz"),
        ] },

        // ── 2-D profile producers (task 5750) ────────────────────────────────
        //
        // The task-5743 `profile` row of the same family table. Same rules as
        // the primitives above; `polygon`'s variadic vertex stream is excluded
        // for the reason recorded there.
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

        // All other names: empty (no dimensioned-scalar arg to check).
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
/// - `Type::Error` — poison sentinel; silently skipped (avoids cascading
///   diagnostics off an unrelated root-cause error).
/// - `Type::TypeParam(_)` — unresolved type variable; silently skipped
///   (constraint-aware / auto-type-param resolution is out of scope for ζ).
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
/// A slot carrying an [`ExpectedArg::Scalar::migration_hint`] appends the hint
/// with `"; "`, matching `ArgRejection::message`'s own shape (task 5750, PRD
/// `docs/prds/v0_6/units-length-gate-completion.md` decision D9):
/// `"{builtin}: {arg_name} argument expects {type_name}, got {actual}; {hint}"`
///
/// Concretely, for a LENGTH slot:
/// `"box: width argument expects Length, got Int; pass a dimensioned length such as `5mm`"`
///
/// The ANGLE slots render the un-hinted form, because the eval layer has no
/// angle hint to mirror; PRD 3 owns closing both halves together.
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
                Type::Error | Type::TypeParam(_) => continue,

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
                        *migration_hint,
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
                        *migration_hint,
                        call_span,
                        diagnostics,
                    );
                }
            },

            ExpectedArg::Int { type_name } => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently.
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
        }
    }
}

/// Emit a single `ArgTypeMismatch` error diagnostic.
///
/// `migration_hint`, when `Some`, is appended as `"; {hint}"` — matching
/// `ArgRejection::message`'s shape in `crates/reify-eval/src/arg_acceptance.rs`
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{
        AFFINE_MAP_CONSTRUCTOR_NAMES, DYNAMICS_CONSTRUCTOR_NAMES, DYNAMICS_QUERY_NAMES,
        FEA_ENVELOPE_NAMES, FIELD_OP_NAMES, GEOMETRY_FUNCTION_NAMES,
        GEOMETRY_KINEMATIC_QUERY_NAMES, GEOMETRY_QUERY_HELPER_NAMES, GEOMETRY_QUERY_NAMES,
        GEOMETRY_TOPOLOGY_SELECTOR_NAMES, TOLERANCING_MARKER_NAMES,
    };
    use reify_core::{DimensionVector, Severity, SourceSpan, Type, identity::ValueCellId};
    use reify_ir::CompiledExpr;

    /// Inclusive upper bound for every arity sweep in this module.
    ///
    /// The largest arity any arm currently guards on is 11 (`linear_pattern_2d`),
    /// so this leaves headroom above every guard in the table — an arity-guarded
    /// arm cannot hide from a sweep by sitting just above the bound. Raise it if a
    /// future arm guards on a higher arity.
    const MAX_PROBED_ARITY: usize = 14;

    /// Every units.rs builtin-name family slice, so a slice-derived sweep reaches
    /// any name that could plausibly gain an arg slot — not just the two families
    /// that happen to host keys today.
    const BUILTIN_NAME_FAMILIES: &[&[&str]] = &[
        GEOMETRY_FUNCTION_NAMES,
        GEOMETRY_QUERY_HELPER_NAMES,
        GEOMETRY_KINEMATIC_QUERY_NAMES,
        GEOMETRY_TOPOLOGY_SELECTOR_NAMES,
        AFFINE_MAP_CONSTRUCTOR_NAMES,
        TOLERANCING_MARKER_NAMES,
        GEOMETRY_QUERY_NAMES,
        DYNAMICS_QUERY_NAMES,
        DYNAMICS_CONSTRUCTOR_NAMES,
        FEA_ENVELOPE_NAMES,
        FIELD_OP_NAMES,
    ];

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
    ///   `is_geometry_topology_selector` (expr.rs:3243) *ahead of*
    ///   `is_geometry_function` (expr.rs:3360), and the selector arm resolves its
    ///   result type with
    ///   `topology_selector_result_type(name).expect("is_geometry_topology_selector implies result type")`.
    ///   Neither pattern name has an entry in `topology_selector_result_type`, so
    ///   slice membership alone would turn every `linear_pattern(…)` call into a
    ///   panic on that `expect` — the names would never reach the CSG arm at all.
    ///   That overlap is not otherwise pinned: units.rs's
    ///   `*_are_disjoint_from_other_families` tests iterate `GEOMETRY_QUERY_NAMES`
    ///   and the other sibling families, but none iterates
    ///   `GEOMETRY_FUNCTION_NAMES` or `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`, so
    ///   assertion (b) of [`arg_slot_keys_are_registered_builtin_names`] is what
    ///   holds this line.
    /// - The task-5750 PRIMITIVE and PROFILE producers — `box`, `box_centered`,
    ///   `cylinder`, `cylinder_centered`, `sphere`, `tube`, `cone`, `wedge`,
    ///   `torus`, `half_space`, `rectangle`, `circle`, `ellipse`. Every one is a
    ///   CSG/profile producer registered in `GEOMETRY_FUNCTION_NAMES`, gaining
    ///   its LENGTH dimension slots in task 5750 (PRD
    ///   `docs/prds/v0_6/units-length-gate-completion.md` leaf η). They are
    ///   exempted here for EXACTLY the reason `linear_pattern` is, and the
    ///   consequence of getting it wrong is identical: moving any of them into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset assertion
    ///   would route every `box(…)` call through the selector arm at
    ///   expr.rs:3243, whose
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
    ///   assertion.
    ///
    /// - The task-5750 TRANSFORM producers — `translate` and `rotate_around`.
    ///   Same story again: both are registered in `GEOMETRY_FUNCTION_NAMES`,
    ///   neither is a topology selector, and neither may be moved into the
    ///   selector slice.
    ///
    /// - The task-5662 PATTERN ORIGIN producers — `mirror` and
    ///   `circular_pattern`. Same story a third time: both are CSG producers
    ///   registered in `GEOMETRY_FUNCTION_NAMES`, neither is a topology
    ///   selector, and neither may be moved into
    ///   `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` to satisfy the subset assertion —
    ///   doing so would route every `mirror(...)` call through the selector arm
    ///   at `expr.rs:3243`, whose
    ///   `topology_selector_result_type(name).expect(...)` has no entry for
    ///   them and would panic.
    pub(crate) const NON_SELECTOR_ARG_SLOT_KEYS: &[&str] = &[
        "generate",
        "linear_pattern",
        "linear_pattern_2d",
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
        // Task 5750 — transforms.
        "translate",
        "rotate_around",
        // Task 5662 — pattern origin triples.
        "mirror",
        "circular_pattern",
    ];

    // ── builtin_arg_slots table contract (step-1) ────────────────────────────

    fn mass_density_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
                migration_hint: Some(DENSITY_MIGRATION_HINT),
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
                // Mirrors the table: eval has no angle hint, so neither has this.
                migration_hint: None,
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
                migration_hint: Some(LENGTH_MIGRATION_HINT),
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

    /// faces_by_normal → arg2 tol (ANGLE).
    #[test]
    fn faces_by_normal_has_angle_slot() {
        let slots = builtin_arg_slots("faces_by_normal", 3);
        assert_eq!(
            slots.len(),
            1,
            "faces_by_normal should have 1 slot, got: {:?}",
            slots
        );
        assert_eq!(slots[0], angle_slot(2, "tol"));
    }

    /// edges_parallel_to → arg2 tol (ANGLE).
    #[test]
    fn edges_parallel_to_has_angle_slot() {
        let slots = builtin_arg_slots("edges_parallel_to", 3);
        assert_eq!(
            slots.len(),
            1,
            "edges_parallel_to should have 1 slot, got: {:?}",
            slots
        );
        assert_eq!(slots[0], angle_slot(2, "tol"));
    }

    /// Task 3523 (step-7 RED). The perpendicular selectors mirror
    /// faces_by_normal/edges_parallel_to: arg2 `tol` is an ANGLE. Each must
    /// expose exactly one checkable ANGLE slot at index 2. RED until step-8
    /// joins them to the ANGLE-tol arm. (faces_by_surface_kind/edges_by_curve_kind
    /// and the extremal ctors take string/length args — no ANGLE slot.)
    #[test]
    fn perpendicular_selectors_have_angle_slot() {
        for name in ["faces_perpendicular_to", "edges_perpendicular_to"] {
            // Canonical call arity: (solid, dir, tol).
            let slots = builtin_arg_slots(name, 3);
            assert_eq!(
                slots.len(),
                1,
                "{name} should have 1 slot, got: {:?}",
                slots
            );
            assert_eq!(
                slots[0],
                angle_slot(2, "tol"),
                "{name} arg2 tol must be ANGLE"
            );
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
    // (boundary row 9). These pin the compile-layer half of the two task-5743
    // rows in `crates/reify-eval/src/arg_acceptance.rs`'s family table — the
    // 21 primitive fields and the 5 profile fields.
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
                 expected LENGTH slots; {name} is not an overloaded name, so its \
                 arm must carry no `if arg_count ==` guard"
            );
        }
    }

    /// box / box_centered → [width@0, height@1, depth@2].
    ///
    /// NOTE index 1 is `height`, NOT `depth`: `geometry.rs`'s shared Box arm
    /// orders the triple width/height/depth, so `height` is the Y extent. A
    /// transposed pair here would still type-check but would misname the
    /// offending argument in every diagnostic the slot emits.
    #[test]
    fn box_primitives_have_three_length_slots() {
        for name in ["box", "box_centered"] {
            assert_slots_at_every_arity(
                name,
                &[
                    length_slot(0, "width"),
                    length_slot(1, "height"),
                    length_slot(2, "depth"),
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
    ///
    /// NOTE the wedge orders its triple width/DEPTH/height — the opposite of
    /// `box`'s width/height/depth. Both orders are copied from their own
    /// lowering site rather than shared, which is why they can differ.
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

    /// half_space → the boundary POINT `px`/`py`/`pz` only.
    ///
    /// The STRADDLE case: `half_space(px, py, pz, nx, ny, nz)` mixes a gated
    /// POINT with an un-gated outward NORMAL in one argument list. The normal is
    /// a dimensionless unit vector whose components are legitimately bare in
    /// correct `.ri`, so slotting indices 3-5 would reject valid code — the same
    /// ORIGIN-vs-DIRECTION split already drawn for the circular pattern
    /// (`crates/reify-eval/src/arg_acceptance.rs:116-119`).
    #[test]
    fn half_space_slots_the_point_but_never_the_normal() {
        assert_slots_at_every_arity(
            "half_space",
            &[
                length_slot(0, "px"),
                length_slot(1, "py"),
                length_slot(2, "pz"),
            ],
        );

        // The exclusion stated POSITIVELY as well as by the list above, so a
        // reader sees the straddle drawn rather than having to infer it from an
        // absence — and so the reason travels with the assertion.
        let slotted: Vec<usize> = builtin_arg_slots("half_space", 6)
            .iter()
            .map(|slot| slot.index)
            .collect();
        for normal_index in [3usize, 4, 5] {
            assert!(
                !slotted.contains(&normal_index),
                "half_space arg{normal_index} is an outward-NORMAL component — a \
                 dimensionless unit vector — and must stay slot-free; got slots at \
                 {slotted:?}"
            );
        }
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
    // The compile-layer half of the two task-5744 rows in
    // `crates/reify-eval/src/arg_acceptance.rs`'s family table. Same
    // name-copied-from-the-lowering-site discipline as the primitive block
    // above; sources are `geometry_modify.rs` and `geometry.rs`'s Sweep arms.
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
    /// legitimately bare, and `arg_acceptance.rs` names this position
    /// explicitly as deliberately-not-gated ("its own production diagnostic
    /// already calls it a direction vec3"), so a LENGTH slot here would reject
    /// correct `.ri`. Stated positively rather than left to the vec assertion
    /// above, because it is a DECISION, not an omission.
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

    /// revolve / revolve_full → the axis ORIGIN triple only.
    ///
    /// The second STRADDLE case, after `half_space`. `revolve(profile, ox, oy,
    /// oz, ax, ay, az, angle)` carries a gated ORIGIN, an un-gated dimensionless
    /// axis DIRECTION and an ANGLE in one argument list:
    /// * `ox`/`oy`/`oz` are a point in space — bare components silently read as
    ///   SI metres, so they are slotted;
    /// * `ax`/`ay`/`az` are a unit vector, legitimately bare;
    /// * `angle` belongs to `docs/prds/v0_6/angle-units-surface-convergence.md`
    ///   by binding seam decree. Gating it HERE would be a scope violation, not
    ///   an improvement, and would make the two layers disagree in the
    ///   direction PRD 3 has to close together.
    ///
    /// `revolve_full` is the same layout minus the angle (the compiler injects
    /// a literal 2π), so the origin sits at the same indices and both are
    /// arity-agnostic single forms.
    #[test]
    fn revolve_slots_the_origin_but_never_the_axis_or_the_angle() {
        for name in ["revolve", "revolve_full"] {
            assert_slots_at_every_arity(
                name,
                &[
                    length_slot(1, "ox"),
                    length_slot(2, "oy"),
                    length_slot(3, "oz"),
                ],
            );

            // The exclusions stated positively: axis at 4/5/6, angle at 7
            // (present only on `revolve`).
            let slotted: Vec<usize> = builtin_arg_slots(name, 8)
                .iter()
                .map(|slot| slot.index)
                .collect();
            for excluded in [4usize, 5, 6, 7] {
                assert!(
                    !slotted.contains(&excluded),
                    "{name} arg{excluded} is an axis DIRECTION component or the \
                     ANGLE, neither of which this leaf may gate; got slots at \
                     {slotted:?}"
                );
            }
        }
    }

    /// `draft` stays wholly slot-free — its only scalar is an ANGLE.
    ///
    /// A control for the seam: `draft` sits in the same `compile_modify_op`
    /// match as every name slotted above, so "the modify family is gated" must
    /// not be read as "every modify argument is gated".
    #[test]
    fn draft_stays_slot_free_because_its_only_scalar_is_an_angle() {
        assert_slots_at_every_arity("draft", &[]);
    }

    // ── Task 5750 (units-length η): TRANSFORM LENGTH slots ───────────────────
    //
    // The task-5623 `transform` row of `crates/reify-eval/src/arg_acceptance.rs`'s
    // family table. Not one of the four families this leaf's task text names —
    // it is pulled in by contract C6, whose named fixtures
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

    /// rotate_around(target, px, py, pz, ax, ay, az, angle) → the PIVOT only.
    ///
    /// The third STRADDLE case, and structurally identical to `revolve`'s: a
    /// gated point in space, an un-gated dimensionless axis DIRECTION, and an
    /// ANGLE that belongs to
    /// `docs/prds/v0_6/angle-units-surface-convergence.md` by binding seam
    /// decree. All three exclusions are asserted positively.
    #[test]
    fn rotate_around_slots_the_pivot_but_never_the_axis_or_the_angle() {
        assert_slots_at_every_arity(
            "rotate_around",
            &[
                length_slot(1, "px"),
                length_slot(2, "py"),
                length_slot(3, "pz"),
            ],
        );

        let slotted: Vec<usize> = builtin_arg_slots("rotate_around", 8)
            .iter()
            .map(|slot| slot.index)
            .collect();
        for excluded in [0usize, 4, 5, 6, 7] {
            assert!(
                !slotted.contains(&excluded),
                "rotate_around arg{excluded} is the geometry handle, an axis \
                 DIRECTION component, or the ANGLE — none of which this leaf may \
                 gate; got slots at {slotted:?}"
            );
        }
    }

    /// `scale` stays wholly slot-free — its factor is DIMENSIONLESS.
    ///
    /// A scale factor is a ratio, so demanding a Length of it would reject
    /// correct `.ri` outright. `arg_acceptance.rs` names dimensionless scale
    /// FACTORS explicitly among the deliberately-not-gated set. Pinned as a
    /// seam control: `scale` sits in the same `compile_transform_op` match as
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
            // width/height/depth LENGTH slots, so it is no longer an unchecked
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
    /// The probe set is EVERY units.rs builtin-name family
    /// ([`BUILTIN_NAME_FAMILIES`]) + `non_family_keys` + the typo/non-geometry
    /// list, swept across arities up to [`MAX_PROBED_ARITY`] so neither a new key
    /// from an unrelated family (say a MASS_DENSITY slot on `mass_properties`)
    /// nor an arity-guarded arm can hide from the invariant.
    #[test]
    fn arg_slot_keys_are_registered_builtin_names() {
        // Keys that belong to NO units.rs family slice and so cannot be reached
        // by any slice-derived sweep — they must be probed by name or the
        // subset assertion stays vacuous for them. `generate` is exactly the
        // name the predecessor test structurally could not reach; listing it
        // here is what actually closes that hole (removing it from
        // NON_SELECTOR_ARG_SLOT_KEYS now fails the assertion below, whereas
        // before it would have gone unnoticed).
        let non_family_keys: &[&str] = &["generate"];

        // Extra non-selector names that must never map to non-empty slots.
        // `"box"` and `"cylinder"` used to head this list. Task 5750
        // (units-length η) gave both LENGTH slots, so they must move out of a
        // must-stay-empty list; they are exempted via NON_SELECTOR_ARG_SLOT_KEYS
        // instead. `union` / `difference` replace them so the list still NAMES a
        // registered CSG producer that carries no dimensioned arg — both are
        // additionally reached by the BUILTIN_NAME_FAMILIES sweep, so the
        // replacement preserves the list's readability, not its reach.
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
        for &name in BUILTIN_NAME_FAMILIES
            .iter()
            .flat_map(|family| family.iter())
            .chain(non_family_keys.iter())
            .chain(extra_non_selector.iter())
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
        let dir_type = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
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
        let dir_type = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
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
        let dir_type = Type::Vector {
            n: 3,
            quantity: Box::new(Type::dimensionless_scalar()),
        };
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

    // ── linear_pattern spacing LENGTH slot (task 5652) ────────────────────────

    /// Build a 6-arg `linear_pattern(target, dx, dy, dz, count, spacing)` arg
    /// list with the given `spacing` type. Args 1-4 (direction + count) are
    /// deliberately unchecked slots — see the `linear_pattern` arm of
    /// [`builtin_arg_slots`].
    fn linear_pattern_args(spacing: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry),               // 0 target
            arg_expr(Type::dimensionless_scalar()), // 1 dx
            arg_expr(Type::dimensionless_scalar()), // 2 dy
            arg_expr(Type::dimensionless_scalar()), // 3 dz
            arg_expr(Type::Int),                    // 4 count
            arg_expr(spacing),                      // 5 spacing
        ]
    }

    /// linear_pattern @ arity 6 → arg5 spacing (LENGTH); every OTHER arity → empty.
    ///
    /// Pinning the empty result at arity 4 NOW is what makes task 5351's
    /// direction-value overload safe to add later — why, on the `linear_pattern`
    /// arm of [`builtin_arg_slots`].
    #[test]
    fn linear_pattern_spacing_slot_is_arity_6_only() {
        assert_eq!(
            builtin_arg_slots("linear_pattern", 6),
            vec![length_slot(5, "spacing")],
            "linear_pattern(target, dx, dy, dz, count, spacing) — arg5 spacing must be LENGTH"
        );
        for arity in [0usize, 1, 3, 4, 5, 7, 11] {
            assert!(
                builtin_arg_slots("linear_pattern", arity).is_empty(),
                "linear_pattern at arity {arity} is not the 6-arg form, so it must \
                 expose NO slots — index 5 does not denote `spacing` there"
            );
        }
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

    /// A non-6-arg call is slot-free, so even a definitely-wrong spacing type
    /// produces nothing: the arity guard, not luck, is what keeps it quiet.
    #[test]
    fn linear_pattern_non_6_arity_gives_no_diagnostics() {
        let four_args = vec![
            arg_expr(Type::Geometry), // 0 target
            arg_expr(Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            }), // 1 direction (task 5351's future form)
            arg_expr(Type::Int),      // 2 count
            arg_expr(Type::Int),      // 3 spacing — bare, but NOT checked at this arity
        ];
        let mut diags = Vec::new();
        check_builtin_arg_types("linear_pattern", &four_args, dummy_span(), &mut diags);
        assert!(
            diags.is_empty(),
            "a 4-arg linear_pattern exposes no slots, got: {:?}",
            diags
        );
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

    /// Build a 7-arg `mirror(target, ox, oy, oz, nx, ny, nz)` arg list with the
    /// given origin-component type. The normal components 4-6 are deliberately
    /// unslotted — see the `mirror` arm of [`builtin_arg_slots`].
    fn mirror_args(origin: Type) -> Vec<CompiledExpr> {
        vec![
            arg_expr(Type::Geometry),               // 0 target
            arg_expr(origin.clone()),               // 1 ox
            arg_expr(origin.clone()),               // 2 oy
            arg_expr(origin),                       // 3 oz
            arg_expr(Type::dimensionless_scalar()), // 4 nx
            arg_expr(Type::dimensionless_scalar()), // 5 ny
            arg_expr(Type::dimensionless_scalar()), // 6 nz
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

    /// mirror @ arity 7 → ox@1 / oy@2 / oz@3 (LENGTH); every other arity → empty.
    ///
    /// Arity 2 gets its own assertion because it is the CONCRETE false positive
    /// the guard prevents, not a hypothetical: `mirror(target, plane)` holds a
    /// `Plane` at index 1, so an arity-agnostic `ox@1 LENGTH` slot would demand
    /// a Length of a Plane on correct code.
    #[test]
    fn mirror_origin_slots_are_arity_7_only() {
        assert_eq!(
            builtin_arg_slots("mirror", 7),
            vec![
                length_slot(1, "ox"),
                length_slot(2, "oy"),
                length_slot(3, "oz"),
            ],
            "mirror(target, ox, oy, oz, nx, ny, nz) — args 1-3 are the mirror-plane \
             ORIGIN and must be LENGTH"
        );
        assert!(
            builtin_arg_slots("mirror", 2).is_empty(),
            "at arity 2 (`mirror(target, plane)`, the task-5745 decoded-value form) \
             index 1 is `plane`, a Plane — an ox@1 LENGTH slot would emit a FALSE \
             ArgTypeMismatch on valid code, so arity 2 must expose NO slots"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 7) {
            assert!(
                builtin_arg_slots("mirror", arity).is_empty(),
                "mirror at arity {arity} is not the 7-arg scalar form, so it must \
                 expose NO slots — index 1 does not denote `ox` there"
            );
        }
    }

    /// circular_pattern @ arity 9 → ox@1 / oy@2 / oz@3 (LENGTH); every other
    /// arity → empty.
    ///
    /// Arity 4 gets its own assertion for the same reason arity 2 does on
    /// `mirror`: `circular_pattern(target, axis, count, angle)` holds an `Axis`
    /// at index 1.
    #[test]
    fn circular_pattern_origin_slots_are_arity_9_only() {
        assert_eq!(
            builtin_arg_slots("circular_pattern", 9),
            vec![
                length_slot(1, "ox"),
                length_slot(2, "oy"),
                length_slot(3, "oz"),
            ],
            "circular_pattern(target, ox, oy, oz, ax, ay, az, count, angle) — \
             args 1-3 are the rotation-axis ORIGIN and must be LENGTH"
        );
        assert!(
            builtin_arg_slots("circular_pattern", 4).is_empty(),
            "at arity 4 (`circular_pattern(target, axis, count, angle)`, the \
             task-5745 decoded-value form) index 1 is `axis`, an Axis — an ox@1 \
             LENGTH slot would emit a FALSE ArgTypeMismatch on valid code, so \
             arity 4 must expose NO slots"
        );
        for arity in (0usize..=MAX_PROBED_ARITY).filter(|n| *n != 9) {
            assert!(
                builtin_arg_slots("circular_pattern", arity).is_empty(),
                "circular_pattern at arity {arity} is not the 9-arg scalar form, so \
                 it must expose NO slots — index 1 does not denote `ox` there"
            );
        }
    }

    /// The STRADDLE case, fourth of five: `mirror`'s 7-arg form mixes a gated
    /// plane ORIGIN with an un-gated plane NORMAL in one argument list.
    ///
    /// Stated POSITIVELY as a decision rather than left to be inferred from an
    /// absence, exactly as `half_space_slots_the_point_but_never_the_normal`
    /// does: `nx`/`ny`/`nz` are a dimensionless unit vector whose components are
    /// legitimately bare in correct `.ri` (a normal's scale is irrelevant to the
    /// plane it defines), so slotting 4-6 would reject valid code.
    #[test]
    fn mirror_slots_the_origin_but_never_the_normal() {
        let slotted: Vec<usize> = builtin_arg_slots("mirror", 7)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert_eq!(
            slotted,
            vec![1, 2, 3],
            "mirror's slotted index set at arity 7 must be exactly the ORIGIN \
             triple {{1,2,3}}; got {slotted:?}"
        );
        for normal_index in [4usize, 5, 6] {
            assert!(
                !slotted.contains(&normal_index),
                "mirror arg{normal_index} is a plane-NORMAL component — a \
                 dimensionless unit vector — and must stay slot-free; got slots at \
                 {slotted:?}"
            );
        }
    }

    /// The STRADDLE case, fifth of five, and the widest: `circular_pattern`'s
    /// 9-arg form carries a gated ORIGIN, an un-gated axis DIRECTION, an Int
    /// `count` and an `angle` this leaf may not touch.
    ///
    /// Modelled on `revolve_slots_the_origin_but_never_the_axis_or_the_angle`:
    ///
    /// * `ox`/`oy`/`oz` are a point in space — bare components silently read as
    ///   SI metres, so they are slotted;
    /// * `ax`/`ay`/`az` are a unit vector, legitimately bare;
    /// * `count` is an Int — a wrong count is an arity/semantic error, not a
    ///   dimension error;
    /// * `angle` belongs to `docs/prds/v0_6/angle-units-surface-convergence.md`
    ///   by binding seam decree. Gating it HERE would be a scope violation.
    #[test]
    fn circular_pattern_slots_the_origin_but_never_the_axis_count_or_angle() {
        let slotted: Vec<usize> = builtin_arg_slots("circular_pattern", 9)
            .iter()
            .map(|slot| slot.index)
            .collect();
        assert_eq!(
            slotted,
            vec![1, 2, 3],
            "circular_pattern's slotted index set at arity 9 must be exactly the \
             ORIGIN triple {{1,2,3}}; got {slotted:?}"
        );
        for excluded in [4usize, 5, 6, 7, 8] {
            assert!(
                !slotted.contains(&excluded),
                "circular_pattern arg{excluded} is an axis DIRECTION component, the \
                 Int `count`, or the ANGLE — none of which this leaf may gate; got \
                 slots at {slotted:?}"
            );
        }
    }

    /// CORRECT: a dimensioned `Length` origin → 0 diagnostics, both builtins.
    #[test]
    fn pattern_origin_length_components_give_no_error() {
        for (name, args) in [
            ("mirror", mirror_args(Type::length())),
            ("circular_pattern", circular_pattern_args(Type::length())),
        ] {
            let mut diags = Vec::new();
            check_builtin_arg_types(name, &args, dummy_span(), &mut diags);
            assert!(
                diags.is_empty(),
                "{name}: a dimensioned Length origin must pass, got: {:?}",
                diags
            );
        }
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
        for (name, args) in [
            ("mirror", mirror_args(Type::Int)),
            ("circular_pattern", circular_pattern_args(Type::Int)),
        ] {
            let mut diags = Vec::new();
            check_builtin_arg_types(name, &args, dummy_span(), &mut diags);
            assert_eq!(
                diags.len(),
                3,
                "{name}: a bare origin triple must produce one diagnostic per \
                 component, got: {:?}",
                diags
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
    }

    /// MISMATCH: a DIMENSIONED but WRONG-dimension origin (`0deg`) names the
    /// expected `Length` AND the offending unit.
    ///
    /// Distinct from the bare-`Int` case at the CODE level, not just the source
    /// level: a bare Int lands in `check_builtin_arg_types`' catch-all `other =>`
    /// arm, whereas a wrong-dimension scalar goes through the
    /// `Type::Scalar { dimension } != expected_dim` arm. Without this case that
    /// arm is unexercised for both new slots.
    #[test]
    fn pattern_wrong_dimension_origin_names_length_and_actual_unit() {
        let angle = Type::Scalar {
            dimension: DimensionVector::ANGLE,
        };
        for (name, args) in [
            ("mirror", mirror_args(angle.clone())),
            ("circular_pattern", circular_pattern_args(angle.clone())),
        ] {
            let mut diags = Vec::new();
            check_builtin_arg_types(name, &args, dummy_span(), &mut diags);
            assert_eq!(
                diags.len(),
                3,
                "{name}: expected 3 diagnostics, got: {:?}",
                diags
            );
            for diag in &diags {
                assert_eq!(diag.severity, Severity::Error);
                assert_eq!(diag.code, Some(DiagnosticCode::ArgTypeMismatch));
                for needle in ["expects Length", "got Scalar[rad]"] {
                    assert!(
                        diag.message.contains(needle),
                        "{name}: message must contain {needle:?} so the user can see \
                         WHICH unit was wrong, not merely that one was: {}",
                        diag.message
                    );
                }
            }
        }
    }

    /// GRADUALISM: an unresolved `TypeParam` / poison `Error` origin → 0
    /// diagnostics, both builtins. The executable proof of the two-layer
    /// relationship stated in this module's "Relationship to the eval-layer
    /// units gate" section.
    #[test]
    fn pattern_unresolved_origin_passes_silently() {
        for ty in [Type::TypeParam("T".to_string()), Type::Error] {
            for (name, args) in [
                ("mirror", mirror_args(ty.clone())),
                ("circular_pattern", circular_pattern_args(ty.clone())),
            ] {
                let mut diags = Vec::new();
                check_builtin_arg_types(name, &args, dummy_span(), &mut diags);
                assert!(
                    diags.is_empty(),
                    "{name}: a {ty} origin must pass silently (gradualism) — the \
                     eval-layer gate stays the backstop; got: {:?}",
                    diags
                );
            }
        }
    }

    /// The ARITY GUARD, not the `compiled_args.get(index)` bounds check, is what
    /// keeps the two decoded-VALUE forms quiet.
    ///
    /// This is the distinction that matters, and it is why the guard is
    /// semantically load-bearing rather than mere forward-compat: both value
    /// forms DO have an index 1, holding a `Plane` / an `Axis`. A definitely-
    /// wrong type there produces nothing only because the arm never matches.
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
            "a 4-arg circular_pattern exposes no slots even though index 1 EXISTS, \
             got: {:?}",
            diags
        );
    }
}
