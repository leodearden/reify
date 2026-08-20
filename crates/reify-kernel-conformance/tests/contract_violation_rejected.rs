//! Negative signal (c): a contract-violating producer fails
//! produce→validate (kernel-seam ε, INV-GEO-2).
//!
//! A deliberately winding-corrupted real OCCT box is rejected by
//! `Mesh::validate`. Post-#4336, honest real OCCT tessellation is correctly
//! wound (proven by the control assertion below, and by
//! `tessellation_winding_integration.rs`), so this exercises the
//! validator's rejection path via deliberate corruption of a real-kernel-
//! derived mesh, not via honest producer output — faithful to
//! `docs/prds/kernel-seam-contracts.md` §11.1.

#![cfg(has_occt)]

mod common;

use reify_ir::geometry::{MeshInvariant, MeshWitness};

#[test]
fn corrupted_winding_is_rejected_by_validate() {
    let mesh = common::occt_box();

    // Control: the honest real OCCT box is valid.
    mesh.validate(0.0)
        .expect("real OCCT-tessellated box must satisfy the mesh contract");

    let corrupted = common::corrupt_winding(&mesh);
    let violation = corrupted
        .validate(0.0)
        .expect_err("winding-corrupted box must be rejected by validate(0.0)");

    assert_eq!(
        violation.invariant,
        MeshInvariant::ConsistentWinding,
        "expected a ConsistentWinding violation; got {violation:?}"
    );
    assert!(
        violation.counts.reversed_edges > 0,
        "expected reversed_edges > 0; got counts = {:?}",
        violation.counts
    );
    assert!(
        matches!(violation.witness, MeshWitness::Edge { .. }),
        "expected a MeshWitness::Edge witness; got {:?}",
        violation.witness
    );
}
