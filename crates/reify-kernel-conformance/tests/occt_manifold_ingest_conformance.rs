//! OCCT→Manifold `ingest_mesh` conformance arm (kernel-seam ε, INV-GEO-2).
//!
//! For each real OCCT-tessellated fixture: produce → validate(α) →
//! consume (`ManifoldKernel::ingest_mesh`) → re-validate. See
//! `docs/prds/kernel-seam-contracts.md` §5 for the producer×consumer matrix
//! this crate implements.
//!
//! File-level gate: `has_occt` only — Manifold is an unconditional consumer
//! (no native-dep gate of its own); OCCT is the producer under test.

#![cfg(has_occt)]

mod common;

use reify_ir::GeometryKernel;
use reify_kernel_manifold::ManifoldKernel;

/// Registered-producers anchor (mirrors `manifold_cross_kernel_real.rs:82-94`):
/// the Manifold linker anchor fires and the registry contains both "occt"
/// and "manifold".
#[test]
fn registry_contains_occt_and_manifold_producers() {
    let anchor = reify_kernel_manifold::register::manifold_capability_descriptor();
    assert!(
        !anchor.supports.is_empty(),
        "manifold_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)"
    );

    let reg = reify_eval::kernel_registry::registry();
    assert!(
        reg.contains_key("occt"),
        "registry must contain \"occt\"; found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
    assert!(
        reg.contains_key("manifold"),
        "registry must contain \"manifold\"; found keys: {:?}",
        reg.keys().collect::<Vec<_>>()
    );
}

/// Full seam cycle for every real fixture: produce (OCCT tessellate) →
/// validate(0.0) Ok → weldedness reported (capability, not gated) → consume
/// (`ManifoldKernel::ingest_mesh`) → re-validate
/// (`manifold.tessellate(handle).validate(0.0)` Ok).
#[test]
fn occt_fixtures_ingest_and_revalidate_through_manifold() {
    for (name, build) in common::fixtures() {
        let mesh = build();

        mesh.validate(0.0).unwrap_or_else(|e| {
            panic!("[{name}] real OCCT-tessellated mesh must satisfy the mesh contract: {e:?}")
        });

        // Weldedness is a reported CAPABILITY, not a gated obligation: raw
        // OCCT per-face tessellation output is unwelded by design and still
        // fully valid (docs/prds/kernel-seam-contracts.md §2). Pinned
        // explicitly for the box, whose 6 independent planar faces are
        // guaranteed to duplicate every corner vertex across incident faces.
        let weldedness = mesh.weldedness(0.0);
        if name == "box" {
            assert!(
                !weldedness.raw_welded,
                "[{name}] raw OCCT box tessellation must be unwelded \
                 (per-face vertex blocks are duplicated across faces); \
                 got weldedness = {weldedness:?}"
            );
        }

        let mut manifold = ManifoldKernel::new();
        let handle = manifold.ingest_mesh(&mesh).unwrap_or_else(|e| {
            panic!("[{name}] ManifoldKernel::ingest_mesh must accept a validated OCCT mesh: {e:?}")
        });

        let re_tessellated = manifold.tessellate(handle.id, 0.0).unwrap_or_else(|e| {
            panic!("[{name}] re-tessellating the ingested Manifold handle must succeed: {e:?}")
        });

        re_tessellated.validate(0.0).unwrap_or_else(|e| {
            panic!(
                "[{name}] re-tessellated Manifold mesh must satisfy the mesh contract: {e:?}"
            )
        });
    }
}

/// Sphere-only seam cycle, isolated from the shared `fixtures()` loop above
/// (see `common::occt_sphere`'s doc comment: real OCCT sphere tessellation
/// fails `Mesh::validate`'s `NonDegenerate` obligation at the poles — a
/// producer defect outside this crate's scope). This is task #5164's
/// acceptance test: once the OCCT producer dedups/drops the zero-area
/// periodic-surface pole triangles, remove this `#[ignore]` and this test
/// must pass the identical produce → validate → consume → re-validate cycle
/// every other fixture already satisfies.
#[test]
#[ignore = "blocked on #5164 — OCCT periodic-surface pole node dedup / zero-area pole triangle"]
fn occt_sphere_ingests_and_revalidates_through_manifold() {
    let mesh = common::occt_sphere();

    mesh.validate(0.0).unwrap_or_else(|e| {
        panic!("real OCCT-tessellated sphere must satisfy the mesh contract: {e:?}")
    });

    let mut manifold = ManifoldKernel::new();
    let handle = manifold.ingest_mesh(&mesh).unwrap_or_else(|e| {
        panic!("ManifoldKernel::ingest_mesh must accept a validated OCCT sphere mesh: {e:?}")
    });

    let re_tessellated = manifold.tessellate(handle.id, 0.0).unwrap_or_else(|e| {
        panic!("re-tessellating the ingested Manifold sphere handle must succeed: {e:?}")
    });

    re_tessellated.validate(0.0).unwrap_or_else(|e| {
        panic!("re-tessellated Manifold sphere mesh must satisfy the mesh contract: {e:?}")
    });
}
