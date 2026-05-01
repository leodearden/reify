/// Shared test assertions for all-error stub `GeometryKernel` implementations.
///
/// # Purpose
///
/// Every stub kernel adapter (`FidgetKernel`, `ManifoldKernel`, …) must satisfy
/// an identical contract: the kernel is `Send + Sync`, implements
/// `GeometryKernel` as a trait object, and every method returns a descriptive
/// `Err(...)` variant whose message contains a kernel-identifying substring.
///
/// The [`assert_stub_kernel_errors!`] macro encapsulates that contract as three
/// independent `#[test]` functions so each concern is reported separately by the
/// test runner.
///
/// # Usage
///
/// ```ignore
/// // Inside a #[cfg(test)] mod tests block:
/// reify_test_support::assert_stub_kernel_errors!(FidgetKernel::new, "Fidget");
/// reify_test_support::assert_stub_kernel_errors!(ManifoldKernel::new, "Manifold");
/// ```
///
/// `$factory` must be an expression that produces a value implementing
/// [`::reify_types::GeometryKernel`] when called as `$factory()`.
/// `$substr` is a string literal; the generated tests assert that every error
/// message returned by the kernel contains this substring.
///
/// # Generated tests
///
/// Invoking the macro expands to three `#[test]` functions:
///
/// 1. `stub_kernel_implements_geometry_kernel_trait` — compile-time `Send + Sync`
///    pin via a local `fn assert_send_sync<T: Send + Sync>(_: &T) {}` call, plus
///    a `Box<dyn GeometryKernel>` upcast.
/// 2. `stub_kernel_execute_returns_descriptive_error` — iterates over
///    `Union/Difference/Intersection` ops and asserts each returns
///    `Err(GeometryError::OperationFailed(msg))` with `msg.contains($substr)`.
/// 3. `stub_kernel_query_export_tessellate_all_error` — asserts that `query`,
///    `export`, and `tessellate` return their respective error variants with
///    messages containing `$substr`.

#[cfg(test)]
mod tests {
    use reify_types::{
        ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId,
        GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
    };

    const STUB_MSG: &str = "TestStub kernel not yet implemented — fixture only";

    /// Minimal all-error stub kernel for testing [`crate::assert_stub_kernel_errors!`].
    ///
    /// Mirrors the `_private: ()` pattern from `reify-kernel-occt/src/stubs.rs`.
    struct TestStubKernel {
        _private: (),
    }

    impl TestStubKernel {
        fn new() -> Self {
            Self { _private: () }
        }
    }

    impl GeometryKernel for TestStubKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed(STUB_MSG.into()))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed(STUB_MSG.into()))
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Err(ExportError::FormatError(STUB_MSG.into()))
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            Err(TessError::TessellationFailed(STUB_MSG.into()))
        }
    }

    // Invoke the macro to generate three #[test] fns against the fixture stub.
    // This line will fail to compile until step-2 implements the macro.
    crate::assert_stub_kernel_errors!(TestStubKernel::new, "TestStub");
}
