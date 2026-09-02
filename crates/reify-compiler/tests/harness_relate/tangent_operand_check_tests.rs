//! End-to-end `reify check` enforcement tests for `tangent`'s operand pair and
//! radius arity — geometric-relations tangent (task 5540).
//!
//! `tangent` is the one relation whose legality is a property of the operand
//! PAIR, not of either slot alone: `(Axis, Plane)` is a cylinder on a plane,
//! `(Plane, Plane)` is two planes that never touch. That does not fit
//! `relation_operand_datum`'s one-`ExpectedDatum`-policed-across-both-slots
//! table, so tangent gets its own arm in `check_relation_arg_types` emitting a
//! new `DiagnosticCode::TangentOperandsUnsupported`.
//!
//! **This gate exists to remove a SILENT no-solve.** Before this task the
//! residual layer had no tangent arm, so a `tangent(...)` call contributed zero
//! Jacobian rows; `partition_driving_set` then filed the relation as *redundant*
//! with `rank_contribution: 0`, and post-solve `max_relation_residual` read
//! `0.0` — a tangency request that was wholly ignored and reported as satisfied.
//! An unsupported combo cannot fail loudly at the residual layer (an unhandled
//! shape there is indistinguishable from a satisfied one), so compile time is
//! the only layer that can reject it. With this gate the residual layer's
//! unsupported arm is unreachable from `.ri`.
//!
//! The four supported combos, and the radius arity each requires (one radius per
//! CURVED surface — radii travel as trailing `Scalar` operands because the
//! kernel's analytic-surface projection discards them; the surface-carried
//! `<HasAxis & HasRadius>` form is sibling task #5588):
//!
//! | combo             | call                          | ΔDOF |
//! |-------------------|-------------------------------|------|
//! | cylinder/cylinder | `tangent(Axis, Axis, r1, r2)` | 1    |
//! | cylinder/plane    | `tangent(Axis, Plane, r)`     | 2    |
//! | sphere/plane      | `tangent(Point, Plane, r)`    | 1    |
//! | sphere/sphere     | `tangent(Point, Point, r1, r2)` | 1  |
//!
//! Safe to police: the one pre-existing `.ri` caller is
//! `solver_unification_tangent_silent_accept.ri` under `tests/prd-gate/fixtures/`
//! (spelled split, NOT as one path — `test_verify_scope.sh`'s PG-DRIFT scenario
//! derives "read by a compiled test target" from a comment-inclusive grep for
//! `<that dir>/<name>.ri` over every tracked `*.rs`, so writing the joined path
//! in this very sentence would assert the opposite of what the sentence says),
//! a PRD-evidence probe for this very silent no-solve — it calls
//! `tangent(Axis, Axis)` and `tangent(Plane, Plane)`, both at arity 2, and pins
//! today's exit-0 `All constraints satisfied.` This gate deliberately turns both
//! into typed rejections: an arity error (cylinder/cylinder needs two radii) and
//! an unsupported-combo error. That probe has no automated consumer — no
//! probe-set entry, no Rust test target reads it, and it is in neither
//! `_RUST_COUPLED_RI_FIXTURES` nor `_GUI_COUPLED_RI_FIXTURES`
//! (`scripts/verify.sh`); the GUI grammar corpus walk sweeps the directory but
//! asserts only over its pinned `EXPECTED_CLEAN` list, which omits this file.
//! Its PRD (geometry-algebra-solver-unification, signal B1, task #6669) asks for
//! `E_RELATION_NOT_LOWERABLE` specifically, so satisfying B1's exact-code
//! assertion remains that task's job, not this one's.
//!
//! RED until the `TangentOperandsUnsupported` variant + the tangent arm land —
//! the file fails to compile against the missing variant, the established
//! RED-by-missing-symbol convention in this suite (see
//! `relate_block_check_tests.rs`'s header).

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// Wrap `members` in a minimal `structure S { … }` and compile with the full
/// stdlib prelude (so dimensioned literals like `5mm` / `30deg` resolve),
/// mirroring `relation_check_tests.rs`'s helper.
fn compile_structure(members: &str) -> reify_compiler::CompiledModule {
    let source = format!("structure S {{\n{members}\n}}");
    compile_source_with_stdlib(&source)
}

/// The error-severity `TangentOperandsUnsupported` diagnostics emitted while
/// compiling `module` — the unsupported-combo signal.
fn combo_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TangentOperandsUnsupported)
                && d.severity == Severity::Error
        })
        .collect()
}

/// The error-severity `ArgTypeMismatch` diagnostics — the UNIT layer's signal,
/// asserted separately from `combo_errors` so a wrong-dimension radius and an
/// unsupported operand pair stay distinguishable failure kinds.
fn unit_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect()
}

/// Every message emitted, for failure output.
fn all_messages(module: &reify_compiler::CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect()
}

// ── (a) The acceptance case: an unsupported pair is rejected, naming BOTH ─────

/// ACCEPTANCE — an operand pair with no well-defined tangency draws
/// `TangentOperandsUnsupported`, and the message NAMES BOTH operand kinds so the
/// author can see which slot is wrong. A `Direction` carries no position, so a
/// `(Direction, Direction)` pair has no tangency at any radius.
#[test]
fn unsupported_operand_pair_is_rejected_naming_both_kinds() {
    let module = compile_structure(
        "    param d1 : Direction\n    param d2 : Direction\n    \
         let r = tangent(d1, d2, 5mm)\n",
    );
    let errs = combo_errors(&module);
    assert!(
        !errs.is_empty(),
        "tangent(Direction, Direction, 5mm) must emit E_TANGENT_OPERANDS_UNSUPPORTED.\n\
         All diagnostics: {:#?}",
        all_messages(&module)
    );
    // Both operand kinds are named — asserted as substrings, not on the whole
    // string, so the wording can evolve without re-pinning the message.
    let msg = &errs[0].message;
    assert!(
        msg.contains("Direction"),
        "the message must NAME the offending operand kinds, got: {msg}"
    );
}

/// A `Frame` operand is likewise not a tangency surface, and the message names
/// the `Frame` kind. Pins that the diagnostic renders each operand kind through
/// the same `format_relation_arg_ty` the rest of the relation family uses (so a
/// `Frame(3)` reads as `Frame`, not `Frame(3)`).
#[test]
fn frame_operand_is_rejected_and_named() {
    let module = compile_structure(
        "    param f1 : Frame\n    param ax : Axis\n    let r = tangent(f1, ax, 5mm)\n",
    );
    let errs = combo_errors(&module);
    assert!(
        !errs.is_empty(),
        "tangent(Frame, Axis, 5mm) must emit E_TANGENT_OPERANDS_UNSUPPORTED.\n\
         All diagnostics: {:#?}",
        all_messages(&module)
    );
    let msg = &errs[0].message;
    assert!(
        msg.contains("Frame") && msg.contains("Axis"),
        "the message must name BOTH operand kinds (Frame and Axis), got: {msg}"
    );
}

/// The diagnostic TEACHES the vocabulary rather than only rejecting: it lists the
/// supported combos, so an author who reached for an unsupported pair learns
/// which pairs exist without opening the design doc.
#[test]
fn unsupported_combo_message_lists_the_supported_combos() {
    let module = compile_structure(
        "    param d1 : Direction\n    param d2 : Direction\n    \
         let r = tangent(d1, d2, 5mm)\n",
    );
    let errs = combo_errors(&module);
    assert!(!errs.is_empty(), "precondition: the combo must be rejected");
    let msg = &errs[0].message;
    for expected in [
        "cylinder/cylinder",
        "cylinder/plane",
        "sphere/plane",
        "sphere/sphere",
    ] {
        assert!(
            msg.contains(expected),
            "the message must list the supported combo {expected:?} so the diagnostic \
             teaches the vocabulary, got: {msg}"
        );
    }
}

// ── (b) The four supported combos compile CLEAN ──────────────────────────────

/// Every supported combo — in BOTH operand orders where the combo is asymmetric —
/// compiles with no `TangentOperandsUnsupported` and no `ArgTypeMismatch`. This is
/// the false-positive guard: a checker that rejected a legal combo would make the
/// relation unusable, which is worse than the silent no-solve it replaces.
#[test]
fn every_supported_combo_compiles_clean() {
    let cases: [(&str, &str); 6] = [
        // (description, structure members)
        (
            "cylinder/cylinder",
            "    param a : Axis\n    param b : Axis\n    let r = tangent(a, b, 5mm, 7mm)\n",
        ),
        (
            "cylinder/plane",
            "    param a : Axis\n    param p : Plane\n    let r = tangent(a, p, 5mm)\n",
        ),
        (
            "plane/cylinder (reversed)",
            "    param a : Axis\n    param p : Plane\n    let r = tangent(p, a, 5mm)\n",
        ),
        (
            "sphere/plane",
            "    param c : Point3<Length>\n    param p : Plane\n    let r = tangent(c, p, 5mm)\n",
        ),
        (
            "plane/sphere (reversed)",
            "    param c : Point3<Length>\n    param p : Plane\n    let r = tangent(p, c, 5mm)\n",
        ),
        (
            "sphere/sphere",
            "    param c1 : Point3<Length>\n    param c2 : Point3<Length>\n    \
             let r = tangent(c1, c2, 5mm, 7mm)\n",
        ),
    ];
    for (what, members) in cases {
        let module = compile_structure(members);
        assert!(
            combo_errors(&module).is_empty(),
            "{what} is a SUPPORTED tangency combo and must compile clean, but drew \
             E_TANGENT_OPERANDS_UNSUPPORTED.\nAll diagnostics: {:#?}",
            all_messages(&module)
        );
        assert!(
            unit_errors(&module).is_empty(),
            "{what} carries correctly-dimensioned Length radii and must draw no \
             E_ARG_TYPE_MISMATCH.\nAll diagnostics: {:#?}",
            all_messages(&module)
        );
    }
}

// ── (c) Two planes have no tangency ──────────────────────────────────────────

/// `tangent(plane_a, plane_b, 5mm)` is rejected: two planes are either parallel
/// (never touching) or intersecting (touching along a whole line) — neither is
/// tangency. This is the combo most likely to be reached for by analogy with
/// `flush`/`offset`, which DO take two planes, so it must be rejected explicitly
/// rather than silently accepted.
#[test]
fn two_planes_have_no_tangency() {
    let module = compile_structure(
        "    param pa : Plane\n    param pb : Plane\n    let r = tangent(pa, pb, 5mm)\n",
    );
    assert!(
        !combo_errors(&module).is_empty(),
        "tangent(Plane, Plane, 5mm) must be rejected — two planes have no tangency.\n\
         All diagnostics: {:#?}",
        all_messages(&module)
    );
}

// ── (d) Arity policing: one radius per CURVED surface ────────────────────────

/// A bare `tangent(a, b)` with NO radius is rejected for every combo. A tangency
/// without a radius is not under-specified in a recoverable way — it is the
/// silent no-solve this task exists to remove, since the residual layer would
/// find no scalar and return zero rows.
#[test]
fn tangent_without_a_radius_is_rejected() {
    let cases: [(&str, &str); 4] = [
        (
            "cylinder/cylinder",
            "    param a : Axis\n    param b : Axis\n    let r = tangent(a, b)\n",
        ),
        (
            "cylinder/plane",
            "    param a : Axis\n    param p : Plane\n    let r = tangent(a, p)\n",
        ),
        (
            "sphere/plane",
            "    param c : Point3<Length>\n    param p : Plane\n    let r = tangent(c, p)\n",
        ),
        (
            "sphere/sphere",
            "    param c1 : Point3<Length>\n    param c2 : Point3<Length>\n    \
             let r = tangent(c1, c2)\n",
        ),
    ];
    for (what, members) in cases {
        let module = compile_structure(members);
        assert!(
            !combo_errors(&module).is_empty(),
            "{what}: a radius-less tangent(a, b) must be rejected.\n\
             All diagnostics: {:#?}",
            all_messages(&module)
        );
    }
}

/// The single-radius combos require EXACTLY 3 args: a second radius is rejected,
/// because a plane has no radius for it to name.
#[test]
fn single_radius_combos_reject_a_second_radius() {
    for (what, members) in [
        (
            "cylinder/plane",
            "    param a : Axis\n    param p : Plane\n    let r = tangent(a, p, 5mm, 7mm)\n",
        ),
        (
            "sphere/plane",
            "    param c : Point3<Length>\n    param p : Plane\n    \
             let r = tangent(c, p, 5mm, 7mm)\n",
        ),
    ] {
        let module = compile_structure(members);
        assert!(
            !combo_errors(&module).is_empty(),
            "{what} takes exactly one radius (the plane has none) — a 4-arg call \
             must be rejected.\nAll diagnostics: {:#?}",
            all_messages(&module)
        );
    }
}

/// The two-radius combos require EXACTLY 4 args: a single radius is rejected,
/// because it cannot say which of the two curved surfaces it belongs to.
#[test]
fn two_radius_combos_reject_a_single_radius() {
    for (what, members) in [
        (
            "cylinder/cylinder",
            "    param a : Axis\n    param b : Axis\n    let r = tangent(a, b, 5mm)\n",
        ),
        (
            "sphere/sphere",
            "    param c1 : Point3<Length>\n    param c2 : Point3<Length>\n    \
             let r = tangent(c1, c2, 5mm)\n",
        ),
    ] {
        let module = compile_structure(members);
        assert!(
            !combo_errors(&module).is_empty(),
            "{what} takes TWO radii (one per curved surface) — a 3-arg call must be \
             rejected.\nAll diagnostics: {:#?}",
            all_messages(&module)
        );
    }
}

// ── (e) The radius slot's DIMENSION is policed ───────────────────────────────

/// A radius slot carrying the wrong physical dimension draws `ArgTypeMismatch`
/// (the UNIT layer's existing code, matching how `relation_metric_slot`'s
/// consumers behave) — NOT `TangentOperandsUnsupported`. The two failure kinds
/// are asserted separately so they stay distinguishable: an angle in a radius
/// slot is a unit error, not an unsupported geometry pairing.
#[test]
fn radius_slot_with_the_wrong_dimension_is_a_unit_mismatch() {
    let module = compile_structure(
        "    param a : Axis\n    param p : Plane\n    let r = tangent(a, p, 30deg)\n",
    );
    assert!(
        !unit_errors(&module).is_empty(),
        "tangent(Axis, Plane, 30deg) must emit E_ARG_TYPE_MISMATCH — a radius is a \
         Length, not an Angle.\nAll diagnostics: {:#?}",
        all_messages(&module)
    );
    assert!(
        combo_errors(&module).is_empty(),
        "(Axis, Plane) IS a supported combo — a wrong-dimension radius must NOT be \
         reported as an unsupported operand pair.\nAll diagnostics: {:#?}",
        all_messages(&module)
    );
}

/// The SECOND radius slot of a two-radius combo is policed too — a checker that
/// only looked at slot 2 would let `tangent(a, b, 5mm, 30deg)` through.
#[test]
fn second_radius_slot_dimension_is_policed_too() {
    let module = compile_structure(
        "    param a : Axis\n    param b : Axis\n    let r = tangent(a, b, 5mm, 30deg)\n",
    );
    assert!(
        !unit_errors(&module).is_empty(),
        "tangent(Axis, Axis, 5mm, 30deg) must emit E_ARG_TYPE_MISMATCH for the SECOND \
         radius slot.\nAll diagnostics: {:#?}",
        all_messages(&module)
    );
}
