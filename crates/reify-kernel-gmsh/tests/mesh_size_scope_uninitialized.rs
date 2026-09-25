//! `MeshSizeScope::entered` refuses an uninitialized gmsh library.
//!
//! Lives in its OWN integration-test binary, and holds exactly one test, for
//! the reason the test needs: it must run against a process in which
//! `init::ensure_initialized` has NEVER been called. Gmsh's initialisation is
//! process-wide and `ensure_initialized`'s `OnceLock` is sticky, so a single
//! sibling test in the same binary — in any order, on any thread — would
//! initialise the library out from under this one and turn the assertion below
//! green for the wrong reason.
//!
//! # What it is worth
//!
//! `MeshSizeScope::entered` is fallible, and its docstring argues that the
//! `Result` must be REACHABLE: "a type whose whole claim is that its numbers
//! are measured cannot carry a failure path nothing can take". Every
//! production call site initialises first, so without this binary that
//! argument is recorded in prose and the `observed != default` arm — and the
//! diagnostic it builds — are dead code on a green tree. This is the one
//! failure the docstring says can actually happen, made falsifiable.
//!
//! It also pins the MEASURED library behaviour the whole read-back design
//! rests on: pre-init, `gmshOptionSetNumber` and `gmshOptionGetNumber` both
//! report `ierr = 0` while changing and returning nothing, so a set-only
//! `entered` would return `Ok` having established nothing.
//!
//! # Measured RED
//!
//! Not green on arrival by argument — by measurement. With the read-back
//! removed from `MeshSizeScope::entered` (`let observed = default;` in place of
//! the `option_get_number` call), leaving the writes and the `Result` intact,
//! this test FAILS on the `Ok(_)` arm: libgmsh 4.15.2 accepted all five
//! pre-init writes with `ierr = 0` and the scope reported a table it had not
//! established. Armed, the same run logs `Error : Gmsh has not been
//! initialized` four times — one set/get pair for `Mesh.MeshSizeMin`, whose
//! default `0.0` happens to match the getter's untouched out-param seed, then
//! the pair for `Mesh.MeshSizeMax`, where `0.0` against `1e22` is the mismatch
//! that fires.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds (no `/opt/reify-deps`) the file is empty and this test binary
//! contains zero tests — preserving the all-OK posture of `cargo test
//! -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::init;
use reify_kernel_gmsh::mesh_size_scope::MeshSizeScope;

/// Entering a scope without `init::ensure_initialized()` fails, with a
/// diagnostic that names the cause.
///
/// Takes `GMSH_LOCK` raw rather than through `init::lock()` — the house idiom
/// for a `tests/` binary that needs the serialisation and nothing else — and
/// deliberately does NOT initialise. The error must be the scope's own
/// read-back arm rather than a propagated FFI failure, because a pre-init
/// `gmshOptionSetNumber` reports success; if this assertion ever fails on the
/// message rather than on the `Err`, the library's pre-init behaviour has
/// changed and `MeshSizeScope::entered`'s "Why each write is read back"
/// docstring is the thing to re-measure.
#[test]
fn entered_refuses_an_uninitialized_gmsh_rather_than_reporting_a_table_it_never_established() {
    let guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    let refused = MeshSizeScope::entered(&guard);

    let message = match refused {
        Err(e) => format!("{e:?}"),
        Ok(_) => panic!(
            "MeshSizeScope::entered must not report success against an uninitialized gmsh: \
             it writes gmsh's size defaults and reads each one back precisely so a table it \
             failed to establish is reported rather than assumed. A pass here means either \
             the read-back was dropped, or gmsh now initialises itself on first option \
             write — re-measure `MeshSizeScope::entered`'s \"Why each write is read back\"",
        ),
    };

    assert!(
        message.contains("MeshSizeScope::entered"),
        "the refusal must come from the scope's own read-back rather than from a \
         propagated FFI error: a pre-init gmshOptionSetNumber reports ierr = 0, so an \
         Err from the WRITE would mean the measured basis for reading each value back \
         has changed. Got: {message}",
    );
    assert!(
        message.contains("ensure_initialized"),
        "the refusal must name the actionable cause, since arming a scope before \
         init::ensure_initialized() is the one way to reach it from this crate. \
         Got: {message}",
    );
}
