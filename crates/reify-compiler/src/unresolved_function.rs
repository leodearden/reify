//! Closed-world membership oracle for **builtin function names**.
//!
//! # Why this module exists
//!
//! The `NoUserFunctions` arm of the `FunctionCall` ladder in
//! [`crate::expr`] ends in a *terminal first-arg fallback*: any callee that no
//! ladder arm claims is typed as its first argument's type (or
//! `Type::dimensionless_scalar()` when zero-arg). That fallback is
//! **open-world** — a genuinely nonexistent name such as
//! `definitely_not_a_reify_builtin_xyz(2.5mm)` compiles with ZERO diagnostics
//! and silently adopts `Scalar<LENGTH>`.
//!
//! This module supplies the missing complement: a single predicate,
//! [`is_known_builtin`], that answers "is this name known to the compiler at
//! all?" by unioning **every** classification family the ladder consults, plus
//! two explicit manifests declared here:
//!
//! * `FIRST_ARG_TYPED_NAMES` — names for which the terminal fallback's
//!   first-arg typing is *verified correct*, so they are named rather than
//!   left open-world.
//! * `EVAL_DEFERRED_BUILTIN_NAMES` — names that are eval-dispatchable but not
//!   yet family-registered, whose typing is deliberately left to the fallback.
//!
//! The two manifests deliberately make DIFFERENT claims, and the difference is
//! load-bearing. Membership in the allowlist asserts that the fallback types
//! the name *correctly* — a falsifiable claim, evidenced per name against the
//! eval body. Membership in the manifest asserts only that the name is
//! *eval-dispatchable and not yet family-registered, with its typing left to
//! the fallback* — a claim that is unconditionally true and says nothing about
//! whether that typing is right. Both suppress the `UnresolvedFunction`
//! warning identically; only the manifest can honestly hold a name whose
//! fallback typing is known to be WRONG (`complex_mul` and friends). Collapsing
//! the two would launder a known-false claim into an allowlist, so a name in
//! both is a hard test failure.
//!
//! With the union in hand, `expr.rs` can emit a
//! `DiagnosticCode::UnresolvedFunction` **warning** at the fallback when the
//! callee is unknown, closing the open world without changing any typing.
//!
//! # Warn-mode-first posture
//!
//! Typing is **unchanged** by this module. Every call that compiled before
//! still compiles to the same type; the only new observable is a diagnostic.
//! That is deliberate (fail-closed warn-first): the corpus sweep must be green
//! before the code can become an error.
//!
//! # Downstream consumers
//!
//! * **#5997** flips `UnresolvedFunction` from Warning to Error behind a
//!   break-glass env knob. It names this module's manifest, allowlist and
//!   corpus sweep as its preconditions.
//! * **#6014** (builtin-signature-registry, task omega) DELETES the terminal
//!   first-arg fallback outright, and with it this module's
//!   `FIRST_ARG_TYPED_NAMES` family — once every name in it holds a real
//!   registry row, the allowlist has no remaining job. Its family-by-family
//!   migration is seeded by this task's warn-sweep violation list
//!   (`docs/notes/unresolved-function-warn-sweep-2026-08-29.md`).

use crate::analysis_signatures::ANALYSIS_FN_NAMES;
use crate::expr::DETERMINACY_PREDICATE_NAMES;
use crate::joint_signatures::JOINT_TYPED_FN_NAMES;
use crate::list_helpers::LIST_HELPER_NAMES;
use crate::math_signatures::{
    MATH_CONSTRUCTION_NAMES, MATH_OPERATION_NAMES, MATH_TRANSCENDENTAL_NAMES,
};
use crate::orientation_signatures::ORIENTATION_TYPED_FN_NAMES;
use crate::parse_signatures::PARSE_FN_NAMES;
use crate::relation_signatures::{RELATION_FN_NAMES, is_relation_shared_verb};
use crate::units::{
    AFFINE_ALGEBRA_NAMES, AFFINE_MAP_CONSTRUCTOR_NAMES, DATUM_CONSTRUCTOR_NAMES,
    DYNAMICS_CONSTRUCTOR_NAMES, DYNAMICS_QUERY_NAMES, FEA_ENVELOPE_NAMES, FIELD_OP_NAMES,
    GEOMETRY_FUNCTION_NAMES, GEOMETRY_KINEMATIC_QUERY_NAMES, GEOMETRY_QUERY_HELPER_NAMES,
    GEOMETRY_QUERY_NAMES, GEOMETRY_TOPOLOGY_SELECTOR_NAMES, SELECTOR_COMPOSITION_NAMES,
    TOLERANCING_MARKER_NAMES,
};

/// Builtins for which the terminal first-arg fallback's typing is **verified
/// correct** — named rather than left open-world.
///
/// # What membership asserts
///
/// This family is deliberately **not a ladder arm** and resolves **no type**.
/// It is a membership set only. An entry is a positive claim, checked against
/// the eval body: *for this name, the result's type is the first argument's
/// type, so the fallback that types it is right.* That is a stronger claim
/// than [`EVAL_DEFERRED_BUILTIN_NAMES`] makes, and it is falsifiable — see the
/// evidence table below.
///
/// The two are not interchangeable. A name whose fallback typing is merely
/// *unverified* belongs in the manifest; only a name whose typing is *verified
/// right* belongs here. Collapsing the distinction is how a known-false claim
/// gets laundered into an allowlist.
///
/// # Evidence table
///
/// | name | eval body | why first-arg-preserving |
/// |---|---|---|
/// | `project` | `reify-stdlib/src/geometry.rs:1059` | frame projection: `Point3<L> -> Point3<L>`, `Vector3<L> -> Vector3<L>` (translation-invariant); the arm's own doc states both signatures |
/// | `mod` | `reify-stdlib/src/numeric.rs:89` | `(Int, Int) -> Int`; every non-Int pair is `Undef` |
/// | `to_global` | `reify-stdlib/src/fea.rs:494` | returns `Value::Field` carrying arg0's `domain_type`/`codomain_type` verbatim — only the data buffer is new |
/// | `effective_tolerance_zone` | `reify-stdlib/src/tolerancing.rs:174` | arg0 validated `DimensionVector::LENGTH`; result is `Scalar { dimension: LENGTH }` |
/// | `input_shape_apply` | `reify-stdlib/src/trajectory/input_shape.rs` (`eval_input_shape`) | echoes arg0's own `StructureInstanceData`, `type_id` included |
/// | `complex_add` | `reify-stdlib/src/complex.rs:111` | requires `ad == bd`, result dimension is `*ad` |
/// | `complex_exp` | `reify-stdlib/src/complex.rs:253` | dimensionless-in (else `Undef`), dimensionless-out |
/// | `complex_sqrt` | `reify-stdlib/src/complex.rs:272` | dimensionless-in (else `Undef`), dimensionless-out |
///
/// # Reconciling the task brief's "16 fallback-correct" figure
///
/// #5371 named sixteen candidates. Eight are here; the other eight were
/// excluded for two distinct reasons, both pinned by
/// `tests::first_arg_typed_names_exclude_the_already_claimed_and_the_dimension_transforming`:
///
/// * **Five are already claimed** by [`crate::orientation_signatures::ORIENTATION_TYPED_FN_NAMES`]
///   (task #5344, landed after the brief was measured): `transform_inverse`,
///   `transform_compose`, `orient_inverse`, `orient_compose`, `orient_slerp`.
///   The fallback never sees them, so an entry here would be an unfalsifiable
///   claim about dead code — and would break that family's disjointness
///   contract.
/// * **Three are dimension-TRANSFORMING**, so first-arg typing is a
///   known-false claim rather than an unverified one: `complex_mul`
///   (`complex.rs:138`, `dimension = ad.mul(bd)`), `complex_div`
///   (`complex.rs:160`, `ad.div(bd)`) and `complex_pow` (`complex.rs:191`,
///   accumulates `dim^n` via `DimensionVector::mul`; `n == 0` yields
///   `DIMENSIONLESS`). Each is wrong whenever the second operand is
///   dimensioned or `n != 1`. This corroborates #6943's ratified
///   reclassification of the three. They are in
///   [`EVAL_DEFERRED_BUILTIN_NAMES`] instead, which suppresses the
///   `UnresolvedFunction` warning identically while claiming only what is
///   true.
///
/// # Lifetime
///
/// **#6014 (registry omega) DELETES this family** together with the terminal
/// fallback itself, once every name here holds a real signature-registry row.
/// The allowlist is interim scaffolding for the warn-mode window, not a
/// permanent vocabulary — do not build on it. #6014's deletion note expects
/// sixteen names; it will find eight, and this comment is the reconciliation
/// it should read (it already says "verify each before deletion").
///
/// Case-sensitive: Reify function names are snake_case.
pub const FIRST_ARG_TYPED_NAMES: &[&str] = &[
    "project",
    "mod",
    "to_global",
    "effective_tolerance_zone",
    "input_shape_apply",
    "complex_add",
    "complex_exp",
    "complex_sqrt",
];

/// Eval-dispatchable names that are not yet family-registered, whose typing is
/// deliberately left to the terminal fallback.
///
/// # What membership asserts — and what it does NOT
///
/// An entry means exactly: *`reify_stdlib::eval_builtin` dispatches this name,
/// no compiler classification family claims it, and its static type is
/// therefore whatever the terminal first-arg fallback produces.* That claim is
/// **unconditionally true** of every name below — it is an observation about
/// the dispatch tables, not a judgement about the resulting type.
///
/// In particular it makes **no** assertion that the fallback types the name
/// *correctly*. That stronger, falsifiable claim belongs to
/// [`FIRST_ARG_TYPED_NAMES`], and the two must never be conflated: it is
/// precisely this distinction that lets `complex_mul` / `complex_div` /
/// `complex_pow` sit inside the closed world (so they do not warn at every
/// call site) without laundering their known-WRONG first-arg typing into an
/// allowlist. `tests::eval_deferred_names_are_disjoint_from_every_registered_family`
/// enforces that no name is in both.
///
/// # Derivation (2026-08-29, re-measured on this branch)
///
/// Walk every arm of the `eval_builtin` dispatch chain
/// (`reify-stdlib/src/lib.rs:225`) and its 25 per-module `eval_*(name: &str, ..)`
/// matchers, then subtract everything [`is_known_builtin`] already accepts.
/// 263 candidate spellings were screened; 119 were unclaimed; 37 of those were
/// **false positives** and are deliberately absent (see "Screened out" below),
/// leaving the 82 names here.
///
/// Every entry has a verified owning registry task — the manifest is a ledger
/// of live deferrals, not a graveyard. Groups below are by owning task.
///
/// # Screened out — names that look eval-dispatchable but never reach a call site
///
/// Re-adding any of these would suppress a warning that *should* fire, so the
/// reasons are recorded rather than left to be rediscovered:
///
/// * **Collection/tensor METHOD names** (`reify-expr/src/lib.rs:3864`
///   `eval_method_call`): `all`, `any`, `concat`, `contains_key`, `count`,
///   `filter`, `fold`, `keys`, `lower`, `map`, `span`, `sum`, `upper`,
///   `values`, and the datum-projection members `dir`, `origin`, `normal`,
///   `x`, `y`, `z`, `xy_plane`. These are `MethodCall`/`MemberAccess`
///   receivers, not `FunctionCall` callees, so the terminal fallback never
///   sees them.
/// * **Euler convention STRING literals** (`reify-stdlib/src/orientation.rs:75-86`):
///   `xyz`, `xzy`, `yxz`, `yzx`, `zxy`, `zyx`, `xyx`, `xzx`, `yxy`, `yzy`,
///   `zxz`, `zyz` — matched against an argument's string VALUE, never a callee.
/// * **DFM rule labels** (`reify-stdlib/src/dfm.rs`'s `diagnose(name, ..)`):
///   `unsupported_overhang_faces`, `min_draft_angle`, `min_wall_thickness`,
///   `min_feature_size_measure`. `diagnose` is a post-eval hook keyed by an
///   internal rule name supplied from `reify-eval/src/engine_constraints.rs:1750`;
///   none is dispatched by `eval_builtin`, and none appears as a callee in any
///   `.ri` source. (`fits_build_volume` IS `eval_dfm`'s only real arm and is
///   manifested below.)
/// * **The joint-KIND string `coupling`** (`reify-stdlib/src/joints.rs:420`,
///   a nested `match kind` inside `eval_joints`): the constructor spelling is
///   `couple`, which `JOINT_TYPED_FN_NAMES` already claims.
///
/// # Lifetime
///
/// Each group is discharged by its owning τ task writing real registry rows;
/// removing the group from this manifest is part of that task's diff, and
/// `eval_deferred_names_are_disjoint_from_every_registered_family` is what
/// turns a forgotten removal from a silent stale claim into a RED test.
///
/// Case-sensitive: Reify function names are snake_case.
pub const EVAL_DEFERRED_BUILTIN_NAMES: &[&str] = &[
    // --- numeric + trig — owner #6003 (registry τ1) / #6943 -----------------
    // Ratified semantics this task must NOT pre-empt: dimensionless-only
    // rulings for the hyperbolics and log10, a NEW 2-arg `floor(x, quantum)`
    // overload, and a compile diagnostic for a dimensioned argument. Listing
    // them here makes the deferral machine-visible without deciding any of it.
    // (`mod` is NOT here — it is in FIRST_ARG_TYPED_NAMES, whose stronger
    // claim holds for its `(Int, Int) -> Int` eval body.)
    "floor",
    "ceil",
    "round",
    "log10",
    "remap",
    "sinh",
    "cosh",
    "tanh",
    // --- complex — owner #6008 (registry τ6) / #6943 ------------------------
    // `re`/`im` are eval aliases of `real`/`imag` (complex.rs:65,73); only the
    // long spellings are in MATH_OPERATION_NAMES, so the aliases fall through.
    // The trio below is dimension-TRANSFORMING — see the exclusion note on
    // FIRST_ARG_TYPED_NAMES.
    "re",
    "im",
    "complex_mul",
    "complex_div",
    "complex_pow",
    // --- joint accessors — owner #6005 (registry τ3) / #6945 ----------------
    // Fallback-MISTYPED today: each is typed as its joint-StructureRef arg0.
    "transform_at",
    "joint_axis",
    "joint_range",
    "joint_ratio",
    "joint_offset",
    // --- orientation decomposers + BoundingBox — owner #6004 (registry τ2) --
    // The four decomposers return heterogeneous Maps that τ2 gives nominal
    // structures (`AxisAngle`, `Twist`); the bbox trio is ruled by #6081.
    "orient_log",
    "orient_to_axis_angle",
    "orient_to_euler",
    "transform_log",
    "bbox",
    "bbox_center",
    "bbox_size",
    // --- fea / flexures / stackup / dfm / tolerancing / loads / tensegrity --
    // --- owner #6006 (registry τ4) -----------------------------------------
    // The `std.fea` MultiCaseResult accessors (fea.rs:47-102) are the group
    // the printer_v01 dogfood surfaced; τ4 names every one of them explicitly.
    "case_names",
    "envelope_argmax",
    "envelope_argmin",
    "envelope_critical_load",
    "envelope_max",
    "envelope_min",
    "linear_combine",
    "min_max_stress",
    "result_for",
    "solve_load_cases",
    "worst_buckling_case",
    "worst_case",
    "prb_cantilever_beam",
    "prb_cartwheel_flexure",
    "prb_cross_spring_pivot",
    "prb_double_parallelogram_flexure",
    "prb_fixed_fixed_beam",
    "prb_let_joint",
    "prb_living_hinge",
    "prb_notch_circular",
    "prb_notch_elliptical",
    "prb_notch_right_circular",
    "prb_parallelogram_flexure",
    "prb_prismatic_blade",
    "prb_two_axis_pivot",
    "contributor",
    "contributor_asym",
    "stackup_worst_case",
    "stackup_rss",
    "monte_carlo_stackup",
    "fits_build_volume",
    "iso_it_tolerance",
    "gravity",
    "tensegrity_wires",
    "tensegrity_surfaces",
    // --- mechanism / snapshot / sweep / dynamics / trajectory ---------------
    // --- owner #6007 (registry τ5) -----------------------------------------
    // The `*_lower` / `*_at` spellings are the undeclared intrinsics the typed
    // `.ri` wrappers delegate to; τ5 registers the direct-eval names only.
    // `piecewise_polynomial` is a permanent `Value::Undef` stub that τ5
    // LEDGERS rather than types.
    "world",
    "bodies",
    "transform_of",
    "sweep_grid",
    "ramp_profile_lower",
    "inverse_dynamics_lower",
    "inverse_dynamics_at_snapshot_lower",
    "gcode_import",
    "gcode_import_lower",
    "input_shape",
    "end_effector_track_at",
    "deviation_from_nominal_at",
    "peak_deviation_at",
    "evaluate_profile",
    "evaluate_profile_at",
    "evaluate_profile_dot",
    "evaluate_profile_dot_at",
    "evaluate_profile_ddot",
    "evaluate_profile_ddot_at",
    "profile_duration",
    "profile_duration_at",
    "piecewise_polynomial",
];

/// Is `name` a builtin function name the compiler knows about *at all*?
///
/// Closed-world union over every classification family the `expr.rs`
/// `NoUserFunctions` ladder consults, plus the two manifests declared in this
/// module. A pure predicate: no allocation, no diagnostics.
///
/// # What this does NOT answer
///
/// Membership is a **name** fact only. A `true` answer says nothing about
/// whether a *particular call* type-checks, has the right arity, or is claimed
/// by the family that owns the name — several families are arg-aware and
/// return `None` for a mis-shaped call by design (`datum_constructor_result_type`'s
/// `offset` arity gate, `selector_composition_result_type`'s CSG fall-through,
/// `infer_list_helper_return_type`'s structural match, `field_op_result_type`,
/// `affine_map_algebra_result_type`).
///
/// Of those, the three covered by [`arg_shape_expectation`] — list-helper,
/// field-op and affine-algebra — are diagnosed at the fallback with
/// `DiagnosticCode::BuiltinArgShapeUnrecognized`. The datum-constructor and
/// selector-composition arms are NOT: their `None` is a deliberate hand-off to
/// a later ladder arm rather than a dead end, so warning on it would be false.
/// Bringing them in needs the same per-name reachability measurement the three
/// covered families got, which is #6014's to do when it deletes the fallback.
///
/// Case-sensitive — Reify function names are snake_case.
pub fn is_known_builtin(name: &str) -> bool {
    // --- The 19 name slices the ladder consults, in ladder order. ---
    GEOMETRY_QUERY_HELPER_NAMES.contains(&name)
        || GEOMETRY_KINEMATIC_QUERY_NAMES.contains(&name)
        || GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name)
        || RELATION_FN_NAMES.contains(&name)
        || GEOMETRY_QUERY_NAMES.contains(&name)
        || TOLERANCING_MARKER_NAMES.contains(&name)
        || GEOMETRY_FUNCTION_NAMES.contains(&name)
        || DYNAMICS_QUERY_NAMES.contains(&name)
        || DYNAMICS_CONSTRUCTOR_NAMES.contains(&name)
        || AFFINE_MAP_CONSTRUCTOR_NAMES.contains(&name)
        || MATH_CONSTRUCTION_NAMES.contains(&name)
        || MATH_OPERATION_NAMES.contains(&name)
        || MATH_TRANSCENDENTAL_NAMES.contains(&name)
        || JOINT_TYPED_FN_NAMES.contains(&name)
        || ANALYSIS_FN_NAMES.contains(&name)
        || FEA_ENVELOPE_NAMES.contains(&name)
        || FIELD_OP_NAMES.contains(&name)
        || PARSE_FN_NAMES.contains(&name)
        || ORIENTATION_TYPED_FN_NAMES.contains(&name)
        // --- The four resolver-only families, promoted to production slices
        // --- by this task so the union can see them (they were previously
        // --- visible only as `match` arms inside their resolvers).
        || DATUM_CONSTRUCTOR_NAMES.contains(&name)
        || SELECTOR_COMPOSITION_NAMES.contains(&name)
        || LIST_HELPER_NAMES.contains(&name)
        || AFFINE_ALGEBRA_NAMES.contains(&name)
        // --- Vocabularies that live outside any slice. ---
        //
        // The arity-gated shared verbs `angle`/`distance` are deliberately
        // absent from RELATION_FN_NAMES (their arity-2 DERIVE forms are
        // geometry queries), so the slices above do not reach them.
        || is_relation_shared_verb(name)
        // The determinacy predicates are a bare `match` in the ladder; #5371
        // promoted them to a slice for exactly this reason.
        || DETERMINACY_PREDICATE_NAMES.contains(&name)
        // --- This module's two manifests. ---
        || FIRST_ARG_TYPED_NAMES.contains(&name)
        || EVAL_DEFERRED_BUILTIN_NAMES.contains(&name)
}

/// The argument shape a mis-shaped builtin call was expected to have, or
/// `None` if `name` belongs to no **arg-aware** family.
///
/// # What a `Some` answer means *at the terminal fallback*
///
/// Three ladder arms read the compiled argument list and return `None` when
/// the name is theirs but the shape is not — `infer_list_helper_return_type`,
/// `affine_map_algebra_result_type`, and `field_op_result_type`. Their arms are
/// the ONLY route by which those names are claimed, so a call that has reached
/// the terminal fallback while `is_list_helper` / `is_affine_map_algebra_name`
/// / `is_field_op` still answers `true` is, **by construction**, a call whose
/// family recognised the name and declined the shape. That is what makes a
/// name-only lookup a sound arg-shape diagnosis at that one site — and why
/// this function must not be called anywhere else, where the same `Some` would
/// mean nothing more than "this name is in one of three families".
///
/// # Honesty of the strings
///
/// Each is the family's own declared parameter list, copied from the resolver's
/// signature table, so the label tells the user what the builtin wants rather
/// than restating what they wrote. It is an upper bound on the diagnosis: for
/// `curl`/`divergence`/`laplacian` a well-formed `Field` can still be declined
/// by `differential_codomain` on its domain/codomain rank, so "expects
/// `Field<D, C>`" is true but not the whole story. The label is phrased to say
/// the arguments do not match rather than to claim exactly which one is wrong.
///
/// # Affine members are unreachable here today
///
/// No `AFFINE_ALGEBRA_NAMES` member can currently reach the fallback:
/// `affine_compose`/`affine_inverse` answer `Some` from the name alone,
/// `determinant` is claimed by the LATER `is_math_typed_fn` arm and
/// `affine_apply` by the EARLIER `is_geometry_function` arm — the shadowing
/// documented on `AFFINE_ALGEBRA_NAMES` itself. They are listed anyway as
/// defense-in-depth against a ladder reorder, and
/// `affine_algebra_names_never_reach_the_terminal_fallback` (in
/// `tests/unresolved_function_tests.rs`) is the guard that makes such a reorder
/// visible instead of silent.
pub(crate) fn arg_shape_expectation(name: &str) -> Option<&'static str> {
    // Guarded by the family predicates rather than by this `match` alone, so a
    // name cannot acquire an expectation string without also being a member of
    // the family whose resolver declined it.
    if !(crate::list_helpers::is_list_helper(name)
        || crate::units::is_affine_map_algebra_name(name)
        || crate::units::is_field_op(name))
    {
        return None;
    }
    Some(match name {
        // list-helper — list_helpers.rs
        "single" => "(List<T>)",
        "flat_map" => "(List<A>, (A) -> List<B>)",
        "generate" => "(Int, (Int) -> B)",
        // field-op — units.rs, PRD §5.1 signature table
        "fn_field" => "((D) -> C)",
        "from_samples" => "(List<D>, List<C>, method)",
        "restrict" => "(Field<D, C>, Geometry)",
        "compose" => "(Field<B, C>, Field<A, B>)",
        "sample" => "(Field<D, C>, D)",
        "gradient" | "divergence" | "curl" | "laplacian" => "(Field<D, C>)",
        // affine-algebra — units.rs (unreachable today; see the note above)
        "affine_compose" => "(AffineMap(3), AffineMap(3))",
        "affine_inverse" | "determinant" => "(AffineMap(3))",
        "affine_apply" => "(AffineMap(3), Point3<Q>)",
        // Unreachable: the guard above admits only the three families, and
        // `arg_shape_expectation_covers_every_arg_aware_family_member` iterates
        // all three slices asserting each name lands on an arm above. A new
        // slice entry with no arm here reds that test rather than silently
        // emitting a shapeless label.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis_signatures::ANALYSIS_FN_NAMES;
    use crate::joint_signatures::JOINT_TYPED_FN_NAMES;
    use crate::math_signatures::{
        MATH_CONSTRUCTION_NAMES, MATH_OPERATION_NAMES, MATH_TRANSCENDENTAL_NAMES,
    };
    use crate::orientation_signatures::ORIENTATION_TYPED_FN_NAMES;
    use crate::parse_signatures::PARSE_FN_NAMES;
    use crate::relation_signatures::RELATION_FN_NAMES;
    use crate::units::{
        AFFINE_MAP_CONSTRUCTOR_NAMES, DYNAMICS_CONSTRUCTOR_NAMES, DYNAMICS_QUERY_NAMES,
        FEA_ENVELOPE_NAMES, FIELD_OP_NAMES, GEOMETRY_FUNCTION_NAMES,
        GEOMETRY_KINEMATIC_QUERY_NAMES, GEOMETRY_QUERY_HELPER_NAMES, GEOMETRY_QUERY_NAMES,
        GEOMETRY_TOPOLOGY_SELECTOR_NAMES, TOLERANCING_MARKER_NAMES,
    };

    /// Every name slice the `NoUserFunctions` ladder consults, paired with its
    /// identifier so a failure names the family that regressed.
    ///
    /// Nineteen families; each `*_are_disjoint_from_other_families` test in
    /// `units.rs` loops the other **18** (it excludes its own).
    const ALL_FAMILY_SLICES: &[(&str, &[&str])] = &[
        ("GEOMETRY_FUNCTION_NAMES", GEOMETRY_FUNCTION_NAMES),
        ("GEOMETRY_QUERY_HELPER_NAMES", GEOMETRY_QUERY_HELPER_NAMES),
        (
            "GEOMETRY_KINEMATIC_QUERY_NAMES",
            GEOMETRY_KINEMATIC_QUERY_NAMES,
        ),
        (
            "GEOMETRY_TOPOLOGY_SELECTOR_NAMES",
            GEOMETRY_TOPOLOGY_SELECTOR_NAMES,
        ),
        ("GEOMETRY_QUERY_NAMES", GEOMETRY_QUERY_NAMES),
        ("AFFINE_MAP_CONSTRUCTOR_NAMES", AFFINE_MAP_CONSTRUCTOR_NAMES),
        ("TOLERANCING_MARKER_NAMES", TOLERANCING_MARKER_NAMES),
        ("DYNAMICS_QUERY_NAMES", DYNAMICS_QUERY_NAMES),
        ("DYNAMICS_CONSTRUCTOR_NAMES", DYNAMICS_CONSTRUCTOR_NAMES),
        ("FEA_ENVELOPE_NAMES", FEA_ENVELOPE_NAMES),
        ("FIELD_OP_NAMES", FIELD_OP_NAMES),
        ("MATH_CONSTRUCTION_NAMES", MATH_CONSTRUCTION_NAMES),
        ("MATH_OPERATION_NAMES", MATH_OPERATION_NAMES),
        ("MATH_TRANSCENDENTAL_NAMES", MATH_TRANSCENDENTAL_NAMES),
        ("ANALYSIS_FN_NAMES", ANALYSIS_FN_NAMES),
        ("RELATION_FN_NAMES", RELATION_FN_NAMES),
        ("JOINT_TYPED_FN_NAMES", JOINT_TYPED_FN_NAMES),
        ("PARSE_FN_NAMES", PARSE_FN_NAMES),
        ("ORIENTATION_TYPED_FN_NAMES", ORIENTATION_TYPED_FN_NAMES),
    ];

    /// The four families promoted from resolver-only `match` arms to real
    /// slices by #5371 — kept separate from `ALL_FAMILY_SLICES` because the
    /// fifteen pre-existing `*_are_disjoint_from_other_families` tests in
    /// `units.rs` iterate the nineteen registered slices only.
    const RESOLVER_ONLY_FAMILY_SLICES: &[(&str, &[&str])] = &[
        ("DATUM_CONSTRUCTOR_NAMES", DATUM_CONSTRUCTOR_NAMES),
        ("SELECTOR_COMPOSITION_NAMES", SELECTOR_COMPOSITION_NAMES),
        ("LIST_HELPER_NAMES", LIST_HELPER_NAMES),
        ("AFFINE_ALGEBRA_NAMES", AFFINE_ALGEBRA_NAMES),
    ];

    /// `is_known_builtin` must accept EVERY member of EVERY classification
    /// family the `expr.rs` ladder consults — not a spot-check per family.
    ///
    /// Iterating each slice in full is what stops the oracle rotting: a name
    /// added to any family slice is covered the moment it lands, with no
    /// parallel edit here.
    ///
    /// The four **resolver-only** families (datum-constructor, affine-map
    /// algebra, list-helper, selector-composition) have no production slice to
    /// iterate at this point in the task, so they are covered by representative
    /// names; step-2 promotes real slices for them and step-3 pins those
    /// slices against their resolvers.
    #[test]
    fn is_known_builtin_recognises_every_compiler_family() {
        for (family, slice) in ALL_FAMILY_SLICES {
            for name in *slice {
                assert!(
                    is_known_builtin(name),
                    "{name:?} is in {family} but is_known_builtin rejects it"
                );
            }
        }

        // Resolver-only families — no production name slice exists yet.
        for name in [
            // datum-constructor (units.rs `datum_constructor_result_type`)
            "frame_at",
            "midplane",
            "plane_through",
            "axis_through",
            "plane_xy",
            "axis_x",
            // affine-map algebra (units.rs `affine_map_algebra_result_type`);
            // `determinant` / `affine_apply` are omitted here because they are
            // already claimed by MATH_OPERATION_NAMES / GEOMETRY_FUNCTION_NAMES.
            "affine_compose",
            "affine_inverse",
            // list-helper (list_helpers.rs `infer_list_helper_return_type`)
            "single",
            "flat_map",
            "generate",
            // selector composition (units.rs `selector_composition_result_type`);
            // `union` / `difference` are also CSG geometry functions, `intersect`
            // is selector-only.
            "union",
            "intersect",
            "difference",
        ] {
            assert!(
                is_known_builtin(name),
                "{name:?} is claimed by a resolver-only family but \
                 is_known_builtin rejects it"
            );
        }

        // Arity-gated shared verbs — deliberately absent from RELATION_FN_NAMES
        // (their arity-2 DERIVE forms are geometry queries), so the slice loop
        // above does not reach them via that family.
        for name in ["angle", "distance"] {
            assert!(
                crate::relation_signatures::is_relation_shared_verb(name),
                "premise guard: {name:?} is no longer a relation shared verb"
            );
            assert!(
                is_known_builtin(name),
                "{name:?} is a relation shared verb but is_known_builtin rejects it"
            );
        }

        // Determinacy predicates — hard-coded in the `expr.rs` ladder as a bare
        // `match`, with no slice anywhere.
        for name in [
            "determined",
            "undetermined",
            "constrained",
            "partially_determined",
        ] {
            assert!(
                is_known_builtin(name),
                "{name:?} is a determinacy predicate but is_known_builtin rejects it"
            );
        }
    }

    /// A hand-maintained name slice is only as good as its tie to the resolver
    /// it claims to describe. Step-2 gated each of the four resolver-only
    /// families ON its slice (a name cannot be in the `match` without being in
    /// the slice); this test pins the CONVERSE direction — every slice entry is
    /// really claimed by its resolver for a well-shaped call — so a stale entry
    /// cannot linger after the resolver arm is removed.
    ///
    /// The premise-guard idiom is copied from
    /// `units::tests::datum_constructor_names_are_disjoint_from_other_families`,
    /// which asserts `datum_constructor_result_type(name, &[]).is_some()`
    /// before its absence asserts for the same reason.
    #[test]
    fn resolver_only_family_slices_match_their_resolvers() {
        use reify_core::Type;
        use reify_core::ty::SelectorKind;
        use reify_ir::{CompiledExpr, Value};

        fn arg(ty: Type) -> CompiledExpr {
            CompiledExpr::literal(Value::Undef, ty)
        }

        // ---- Construction-datum constructors -----------------------------
        //
        // Arity 2 satisfies `offset`'s arity gate (units.rs); the other ten
        // members are arity-blind, so one arg vector serves all eleven.
        let datum_args = vec![arg(Type::Plane), arg(Type::length())];
        for name in DATUM_CONSTRUCTOR_NAMES {
            assert!(
                crate::units::datum_constructor_result_type(name, &datum_args).is_some(),
                "DATUM_CONSTRUCTOR_NAMES entry {name:?} is not claimed by \
                 datum_constructor_result_type at arity 2"
            );
        }
        assert_eq!(
            crate::units::datum_constructor_result_type("not_a_datum_ctor", &datum_args),
            None,
            "converse: a non-member must not be claimed"
        );
        // `offset` really is the arity-gated member — pin the gate so the
        // arity-2 fixture above is not silently testing an arity-blind name.
        assert_eq!(
            crate::units::datum_constructor_result_type("offset", &[]),
            None,
            "offset is a construction datum at arity 2 ONLY (arity 3 is a relation)"
        );

        // ---- AffineMap algebra -------------------------------------------
        //
        // Two members are first-arg-gated, so each name needs its OWN
        // well-shaped first arg: `affine_apply` wants a Point, the rest want an
        // AffineMap. A single shared fixture would silently under-test them.
        for name in AFFINE_ALGEBRA_NAMES {
            let first_arg = if *name == "affine_apply" {
                Type::point3(Type::length())
            } else {
                Type::AffineMap(3)
            };
            assert!(
                crate::units::affine_map_algebra_result_type(name, Some(&first_arg)).is_some(),
                "AFFINE_ALGEBRA_NAMES entry {name:?} is not claimed by \
                 affine_map_algebra_result_type for a well-shaped first arg"
            );
        }
        assert_eq!(
            crate::units::affine_map_algebra_result_type(
                "not_an_affine_op",
                Some(&Type::AffineMap(3))
            ),
            None,
            "converse: a non-member must not be claimed"
        );

        // ---- List helpers -------------------------------------------------
        //
        // Each helper has a different well-shaped arg vector; `generate` is the
        // entry the older test-only fixtures omitted, so it is exactly the drift
        // this loop catches.
        let list_of_int = Type::List(Box::new(Type::Int));
        let lambda_to_list = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::List(Box::new(Type::Bool))),
        };
        let lambda_to_int = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        };
        for name in LIST_HELPER_NAMES {
            let args = match *name {
                "single" => vec![arg(list_of_int.clone())],
                "flat_map" => vec![arg(list_of_int.clone()), arg(lambda_to_list.clone())],
                "generate" => vec![arg(Type::Int), arg(lambda_to_int.clone())],
                other => panic!(
                    "LIST_HELPER_NAMES gained {other:?} with no well-shaped arg \
                     fixture here — add one so the entry is really covered"
                ),
            };
            assert!(
                crate::list_helpers::infer_list_helper_return_type(name, &args).is_some(),
                "LIST_HELPER_NAMES entry {name:?} is not claimed by \
                 infer_list_helper_return_type for a well-shaped call"
            );
        }
        assert_eq!(
            crate::list_helpers::infer_list_helper_return_type("take", &[arg(list_of_int.clone())]),
            None,
            "converse: a non-member must not be claimed"
        );

        // ---- Selector composition -----------------------------------------
        //
        // Operand-shaped, not name-shaped: two Selector-typed operands satisfy
        // both the variadic union/intersect and the strictly-binary difference.
        let selector_args = vec![
            arg(Type::Selector(SelectorKind::Face)),
            arg(Type::Selector(SelectorKind::Face)),
        ];
        for name in SELECTOR_COMPOSITION_NAMES {
            let mut diags = Vec::new();
            let resolved = crate::units::selector_composition_result_type(
                name,
                &selector_args,
                reify_core::SourceSpan::new(0, 0),
                &mut diags,
            );
            assert!(
                resolved.is_some(),
                "SELECTOR_COMPOSITION_NAMES entry {name:?} is not claimed by \
                 selector_composition_result_type for two Selector operands"
            );
            assert!(
                diags.is_empty(),
                "well-shaped {name:?} composition should emit no diagnostics, got {diags:?}"
            );
        }
        let mut diags = Vec::new();
        assert_eq!(
            crate::units::selector_composition_result_type(
                "not_a_selector_op",
                &selector_args,
                reify_core::SourceSpan::new(0, 0),
                &mut diags,
            ),
            None,
            "converse: a non-member must not be claimed"
        );
    }

    /// The allowlist is pinned against an INDEPENDENT literal, not against the
    /// slice itself, so drift in EITHER direction fails — mirroring the
    /// `EXPECTED_NAMES` idiom in `orientation_signatures.rs`. A test that read
    /// `FIRST_ARG_TYPED_NAMES` back would pass for any content at all.
    ///
    /// Each of the eight was verified against its EVAL BODY, not inferred from
    /// its name; the per-name evidence table lives on the slice's doc comment.
    #[test]
    fn first_arg_typed_names_are_exactly_the_eight_verified_names() {
        const EXPECTED_NAMES: &[&str] = &[
            "project",
            "mod",
            "to_global",
            "effective_tolerance_zone",
            "input_shape_apply",
            "complex_add",
            "complex_exp",
            "complex_sqrt",
        ];
        assert_eq!(
            FIRST_ARG_TYPED_NAMES, EXPECTED_NAMES,
            "FIRST_ARG_TYPED_NAMES drifted. Every entry asserts the terminal \
             first-arg fallback types that name CORRECTLY — a claim that must \
             be re-verified against the eval body before a name is added, and \
             the doc comment's evidence table updated with it."
        );
    }

    /// The two ways an entry could be WRONG, pinned as explicit negatives with
    /// their reasons. Both were live candidates: the task's own brief listed
    /// all eight of these names among "16 FALLBACK-CORRECT" callees.
    #[test]
    fn first_arg_typed_names_exclude_the_already_claimed_and_the_dimension_transforming() {
        // (1) Already claimed by ORIENTATION_TYPED_FN_NAMES (#5344). These are
        // not fallback-correct or fallback-incorrect — the fallback never sees
        // them, because the orientation arm claims them first. Listing one
        // here would break that family's disjointness contract AND make an
        // unfalsifiable claim about dead code.
        for name in [
            "transform_inverse",
            "transform_compose",
            "orient_inverse",
            "orient_compose",
            "orient_slerp",
        ] {
            assert!(
                ORIENTATION_TYPED_FN_NAMES.contains(&name),
                "premise guard: {name:?} is no longer claimed by \
                 ORIENTATION_TYPED_FN_NAMES, so the exclusion below has lost \
                 its reason — re-derive before editing the allowlist"
            );
            assert!(
                !FIRST_ARG_TYPED_NAMES.contains(&name),
                "{name:?} must NOT be in FIRST_ARG_TYPED_NAMES — \
                 ORIENTATION_TYPED_FN_NAMES already claims it (#5344)"
            );
        }

        // (2) Dimension-TRANSFORMING, so first-arg typing is a KNOWN-FALSE
        // claim, not merely an unverified one:
        //   complex_mul  complex.rs:138  dimension = ad.mul(bd)
        //   complex_div  complex.rs:160  dimension = ad.div(bd)
        //   complex_pow  complex.rs:191  accumulates dim^n; n=0 -> DIMENSIONLESS
        // Each is wrong whenever the second operand is dimensioned (or n != 1).
        // They belong in EVAL_DEFERRED_BUILTIN_NAMES, whose claim is only
        // "eval-dispatchable, not yet family-registered" — unconditionally
        // true, and it suppresses the warning identically. That membership is
        // pinned by `eval_deferred_manifest_contains_the_known_deferred_names`.
        for name in ["complex_mul", "complex_div", "complex_pow"] {
            assert!(
                !FIRST_ARG_TYPED_NAMES.contains(&name),
                "{name:?} must NOT be in FIRST_ARG_TYPED_NAMES — it is \
                 dimension-transforming, so first-arg typing is wrong for it \
                 whenever the second operand is dimensioned"
            );
        }
    }

    /// The allowlist is one of the unions inside `is_known_builtin`, so every
    /// member must be visible to the oracle. Cheap, but it is what makes the
    /// allowlist actually SUPPRESS the `UnresolvedFunction` warning rather
    /// than merely document an intention.
    #[test]
    fn first_arg_typed_names_are_all_known_builtins() {
        for name in FIRST_ARG_TYPED_NAMES {
            assert!(
                is_known_builtin(name),
                "FIRST_ARG_TYPED_NAMES entry {name:?} is not accepted by \
                 is_known_builtin — the family is not wired into the union"
            );
        }
    }

    /// A manifest entry says "no family owns this name yet". The moment a
    /// family DOES own it, the entry becomes a lie — and, worse, a silent one:
    /// `is_known_builtin` would still return `true`, so nothing would surface
    /// the stale claim. This test is what forces the removal.
    ///
    /// It also rules out the subtler double-claim with `FIRST_ARG_TYPED_NAMES`:
    /// the two manifests make DIFFERENT claims about the same fallback (one
    /// says its typing is verified right, the other says its typing is simply
    /// unexamined), so a name in both would be asserting two things at once.
    #[test]
    fn eval_deferred_names_are_disjoint_from_every_registered_family() {
        for name in EVAL_DEFERRED_BUILTIN_NAMES {
            for (family, slice) in ALL_FAMILY_SLICES {
                assert!(
                    !slice.contains(name),
                    "EVAL_DEFERRED_BUILTIN_NAMES entry {name:?} is now claimed \
                     by {family} — remove it from the manifest; the deferral it \
                     records has been discharged"
                );
            }
            for (family, slice) in RESOLVER_ONLY_FAMILY_SLICES {
                assert!(
                    !slice.contains(name),
                    "EVAL_DEFERRED_BUILTIN_NAMES entry {name:?} is now claimed \
                     by {family} — remove it from the manifest"
                );
            }
            assert!(
                !FIRST_ARG_TYPED_NAMES.contains(name),
                "{name:?} is in BOTH manifests. They make different claims — \
                 FIRST_ARG_TYPED_NAMES asserts the fallback types it CORRECTLY, \
                 EVAL_DEFERRED_BUILTIN_NAMES asserts only that it is \
                 eval-dispatchable and unregistered. Pick one."
            );
            assert!(
                !crate::relation_signatures::is_relation_shared_verb(name),
                "{name:?} is an arity-gated relation shared verb"
            );
            assert!(
                !DETERMINACY_PREDICATE_NAMES.contains(name),
                "{name:?} in DETERMINACY_PREDICATE_NAMES"
            );
        }
    }

    /// The manifest must actually contain the deferrals we know about, or it
    /// is a closed world in name only — an unlisted eval-dispatchable name
    /// produces a false `UnresolvedFunction` warning at every call site.
    ///
    /// Two groups, with different owners:
    ///
    /// * the numeric + complex names owned by #6003 (registry tau1) and #6943,
    ///   whose ratified semantics this task must NOT pre-empt (dimensionless-only
    ///   rulings, a new 2-arg `floor(x, quantum)` overload, dimensioned-arg ->
    ///   compile diagnostic). Manifesting them makes the deferral
    ///   machine-visible without deciding anything for those tasks.
    /// * the `std.fea` MultiCaseResult accessors, name-dispatched in
    ///   `reify-stdlib/src/fea.rs`'s `eval_fea` and absent from every compiler
    ///   family. Owned by #6006 (registry τ4), which names every one of them
    ///   explicitly — an earlier draft of this comment called them un-owned,
    ///   which was wrong.
    #[test]
    fn eval_deferred_manifest_contains_the_known_deferred_names() {
        for name in [
            // --- #6003 / #6943: numeric ---
            "floor", "ceil", "round", "sinh", "cosh", "tanh", "log10",
            // --- #6943: dimension-transforming complex ---
            "complex_mul", "complex_div", "complex_pow",
            // --- un-owned: std.fea MultiCaseResult accessors ---
            "result_for", "case_names", "worst_case", "linear_combine",
            "envelope_max", "envelope_min", "min_max_stress",
        ] {
            assert!(
                EVAL_DEFERRED_BUILTIN_NAMES.contains(&name),
                "{name:?} is eval-dispatchable and claimed by no compiler \
                 family, so it MUST be manifested — otherwise every call site \
                 gets a false UnresolvedFunction warning"
            );
        }

        // Premise guard deferred from
        // `first_arg_typed_names_exclude_the_already_claimed_and_the_dimension_transforming`
        // (step-5, written before this manifest existed): the three
        // dimension-transforming complex names are excluded from the allowlist
        // ONLY because the manifest catches them instead. If that stopped
        // being true they would fall out of the closed world entirely and
        // warn at every call site.
        for name in ["complex_mul", "complex_div", "complex_pow"] {
            assert!(
                is_known_builtin(name),
                "{name:?} is excluded from FIRST_ARG_TYPED_NAMES as \
                 dimension-transforming; the manifest must still keep it \
                 inside the closed world"
            );
        }
    }

    /// Same wiring check as `first_arg_typed_names_are_all_known_builtins`,
    /// for the other manifest: membership is inert unless the union reads it.
    #[test]
    fn eval_deferred_names_are_all_known_builtins() {
        for name in EVAL_DEFERRED_BUILTIN_NAMES {
            assert!(
                is_known_builtin(name),
                "EVAL_DEFERRED_BUILTIN_NAMES entry {name:?} is not accepted by \
                 is_known_builtin — the manifest is not wired into the union"
            );
        }
    }

    /// The closed world must actually be closed: a name in no family at all is
    /// rejected. Without this the oracle could trivially satisfy the test above
    /// by returning `true` unconditionally.
    #[test]
    fn is_known_builtin_rejects_a_genuinely_nonexistent_name() {
        assert!(!is_known_builtin("definitely_not_a_reify_builtin_xyz"));
        assert!(!is_known_builtin("line_of_nonsense"));
    }

    // --- arg_shape_expectation: the three arg-aware families ---

    /// Every member of all three arg-aware slices has an expectation string.
    ///
    /// Iterates the slices themselves, not a hand-maintained fixture, so a
    /// name added to `LIST_HELPER_NAMES` / `FIELD_OP_NAMES` /
    /// `AFFINE_ALGEBRA_NAMES` without a parallel arm reds HERE rather than
    /// shipping a `BuiltinArgShapeUnrecognized` label with no shape in it — the
    /// same maintenance contract the three slices already carry towards their
    /// resolvers.
    #[test]
    fn arg_shape_expectation_covers_every_arg_aware_family_member() {
        for name in LIST_HELPER_NAMES
            .iter()
            .chain(FIELD_OP_NAMES.iter())
            .chain(AFFINE_ALGEBRA_NAMES.iter())
        {
            let expected = arg_shape_expectation(name).unwrap_or_else(|| {
                panic!(
                    "{name:?} is in an arg-aware family slice but has no arm in \
                     arg_shape_expectation — add one, or the fallback will label \
                     the diagnostic with no expected shape"
                )
            });
            assert!(
                expected.starts_with('(') && expected.ends_with(')'),
                "{name:?}'s expectation {expected:?} must be a parenthesised \
                 parameter list, so the label reads as a signature"
            );
        }
    }

    /// …and nothing else does. A name outside the three families must answer
    /// `None`, including a name that is a perfectly good builtin elsewhere.
    ///
    /// This is what keeps the `Some` answer meaningful at the fallback: it says
    /// "an arg-aware family declined this call", not "the compiler knows this
    /// name". `sqrt` is known, `volume` is known, and neither may acquire a
    /// shape expectation.
    #[test]
    fn arg_shape_expectation_rejects_names_outside_the_three_families() {
        for name in ["sqrt", "volume", "point3", "world", "mod", "floor"] {
            assert_eq!(
                arg_shape_expectation(name),
                None,
                "{name:?} is not in an arg-aware family and must have no shape \
                 expectation"
            );
            assert!(
                is_known_builtin(name),
                "premise: {name:?} is a known builtin, so the assertion above is \
                 about arg-awareness and not about membership"
            );
        }
        assert_eq!(
            arg_shape_expectation("definitely_not_a_reify_builtin_xyz"),
            None
        );
    }

    /// A `Some` answer implies `is_known_builtin` — the property the fallback's
    /// mutual exclusion between `BuiltinArgShapeUnrecognized` and
    /// `UnresolvedFunction` rests on, asserted here rather than assumed.
    ///
    /// It holds because all three arg-aware families contribute production
    /// slices to the union. If a future edit dropped one of those slices from
    /// `is_known_builtin`, the fallback would emit BOTH codes for one call and
    /// only this test would say why.
    #[test]
    fn a_shape_expectation_implies_the_name_is_known() {
        for name in LIST_HELPER_NAMES
            .iter()
            .chain(FIELD_OP_NAMES.iter())
            .chain(AFFINE_ALGEBRA_NAMES.iter())
        {
            assert!(
                is_known_builtin(name),
                "{name:?} has a shape expectation but is outside the closed \
                 world — the fallback would double-report it"
            );
        }
    }
}
