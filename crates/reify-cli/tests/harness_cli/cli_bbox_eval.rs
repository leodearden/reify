//! End-to-end CLI tests for the BoundingBox dimension rejection (task 6081).
//!
//! A bounding box is spatial by construction: `bbox` admits only
//! `Point3<Length>` corners. `point3` is dimension-POLYMORPHIC at eval and has
//! no compile-layer signature row, so `point3(0deg, 0deg, 0deg)` really does
//! reach the `bbox` arm — where it used to SUCCEED (the only gate was "both
//! corners agree") and silently produce a quantity-polymorphic BoundingBox.
//!
//! These tests pin the user-observable half of the ruling: the rejection is
//! reported with the builtin name and the offending dimension, rather than as a
//! silent `Value::Undef`.

use crate::common;

/// `reify eval` on an Angle-cornered `bbox` emits the dimension-rejection Error
/// on stderr via the post-Undef `geometry_diagnose` hook.
///
/// EXIT CODE: 1, determined empirically (`reify eval
/// crates/reify-cli/tests/fixtures/bbox_angle_dim.ri` → exit 1). This differs
/// from the sibling `affine_scale` fixtures, which exit 0: those emit a
/// `Severity::Warning` (drop-and-continue — the offending factor is discarded
/// and evaluation proceeds), whereas a non-Length bbox corner is an outright
/// construction failure and is a `Severity::Error`.
#[test]
fn eval_bbox_angle_corner_errors_naming_the_dimension() {
    let path = common::fixture_path("bbox_angle_dim.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        !status.success(),
        "an Angle bbox corner is an Error (not a Warning), so reify eval should \
         exit non-zero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    // ONE contiguous anchor carrying the builtin name, the offending corner, the
    // expected quantity AND the offending dimension — so no other diagnostic on
    // stderr can supply part of it for free. Deliberately stops before the
    // trailing "(a bounding box is spatial by construction)" parenthetical, which
    // is the drift-prone prose tail; the anchored span is the
    // `ArgRejection::message`-shaped core.
    assert!(
        stderr.contains("bbox: min argument expects Point3<Length>, got Point3<Angle>"),
        "stderr should contain the bbox dimension-rejection error naming both the \
         expected Length and the offending Angle; got: {stderr}"
    );
    // Guards the fixture's `module bbox_angle_dim` decl: without it,
    // W_MODULE_DECL_MISSING ("expected `module bbox_angle_dim`") re-supplies the
    // "bbox" substring for free and weakens the anchor above.
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "bbox_angle_dim.ri declares its module, so no module-decl warning should \
         appear; got: {stderr}"
    );
}

/// Regression guard: the only real `.ri` consumer of `bbox` is metre-valued and
/// must stay entirely clean of the new dimension diagnostic.
///
/// `examples/differential_field_ops.ri` calls `bbox(point3(0.0m, 0.0m, 0.0m),
/// point3(4.0m, 0.0m, 0.0m))` twice. It DOES emit unrelated warnings (a
/// module-decl warning and a shell-solve fallback warning), so stderr is not
/// asserted empty — only that no bbox dimension rejection appears.
#[test]
fn eval_metre_valued_bbox_example_is_free_of_dimension_diagnostics() {
    let path = common::example_path("differential_field_ops.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval differential_field_ops.ri should still exit 0;\nstdout: \
         {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("bbox: min argument expects"),
        "a metre-valued bbox must not be rejected on dimension grounds; got: {stderr}"
    );
    assert!(
        !stderr.contains("bbox: max argument expects"),
        "a metre-valued bbox must not be rejected on dimension grounds; got: {stderr}"
    );
}
