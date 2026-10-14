//! Unsupported-variant rejection contract for `ManifoldKernel`.
//!
//! An op, query or export format the manifold kernel does not handle is
//! rejected with an error whose message names the rejected variant and the
//! kernel, so a reader can attribute the failure without a debugger.
//!
//! The rejected ops and queries are B-rep-only, which a mesh kernel cannot
//! answer by construction. A mesh-answerable variant could be implemented
//! later, and this contract would then fail for a reason unrelated to it.

use reify_ir::{
    ExportError, ExportFormat, ExportOptions, GeometryError, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, QueryCapability, QueryError, Value,
};
use reify_kernel_manifold::ManifoldKernel;

fn assert_names_variant_and_kernel(msg: &str, token: &str) {
    assert!(
        msg.contains(token),
        "message does not name {token:?}: {msg:?}"
    );
    assert!(
        msg.contains("manifold"),
        "message does not name the kernel: {msg:?}"
    );
}

#[test]
fn execute_rejects_edge_selecting_op_naming_its_kind() {
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
        let mut kernel = ManifoldKernel::new();
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

#[test]
fn query_rejects_brep_only_variant_naming_its_kind() {
    let queries = [
        GeometryQuery::EdgeLength(GeometryHandleId(1)),
        GeometryQuery::ShapeLocalTolerance(GeometryHandleId(1)),
    ];
    let kernel = ManifoldKernel::new();
    for query in queries {
        assert_eq!(
            query.capability_kind(),
            QueryCapability::BRepOnly,
            "{} is no longer B-rep-only; pick another rejected query",
            query.kind_name()
        );
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

#[test]
fn export_rejects_unsupported_format_naming_it() {
    let kernel = ManifoldKernel::new();
    for format in [ExportFormat::Step, ExportFormat::Obj] {
        let token = format!("{format:?}");
        let mut buf: Vec<u8> = Vec::new();
        match kernel.export(GeometryHandleId(1), format, &mut buf) {
            Err(ExportError::FormatError(msg)) => assert_names_variant_and_kernel(&msg, &token),
            other => panic!("expected FormatError from export({token}), got {other:?}"),
        }
        assert!(buf.is_empty(), "export({token}) wrote {} bytes", buf.len());

        let mut buf: Vec<u8> = Vec::new();
        let options = ExportOptions::default();
        match kernel.export_with_options(GeometryHandleId(1), format, &options, &mut buf) {
            Err(ExportError::FormatError(msg)) => assert_names_variant_and_kernel(&msg, &token),
            other => {
                panic!("expected FormatError from export_with_options({token}), got {other:?}")
            }
        }
        assert!(
            buf.is_empty(),
            "export_with_options({token}) wrote {} bytes",
            buf.len()
        );
    }
}
