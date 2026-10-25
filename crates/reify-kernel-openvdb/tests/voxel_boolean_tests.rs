//! `GeometryKernel::execute` runs the three voxel Booleans the OpenVDB
//! capability descriptor declares (`(BooleanUnion | BooleanDifference |
//! BooleanIntersection, Voxel)`) as level-set CSG on registered grids.
//!
//! Structure follows the `cfg(has_openvdb)` real-test / `cfg(not(has_openvdb))`
//! skip-stub pattern of `tests/dense_sdf_ingest_tests.rs`.
//!
//! # Why the assertions are exact set relations
//!
//! Both operand spheres are sampled on ONE shared lattice (same grid centre,
//! dims and voxel size), so the two grids carry identical transforms and the
//! CSG runs voxel-for-voxel: union is the per-voxel min, intersection the max,
//! and difference `max(a, −b)`. The negative-voxel sets of the results are
//! therefore exactly A∪B, A∩B and A∖B. With centre distance d = R the lens is
//! π(4R+d)(2R−d)²/12 ≈ 35 mm³ ≈ 280 voxels against ≈ 900 voxels per sphere, so
//! every strict inequality below holds by hundreds of voxels.

#[cfg(has_openvdb)]
mod fixture {
    use reify_ir::{DenseSdfGrid, GeometryHandleId, GeometryKernel, ImplicitOptions};
    use reify_kernel_openvdb::OpenVdbKernel;
    use reify_kernel_openvdb::test_fixtures::sphere_sdf_grid;

    /// Voxel size of the shared lattice, metres.
    pub const H: f64 = 0.5e-3;
    pub const BAND: u32 = 3;
    pub const R: f64 = 3e-3;
    pub const GRID_CENTRE: [f64; 3] = [0.0, 0.0, 0.0];
    pub const OFFSET_A: [f64; 3] = [-1.5e-3, 0.0, 0.0];
    pub const OFFSET_B: [f64; 3] = [1.5e-3, 0.0, 0.0];
    /// Extent both spheres need together, before band padding: x spans
    /// ±4.5 mm (centres ±1.5 mm, radius 3 mm), y and z span ±3 mm.
    pub const EXTENT: [f64; 3] = [9e-3, 6e-3, 6e-3];

    /// Probe points, each ≥ 1 mm (two voxels) from every sphere surface.
    pub const P_A_ONLY: [f64; 3] = [-3.5e-3, 0.0, 0.0];
    pub const P_LENS: [f64; 3] = [0.0, 0.0, 0.0];
    pub const P_B_ONLY: [f64; 3] = [3.5e-3, 0.0, 0.0];

    /// The B5 oracle shape: n = ⌈E/h⌉ + 2·band per axis, so both spheres and
    /// their band lie inside the block.
    pub fn dims(h: f64) -> [u32; 3] {
        std::array::from_fn(|a| (EXTENT[a] / h).ceil() as u32 + 2 * BAND)
    }

    pub fn sphere(offset: [f64; 3], h: f64) -> DenseSdfGrid {
        sphere_sdf_grid(GRID_CENTRE, offset, R, dims(h), h)
    }

    pub fn options(h: f64) -> ImplicitOptions {
        ImplicitOptions::new(h, BAND).expect("valid fixture options")
    }

    pub fn ingest(kernel: &mut OpenVdbKernel, offset: [f64; 3], h: f64) -> GeometryHandleId {
        kernel
            .ingest_dense_sdf(&sphere(offset, h), &options(h))
            .expect("ingest_dense_sdf must accept the fixture sphere")
            .id
    }

    /// Spheres A and B on the shared lattice.
    pub fn overlapping_pair(kernel: &mut OpenVdbKernel) -> (GeometryHandleId, GeometryHandleId) {
        (ingest(kernel, OFFSET_A, H), ingest(kernel, OFFSET_B, H))
    }

    pub fn interior_voxels(kernel: &mut OpenVdbKernel, id: GeometryHandleId) -> usize {
        kernel
            .densify_grid_to_sampled(id)
            .expect("densify_grid_to_sampled on a registered grid")
            .data
            .iter()
            .filter(|v| **v < 0.0)
            .count()
    }

    pub fn sdf_at(kernel: &OpenVdbKernel, id: GeometryHandleId, p: [f64; 3]) -> f64 {
        kernel
            .sample_sdf_at(id, p[0], p[1], p[2])
            .expect("sample_sdf_at on a registered grid")
    }
}

#[cfg(has_openvdb)]
fn run(
    kernel: &mut reify_kernel_openvdb::OpenVdbKernel,
    op: &reify_ir::GeometryOp,
) -> reify_ir::GeometryHandleId {
    use reify_ir::GeometryKernel;

    let handle = kernel.execute(op).unwrap_or_else(|e| {
        panic!(
            "{} on two registered level sets must succeed; got {e}",
            op.kind_name()
        )
    });
    assert!(
        handle.repr.is_none(),
        "a Voxel-repr result carries no BRep repr"
    );
    handle.id
}

#[cfg(has_openvdb)]
#[test]
fn voxel_union_of_overlapping_spheres_contains_both() {
    use fixture::*;
    use reify_ir::GeometryOp;
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let (a, b) = overlapping_pair(&mut kernel);
    let u = run(&mut kernel, &GeometryOp::Union { left: a, right: b });
    assert!(
        u != a && u != b,
        "the union must be a FRESH handle, not alias an operand"
    );

    let (ia, ib, iu) = (
        interior_voxels(&mut kernel, a),
        interior_voxels(&mut kernel, b),
        interior_voxels(&mut kernel, u),
    );
    assert!(
        iu > ia && iu > ib,
        "A∪B must strictly contain each operand: |A| {ia}, |B| {ib}, |A∪B| {iu}",
    );
    for (name, p) in [
        ("P_A_ONLY", P_A_ONLY),
        ("P_LENS", P_LENS),
        ("P_B_ONLY", P_B_ONLY),
    ] {
        let s = sdf_at(&kernel, u, p);
        assert!(s < 0.0, "{name} {p:?} lies inside A∪B; sdf = {s}");
    }
}

#[cfg(has_openvdb)]
#[test]
fn voxel_intersection_of_overlapping_spheres_is_the_lens() {
    use fixture::*;
    use reify_ir::GeometryOp;
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let (a, b) = overlapping_pair(&mut kernel);
    let i = run(&mut kernel, &GeometryOp::Intersection { left: a, right: b });

    let (ia, ib, ii) = (
        interior_voxels(&mut kernel, a),
        interior_voxels(&mut kernel, b),
        interior_voxels(&mut kernel, i),
    );
    assert!(
        0 < ii && ii < ia.min(ib),
        "A∩B must be a non-empty lens strictly smaller than either operand: \
         |A| {ia}, |B| {ib}, |A∩B| {ii}",
    );
    let lens = sdf_at(&kernel, i, P_LENS);
    assert!(lens < 0.0, "P_LENS lies inside A∩B; sdf = {lens}");
    for (name, p) in [("P_A_ONLY", P_A_ONLY), ("P_B_ONLY", P_B_ONLY)] {
        let s = sdf_at(&kernel, i, p);
        assert!(s > 0.0, "{name} {p:?} lies outside A∩B; sdf = {s}");
    }
}

#[cfg(has_openvdb)]
#[test]
fn voxel_difference_removes_the_subtrahend_from_the_minuend() {
    use fixture::*;
    use reify_ir::GeometryOp;
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let (a, b) = overlapping_pair(&mut kernel);
    let d = run(&mut kernel, &GeometryOp::Difference { left: a, right: b });

    let (ia, id) = (
        interior_voxels(&mut kernel, a),
        interior_voxels(&mut kernel, d),
    );
    assert!(
        0 < id && id < ia,
        "A∖B must be non-empty and strictly smaller than A: |A| {ia}, |A∖B| {id}",
    );
    let kept = sdf_at(&kernel, d, P_A_ONLY);
    assert!(kept < 0.0, "P_A_ONLY lies inside A∖B; sdf = {kept}");
    for (name, p) in [("P_LENS", P_LENS), ("P_B_ONLY", P_B_ONLY)] {
        let s = sdf_at(&kernel, d, p);
        assert!(s > 0.0, "{name} {p:?} lies outside A∖B; sdf = {s}");
    }
}

/// OpenVDB's in-place `csgUnion`/`csgDifference`/`csgIntersection` always
/// leave their B grid empty; a registered operand must survive every op.
#[cfg(has_openvdb)]
#[test]
fn voxel_booleans_leave_their_operands_intact() {
    use fixture::*;
    use reify_ir::{GeometryHandleId, GeometryOp};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let (a, b) = overlapping_pair(&mut kernel);
    let snapshot = |kernel: &mut OpenVdbKernel, id: GeometryHandleId| {
        (
            interior_voxels(kernel, id),
            kernel.active_voxel_count(id).expect("registered handle"),
        )
    };
    let before = (snapshot(&mut kernel, a), snapshot(&mut kernel, b));

    for op in [
        GeometryOp::Union { left: a, right: b },
        GeometryOp::Difference { left: a, right: b },
        GeometryOp::Intersection { left: a, right: b },
    ] {
        run(&mut kernel, &op);
    }

    let after = (snapshot(&mut kernel, a), snapshot(&mut kernel, b));
    assert_eq!(
        after, before,
        "((interior, active) of A, (interior, active) of B) must be unchanged by the Booleans",
    );
}

/// Operands on different lattices: the right operand is resampled onto the
/// left's transform, so each result lives on the left's lattice, and operand
/// order survives the alignment. Difference is the order-sensitive op: a
/// resample of the wrong operand, or a swap after alignment, shows up there.
#[cfg(has_openvdb)]
#[test]
fn voxel_booleans_across_mismatched_lattices_resample_right_onto_left() {
    use fixture::*;
    use reify_ir::{GeometryHandleId, GeometryKernel, GeometryOp};
    use reify_kernel_openvdb::OpenVdbKernel;

    const H_B: f64 = 0.4e-3;

    let mut kernel = OpenVdbKernel::new();
    let a = ingest(&mut kernel, OFFSET_A, H);
    let b = ingest(&mut kernel, OFFSET_B, H_B);

    let assert_on_lattice =
        |kernel: &mut OpenVdbKernel, id: GeometryHandleId, h: f64, what: &str| {
            let spacing = kernel
                .densify_grid_to_sampled(id)
                .expect("densify_grid_to_sampled on the Boolean result")
                .spacing;
            for (axis, s) in spacing.iter().enumerate() {
                assert!(
                    (s - h).abs() <= 1e-12,
                    "{what}: axis {axis} spacing {s} != the left operand's voxel size {h}",
                );
            }
        };

    // Whether each of P_A_ONLY, P_LENS, P_B_ONLY lies inside the result.
    for (op, inside) in [
        (GeometryOp::Union { left: a, right: b }, [true, true, true]),
        (
            GeometryOp::Difference { left: a, right: b },
            [true, false, false],
        ),
        (
            GeometryOp::Intersection { left: a, right: b },
            [false, true, false],
        ),
    ] {
        let what = format!("{}(a, b)", op.kind_name());
        let result = run(&mut kernel, &op);
        assert_on_lattice(&mut kernel, result, H, &what);
        for ((name, p), inside) in [
            ("P_A_ONLY", P_A_ONLY),
            ("P_LENS", P_LENS),
            ("P_B_ONLY", P_B_ONLY),
        ]
        .into_iter()
        .zip(inside)
        {
            let s = sdf_at(&kernel, result, p);
            if inside {
                assert!(s < 0.0, "{name} {p:?} lies inside {what}; sdf = {s}");
            } else {
                assert!(s > 0.0, "{name} {p:?} lies outside {what}; sdf = {s}");
            }
        }
    }

    let ba = run(&mut kernel, &GeometryOp::Union { left: b, right: a });
    assert_on_lattice(&mut kernel, ba, H_B, "Union(b, a)");
}

#[cfg(has_openvdb)]
#[test]
fn voxel_boolean_rejects_unknown_operand() {
    use fixture::*;
    use reify_ir::{GeometryError, GeometryHandleId, GeometryKernel, GeometryOp};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let a = ingest(&mut kernel, OFFSET_A, H);
    let unknown = GeometryHandleId(9_999);
    match kernel.execute(&GeometryOp::Union {
        left: a,
        right: unknown,
    }) {
        Err(GeometryError::InvalidReference(id)) => assert_eq!(id, unknown),
        other => panic!("expected Err(InvalidReference({unknown:?})); got {other:?}"),
    }
}

#[cfg(has_openvdb)]
#[test]
fn voxel_boolean_rejects_mesh_handle_operand() {
    use fixture::*;
    use reify_ir::{GeometryError, GeometryKernel, GeometryOp};
    use reify_kernel_openvdb::OpenVdbKernel;
    use reify_kernel_openvdb::test_fixtures::box_mesh;

    let mut kernel = OpenVdbKernel::new();
    let a = ingest(&mut kernel, OFFSET_A, H);
    let mesh = kernel
        .register_mesh_handle(&box_mesh(1e-3, 1e-3, 1e-3))
        .expect("register_mesh_handle")
        .id;
    match kernel.execute(&GeometryOp::Union {
        left: a,
        right: mesh,
    }) {
        Err(GeometryError::OperationFailed(msg)) => {
            let token = format!("{mesh:?}");
            assert!(
                msg.contains(&token) && msg.contains("Mesh"),
                "the refusal must name the operand {token} and say it is a Mesh handle; got {msg:?}",
            );
        }
        other => {
            panic!("expected Err(OperationFailed(_)) for a Mesh-handle operand; got {other:?}")
        }
    }
}

/// A fog volume (an imported density field, say) is not a level set, so the
/// CSG refuses it on either side, naming the side and the grid class.
#[cfg(has_openvdb)]
#[test]
fn voxel_boolean_rejects_fog_volume_operand() {
    use fixture::*;
    use reify_ir::{GeometryError, GeometryKernel, GeometryOp};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let (a, b) = overlapping_pair(&mut kernel);
    let fog = kernel
        .register_fog_volume_for_test(b)
        .expect("register_fog_volume_for_test on a registered level set");

    for (role, op) in [
        (
            "left",
            GeometryOp::Union {
                left: fog,
                right: a,
            },
        ),
        (
            "right",
            GeometryOp::Union {
                left: a,
                right: fog,
            },
        ),
    ] {
        match kernel.execute(&op) {
            Err(GeometryError::OperationFailed(msg)) => assert!(
                msg.contains(&format!("the {role} operand")) && msg.contains("fog volume"),
                "the refusal must name the {role} operand and its grid class; got {msg:?}",
            ),
            other => panic!(
                "expected Err(OperationFailed(_)) for a {role} fog-volume operand; got {other:?}"
            ),
        }
    }
}

#[cfg(not(has_openvdb))]
#[test]
fn voxel_boolean_tests_skipped_without_cfg() {
    println!("voxel_boolean_tests: has_openvdb cfg not set, skipping voxel Boolean tests");
}
