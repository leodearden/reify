//! CLI integration gate for `reify check` on the committed datum-projection
//! example (task 4382 β / PRD docs/prds/v0_6/geometric-relations.md §9 β).
//!
//! `examples/datum_projections.ri` exercises only the VALID datum projections
//! (`axis.dir`/`.origin`, `plane.normal`/`.origin`,
//! `frame.x`/`.y`/`.z`/`.origin`/`.xy_plane`, and `direction` component
//! access). `reify check` must accept the file cleanly — exit 0 with zero
//! `error:` diagnostics. This pins the user-observable β signal end-to-end:
//! `resolve_type_name` datum names + first-class `Direction` + datum-projection
//! member-access type-checking.
//!
//! RED (step-13): the example file does not exist yet, so `reify check` fails;
//! step-14 creates it.

mod common;

#[test]
fn check_datum_projections_example_is_clean() {
    let path = common::example_path("datum_projections.ri");
    let (status, stdout, stderr) = common::run_with_args(&["check", &path]);

    assert!(
        status.success(),
        "reify check should exit 0 on the valid datum-projection example.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("error:") && !stderr.contains("error:"),
        "reify check should emit no 'error:' diagnostics on the valid example.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
