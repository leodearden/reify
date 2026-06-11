//! End-to-end CLI tests for the AffineMap constructor eval integration (task β).
//!
//! The positive test (`affine_constructors.ri`) passes once the constructors and
//! their `Type::AffineMap(3)` registration land (steps 2–14). The two diagnostic
//! tests stay RED until the post-Undef `geometry_diagnose` hook is wired into
//! `reify-expr` (step-18): `affine_scale` returns `Value::Undef` for a zero or
//! dimensioned factor, but the warning only reaches stderr through that hook.

mod common;

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
    assert!(
        stderr.contains("affine_scale") && stderr.contains("degenerate"),
        "stderr should contain the affine_scale degenerate (det=0) warning; got: {stderr}"
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
    assert!(
        stderr.contains("affine_scale") && stderr.contains("dimensionless"),
        "stderr should contain the affine_scale dimensionless-requirement warning; got: {stderr}"
    );
}
