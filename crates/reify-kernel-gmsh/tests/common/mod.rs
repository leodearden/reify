//! What this crate's test binaries share: a re-export shim over the
//! workspace-canonical fixtures in [`reify_test_support::fixtures`], plus the
//! raw-FFI entity census [`entity_census`].
//!
//! A `tests/common/` subdirectory (rather than a sibling `tests/*.rs` file) is
//! the cargo idiom for both: files under `tests/` are each compiled as their
//! own test binary, files under `tests/common/` are not.
//!
//! # The fixture re-exports are a shim, and will shrink away
//!
//! Task #6387 hoisted every fixture below into `reify_test_support::fixtures`,
//! now the single definition for the whole workspace. The `pub use` list
//! survives only so the consumers that spell `common::` paths today keep
//! compiling — `tests/fill_metrics_tests.rs`, `tests/volume_fill_fraction.rs`,
//! `tests/classify_feature_angle.rs`, `tests/mesh_to_volume_clamp_hermeticity.rs`
//! and `tests/node_attachment_producer.rs`. They sat outside #6387's locked
//! file set, so re-pointing them was not #6387's to do.
//!
//! That re-point is tracked, not aspirational: follow-up ticket
//! `tkt_0RTBP18NC8PBRZGQ49RYZD7ATJ` (escalation id `agent-followup-6387`)
//! changes those import lines plus `volume_fill_fraction.rs`'s
//! `common::assert_rel` call sites. No new shared FIXTURE belongs here — put it
//! in `reify_test_support`.
//!
//! # The census is NOT a fixture, and stays (#6830)
//!
//! #6387's plan ended with "then remove `tests/common/` outright". That last
//! step is no longer reachable, so the ticket above shrinks: re-pointing the
//! import lines leaves this file holding [`entity_census`] alone rather than
//! leaving it empty.
//!
//! [`entity_census`] cannot follow the fixtures into `reify_test_support`, for
//! two independent reasons. It is not a fixture: it holds no geometry, it is a
//! raw-FFI harness that takes geometry as an argument. And it structurally
//! cannot move: its body calls `reify_kernel_gmsh::{ffi, init,
//! CLASSIFY_FEATURE_ANGLE, CLASSIFY_CURVE_ANGLE}`, all `#[cfg(has_gmsh)]`
//! crate-root items, while `reify-test-support` has no reify-kernel-gmsh edge.
//! Adding one would drag the gmsh FFI and its libgmsh link closure into
//! `crates/reify-audit`'s PRODUCTION build graph — reify-audit carries
//! reify-test-support as a normal dependency, not a dev-dependency — and would
//! invert the adapter -> test-support direction the workspace maintains.
//!
//! # The lints below are load-bearing
//!
//! Every consumer binary compiles its own copy of this module and uses only
//! part of it, so the unused remainder must not be an error under
//! `-D warnings`. A `pub use` a given binary does not exercise is an
//! `unused_imports` warning rather than the `dead_code` the pre-#6387 inlined
//! fixture bodies produced, and `entity_census` is `dead_code` in every binary
//! that compiles it without calling it, so both are allowed.

#![allow(dead_code, unused_imports)]

// Spelled with the explicit `fixtures::` module path rather than importing from
// the crate root: `reify_test_support`'s `lib.rs` glob-re-exports several
// modules (`pub use fixtures::*;`, `pub use helpers::*;`, …), so a root-path
// import would become an E0659 ambiguity the moment any other glob-exported
// module grew a same-named item.
pub use reify_test_support::fixtures::{
    F32_STORAGE_REL, assert_rel, prismatic_box_mesh, tessellated_cylinder_mesh,
    tessellated_cylinder_volume, unit_cube_mesh, unwelded_prismatic_box_mesh,
};

// ---------------------------------------------------------------------------
// Raw-FFI entity census (has_gmsh only)
// ---------------------------------------------------------------------------

// GATED DELIBERATELY, and the gate is load-bearing — see the module docs above.
// `ffi`, `init`, `CLASSIFY_FEATURE_ANGLE` and `CLASSIFY_CURVE_ANGLE` are all
// `#[cfg(has_gmsh)]` at the crate root (`src/lib.rs`), while
// `fill_metrics_tests.rs` includes this module UNCONDITIONALLY by design (it is
// pure reify_ir arithmetic and must stay verified on stub hosts). Un-gating
// either `use` or the `fn` below compiles fine on a libgmsh host and silently
// breaks every stub-host build.
#[cfg(has_gmsh)]
use reify_ir::Mesh;
#[cfg(has_gmsh)]
use reify_kernel_gmsh::{CLASSIFY_CURVE_ANGLE, CLASSIFY_FEATURE_ANGLE, ffi, init};

/// Entity census after classify + createGeometry, as `(dim0, dim1, dim2)`.
///
/// Replays only the classify half of `GmshKernel::mesh_to_volume`
/// (`kernel_real.rs`) / the `run_meshing_with_entity_queries` prefix
/// (`mesh_boundary.rs`), stopping before surface-loop / volume /
/// `mesh_generate(3)`. That keeps it fast and isolates just the topology
/// reconstruction step.
///
/// This is a RAW-FFI helper, so it MUST hold `init::GMSH_LOCK` itself — unlike
/// `volume_fill_fraction.rs`, which goes through the public API and would
/// self-deadlock if it took the lock. The guard is scoped to one invocation and
/// drops at return, so back-to-back calls do not deadlock.
///
/// It uses the PRODUCTION angle constants rather than re-typed literals. A test
/// carrying its own copy of the angle would re-create exactly the gap that
/// caused #6200: someone could change the production constant and the guard
/// would keep passing against a stale literal. Centralising the call here means
/// ONE import site instead of two that could drift apart.
///
/// `model_name` is PURELY DIAGNOSTIC — it labels gmsh's own log output when a
/// census test fails, and cannot affect the result because `ffi::clear()` runs
/// first and wipes all models. It is a parameter rather than a fixed constant
/// so each call site keeps its own diagnostic identity. That safety is a TESTED
/// property, not a comment: `classify_feature_angle.rs`'s
/// `entity_census_is_isolated_across_invocations` passes two different names
/// for identical geometry and asserts one triple (measured `(8,14,8)` both
/// times).
#[cfg(has_gmsh)]
pub fn entity_census(surface: &Mesh, model_name: &str) -> (usize, usize, usize) {
    let n_verts = surface.vertices.len() / 3;
    let n_tris = surface.indices.len() / 3;

    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();

    ffi::clear().expect("clear");
    ffi::option_set_number("General.Terminal", 0.0).expect("terminal off");
    ffi::model_add(model_name).expect("model_add");
    let surf_tag = ffi::add_discrete_entity(2, &[]).expect("add_discrete_entity");

    let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64).expect("add_nodes_2d");

    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags).expect("add_elements_2d");

    // The PRODUCTION constants, imported rather than re-typed — see above.
    ffi::classify_surfaces(CLASSIFY_FEATURE_ANGLE, 1, 1, CLASSIFY_CURVE_ANGLE, 0)
        .expect("classify_surfaces");
    ffi::create_geometry(&[]).expect("create_geometry");

    let n0 = ffi::get_entity_tags(0).expect("get_entity_tags(0)").len();
    let n1 = ffi::get_entity_tags(1).expect("get_entity_tags(1)").len();
    let n2 = ffi::get_entity_tags(2).expect("get_entity_tags(2)").len();

    let _ = ffi::clear();
    (n0, n1, n2)
}
