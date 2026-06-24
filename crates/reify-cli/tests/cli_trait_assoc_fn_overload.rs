//! CLI integration gate for `reify check` on the trait assoc-fn overload example
//! (task ε #3943, PRD docs/prds/v0_6/trait-associated-functions.md §5.3/§7.2/§8).
//!
//! Asserts that `examples/trait_assoc_fn_overload.ri` (committed in step-8):
//!   - exits 0 under `reify check`,
//!   - emits no `"error:"` diagnostics.
//!
//! RED until step-8 creates the example file.

mod common;

use std::path::Path;

/// `reify check examples/trait_assoc_fn_overload.ri` must succeed with no errors.
///
/// The example demonstrates intra-trait overload dispatch (two `fn f` overloads
/// in one trait, resolved by param type) and two-trait same-name disambiguation
/// (two traits each providing a default `fn f`, consumed with explicit trait
/// qualifier).  Both patterns must compile cleanly.
///
/// RED until step-8: the example file does not exist yet (created in step-8).
#[test]
fn check_trait_assoc_fn_overload_example_succeeds() {
    let path = common::example_path("trait_assoc_fn_overload.ri");
    assert!(
        Path::new(&path).exists(),
        "example file not found at {path} — create it in step-8 (#3943)"
    );

    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check should exit 0 for trait_assoc_fn_overload.ri.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr must contain no 'error:' diagnostics.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
