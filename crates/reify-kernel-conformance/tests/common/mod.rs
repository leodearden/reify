//! Shared test harness for the kernel-pair conformance matrix (kernel-seam ε,
//! INV-GEO-2). Each integration-test binary in this crate `mod common;`s
//! this file and consumes a cfg-dependent subset of its items (e.g. the
//! gmsh-only helpers are absent from a has_occt-only build) — allowed
//! crate-wide for this module since no single binary uses everything.
#![allow(dead_code)]

#[cfg(has_occt)]
use reify_ir::{GeometryOp, Value};
#[cfg(has_occt)]
use reify_kernel_occt::OcctKernel;

// ── OCCT fixture builders ───────────────────────────────────────────────────
//
// Every fixture is a REAL OCCT-tessellated `reify_ir::Mesh` (dimensions in SI
// metres — mirrors `tessellation_winding_integration.rs`'s scale, which
// proves real OCCT box tessellation satisfies `validate(0.0)` post-#4336).
//
// `OCCT_TESS_TOL` is the `BRepMesh_IncrementalMesh` linear-deflection
// parameter passed to `OcctKernel::tessellate` — NOT the same tolerance as
// `Mesh::validate(0.0)`'s `NonDegenerate` epsilon used downstream. OCCT
// rejects a deflection of exactly `0.0` outright
// (`BRepMesh_IncrementalMesh::initParameters: invalid parameter value`), so
// the producer step needs a small positive deflection (0.1 mm, well under
// the 2 mm fillet radius); the *separate* `validate(0.0)` call each test
// makes afterward is the real, non-tolerant mesh-contract check (§5).
#[cfg(has_occt)]
const OCCT_TESS_TOL: f64 = 1.0e-4;

/// 10×20×30 mm box.
#[cfg(has_occt)]
pub fn occt_box() -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(20.0e-3),
            depth: Value::Real(30.0e-3),
        })
        .expect("box creation should succeed");
    kernel
        .tessellate(h.id, OCCT_TESS_TOL)
        .expect("box tessellate should succeed")
}

/// 8 mm-radius sphere.
#[cfg(has_occt)]
pub fn occt_sphere() -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(8.0e-3),
        })
        .expect("sphere creation should succeed");
    kernel
        .tessellate(h.id, OCCT_TESS_TOL)
        .expect("sphere tessellate should succeed")
}

/// 5 mm-radius × 15 mm-height cylinder.
#[cfg(has_occt)]
pub fn occt_cylinder() -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0e-3),
            height: Value::Real(15.0e-3),
        })
        .expect("cylinder creation should succeed");
    kernel
        .tessellate(h.id, OCCT_TESS_TOL)
        .expect("cylinder tessellate should succeed")
}

/// Union of two 10 mm cubes with 50% X-overlap (dx = 5 mm). Mirrors
/// `manifold_cross_kernel_real.rs`'s
/// `real_occt_tessellated_union_ingests_and_unions_through_manifold` fixture:
/// `BRepPrimAPI_MakeBox` assigns some faces `TopAbs_REVERSED`, and the
/// boolean union's result shell carries that mixed orientation forward,
/// making this the REVERSED-faces fixture in the producer×fixture matrix.
#[cfg(has_occt)]
pub fn occt_boolean_reversed() -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("box_a creation should succeed");
    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("box_b_raw creation should succeed");
    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 5.0e-3,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate should succeed");
    let u = kernel
        .execute(&GeometryOp::Union {
            left: box_a.id,
            right: box_b.id,
        })
        .expect("union should succeed");
    kernel
        .tessellate(u.id, OCCT_TESS_TOL)
        .expect("union tessellate should succeed")
}

/// 10 mm cube with a single edge filleted (2 mm radius).
#[cfg(has_occt)]
pub fn occt_fillet() -> reify_ir::Mesh {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0e-3),
            height: Value::Real(10.0e-3),
            depth: Value::Real(10.0e-3),
        })
        .expect("box creation should succeed");
    let edges = kernel
        .extract_edges(box_h.id)
        .expect("extract_edges should succeed on a solid box");
    let one_edge = vec![edges[0]];
    let f = kernel
        .execute(&GeometryOp::Fillet {
            target: box_h.id,
            edges: one_edge,
            radius: Value::Real(2.0e-3),
        })
        .expect("fillet should succeed");
    kernel
        .tessellate(f.id, OCCT_TESS_TOL)
        .expect("fillet tessellate should succeed")
}

/// Enumerate the five real fixtures for the producer×fixture matrix — every
/// arm in this crate iterates this same set.
#[cfg(has_occt)]
pub fn fixtures() -> Vec<(&'static str, fn() -> reify_ir::Mesh)> {
    vec![
        ("box", occt_box as fn() -> reify_ir::Mesh),
        ("sphere", occt_sphere as fn() -> reify_ir::Mesh),
        ("cylinder", occt_cylinder as fn() -> reify_ir::Mesh),
        ("boolean", occt_boolean_reversed as fn() -> reify_ir::Mesh),
        ("fillet", occt_fillet as fn() -> reify_ir::Mesh),
    ]
}
