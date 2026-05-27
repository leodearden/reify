//! Shared test assertions for all-error stub `GeometryKernel` implementations.
//!
//! # Purpose
//!
//! Every stub kernel adapter (`FidgetKernel`, `ManifoldKernel`, …) must satisfy
//! an identical contract: the kernel is `Send + Sync`, implements
//! `GeometryKernel` as a trait object, and every method returns a descriptive
//! `Err(...)` variant whose message contains a kernel-identifying substring.
//!
//! The [`assert_stub_kernel_errors!`] macro encapsulates that contract as three
//! independent `#[test]` functions so each concern is reported separately by the
//! test runner.
//!
//! # Usage
//!
//! ```ignore
//! // Inside a #[cfg(test)] mod tests block:
//! reify_test_support::assert_stub_kernel_errors!(FidgetKernel::new, "Fidget");
//! reify_test_support::assert_stub_kernel_errors!(ManifoldKernel::new, "Manifold");
//! ```
//!
//! `$factory` must be an expression that produces a value implementing
//! [`::reify_types::GeometryKernel`] when called as `$factory()`.
//! `$substr` is a string literal; the generated tests assert that every error
//! message returned by the kernel contains this substring.
//!
//! # Generated tests
//!
//! Invoking the macro expands to three `#[test]` functions:
//!
//! 1. `stub_kernel_implements_geometry_kernel_trait` — compile-time `Send + Sync`
//!    pin via a local `fn assert_send_sync<T: Send + Sync>(_: &T) {}` call, plus
//!    a `Box<dyn GeometryKernel>` upcast.
//! 2. `stub_kernel_execute_returns_descriptive_error` — iterates over
//!    `Union/Difference/Intersection` ops and asserts each returns
//!    `Err(GeometryError::OperationFailed(msg))` with `msg.contains($substr)`.
//! 3. `stub_kernel_query_export_tessellate_all_error` — asserts that `query`,
//!    `export`, and `tessellate` return their respective error variants with
//!    messages containing `$substr`.

/// Assert the all-error stub-kernel contract for a [`::reify_types::GeometryKernel`]
/// implementation by generating three independent `#[test]` functions.
///
/// # Signature
///
/// ```ignore
/// assert_stub_kernel_errors!($factory:expr, $substr:literal);
/// ```
///
/// - `$factory` — a callable expression (function path or closure) that returns a
///   fresh kernel instance each time it is invoked, e.g. `FidgetKernel::new` or
///   `|| FidgetKernel::new()`.
/// - `$substr` — a string literal that must appear in every error message returned
///   by the kernel, e.g. `"Fidget"` or `"Manifold"`.
///
/// # Generated test functions
///
/// | Name | What it verifies |
/// |------|-----------------|
/// | `stub_kernel_implements_geometry_kernel_trait` | `Send + Sync` pin + `Box<dyn GeometryKernel>` upcast |
/// | `stub_kernel_execute_returns_descriptive_error` | `execute` returns `Err(GeometryError::OperationFailed(msg))` with `msg.contains($substr)` for Union/Difference/Intersection |
/// | `stub_kernel_query_export_tessellate_all_error` | `query`/`export`/`tessellate` return matching error variants with `msg.contains($substr)` |
///
/// # Example
///
/// ```ignore
/// #[cfg(test)]
/// mod tests {
///     use super::*;
///     reify_test_support::assert_stub_kernel_errors!(FidgetKernel::new, "Fidget");
/// }
/// ```
///
/// The three generated functions live in the enclosing `mod tests` scope alongside
/// any kernel-specific tests you add. Their fixed names (`stub_kernel_*`) don't
/// collide with kernel-specific names (`fidget_kernel_*`, `manifold_kernel_*`).
#[macro_export]
macro_rules! assert_stub_kernel_errors {
    ($factory:expr, $substr:literal $(,)?) => {
        /// Compile-time `Send + Sync` pin and `Box<dyn GeometryKernel>` upcast.
        ///
        /// The inner `assert_send_sync` function takes `_: &T` so type inference
        /// eliminates the need for turbofish at the call site. The `Box<dyn …>`
        /// upcast fails to compile if the kernel is missing `Send` or `Sync`.
        #[test]
        fn stub_kernel_implements_geometry_kernel_trait() {
            fn assert_send_sync<T: ::core::marker::Send + ::core::marker::Sync>(_: &T) {}
            let kernel = ($factory)();
            assert_send_sync(&kernel);
            // Move `kernel` into the Box rather than constructing a second instance.
            let _boxed: ::std::boxed::Box<dyn ::reify_ir::GeometryKernel> =
                ::std::boxed::Box::new(kernel);
        }

        /// `execute` returns `Err(GeometryError::OperationFailed(msg))` for
        /// Union, Difference, and Intersection, and `msg` contains `$substr`.
        #[test]
        fn stub_kernel_execute_returns_descriptive_error() {
            let mut kernel = ($factory)();
            let ops = [
                ::reify_ir::GeometryOp::Union {
                    left: ::reify_ir::GeometryHandleId(1),
                    right: ::reify_ir::GeometryHandleId(2),
                },
                ::reify_ir::GeometryOp::Difference {
                    left: ::reify_ir::GeometryHandleId(1),
                    right: ::reify_ir::GeometryHandleId(2),
                },
                ::reify_ir::GeometryOp::Intersection {
                    left: ::reify_ir::GeometryHandleId(1),
                    right: ::reify_ir::GeometryHandleId(2),
                },
            ];
            for op in &ops {
                let result = ::reify_ir::GeometryKernel::execute(&mut kernel, op);
                match result {
                    Err(::reify_ir::GeometryError::OperationFailed(msg)) => {
                        assert!(
                            msg.contains($substr),
                            "execute error message must contain {:?} for op {:?}, got: {:?}",
                            $substr,
                            op,
                            msg,
                        );
                    }
                    other => panic!(
                        "expected Err(GeometryError::OperationFailed(_)) for op {:?}, got {:?}",
                        op, other
                    ),
                }
            }
        }

        /// `query`, `export`, and `tessellate` all return their respective error
        /// variants and the message contains `$substr`.
        #[test]
        fn stub_kernel_query_export_tessellate_all_error() {
            let kernel = ($factory)();

            match ::reify_ir::GeometryKernel::query(
                &kernel,
                &::reify_ir::GeometryQuery::Volume(::reify_ir::GeometryHandleId(1)),
            ) {
                Err(::reify_ir::QueryError::QueryFailed(msg)) => {
                    assert!(
                        msg.contains($substr),
                        "query error message must contain {:?}, got: {:?}",
                        $substr,
                        msg,
                    );
                }
                other => panic!(
                    "expected Err(QueryError::QueryFailed(_)) from query, got {:?}",
                    other
                ),
            }

            match ::reify_ir::GeometryKernel::export(
                &kernel,
                ::reify_ir::GeometryHandleId(1),
                ::reify_ir::ExportFormat::Step,
                &mut ::std::vec::Vec::<u8>::new(),
            ) {
                Err(::reify_ir::ExportError::FormatError(msg)) => {
                    assert!(
                        msg.contains($substr),
                        "export error message must contain {:?}, got: {:?}",
                        $substr,
                        msg,
                    );
                }
                other => panic!(
                    "expected Err(ExportError::FormatError(_)) from export, got {:?}",
                    other
                ),
            }

            match ::reify_ir::GeometryKernel::tessellate(
                &kernel,
                ::reify_ir::GeometryHandleId(1),
                0.1,
            ) {
                Err(::reify_ir::TessError::TessellationFailed(msg)) => {
                    assert!(
                        msg.contains($substr),
                        "tessellate error message must contain {:?}, got: {:?}",
                        $substr,
                        msg,
                    );
                }
                other => panic!(
                    "expected Err(TessError::TessellationFailed(_)) from tessellate, got {:?}",
                    other
                ),
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

    const STUB_MSG: &str = "TestStub kernel not available — fixture only";

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
    crate::assert_stub_kernel_errors!(TestStubKernel::new, "TestStub");
}
