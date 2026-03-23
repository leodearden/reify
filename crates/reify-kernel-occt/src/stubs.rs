//! Stub types for when OCCT libraries are not available at build time.
//!
//! These provide the same public API surface as the real OcctKernel and
//! OcctKernelHandle, but all operations return errors. This allows
//! downstream crates to compile and fail gracefully at runtime.

// Stub implementations will be added in step-4.

#[cfg(all(test, not(has_occt)))]
mod tests {
    use super::*;
    use reify_types::{
        ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, Value, WarmStartable,
    };

    #[test]
    fn stub_kernel_new_succeeds() {
        let _kernel = OcctKernel::new();
    }

    #[test]
    fn stub_kernel_execute_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err:?}").contains("OCCT"),
            "error should mention OCCT: {err:?}"
        );
    }

    #[test]
    fn stub_kernel_query_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_types::GeometryQuery::Volume(GeometryHandleId(1)));
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_export_returns_error() {
        let kernel = OcctKernel::new();
        let mut buf = Vec::new();
        let result = kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_tessellate_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.tessellate(GeometryHandleId(1), 0.1);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_warm_state_returns_none() {
        let kernel = OcctKernel::new();
        assert!(kernel.warm_state().is_none());
    }

    #[test]
    fn stub_handle_spawn_succeeds() {
        let _handle = OcctKernelHandle::spawn();
    }

    #[test]
    fn stub_handle_execute_returns_error() {
        let mut handle = OcctKernelHandle::spawn();
        let result = handle.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
    }

    #[test]
    fn stub_handle_is_geometry_kernel() {
        let handle = OcctKernelHandle::spawn();
        // Verify it can be used as Box<dyn GeometryKernel>
        let _boxed: Box<dyn GeometryKernel> = Box::new(handle);
    }
}
