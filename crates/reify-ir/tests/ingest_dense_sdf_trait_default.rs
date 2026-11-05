//! `GeometryKernel::ingest_dense_sdf`'s trait default is the not-supported
//! contract: a kernel that does not override it refuses dense SDF ingest with
//! `OperationFailed`, and the method keeps the trait object-safe.

use reify_ir::{
    DenseSdfGrid, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, ImplicitOptions, Mesh, QueryError, TessError, Value,
};

/// Implements only the trait's required methods, so every provided method
/// runs its default.
struct RequiredMethodsOnlyKernel;

impl GeometryKernel for RequiredMethodsOnlyKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed("stub".into()))
    }

    fn query(&self, _q: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed("stub".into()))
    }

    fn export(
        &self,
        _h: GeometryHandleId,
        _f: ExportFormat,
        _w: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError("stub".into()))
    }

    fn tessellate(&self, _h: GeometryHandleId, _t: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed("stub".into()))
    }
}

#[test]
fn ingest_dense_sdf_default_refuses_via_trait_object() {
    let mut kernel: Box<dyn GeometryKernel> = Box::new(RequiredMethodsOnlyKernel);
    let grid = DenseSdfGrid::from_fn([2, 2, 2], [0.0; 3], 1e-3, |_| 0.0).expect("valid grid");
    let opts = ImplicitOptions::new(1e-3, 1).expect("valid options");

    match kernel.ingest_dense_sdf(&grid, &opts) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains("does not ingest dense SDF grids"),
            "default must name the unsupported capability; got {msg:?}",
        ),
        other => panic!("expected Err(OperationFailed(_)) from the trait default; got {other:?}"),
    }
}
