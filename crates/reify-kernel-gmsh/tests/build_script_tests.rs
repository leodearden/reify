//! Tests that pin the `cargo:rerun-if-env-changed` directives in `build.rs`.
//!
//! Build scripts execute at cargo's compile phase; their stdout is consumed by
//! cargo itself and is NOT surfaced to the test harness. The closest practical
//! regression fence is a text-substring assertion over the build script source.
//!
//! Contract: once cargo has cached a build with `has_gmsh` un-set, installing
//! libgmsh later will NOT invalidate the cached build script run unless cargo
//! has been told the env-var inputs are relevant. These two directives are that
//! signal.

/// Both `GMSH_INCLUDE_DIR` and `GMSH_LIB_DIR` rerun-if-env-changed directives
/// must be present in the build script so cargo invalidates its cached run when
/// either env var is set or cleared.
#[test]
fn build_rs_declares_rerun_if_env_changed_for_gmsh_dirs() {
    let src = include_str!("../build.rs");

    assert!(
        src.contains("cargo:rerun-if-env-changed=GMSH_INCLUDE_DIR"),
        "build.rs must emit `cargo:rerun-if-env-changed=GMSH_INCLUDE_DIR` so that \
         installing libgmsh (and then setting GMSH_INCLUDE_DIR) invalidates the \
         cached build script run; without this line, cargo silently reuses a stale \
         build where `has_gmsh` was never set, even after libgmsh is installed"
    );

    assert!(
        src.contains("cargo:rerun-if-env-changed=GMSH_LIB_DIR"),
        "build.rs must emit `cargo:rerun-if-env-changed=GMSH_LIB_DIR` so that \
         installing libgmsh (and then setting GMSH_LIB_DIR) invalidates the \
         cached build script run; without this line, cargo silently reuses a stale \
         build where `has_gmsh` was never set, even after libgmsh is installed"
    );
}
