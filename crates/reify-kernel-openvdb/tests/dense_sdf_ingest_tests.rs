//! `GeometryKernel::ingest_dense_sdf` on the real OpenVDB kernel: the B5
//! round trip under the D-8 activation policy
//! (`docs/prds/v0_6/implicit-solids.md`), plus tessellation, units and the
//! voxel-size agreement refusal.
//!
//! Structure follows the `cfg(has_openvdb)` real-test / `cfg(not(has_openvdb))`
//! skip-stub pattern of `tests/ingest_mesh_densify_tests.rs`.
//!
//! The fixture is `test_fixtures::b5_sphere`, shared with the run-twice
//! determinism tests so both cover the same grid.

#[cfg(has_openvdb)]
use reify_kernel_openvdb::test_fixtures::b5_sphere as fixture;

/// B5: every voxel of the padded block is active, the voxel size is h, the
/// bounds are centre ± (n−1)·h/2, and the densified values are exactly the
/// stored samples clamped to ±band·h.
#[cfg(has_openvdb)]
#[test]
fn b5_dense_ingest_round_trip_every_voxel_active() {
    use fixture::{BAND, GRID_CENTRE, H};
    use reify_ir::GeometryKernel;
    use reify_kernel_openvdb::OpenVdbKernel;

    let dims = fixture::dims();
    assert_eq!(dims, [30, 34, 38], "B5 oracle n = ⌈E/h⌉ + 2·band");
    let voxel_count: usize = dims.iter().map(|&n| n as usize).product();
    assert_eq!(voxel_count, 38_760);

    let grid = fixture::grid();
    let bg32 = (f64::from(BAND) * H) as f32;
    let [_, ny, nz] = dims.map(|n| n as usize);
    let is_shell = |i: usize| {
        let index = [i / (ny * nz), (i / nz) % ny, i % nz];
        (0..3).any(|a| index[a] == 0 || index[a] + 1 == dims[a] as usize)
    };
    assert!(
        grid.data()
            .iter()
            .enumerate()
            .filter(|&(i, _)| is_shell(i))
            .all(|(_, &s)| s >= bg32),
        "fixture premise: the whole outer shell must clamp to +band·h, or the \
         activation assertion below would not discriminate",
    );

    let mut kernel = OpenVdbKernel::new();
    let (handle, field) = {
        let k: &mut dyn GeometryKernel = &mut kernel;
        let handle = k
            .ingest_dense_sdf(&grid, &fixture::options())
            .expect("ingest_dense_sdf must accept a valid grid with matching options");
        let field = k
            .densify_grid_to_sampled(handle.id)
            .expect("densify_grid_to_sampled must read the ingested grid back");
        (handle, field)
    };

    assert!(
        handle.repr.is_none(),
        "a Voxel-repr solid carries no BRep repr"
    );
    assert_eq!(
        kernel
            .active_voxel_count(handle.id)
            .expect("registered handle"),
        voxel_count,
        "D-8 activation policy: every voxel of the padded block stays active",
    );

    for a in 0..3 {
        assert!(
            (field.spacing[a] - H).abs() <= 1e-15,
            "axis {a}: spacing {} != h {H}",
            field.spacing[a],
        );
        let half_span = f64::from(dims[a] - 1) * H / 2.0;
        let (lo, hi) = (GRID_CENTRE[a] - half_span, GRID_CENTRE[a] + half_span);
        assert!(
            (field.bounds_min[a] - lo).abs() <= 1e-12,
            "axis {a}: bounds_min {} != centre − (n−1)·h/2 = {lo}",
            field.bounds_min[a],
        );
        assert!(
            (field.bounds_max[a] - hi).abs() <= 1e-12,
            "axis {a}: bounds_max {} != centre + (n−1)·h/2 = {hi}",
            field.bounds_max[a],
        );
        assert_eq!(
            field.axis_grids[a].len(),
            dims[a] as usize,
            "axis {a} length"
        );
    }
    assert_eq!(field.data.len(), voxel_count);

    let (mut at_plus_bg, mut at_minus_bg, mut unclamped) = (0usize, 0usize, 0usize);
    for (i, (&densified, &stored)) in field.data.iter().zip(grid.data()).enumerate() {
        let expected = stored.clamp(-bg32, bg32);
        assert!(
            densified == f64::from(expected),
            "voxel {i}: densified {densified} != clamp(±bg, stored {stored}) = {expected}",
        );
        if expected == bg32 {
            at_plus_bg += 1;
        } else if expected == -bg32 {
            at_minus_bg += 1;
        } else {
            unclamped += 1;
        }
    }
    assert!(
        at_plus_bg > 0 && at_minus_bg > 0 && unclamped > 0,
        "anti-vacuity: both clamp sides and an unclamped band must be exercised; \
         +bg {at_plus_bg}, −bg {at_minus_bg}, unclamped {unclamped}",
    );
}

/// The ingested level set surfaces where the analytic sphere is: proves the
/// sign convention, LEVEL_SET semantics and the origin translation.
#[cfg(has_openvdb)]
#[test]
fn ingested_sphere_tessellates_at_its_analytic_position() {
    use fixture::{H, RADIUS};
    use reify_ir::GeometryKernel;
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_dense_sdf(&fixture::grid(), &fixture::options())
        .expect("ingest_dense_sdf");
    let mesh = kernel.tessellate(handle.id, 0.0).expect("tessellate");
    assert!(
        !mesh.vertices.is_empty() && !mesh.indices.is_empty(),
        "the sphere's zero level set must surface",
    );

    let centre = fixture::sphere_centre();
    for a in 0..3 {
        let coords = mesh.vertices.chunks_exact(3).map(|v| f64::from(v[a]));
        let lo = coords.clone().fold(f64::INFINITY, f64::min);
        let hi = coords.fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (lo - (centre[a] - RADIUS)).abs() <= H && (hi - (centre[a] + RADIUS)).abs() <= H,
            "axis {a}: vertex range [{lo}, {hi}] not within h of sphere centre ± r = [{}, {}]",
            centre[a] - RADIUS,
            centre[a] + RADIUS,
        );
    }
}

/// The ingested grid declares metre units: a `.vdb` round trip validates
/// against a Length codomain and is refused against a dimensionless one.
#[cfg(has_openvdb)]
#[test]
fn ingested_grid_declares_length_units() {
    use reify_core::Type;
    use reify_ir::GeometryKernel;
    use reify_kernel_openvdb::{IngestError, OpenVdbKernel, read_vdb_file};

    let mut kernel = OpenVdbKernel::new();
    let handle = kernel
        .ingest_dense_sdf(&fixture::grid(), &fixture::options())
        .expect("ingest_dense_sdf");

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("x.vdb");
    kernel
        .write_vdb_grid(handle.id, &path, "sdf")
        .expect("write_vdb_grid");
    let path = path.to_str().expect("UTF-8 tempdir path");

    read_vdb_file(path, "sdf", &Type::length())
        .expect("an ingested SDF must validate against a Length codomain");
    match read_vdb_file(path, "sdf", &Type::dimensionless_scalar()) {
        Err(IngestError::UnitMismatch { found_unit, .. }) => assert_eq!(found_unit, "m"),
        other => panic!("expected UnitMismatch against a dimensionless codomain; got {other:?}"),
    }
}

/// Options whose voxel size disagrees with the grid's spacing are refused,
/// naming both sizes.
#[cfg(has_openvdb)]
#[test]
fn ingest_refuses_mismatched_voxel_size() {
    use fixture::{BAND, H};
    use reify_ir::{GeometryError, GeometryKernel, ImplicitOptions};
    use reify_kernel_openvdb::OpenVdbKernel;

    let mut kernel = OpenVdbKernel::new();
    let opts = ImplicitOptions::new(2.0 * H, BAND).expect("valid options");
    match kernel.ingest_dense_sdf(&fixture::grid(), &opts) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains(&H.to_string()) && msg.contains(&(2.0 * H).to_string()),
            "the refusal must name both voxel sizes; got {msg:?}",
        ),
        other => panic!("expected Err(OperationFailed(_)); got {other:?}"),
    }
}

#[cfg(not(has_openvdb))]
#[test]
fn dense_sdf_ingest_skipped_without_cfg() {
    println!("dense_sdf_ingest_tests: has_openvdb cfg not set, skipping dense SDF ingest tests");
}
