//! Unsupported-variant rejection contract for the real `OpenVdbKernel`.
//!
//! An op or query the OpenVDB kernel does not run is rejected with an error
//! whose message names the rejected variant and the kernel, so a reader can
//! attribute the failure without a debugger. Mirrors
//! `crates/reify-kernel-manifold/tests/unsupported_variant_errors.rs`.
//!
//! The rejected ops are B-rep edge ops, which a voxel kernel cannot run by
//! construction, so a voxel op implemented later cannot break this contract
//! for an unrelated reason. `query()` refuses EVERY query: the implicit-solids
//! capability manifest's 'openvdb-cannot-serve-queries' row relies on that.
//!
//! Structure follows the `cfg(has_openvdb)` real-test / `cfg(not(has_openvdb))`
//! skip-stub pattern of `tests/dense_sdf_ingest_tests.rs`.

#[cfg(has_openvdb)]
fn assert_names_variant_and_kernel(msg: &str, token: &str) {
    assert!(
        msg.contains(token),
        "message does not name {token:?}: {msg:?}"
    );
    assert!(
        msg.contains("openvdb"),
        "message does not name the kernel: {msg:?}"
    );
}

#[cfg(has_openvdb)]
#[test]
fn execute_rejects_op_openvdb_does_not_run_naming_its_kind() {
    use reify_ir::{GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, Value};
    use reify_kernel_openvdb::OpenVdbKernel;

    let ops = [
        GeometryOp::Fillet {
            target: GeometryHandleId(1),
            edges: vec![GeometryHandleId(2)],
            radius: Value::Real(0.1),
        },
        GeometryOp::Chamfer {
            target: GeometryHandleId(1),
            edges: vec![GeometryHandleId(2)],
            distance: Value::Real(0.1),
        },
    ];
    for op in ops {
        let mut kernel = OpenVdbKernel::new();
        match kernel.execute(&op) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert_names_variant_and_kernel(&msg, op.kind_name())
            }
            other => panic!(
                "expected OperationFailed for {}, got {other:?}",
                op.kind_name()
            ),
        }
    }
}

#[cfg(has_openvdb)]
#[test]
fn query_rejects_every_query_naming_its_kind() {
    use reify_ir::{GeometryHandleId, GeometryKernel, GeometryQuery, QueryError};
    use reify_kernel_openvdb::OpenVdbKernel;

    let queries = [
        GeometryQuery::Volume(GeometryHandleId(1)),
        GeometryQuery::BoundingBox(GeometryHandleId(1)),
        GeometryQuery::Centroid(GeometryHandleId(1)),
    ];
    let kernel = OpenVdbKernel::new();
    for query in queries {
        match kernel.query(&query) {
            Err(QueryError::QueryFailed(msg)) => {
                assert_names_variant_and_kernel(&msg, query.kind_name())
            }
            other => panic!(
                "expected QueryFailed for {}, got {other:?}",
                query.kind_name()
            ),
        }
    }
}

#[cfg(not(has_openvdb))]
#[test]
fn unsupported_variant_errors_skipped_without_cfg() {
    println!(
        "unsupported_variant_errors: has_openvdb cfg not set, skipping the real-kernel \
         catch-all contract"
    );
}
