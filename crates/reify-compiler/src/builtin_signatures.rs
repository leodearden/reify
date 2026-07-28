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
//! # Checked vs. unchecked slots (decision-4 gradualism)
//!
//! CHECKED (definite dimension mismatch, zero false positives):
//! - `center_of_mass` / `moment_of_inertia` arg1 `density` → MASS_DENSITY ("Density")
//! - `faces_by_normal` / `edges_parallel_to` arg2 `tol` → ANGLE ("Angle")
//! - `faces_perpendicular_to` / `edges_perpendicular_to` arg2 `tol` → ANGLE ("Angle")
//! - `edges_at_height` arg1 `h` → LENGTH ("Length"), arg2 `tol` → LENGTH ("Length")
//! - `extremal_by_bbox` / `extremal_by_centroid` arg3 `tol` → LENGTH ("Length")
//! - `generate` arg0 `n` → `Int` (task 3994; the lone non-geometry, non-`Scalar`
//!   slot — uses the generic name-keyed mechanism via `ExpectedArg::Int`)
//! - `linear_pattern` (arity 6) arg5 `spacing` → LENGTH ("Length") (task 5652)
//! - `linear_pattern_2d` (arity 11) arg5 `spacing1` and arg10 `spacing2` →
//!   LENGTH ("Length") (task 5652)
//!
//! UNCHECKED (would false-positive on valid call sites or is out-of-scope):
//! - arg0 (geometry handle) — ε=4358's territory
//! - `dir` Vec3 slot — accepts list literals `[0,0,1]` that coerce
//! - Range slots (`edges_by_length` / `faces_by_area`)
//! - Names without dimensioned-scalar args (`split`, `face`, `edge`, `solid_body`, …)
//! - Pattern DIRECTION components and COUNTS (`linear_pattern` args1-4;
//!   `linear_pattern_2d` args1-4 and 6-9) — why each is exempt is on the
//!   `linear_pattern` arm of [`builtin_arg_slots`].
//! - `mirror` (arity 7) / `circular_pattern` (arity 9) origin triples
//!   `ox`/`oy`/`oz` — length-semantic and eligible, but deliberately DEFERRED by
//!   task 5652: LENGTH slots there turn at least six existing valid-today call
//!   sites into hard compile errors, spanning `reify-eval` tests and
//!   `examples/` — a separable breaking-surface migration outside 5652's scope.
//!   TODO(#5662): add the ox/oy/oz LENGTH slots and migrate those call sites.
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
    },
    /// The integer type `Type::Int` (e.g. `generate`'s count argument, task 3994).
    ///
    /// Distinct from `Scalar { DIMENSIONLESS }` (a dimensionless `Real`): a count
    /// must be a true `Int`, so `generate(2.5, …)` and `generate(3mm, …)` are both
    /// rejected while `generate(3, …)` passes.
    Int {
        /// Human-readable type name for diagnostic messages (always `"Int"`).
        type_name: &'static str,
    },
}

/// Return the checkable dimensioned-scalar argument slots for a named builtin
/// call of a given arity.
///
/// Returns an empty `Vec` for:
/// - Unrecognized names.
/// - Names with no checked dimensioned-scalar arg (e.g. `split`, `face`, `edge`,
///   `solid_body`, `volume`, `edges`, `faces`, …).
/// - A recognized *overloaded* name called at an arity whose positional layout
///   has no checked slot (see "Arity awareness" below).
///
/// The returned slots correspond exactly to the CHECKED arg positions listed
/// in the module-level docs.  Mirrors the name-keyed structure of
/// `math_fn_result_type` (task 4182 result-type precedent).
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
pub(crate) fn builtin_arg_slots(name: &str, arg_count: usize) -> Vec<CheckableArg> {
    match name {
        // ── Mass-properties topology selectors ───────────────────────────────
        // arg0: geometry handle (unchecked — ε=4358's territory)
        // arg1: density → MASS_DENSITY ("Density")
        "center_of_mass" | "moment_of_inertia" => vec![CheckableArg {
            index: 1,
            name: "density",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
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
        | "edges_perpendicular_to" => vec![CheckableArg {
            index: 2,
            name: "tol",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::ANGLE,
                type_name: "Angle",
            },
        }],

        // ── Height-based topology selectors ──────────────────────────────────
        // arg0: geometry handle (unchecked)
        // arg1: h → LENGTH ("Length")
        // arg2: tol → LENGTH ("Length")
        "edges_at_height" => vec![
            CheckableArg {
                index: 1,
                name: "h",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                },
            },
            CheckableArg {
                index: 2,
                name: "tol",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
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
        "extremal_by_bbox" | "extremal_by_centroid" => vec![CheckableArg {
            index: 3,
            name: "tol",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
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
        "generate" => vec![CheckableArg {
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
        "linear_pattern" if arg_count == 6 => vec![CheckableArg {
            index: 5,
            name: "spacing",
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::LENGTH,
                type_name: "Length",
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
        "linear_pattern_2d" if arg_count == 11 => vec![
            CheckableArg {
                index: 5,
                name: "spacing1",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                },
            },
            CheckableArg {
                index: 10,
                name: "spacing2",
                expected: ExpectedArg::Scalar {
                    dimension: DimensionVector::LENGTH,
                    type_name: "Length",
                },
            },
        ],

        // All other names: empty (no dimensioned-scalar arg to check).
        _ => vec![],
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
    for slot in &slots {
        let Some(arg) = compiled_args.get(slot.index) else {
            // Arg absent (call is short) — skip. Arity errors are handled
            // elsewhere; a short-arg call is not a type-mismatch.
            continue;
        };

        match &slot.expected {
            ExpectedArg::Scalar {
                dimension: expected_dim,
                type_name,
            } => match &arg.result_type {
                // Gradualism: poison + unresolved pass silently.
                Type::Error | Type::TypeParam(_) => continue,

                // Dimensioned scalar: mismatch only when the dimension differs.
                Type::Scalar { dimension } => {
                    if dimension == expected_dim {
                        continue; // correct — no diagnostic
                    }
                    let actual = &arg.result_type;
                    emit_mismatch(name, slot.name, type_name, actual, call_span, diagnostics);
                }

                // Any other concrete type (Bool, Geometry, Vector, …): definite
                // kind mismatch where a dimensioned scalar is required.
                other => {
                    emit_mismatch(name, slot.name, type_name, other, call_span, diagnostics);
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
                    emit_mismatch(name, slot.name, type_name, other, call_span, diagnostics);
                }
            },
        }
    }
}

/// Emit a single `ArgTypeMismatch` error diagnostic.
fn emit_mismatch(
    builtin: &str,
    arg_name: &str,
    type_name: &str,
    actual: &Type,
    call_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let msg = format!("{builtin}: {arg_name} argument expects {type_name}, got {actual}");
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
    pub(crate) const NON_SELECTOR_ARG_SLOT_KEYS: &[&str] =
        &["generate", "linear_pattern", "linear_pattern_2d"];

    // ── builtin_arg_slots table contract (step-1) ────────────────────────────

    fn mass_density_slot(index: usize, name: &'static str) -> CheckableArg {
        CheckableArg {
            index,
            name,
            expected: ExpectedArg::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
                type_name: "Density",
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
            "box",
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
        let extra_non_selector: &[&str] = &[
            "box",
            "cylinder",
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
        for &name in &["linear_pattern", "linear_pattern_2d"] {
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
}
