//! Per-argument type signatures for geometry topology-selector builtins
//! (task 4493, type-hygiene ζ).
//!
//! Hosts the checkable argument-slot table ([`builtin_arg_slots`]) and the
//! call-site checker ([`check_builtin_arg_types`]) for the geometry
//! topology-selector family.  The mechanism is generic (name-keyed), but only
//! geometry-selector dimensioned-scalar slots are populated here; math args
//! (polymorphic, no fixed dimension) and geometry-handle arg0 (ε=4358's
//! territory, PRD §4 out-of-scope) are intentionally absent.
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
//!
//! UNCHECKED (would false-positive on valid call sites or is out-of-scope):
//! - arg0 (geometry handle) — ε=4358's territory
//! - `dir` Vec3 slot — accepts list literals `[0,0,1]` that coerce
//! - Range slots (`edges_by_length` / `faces_by_area`)
//! - Names without dimensioned-scalar args (`split`, `face`, `edge`, `solid_body`, …)

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
/// `arg_count` is the number of arguments at the call site.  The table is
/// keyed on `(name, arity)` rather than `name` alone because a builtin's
/// positional layout is only stable *within* one overload: for an overloaded
/// name, index `i` denotes a different parameter in each form, so a
/// fixed-index slot would be semantically lying and would eventually fire on
/// valid code.  The precedent is
/// [`crate::relation_signatures::relation_operand_datum`], which discriminates
/// `"offset" if args.len() == 3` / `"angle" if args.len() == 3` for exactly
/// this reason.
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
    use crate::units::GEOMETRY_TOPOLOGY_SELECTOR_NAMES;
    use reify_core::{DimensionVector, Severity, SourceSpan, Type, identity::ValueCellId};
    use reify_ir::CompiledExpr;

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

    /// Task 5652 (step-1 RED). The arg-slot table is keyed on `(name, arity)`,
    /// not on `name` alone — `builtin_arg_slots` takes a second `arg_count`
    /// parameter so genuinely overloaded builtins can expose different slots
    /// per call arity (the precedent is
    /// `relation_signatures::relation_operand_datum(name, args)`, which
    /// discriminates `"offset" if args.len() == 3`).
    ///
    /// This test pins the OTHER half of that contract: the arity parameter is
    /// *available* to every arm but *used* only by genuinely overloaded names.
    /// Every existing non-overloaded selector arm must stay arity-AGNOSTIC, so
    /// that adding the dimension changes no current behaviour. In particular,
    /// gating e.g. `edges_at_height` on `arg_count == 3` would silently hollow
    /// out `edges_at_height_short_args_no_panic` (which passes only 2 args and
    /// expects 0 diagnostics): that test would then pass for a new and weaker
    /// reason — no slots at all, rather than a correct `h` + an absent `tol`.
    ///
    /// RED for a compile reason: `builtin_arg_slots` currently takes one
    /// parameter, so the crate's test build fails. That failure IS the pinned
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
        for arity in 0usize..=12 {
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
            for arg_count in 0usize..=12 {
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

    /// Coverage invariant: every key in the table's domain is a member of
    /// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` — catching typos and keeping the
    /// arg-slot table consistent with the recognized family even as new
    /// selector names land.
    ///
    /// The test probes `builtin_arg_slots` on every name in
    /// `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` PLUS a set of known-non-selector
    /// names, and asserts that every name for which it returns non-empty slots
    /// is actually in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`.  This ties the
    /// invariant to the real match arms (rather than a parallel hardcoded list)
    /// so a typo'd new key — e.g. `"moment_of_inertta"` — would be caught.
    #[test]
    fn arg_slot_keys_are_subset_of_topology_selector_names() {
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
        ];

        let mut any_nonempty = false;
        for &name in GEOMETRY_TOPOLOGY_SELECTOR_NAMES
            .iter()
            .chain(extra_non_selector.iter())
        {
            // Swept across arities so an arity-guarded arm cannot hide from
            // the invariant by being empty at the one arity probed.
            for arg_count in 0usize..=12 {
                let slots = builtin_arg_slots(name, arg_count);
                if !slots.is_empty() {
                    any_nonempty = true;
                    assert!(
                        GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name),
                        "builtin_arg_slots({:?}, {}) returned non-empty slots, but {:?} \
                         is not in GEOMETRY_TOPOLOGY_SELECTOR_NAMES; \
                         fix the name or add it to the selector slice",
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

        // Smoke: the five canonical checked names must individually return
        // non-empty at their canonical call arity.
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
    /// list with the given `spacing` type. Args 1-3 are the DIMENSIONLESS
    /// direction vector components and arg 4 the `Int` count — both
    /// deliberately unchecked (task 5214 established the direction is a unit
    /// vector, so there is no silent-metres hazard there).
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
    /// The `arg_count == 6` guard is a forward-compat guard, not decoration:
    /// task 5351 lands a 4-arg `(target, direction, count, spacing)` value form
    /// in which `spacing` moves to index 3, so an arity-agnostic `spacing@5`
    /// slot would silently point at nothing there. Pinning the empty result at
    /// arity 4 NOW is what makes that overload safe to add later.
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

    /// GRADUALISM: an unresolved `TypeParam` spacing → 0 diagnostics.
    ///
    /// This is the explicit proof that the compile-layer slot COMPLEMENTS and
    /// never REPLACES task 5214's eval-layer gate (`required_length_value`): a
    /// dynamically-typed spacing types as `TypeParam`/`Error` here and is
    /// skipped by PRD-decision-6 gradualism, so eval remains the backstop.
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
}
