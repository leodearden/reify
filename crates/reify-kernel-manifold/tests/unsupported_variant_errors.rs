//! Unsupported-variant rejection contract for `ManifoldKernel`.
//!
//! An op, query or export format the manifold kernel does not handle is
//! rejected with an error whose message names the rejected variant and the
//! kernel, so a reader can attribute the failure without a debugger.

use reify_ir::{
    ExportError, ExportFormat, ExportOptions, GeometryError, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, QueryError, Value,
};
use reify_kernel_manifold::ManifoldKernel;

#[test]
fn execute_rejects_non_boolean_op_naming_its_kind() {
    let ops = [
        GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        },
        GeometryOp::Translate {
            target: GeometryHandleId(1),
            dx: 0.0,
            dy: 0.0,
            dz: 0.0,
        },
    ];
    for op in ops {
        let mut kernel = ManifoldKernel::new();
        match kernel.execute(&op) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(msg.contains(op.kind_name()), "{msg:?}");
                assert!(msg.contains("manifold"), "{msg:?}");
            }
            other => panic!(
                "expected OperationFailed for {}, got {other:?}",
                op.kind_name()
            ),
        }
    }
}

#[test]
fn query_rejects_unanswered_variant_naming_its_kind() {
    let queries = [
        GeometryQuery::Volume(GeometryHandleId(1)),
        GeometryQuery::Centroid(GeometryHandleId(1)),
    ];
    let kernel = ManifoldKernel::new();
    for query in queries {
        match kernel.query(&query) {
            Err(QueryError::QueryFailed(msg)) => {
                assert!(msg.contains(query.kind_name()), "{msg:?}");
                assert!(msg.contains("manifold"), "{msg:?}");
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
            Err(ExportError::FormatError(msg)) => {
                assert!(msg.contains(&token), "{msg:?}");
                assert!(msg.contains("manifold"), "{msg:?}");
            }
            other => panic!("expected FormatError from export({token}), got {other:?}"),
        }
        assert!(buf.is_empty(), "export({token}) wrote {} bytes", buf.len());

        let mut buf: Vec<u8> = Vec::new();
        let options = ExportOptions::default();
        match kernel.export_with_options(GeometryHandleId(1), format, &options, &mut buf) {
            Err(ExportError::FormatError(msg)) => {
                assert!(msg.contains(&token), "{msg:?}");
                assert!(msg.contains("manifold"), "{msg:?}");
            }
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
