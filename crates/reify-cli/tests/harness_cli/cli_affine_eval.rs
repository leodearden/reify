//! End-to-end CLI tests for the AffineMap constructor eval integration (task β).
//!
//! The positive test (`affine_constructors.ri`) passes once the constructors and
//! their `Type::AffineMap(3)` registration land (steps 2–14). The two diagnostic
//! tests stay RED until the post-Undef `geometry_diagnose` hook is wired into
//! `reify-expr` (step-18): `affine_scale` returns `Value::Undef` for a zero or
//! dimensioned factor, but the warning only reaches stderr through that hook.

use crate::common;

/// `reify eval` on a structure that constructs two valid AffineMaps prints each
/// `affine_map(...)` value on stdout and exits 0 (no Error diagnostic).
///
/// A benign zero-arg-return-type Warning for `transform3_identity` and a missing
/// `module` declaration Warning may appear on stderr — we do NOT assert stderr is
/// empty here.
#[test]
fn eval_affine_constructors_prints_affine_maps() {
    let path = common::fixture_path("affine_constructors.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval affine_constructors.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // affine_scale(2.0, 1.0, 0.5) → diag(2, 1, 0.5), zero translation.
    assert!(
        stdout.contains(
            "affine_map(linear=[[2, 0, 0], [0, 1, 0], [0, 0, 0.5]], translation=[0, 0, 0])"
        ),
        "stdout should print the affine_scale AffineMap value; got: {stdout}"
    );
    // affine_from_transform(transform3_identity()) → identity AffineMap.
    assert!(
        stdout.contains(
            "affine_map(linear=[[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation=[0, 0, 0])"
        ),
        "stdout should print the identity AffineMap value for id; got: {stdout}"
    );
}

/// `reify eval` on a zero scale factor emits the degenerate (det=0) Warning on
/// stderr via the post-Undef geometry diagnose hook, and still exits 0 (Warning,
/// not Error).
#[test]
fn eval_affine_scale_zero_warns_degenerate() {
    let path = common::fixture_path("affine_scale_zero.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a zero factor is a Warning (not an Error), so reify eval should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Minimum disambiguating anchor: builtin name + failure cause, contiguous.
    // Deliberately stops before "produces a degenerate (det=0) non-invertible
    // map" — this warning carries no DiagnosticCode (reify-stdlib
    // `geometry::diagnose`), so its tail prose is the drift-prone part;
    // reify-eval's `geometry_ops` already words the same condition differently
    // ("scale dropped: factor=0 produces degenerate (zero-volume) geometry").
    assert!(
        stderr.contains("affine_scale dropped: factor=0"),
        "stderr should contain the affine_scale degenerate (factor=0) warning; got: {stderr}"
    );
    // Guards the fixture's `module affine_scale_zero` decl: without it,
    // W_MODULE_DECL_MISSING ("expected `module affine_scale_zero`") re-supplies
    // the "affine_scale" substring for free and weakens the anchor above.
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "affine_scale_zero.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// `reify eval` on a structure that composes a scale and shear, then computes
/// `determinant(composed)`, prints the volume factor 24 on stdout and exits 0.
///
/// det(affine_scale(2,3,4)) · det(affine_shear_xy(0.5)) = 24 · 1 = 24 (exact).
/// This is the §9 γ user-observable signal: algebra free-functions integrate
/// end-to-end through eval.
#[test]
fn eval_affine_algebra_determinant_prints_24() {
    let path = common::fixture_path("affine_algebra.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval affine_algebra.ri should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Check that the determinant cell `d` carries the exact value 24 (not a
    // coincidental substring of another number or a line/column in a diagnostic).
    // Format: "<Structure>.<cell> = <value>" — the same form the sibling
    // constructor test uses to anchor its affine_map(...) assertions.
    assert!(
        stdout.contains("AffineAlgebra.d = 24"),
        "stdout should contain 'AffineAlgebra.d = 24' (cell label anchors the determinant result); got:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// `reify eval` on a dimensioned scale factor emits the dimensionless-requirement
/// Warning on stderr via the post-Undef geometry diagnose hook, and still exits 0.
#[test]
fn eval_affine_scale_dimensioned_warns_dimensionless() {
    let path = common::fixture_path("affine_scale_dim.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a dimensioned factor is a Warning (not an Error), so reify eval should exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Minimum disambiguating anchor: builtin name + failure cause, contiguous.
    // Deliberately stops before "(Real); a dimensioned factor was dropped ..."
    // — this warning carries no DiagnosticCode (reify-stdlib
    // `geometry::diagnose`), so its tail prose is the drift-prone part.
    assert!(
        stderr.contains("affine_scale: scale factors must be dimensionless"),
        "stderr should contain the affine_scale dimensionless-requirement warning; got: {stderr}"
    );
    // Guards the fixture's `module affine_scale_dim` decl: without it,
    // W_MODULE_DECL_MISSING ("expected `module affine_scale_dim`") re-supplies
    // the "affine_scale" substring for free and weakens the anchor above.
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "affine_scale_dim.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

// ── units-length ζ (task 5747): the R12 exit-code signal (boundary row 8) ─────
//
// THE GENUINE 0 -> 1 FLIP, asserted where the user actually sees it. Unlike ζ's
// R8 half — whose fixtures already exited 1 pre-ζ, so its e2e suite asserts the
// DIAGNOSTIC only — this pair measures a real exit-code change: before the gate,
// `affine_translate(5kg, 0kg, 0kg)` exited 0 and printed
// `affine_map(…, translation=[5, 0, 0])`, silently discarding the MASS dimension
// and turning 5 kg into 5 METRES.

/// `reify eval` on a MASS translation exits 1 with the coded units Error on
/// stderr, via `reify_stdlib::geometry::diagnose`'s post-`Undef` hook.
#[test]
fn eval_affine_translate_mass_exits_1_with_a_units_error() {
    let path = common::fixture_path("affine_translate_mass.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "a non-LENGTH translation is an Error (not a Warning), so reify eval should \
         exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // A single CONTIGUOUS anchor: builtin name + arg name + expected + got, in
    // one span. Unlike the `affine_scale` warnings above, this message IS pinned
    // byte-for-byte at the unit level (reify-stdlib) and against
    // `ArgRejection::message` itself (the cross-crate drift guard in
    // reify-eval), so anchoring the whole span here costs nothing extra.
    assert!(
        stderr.contains("affine_translate: dx/dy/dz argument expects Length, got Mass Scalar"),
        "stderr should carry the ζ units rejection naming the builtin, the gesture \
         and the offending dimension; got: {stderr}"
    );
    // The migration hint is checked SEPARATELY so a drop of just the hint is
    // distinguishable from a reword of the base message.
    assert!(
        stderr.contains("pass a dimensioned length such as `5mm`"),
        "stderr should carry the migration hint; got: {stderr}"
    );
    // Guards the fixture's `module affine_translate_mass` decl: without it,
    // W_MODULE_DECL_MISSING ("expected `module affine_translate_mass`") re-supplies
    // the "affine_translate" substring for free and weakens the anchor above
    // (the trap task 6155 documented for affine_scale_dim.ri).
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "affine_translate_mass.ri declares its module, so no module-decl warning \
         should appear; got: {stderr}"
    );
}

/// The INSEPARABLE control: the same builtin with DIMENSIONED literals still
/// exits 0 and prints the identity-linear AffineMap with an SI-metre
/// translation.
///
/// Without it, the row above could pass for the wrong reason (the builtin broken
/// outright rather than gated). The printed form was verified against the real
/// `target/debug/reify` binary before being pinned — the value printer's number
/// formatting is the drift-prone part.
#[test]
fn eval_affine_translate_length_exits_0_and_prints_si_metres() {
    let path = common::fixture_path("affine_translate_length.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "a LENGTH translation must still build;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(
            "affine_map(linear=[[1, 0, 0], [0, 1, 0], [0, 0, 1]], translation=[0.005, 0, 0])"
        ),
        "stdout should print the AffineMap with the translation in SI metres; \
         got: {stdout}"
    );
}
