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

use reify_ir::{
    ExportError, ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp,
    GeometryQuery, QueryError, TessError,
};

/// Assert the `query_many` length invariant documented on
/// [`::reify_ir::GeometryKernel::query_many`]: the output length must
/// track the input length, and a single-element batch must agree with
/// the equivalent [`GeometryKernel::query`] call.
///
/// Panics with a descriptive message if any of the following fail:
/// - `query_many(&[])` returns `Ok(v)` with `v.is_empty()`.
/// - `query_many` on a 2-element input returns `Err(_)` or `Ok(v)` with
///   `v.len() == 2`.
/// - `query_many` on a 1-element input is `Debug`-equal to the
///   corresponding single `query` call (both `Err` with the same debug
///   representation, or both `Ok` with the same single value).
pub fn assert_query_many_length_invariant<K: GeometryKernel + ?Sized>(
    kernel: &K,
    probe: GeometryHandleId,
) {
    match kernel.query_many(&[]) {
        Ok(v) if v.is_empty() => {}
        other => panic!(
            "query_many(&[]) must return Ok(v) with v.is_empty(), got {:?}",
            other
        ),
    }

    let two = [
        GeometryQuery::Volume(probe),
        GeometryQuery::SurfaceArea(probe),
    ];
    match kernel.query_many(&two) {
        Err(_) => {}
        Ok(v) if v.len() == two.len() => {}
        Ok(v) => panic!(
            "query_many on {} queries must return Err(_) or Ok(v) with v.len() == queries.len(), got Ok(v) with v.len() == {}: {:?}",
            two.len(),
            v.len(),
            v
        ),
    }

    let one = [GeometryQuery::Volume(probe)];
    let many_result = kernel.query_many(&one);
    let single_result = kernel.query(&one[0]);
    let many_desc = match &many_result {
        Ok(v) if v.len() == 1 => format!("Ok({:?})", v[0]),
        Ok(v) => format!("Ok(<wrong length {}>: {:?})", v.len(), v),
        Err(e) => format!("Err({:?})", e),
    };
    let single_desc = match &single_result {
        Ok(v) => format!("Ok({:?})", v),
        Err(e) => format!("Err({:?})", e),
    };
    assert_eq!(
        many_desc, single_desc,
        "query_many(&[q]) must agree with query(&q): query_many gave {:?}, query gave {:?}",
        many_result, single_result
    );
}

/// Assert that `extract_edges`/`extract_faces`/`extract_vertices` are
/// idempotent per parent handle: calling the same extractor twice on the
/// same `handle` must yield the same observable result (both `Err` with
/// the same debug representation, or both `Ok` with the same id `Vec`).
///
/// This mirrors the real-OCCT contract (`reify-kernel-occt`'s
/// `extract_edges` doc: "a second call with the same `handle` returns the
/// same handle list as the first call"), which the v0.2 selector
/// vocabulary's `adjacent_to_face` relies on.
///
/// Panics naming the offending method on divergence.
pub fn assert_extract_determinism<K: GeometryKernel + ?Sized>(kernel: &mut K, handle: GeometryHandleId) {
    let edges1 = kernel.extract_edges(handle);
    let edges2 = kernel.extract_edges(handle);
    assert_eq!(
        format!("{:?}", edges1),
        format!("{:?}", edges2),
        "extract_edges must be idempotent per handle: first call {:?}, second call {:?}",
        edges1,
        edges2
    );

    let faces1 = kernel.extract_faces(handle);
    let faces2 = kernel.extract_faces(handle);
    assert_eq!(
        format!("{:?}", faces1),
        format!("{:?}", faces2),
        "extract_faces must be idempotent per handle: first call {:?}, second call {:?}",
        faces1,
        faces2
    );

    let vertices1 = kernel.extract_vertices(handle);
    let vertices2 = kernel.extract_vertices(handle);
    assert_eq!(
        format!("{:?}", vertices1),
        format!("{:?}", vertices2),
        "extract_vertices must be idempotent per handle: first call {:?}, second call {:?}",
        vertices1,
        vertices2
    );
}

/// Assert that a never-created ("dangling") handle is rejected with an
/// `Err(_)` by every method that takes a handle as input — `execute`
/// (embedded as an operand), `query`, `export`, and `tessellate`. A
/// conforming kernel must never silently succeed (`Ok`) on a handle it
/// never produced via a prior `execute` call.
///
/// This is the shared, mode-independent axis of the contract: both a
/// STUB kernel (everything errors) and a REAL kernel (only invalid input
/// errors) must reject a dangling handle — they differ only in which
/// error *variant* they report (see [`assert_all_error_taxonomy`] and
/// [`assert_dangling_reference_taxonomy`] for the mode-specific variant
/// checks).
///
/// Panics naming the offending method if it returns `Ok` for `dangling`.
pub fn assert_dangling_handle_is_err<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    dangling: GeometryHandleId,
) {
    match kernel.execute(&GeometryOp::Union {
        left: dangling,
        right: dangling,
    }) {
        Err(_) => {}
        Ok(handle) => panic!(
            "execute on a dangling handle {:?} must return Err(_), got Ok({:?})",
            dangling, handle
        ),
    }

    match kernel.query(&GeometryQuery::Volume(dangling)) {
        Err(_) => {}
        Ok(value) => panic!(
            "query on a dangling handle {:?} must return Err(_), got Ok({:?})",
            dangling, value
        ),
    }

    match kernel.export(dangling, ExportFormat::Step, &mut Vec::new()) {
        Err(_) => {}
        Ok(()) => panic!(
            "export on a dangling handle {:?} must return Err(_), got Ok(())",
            dangling
        ),
    }

    match kernel.tessellate(dangling, 0.1) {
        Err(_) => {}
        Ok(mesh) => panic!(
            "tessellate on a dangling handle {:?} must return Err(_), got Ok({:?})",
            dangling, mesh
        ),
    }
}

/// Assert the STUB-arm error taxonomy (the mode-dependent axis for an
/// all-error kernel): every method returns a specific, kernel-identifying
/// error variant. This is the superset of the retired
/// [`assert_stub_kernel_errors!`] coverage plus `extract_edges`/
/// `extract_faces`/`extract_vertices`.
///
/// | Method | Required variant | `substr` check |
/// |--------|-------------------|----------------|
/// | `execute` (Union/Difference/Intersection) | `GeometryError::OperationFailed` | yes |
/// | `query` (Volume) | `QueryError::QueryFailed` | yes |
/// | `export` (Step) | `ExportError::FormatError` | yes |
/// | `tessellate` | `TessError::TessellationFailed` | yes |
/// | `extract_edges`/`extract_faces`/`extract_vertices` | `QueryError::QueryFailed` | no |
///
/// `extract_*` is checked for variant only, not `substr`: the
/// [`GeometryKernel::extract_edges`] trait default (inherited by every
/// stub that doesn't implement topology extraction) returns a fixed,
/// kernel-agnostic message ("topology extraction not supported by this
/// kernel") rather than one parameterized by the kernel's identity, so
/// requiring `substr` there would fail every conforming stub that relies
/// on the default.
///
/// Panics naming the offending method on a wrong variant or (for the
/// `substr`-checked methods) a message missing `substr`.
pub fn assert_all_error_taxonomy<K: GeometryKernel + ?Sized>(kernel: &mut K, substr: &str) {
    let ops = [
        GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        },
        GeometryOp::Difference {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        },
        GeometryOp::Intersection {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        },
    ];
    for op in &ops {
        match kernel.execute(op) {
            Err(GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains(substr),
                    "execute error message must contain {:?} for op {:?}, got: {:?}",
                    substr,
                    op,
                    msg
                );
            }
            other => panic!(
                "expected Err(GeometryError::OperationFailed(_)) for op {:?}, got {:?}",
                op, other
            ),
        }
    }

    match kernel.query(&GeometryQuery::Volume(GeometryHandleId(1))) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains(substr),
                "query error message must contain {:?}, got: {:?}",
                substr,
                msg
            );
        }
        other => panic!(
            "expected Err(QueryError::QueryFailed(_)) from query, got {:?}",
            other
        ),
    }

    match kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut Vec::new()) {
        Err(ExportError::FormatError(msg)) => {
            assert!(
                msg.contains(substr),
                "export error message must contain {:?}, got: {:?}",
                substr,
                msg
            );
        }
        other => panic!(
            "expected Err(ExportError::FormatError(_)) from export, got {:?}",
            other
        ),
    }

    match kernel.tessellate(GeometryHandleId(1), 0.1) {
        Err(TessError::TessellationFailed(msg)) => {
            assert!(
                msg.contains(substr),
                "tessellate error message must contain {:?}, got: {:?}",
                substr,
                msg
            );
        }
        other => panic!(
            "expected Err(TessError::TessellationFailed(_)) from tessellate, got {:?}",
            other
        ),
    }

    match kernel.extract_edges(GeometryHandleId(1)) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryError::QueryFailed(_)) from extract_edges, got {:?}",
            other
        ),
    }

    match kernel.extract_faces(GeometryHandleId(1)) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryError::QueryFailed(_)) from extract_faces, got {:?}",
            other
        ),
    }

    match kernel.extract_vertices(GeometryHandleId(1)) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryError::QueryFailed(_)) from extract_vertices, got {:?}",
            other
        ),
    }
}

/// Assert the REAL-arm dangling-handle taxonomy (the mode-dependent axis
/// for a genuine kernel): the two verified `reify-kernel-occt` mappings —
/// `execute` on an operand it never produced returns
/// `GeometryError::InvalidReference`, and `query` on a handle it never
/// produced returns `QueryError::InvalidHandle`.
///
/// Only these two mappings are asserted (not `export`/`tessellate`
/// variants): θ cannot depend on a kernel crate to observe real OCCT, so
/// pinning unverified variants here risks red-ing ι's real instantiation.
/// See [`assert_all_error_taxonomy`] for the STUB-arm counterpart.
///
/// Panics with the observed variant on mismatch.
pub fn assert_dangling_reference_taxonomy<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    dangling: GeometryHandleId,
) {
    match kernel.execute(&GeometryOp::Union {
        left: dangling,
        right: dangling,
    }) {
        Err(GeometryError::InvalidReference(_)) => {}
        other => panic!(
            "expected Err(GeometryError::InvalidReference(_)) from execute on a dangling handle, got {:?}",
            other
        ),
    }

    match kernel.query(&GeometryQuery::Volume(dangling)) {
        Err(QueryError::InvalidHandle(_)) => {}
        other => panic!(
            "expected Err(QueryError::InvalidHandle(_)) from query on a dangling handle, got {:?}",
            other
        ),
    }
}

#[cfg(test)]
mod tests {
    use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};
    use std::panic::{catch_unwind, AssertUnwindSafe};

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

    /// All-error kernel whose `query_many` override violates the length
    /// invariant by always returning an empty `Vec`, regardless of how
    /// many queries were passed in.
    struct WrongLengthQueryManyKernel;

    impl GeometryKernel for WrongLengthQueryManyKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed(STUB_MSG.into()))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed(STUB_MSG.into()))
        }

        fn query_many(&self, _queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            Ok(Vec::new())
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

    #[test]
    fn query_many_length_helper_passes_conforming_and_catches_wrong_length() {
        // TestStubKernel inherits the trait default `query_many`, which
        // trivially preserves the length invariant — must not panic.
        super::assert_query_many_length_invariant(&TestStubKernel::new(), GeometryHandleId(1));

        // WrongLengthQueryManyKernel always returns an empty Vec — must panic.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_query_many_length_invariant(
                &WrongLengthQueryManyKernel,
                GeometryHandleId(1),
            );
        }));
        assert!(
            result.is_err(),
            "assert_query_many_length_invariant must panic when query_many violates the length invariant"
        );
    }

    /// All-error kernel whose `extract_edges`/`extract_faces`/`extract_vertices`
    /// each mint a fresh id on every call, violating the determinism the real
    /// contract requires (idempotent per parent handle).
    struct UnstableExtractKernel {
        counter: u64,
    }

    impl UnstableExtractKernel {
        fn new() -> Self {
            Self { counter: 0 }
        }

        fn next_id(&mut self) -> GeometryHandleId {
            let id = self.counter;
            self.counter += 1;
            GeometryHandleId(id)
        }
    }

    impl GeometryKernel for UnstableExtractKernel {
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

        fn extract_edges(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(vec![self.next_id()])
        }

        fn extract_faces(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(vec![self.next_id()])
        }

        fn extract_vertices(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(vec![self.next_id()])
        }
    }

    #[test]
    fn extract_determinism_helper_passes_conforming_and_catches_unstable() {
        // TestStubKernel inherits the trait default extract_*, which always
        // returns the same Err — must not panic.
        super::assert_extract_determinism(&mut TestStubKernel::new(), GeometryHandleId(1));

        // UnstableExtractKernel mints a fresh id every call — must panic.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_extract_determinism(&mut UnstableExtractKernel::new(), GeometryHandleId(1));
        }));
        assert!(
            result.is_err(),
            "assert_extract_determinism must panic when extract_* is not idempotent per handle"
        );
    }

    /// Kernel that silently accepts any handle — including one it never
    /// produced via `execute` — instead of reporting it as invalid. Models
    /// a kernel that skips handle validation entirely.
    struct AcceptsDanglingKernel;

    impl GeometryKernel for AcceptsDanglingKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Ok(GeometryHandle {
                id: GeometryHandleId(0),
                repr: None,
            })
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Ok(Value::Real(0.0))
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Ok(())
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            Ok(Mesh {
                vertices: Vec::new(),
                indices: Vec::new(),
                normals: None,
            })
        }
    }

    #[test]
    fn dangling_is_err_helper_passes_stub_and_catches_accepting_kernel() {
        // TestStubKernel errors unconditionally, including for a dangling
        // handle — must not panic.
        super::assert_dangling_handle_is_err(&mut TestStubKernel::new(), GeometryHandleId(999));

        // AcceptsDanglingKernel returns Ok for a handle it never created — must panic.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_dangling_handle_is_err(&mut AcceptsDanglingKernel, GeometryHandleId(999));
        }));
        assert!(
            result.is_err(),
            "assert_dangling_handle_is_err must panic when the kernel accepts a dangling handle"
        );
    }

    /// All-error stub kernel whose `execute` returns the wrong error
    /// variant for the stub taxonomy: `GeometryError::InvalidReference`
    /// (the REAL-kernel dangling-handle variant, see
    /// `assert_dangling_reference_taxonomy`) instead of the stub's
    /// `GeometryError::OperationFailed`. Used to prove
    /// [`assert_all_error_taxonomy`] catches a stub that diverges from the
    /// stub-arm taxonomy on a single method.
    struct WrongTaxonomyStub {
        _private: (),
    }

    impl WrongTaxonomyStub {
        fn new() -> Self {
            Self { _private: () }
        }
    }

    impl GeometryKernel for WrongTaxonomyStub {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::InvalidReference(GeometryHandleId(0)))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed(
                "WrongTaxonomy kernel not available — fixture only".into(),
            ))
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Err(ExportError::FormatError(
                "WrongTaxonomy kernel not available — fixture only".into(),
            ))
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            Err(TessError::TessellationFailed(
                "WrongTaxonomy kernel not available — fixture only".into(),
            ))
        }
    }

    #[test]
    fn all_error_taxonomy_helper_passes_stub_and_catches_wrong_variant() {
        // TestStubKernel matches the stub-arm taxonomy exactly — must not panic.
        super::assert_all_error_taxonomy(&mut TestStubKernel::new(), "TestStub");

        // WrongTaxonomyStub's execute returns InvalidReference, not
        // OperationFailed — must panic.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_all_error_taxonomy(&mut WrongTaxonomyStub::new(), "WrongTaxonomy");
        }));
        assert!(
            result.is_err(),
            "assert_all_error_taxonomy must panic when execute returns the wrong error variant"
        );
    }

    /// Real-like kernel fixture that models the verified OCCT dangling-handle
    /// taxonomy (`execute` on an unknown operand → `InvalidReference`;
    /// `query`/`export`/`tessellate`/`extract_*` on an unknown handle →
    /// `InvalidHandle`) without depending on a kernel crate. Mirrors
    /// `MockGeometryKernel`'s handle-set + next_id pattern (mocks.rs:830),
    /// but — unlike that mock — actually rejects unknown handles.
    struct TestRealKernel {
        next_id: u64,
        handles: std::collections::HashSet<GeometryHandleId>,
    }

    impl TestRealKernel {
        fn new() -> Self {
            Self {
                next_id: 1,
                handles: std::collections::HashSet::new(),
            }
        }

        fn fresh_handle(&mut self) -> GeometryHandleId {
            let id = GeometryHandleId(self.next_id);
            self.next_id += 1;
            self.handles.insert(id);
            id
        }

        fn extract_sub(&self, handle: GeometryHandleId) -> Result<Vec<GeometryHandleId>, QueryError> {
            if self.handles.contains(&handle) {
                // Deterministic per-parent id, distinct from real handle ids.
                Ok(vec![GeometryHandleId(handle.0 * 1000 + 1)])
            } else {
                Err(QueryError::InvalidHandle(handle))
            }
        }
    }

    impl GeometryKernel for TestRealKernel {
        fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            if let GeometryOp::Union { left, .. } = op {
                if !self.handles.contains(left) {
                    return Err(GeometryError::InvalidReference(*left));
                }
            }
            let id = self.fresh_handle();
            Ok(GeometryHandle {
                id,
                repr: Some(reify_ir::BRepKind::Solid),
            })
        }

        fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
            let handle = match query {
                GeometryQuery::Volume(id) | GeometryQuery::SurfaceArea(id) => *id,
                _ => {
                    return Err(QueryError::QueryFailed(
                        "TestRealKernel: query variant not modeled".into(),
                    ));
                }
            };
            if self.handles.contains(&handle) {
                Ok(Value::Real(1.0))
            } else {
                Err(QueryError::InvalidHandle(handle))
            }
        }

        fn export(
            &self,
            handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            if self.handles.contains(&handle) {
                Ok(())
            } else {
                Err(ExportError::InvalidHandle(handle))
            }
        }

        fn tessellate(&self, handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
            if self.handles.contains(&handle) {
                Ok(Mesh {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    normals: None,
                })
            } else {
                Err(TessError::InvalidHandle(handle))
            }
        }

        fn extract_edges(
            &mut self,
            handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.extract_sub(handle)
        }

        fn extract_faces(
            &mut self,
            handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.extract_sub(handle)
        }

        fn extract_vertices(
            &mut self,
            handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.extract_sub(handle)
        }
    }

    #[test]
    fn dangling_reference_taxonomy_helper_passes_real_and_catches_stub_divergence() {
        let mut kernel = TestRealKernel::new();
        kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .expect("TestRealKernel::execute(Box) must succeed to create a valid handle");

        // TestRealKernel maps a dangling handle to InvalidReference/InvalidHandle — must not panic.
        super::assert_dangling_reference_taxonomy(&mut kernel, GeometryHandleId(9999));

        // TestStubKernel returns OperationFailed, not InvalidReference — must
        // panic. This is the canonical stub/real taxonomy divergence.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_dangling_reference_taxonomy(&mut TestStubKernel::new(), GeometryHandleId(9999));
        }));
        assert!(
            result.is_err(),
            "assert_dangling_reference_taxonomy must panic when given a stub kernel in place of a real kernel"
        );
    }

    // The both-arms-from-one-source suite self-test: instantiate
    // `assert_kernel_contract!` for a conforming stub and a conforming
    // real-like kernel. Once the macro exists, each instantiation expands
    // to separately-named `#[test]` fns that must pass under whatever cfg
    // this crate is built with.
    crate::assert_kernel_contract!(stub; TestStubKernel::new, "TestStub");
    crate::assert_kernel_contract!(real; TestRealKernel::new, valid_op = GeometryOp::Box {
        width: Value::Real(10.0),
        height: Value::Real(10.0),
        depth: Value::Real(10.0),
    });
}
