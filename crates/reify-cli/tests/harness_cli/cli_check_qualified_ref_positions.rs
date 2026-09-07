//! End-to-end `reify check` gate: an UNBOUND qualified reference (one whose
//! qualifier is not a declared import binding) must stay LOUD in TYPE position
//! and in `sub` structure_name position (task #6499).
//!
//! This makes the "pre-ν loudness is per-POSITION, not blanket" claim on
//! `namespaced_name_text` (`crates/reify-syntax/src/ts_parser.rs`) EXECUTABLE.
//! Before this file the claim was prose plus a one-off manual `reify check`
//! run; task #5495's ~35 tests all stop at the parse/lowering surface
//! (`lower_namespaced_call` covers EXPRESSION position only) and none of them
//! executes the compiler, so every one of them stays green under a change that
//! silences these two diagnostics. These two do not.
//!
//! What that guards: task #5505 (ν) rewrites resolution around a
//! `name.contains('.')` discriminator. Any fixup there that softens either
//! diagnostic — downgrading it to a Warning, suppressing-when-dotted, or
//! routing it through a phase `check` does not consult — flips `reify check`
//! to exit 0 and/or drops the message, which these tests catch.
//!
//! VACUITY RULE — every case here asserts stderr TEXT, never exit status
//! alone. Measured: `reify check /nonexistent/zzz.ri` prints
//! `Error reading …: No such file or directory (os error 2)` and ALSO exits 1,
//! so a test asserting only `!status.success()` would pass vacuously the moment
//! its fixture were renamed, moved, or deleted — reintroducing, inside the
//! guard itself, exactly the silent-hole failure mode this task exists to
//! prevent. Pinning the DOTTED `obj.width` inside each message additionally
//! proves μ's dot-join encoding survived lowering into the diagnostic, rather
//! than a bare `obj`.
//!
//! The CLI surfaces diagnostic MESSAGE text, not the typed `DiagnosticCode`
//! (and the `sub` diagnostic carries no code at all), so substring matching is
//! the only available anchor at this surface — which is also the surrounding
//! `cli_check.rs` convention.

use crate::common;

/// TYPE position: `param p : obj.width = 5mm` where `obj` is undeclared.
///
/// Emitted by `crates/reify-compiler/src/guards.rs` as
/// `DiagnosticCode::UnresolvedType`.
#[test]
fn check_unbound_qualified_ref_in_type_position_is_loud() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("qualified_ref_undeclared_type.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for an unbound qualified ref in TYPE position.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unresolved type: obj.width"),
        "stderr should contain 'unresolved type: obj.width' — the load-bearing assertion: it pins that the undeclared qualifier is rejected at all, that the dotted form (not a bare 'obj') reached the diagnostic, and that a missing/renamed fixture fails here instead of passing vacuously.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
