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
//! [`::reify_ir::GeometryKernel`] when called as `$factory()`.
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
//!
//! # Superseded by `assert_kernel_contract!`
//!
//! [`assert_kernel_contract!`] is the shared, cross-cfg successor to this
//! macro: one suite source (this file's `pub fn assert_*` helpers) that a
//! consumer instantiates for either a `stub` kernel (`not(has_occt)`) or a
//! `real` kernel (`has_occt`). It covers this macro's all-error taxonomy
//! plus the `query_many` length invariant and `extract_*` stability, and
//! adds the `real` arm this macro has no equivalent for.
//!
//! `assert_stub_kernel_errors!` is retained byte-for-byte for its existing
//! `reify-kernel-openvdb`/`reify-kernel-gmsh` consumers; migrating them to
//! `assert_kernel_contract!(stub; ..)` is a follow-up. New consumers
//! (starting with the OCCT kernel, task ι) should use
//! [`assert_kernel_contract!`] instead of this macro.

// Used by the public `assert_*` helper fns below. The exported macros
// (`assert_stub_kernel_errors!`, `assert_kernel_contract!` and friends) use
// fully-qualified `::reify_ir::` paths instead, since macro-generated code
// expands in the caller's scope.
use reify_ir::{
    ExportError, ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp,
    GeometryQuery, QueryError, TessError,
};

/// Assert the all-error stub-kernel contract for a [`::reify_ir::GeometryKernel`]
/// implementation by generating three independent `#[test]` functions.
///
/// Superseded by [`assert_kernel_contract!`] for new consumers; retained
/// unchanged for its existing `reify-kernel-openvdb`/`reify-kernel-gmsh`
/// call sites (see the module-level docs).
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

/// Assert the `query_many` length invariant documented on
/// [`::reify_ir::GeometryKernel::query_many`]: the output length must
/// track the input length, and a single-element batch must agree with
/// the equivalent [`GeometryKernel::query`] call.
///
/// Panics with a descriptive message if any of the following fail:
/// - `query_many(&[])` returns `Ok(v)` with `v.is_empty()`.
/// - `query_many` on a homogeneous 2-element `Volume` batch, when `probe`
///   is a handle the per-element `Volume` query succeeds on, returns
///   `Ok(v)` with `v.len() == 2` — a conforming kernel's `query_many`
///   must not error on a batch it would accept element-by-element. The
///   batch intentionally repeats a single query kind (`Volume`) instead
///   of mixing in `SurfaceArea`, so this is exercised for any kernel that
///   supports at least one query kind, independent of whether it has
///   wired up `SurfaceArea`. `Err(_)` is only tolerated when `probe` is
///   itself invalid (e.g. the STUB arm's dangling probe, where every
///   query errors).
/// - `query_many` on a 1-element input is `Debug`-equal to the
///   corresponding single `query` call (both `Err` with the same debug
///   representation, or both `Ok` with the same single value). Debug
///   representations are compared rather than `==` because neither
///   `Value` nor `QueryError` derive `PartialEq` (both derive only
///   `Debug, Clone`, and `QueryError` is `#[non_exhaustive]`); this
///   relies on their derived `Debug` output being deterministic and
///   content-complete.
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

    // Determine whether `probe` is a handle this kernel considers valid
    // by observing the per-element `query` outcome for the query kind
    // used in the batch below. `volume_result` is also reused for the
    // single-vs-many agreement check further down, rather than issuing a
    // second, redundant `query` call for the same input.
    let volume_result = kernel.query(&GeometryQuery::Volume(probe));
    let probe_queries_succeed = volume_result.is_ok();

    // Homogeneous batch (both elements the same query kind) so the length
    // invariant is exercised for any kernel that supports at least
    // `Volume`, independent of `SurfaceArea` support — the invariant
    // itself has no dependency on batch heterogeneity.
    let two = [GeometryQuery::Volume(probe), GeometryQuery::Volume(probe)];
    match kernel.query_many(&two) {
        Ok(v) if v.len() == two.len() => {}
        // Only tolerated when the probe itself is invalid: callers always
        // pass a probe the kernel considers canonical for its mode (a
        // dangling id for the STUB arm, a freshly-executed handle for the
        // REAL arm), so unconditionally accepting `Err(_)` here would let
        // a kernel that unexpectedly errors on a valid batch pass this
        // check unexercised.
        Err(_) if !probe_queries_succeed => {}
        other => panic!(
            "query_many on {} queries must return {}, got {:?}",
            two.len(),
            if probe_queries_succeed {
                "Ok(v) with v.len() == queries.len(), since the per-element Volume query on probe succeeds"
            } else {
                "Err(_) or Ok(v) with v.len() == queries.len()"
            },
            other
        ),
    }

    let many_result = kernel.query_many(&[GeometryQuery::Volume(probe)]);
    let many_desc = match &many_result {
        Ok(v) if v.len() == 1 => format!("Ok({:?})", v[0]),
        Ok(v) => format!("Ok(<wrong length {}>: {:?})", v.len(), v),
        Err(e) => format!("Err({:?})", e),
    };
    let single_desc = match &volume_result {
        Ok(v) => format!("Ok({:?})", v),
        Err(e) => format!("Err({:?})", e),
    };
    assert_eq!(
        many_desc, single_desc,
        "query_many(&[q]) must agree with query(&q): query_many gave {:?}, query gave {:?}",
        many_result, volume_result
    );
}

/// Calls `extract_edges`/`extract_faces`/`extract_vertices` on `handle`
/// and pairs each result with a `&'static str` label. The single point of
/// edit for a fourth extractor: [`assert_extract_determinism`],
/// [`assert_extract_succeeds`], and [`assert_all_error_taxonomy`] all
/// iterate over this instead of repeating a near-identical
/// edges/faces/vertices block each.
fn extract_all<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
) -> [(&'static str, Result<Vec<GeometryHandleId>, QueryError>); 3] {
    [
        ("extract_edges", kernel.extract_edges(handle)),
        ("extract_faces", kernel.extract_faces(handle)),
        ("extract_vertices", kernel.extract_vertices(handle)),
    ]
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
/// The two calls are compared via `format!("{:?}", _)` rather than `==`:
/// the `Ok` side (`Vec<GeometryHandleId>`) does implement `PartialEq`,
/// but `QueryError` derives only `Debug, Clone` (and is
/// `#[non_exhaustive]`), so `Result<Vec<GeometryHandleId>, QueryError>`
/// has no `PartialEq` impl as a whole. Debug-string equality compares
/// both arms uniformly, relying on the derived `Debug` output being
/// deterministic and content-complete.
///
/// Panics naming the offending method on divergence.
pub fn assert_extract_determinism<K: GeometryKernel + ?Sized>(kernel: &mut K, handle: GeometryHandleId) {
    let first = extract_all(kernel, handle);
    let second = extract_all(kernel, handle);
    for ((name, r1), (_, r2)) in first.into_iter().zip(second) {
        assert_eq!(
            format!("{:?}", r1),
            format!("{:?}", r2),
            "{} must be idempotent per handle: first call {:?}, second call {:?}",
            name,
            r1,
            r2
        );
    }
}

/// Assert that `extract_edges`/`extract_faces`/`extract_vertices` succeed
/// (`Ok(_)`) on `handle` — the REAL-arm positive-signal counterpart to
/// [`assert_extract_determinism`]. Determinism alone passes trivially for
/// a kernel whose topology extraction is broken or unimplemented (two
/// calls returning the *same* `Err` are still "idempotent"), so a real
/// kernel's instantiation additionally needs proof that extraction
/// actually works on a valid handle. The STUB arm has no equivalent
/// call: a stub kernel's `extract_*` is expected to always error (see
/// [`assert_all_error_taxonomy`]), so asserting `Ok(_)` there would fail
/// every conforming stub.
///
/// Panics naming the offending method if it returns `Err` for `handle`.
pub fn assert_extract_succeeds<K: GeometryKernel + ?Sized>(kernel: &mut K, handle: GeometryHandleId) {
    for (name, result) in extract_all(kernel, handle) {
        if let Err(e) = result {
            panic!(
                "{} on a valid handle {:?} must return Ok(_), got Err({:?})",
                name, handle, e
            );
        }
    }
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

    for (name, result) in extract_all(kernel, GeometryHandleId(1)) {
        match result {
            Err(QueryError::QueryFailed(_)) => {}
            other => panic!(
                "expected Err(QueryError::QueryFailed(_)) from {}, got {:?}",
                name, other
            ),
        }
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

/// Shared body for [`assert_kernel_contract!`]'s `Send + Sync` +
/// `Box<dyn GeometryKernel>` upcast test, factored out so the `stub` and
/// `real` arms — otherwise byte-identical here except for the generated
/// fn's name — can't drift out of sync. Not part of the public API; use
/// [`assert_kernel_contract!`] instead.
#[doc(hidden)]
#[macro_export]
macro_rules! __kernel_contract_send_sync_and_box_upcast_test {
    ($test_name:ident, $factory:expr) => {
        /// Compile-time `Send + Sync` pin and `Box<dyn GeometryKernel>` upcast.
        #[test]
        fn $test_name() {
            fn assert_send_sync<T: ::core::marker::Send + ::core::marker::Sync>(_: &T) {}
            let kernel = ($factory)();
            assert_send_sync(&kernel);
            let _boxed: ::std::boxed::Box<dyn ::reify_ir::GeometryKernel> =
                ::std::boxed::Box::new(kernel);
        }
    };
}

/// Shared body for [`assert_kernel_contract!`]'s
/// [`assert_dangling_handle_is_err`] wiring, factored out so the `stub`
/// and `real` arms — otherwise byte-identical here except for the
/// generated fn's name — can't drift out of sync. Not part of the public
/// API; use [`assert_kernel_contract!`] instead.
#[doc(hidden)]
#[macro_export]
macro_rules! __kernel_contract_dangling_handle_is_err_test {
    ($test_name:ident, $factory:expr) => {
        /// See [`$crate::kernel_assertions::assert_dangling_handle_is_err`].
        #[test]
        fn $test_name() {
            let mut kernel = ($factory)();
            $crate::kernel_assertions::assert_dangling_handle_is_err(
                &mut kernel,
                ::reify_ir::GeometryHandleId(u64::MAX),
            );
        }
    };
}

/// Assert the shared, cross-cfg `GeometryKernel` contract — error
/// taxonomy, the `query_many` length invariant, and `extract_*`
/// stability — by generating a set of independently-named `#[test]`
/// functions from a single suite source. A consumer instantiates this
/// once per kernel, in whichever cfg (`has_occt` / `not(has_occt)`) that
/// kernel builds under; a stub/real taxonomy divergence fails whichever
/// generated test observes it.
///
/// See the module-level docs for how this relates to
/// [`assert_stub_kernel_errors!`].
///
/// # Arms
///
/// - `assert_kernel_contract!(stub; $factory, $substr);` — for an
///   all-error stub kernel. `$factory` is a callable expression producing
///   a fresh kernel (e.g. `FidgetKernel::new`); `$substr` is a string
///   literal every error message must contain. Runs
///   [`assert_all_error_taxonomy`], [`assert_query_many_length_invariant`],
///   [`assert_extract_determinism`], and [`assert_dangling_handle_is_err`].
/// - `assert_kernel_contract!(real; $factory, valid_op = $op);` — for a
///   kernel that only fails on invalid input. `$op` is a
///   [`::reify_ir::GeometryOp`] expression that `$factory()` can execute
///   successfully; used to obtain a valid handle for the
///   `query_many`/`extract_*` checks. Runs
///   [`assert_dangling_reference_taxonomy`], [`assert_dangling_handle_is_err`],
///   an `$op`-executes-`Ok` check, [`assert_query_many_length_invariant`],
///   [`assert_extract_determinism`], and [`assert_extract_succeeds`] (the
///   positive-signal check that extraction actually works on a valid
///   handle, since determinism alone passes trivially for a kernel that
///   always errors).
///
/// Both arms also generate a `Send + Sync` + `Box<dyn GeometryKernel>`
/// upcast test, mirroring [`assert_stub_kernel_errors!`]'s first test.
///
/// # Instantiation pattern (task ι, the OCCT-adoption leaf this suite
/// exists for)
///
/// ```ignore
/// #[cfg(test)]
/// mod tests {
///     #[cfg(has_occt)]
///     reify_test_support::assert_kernel_contract!(
///         real;
///         OcctKernel::new,
///         valid_op = ::reify_ir::GeometryOp::Box {
///             width: Value::Real(10.0),
///             height: Value::Real(10.0),
///             depth: Value::Real(10.0),
///         },
///     );
///
///     #[cfg(not(has_occt))]
///     reify_test_support::assert_kernel_contract!(stub; OcctKernel::new, "OCCT");
/// }
/// ```
#[macro_export]
macro_rules! assert_kernel_contract {
    (stub; $factory:expr, $substr:literal $(,)?) => {
        $crate::__kernel_contract_send_sync_and_box_upcast_test!(
            kernel_contract_stub_send_sync_and_box_upcast,
            $factory
        );

        /// See [`$crate::kernel_assertions::assert_all_error_taxonomy`].
        #[test]
        fn kernel_contract_stub_all_error_taxonomy() {
            let mut kernel = ($factory)();
            $crate::kernel_assertions::assert_all_error_taxonomy(&mut kernel, $substr);
        }

        /// See [`$crate::kernel_assertions::assert_query_many_length_invariant`].
        #[test]
        fn kernel_contract_stub_query_many_length_invariant() {
            let kernel = ($factory)();
            $crate::kernel_assertions::assert_query_many_length_invariant(
                &kernel,
                ::reify_ir::GeometryHandleId(1),
            );
        }

        /// See [`$crate::kernel_assertions::assert_extract_determinism`].
        #[test]
        fn kernel_contract_stub_extract_determinism() {
            let mut kernel = ($factory)();
            $crate::kernel_assertions::assert_extract_determinism(
                &mut kernel,
                ::reify_ir::GeometryHandleId(1),
            );
        }

        $crate::__kernel_contract_dangling_handle_is_err_test!(
            kernel_contract_stub_dangling_handle_is_err,
            $factory
        );
    };

    (real; $factory:expr, valid_op = $op:expr $(,)?) => {
        $crate::__kernel_contract_send_sync_and_box_upcast_test!(
            kernel_contract_real_send_sync_and_box_upcast,
            $factory
        );

        /// See [`$crate::kernel_assertions::assert_dangling_reference_taxonomy`].
        #[test]
        fn kernel_contract_real_dangling_reference_taxonomy() {
            let mut kernel = ($factory)();
            $crate::kernel_assertions::assert_dangling_reference_taxonomy(
                &mut kernel,
                ::reify_ir::GeometryHandleId(u64::MAX),
            );
        }

        $crate::__kernel_contract_dangling_handle_is_err_test!(
            kernel_contract_real_dangling_handle_is_err,
            $factory
        );

        /// `valid_op` must execute `Ok` on a fresh kernel from `$factory`.
        #[test]
        fn kernel_contract_real_valid_op_executes_ok() {
            let mut kernel = ($factory)();
            let result = ::reify_ir::GeometryKernel::execute(&mut kernel, &($op));
            assert!(
                result.is_ok(),
                "valid_op must execute Ok on a fresh kernel from $factory, got {:?}",
                result
            );
        }

        /// See [`$crate::kernel_assertions::assert_query_many_length_invariant`].
        #[test]
        fn kernel_contract_real_query_many_length_invariant() {
            let mut kernel = ($factory)();
            let handle = ::reify_ir::GeometryKernel::execute(&mut kernel, &($op))
                .expect("valid_op must execute Ok to produce a probe handle");
            $crate::kernel_assertions::assert_query_many_length_invariant(&kernel, handle.id);
        }

        /// See [`$crate::kernel_assertions::assert_extract_determinism`].
        #[test]
        fn kernel_contract_real_extract_determinism() {
            let mut kernel = ($factory)();
            let handle = ::reify_ir::GeometryKernel::execute(&mut kernel, &($op))
                .expect("valid_op must execute Ok to produce a probe handle");
            $crate::kernel_assertions::assert_extract_determinism(&mut kernel, handle.id);
        }

        /// See [`$crate::kernel_assertions::assert_extract_succeeds`].
        #[test]
        fn kernel_contract_real_extract_succeeds() {
            let mut kernel = ($factory)();
            let handle = ::reify_ir::GeometryKernel::execute(&mut kernel, &($op))
                .expect("valid_op must execute Ok to produce a probe handle");
            $crate::kernel_assertions::assert_extract_succeeds(&mut kernel, handle.id);
        }
    };
}

#[cfg(test)]
mod tests {
    use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const STUB_MSG: &str = "TestStub kernel not available — fixture only";

    /// Unwrap a [`catch_unwind`] result from a negative self-test below and
    /// assert the panic message contains `fragment`, pinning the test to
    /// the *intended* assertion's panic rather than merely "something
    /// panicked". Without this, a future edit that makes an unrelated
    /// assertion fire first would leave these tests green while no longer
    /// verifying the property they claim to.
    fn assert_panic_contains(result: std::thread::Result<()>, fragment: &str) {
        let payload = match result {
            Err(payload) => payload,
            Ok(()) => panic!("expected the wrapped call to panic, but it returned normally"),
        };
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| panic!("panic payload was neither a &str nor a String"));
        assert!(
            message.contains(fragment),
            "expected panic message to contain {:?}, got {:?}",
            fragment,
            message
        );
    }

    /// Generates a `GeometryKernel` impl whose `execute`/`query`/`export`/
    /// `tessellate` all return the stub error shape carrying `$msg`
    /// (`OperationFailed`/`QueryFailed`/`FormatError`/`TessellationFailed`
    /// respectively). An optional `extra { .. }` block is spliced into the
    /// same `impl`, so a fixture that only needs to add or override
    /// `query_many`/`extract_*` (e.g. `WrongLengthQueryManyKernel` below)
    /// doesn't have to repeat the four baseline bodies.
    ///
    /// Fixtures that must diverge on one of the four baseline methods
    /// themselves — not just their message — don't fit this shape and
    /// stay hand-written: `AcceptsDanglingKernel` (all four return `Ok`),
    /// `WrongTaxonomyStub` (`execute` returns the wrong variant), and
    /// `TestRealKernel` (stateful handle validity, not all-error).
    macro_rules! impl_all_error_kernel {
        ($ty:ty, $msg:expr $(, extra { $($extra:tt)* })?) => {
            impl GeometryKernel for $ty {
                fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
                    Err(GeometryError::OperationFailed(($msg).into()))
                }

                fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
                    Err(QueryError::QueryFailed(($msg).into()))
                }

                fn export(
                    &self,
                    _handle: GeometryHandleId,
                    _format: ExportFormat,
                    _writer: &mut dyn std::io::Write,
                ) -> Result<(), ExportError> {
                    Err(ExportError::FormatError(($msg).into()))
                }

                fn tessellate(
                    &self,
                    _handle: GeometryHandleId,
                    _tolerance: f64,
                ) -> Result<Mesh, TessError> {
                    Err(TessError::TessellationFailed(($msg).into()))
                }

                $($($extra)*)?
            }
        };
    }

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

    impl_all_error_kernel!(TestStubKernel, STUB_MSG);

    // Invoke the macro to generate three #[test] fns against the fixture stub.
    crate::assert_stub_kernel_errors!(TestStubKernel::new, "TestStub");

    /// All-error kernel whose `query_many` override violates the length
    /// invariant by always returning an empty `Vec`, regardless of how
    /// many queries were passed in.
    struct WrongLengthQueryManyKernel;

    impl_all_error_kernel!(WrongLengthQueryManyKernel, STUB_MSG, extra {
        fn query_many(&self, _queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            Ok(Vec::new())
        }
    });

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
        assert_panic_contains(result, "query_many on");
    }

    /// All-error kernel whose `query_many` returns a length-correct but
    /// content-divergent result for a single-element batch:
    /// `query_many(&[q])` returns `Ok(vec![Value::Real(42.0)])`, while the
    /// equivalent `query(&q)` call returns `Err(_)` (inherited from
    /// [`impl_all_error_kernel!`]). `WrongLengthQueryManyKernel` above
    /// panics on the earlier length check and never reaches the
    /// single-vs-many agreement `assert_eq!`, so this fixture isolates it:
    /// same length (1), disagreeing content.
    struct DivergentSingleQueryManyKernel;

    impl_all_error_kernel!(DivergentSingleQueryManyKernel, STUB_MSG, extra {
        fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            match queries.len() {
                0 => Ok(Vec::new()),
                1 => Ok(vec![Value::Real(42.0)]),
                _ => Err(QueryError::QueryFailed(STUB_MSG.into())),
            }
        }
    });

    #[test]
    fn query_many_length_helper_catches_single_batch_content_divergence() {
        // DivergentSingleQueryManyKernel's query_many(&[q]) returns
        // Ok(42.0) while query(&q) returns Err(_) — same length, wrong
        // content — must panic on the agreement check specifically.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_query_many_length_invariant(
                &DivergentSingleQueryManyKernel,
                GeometryHandleId(1),
            );
        }));
        assert_panic_contains(result, "query_many(&[q]) must agree with query(&q)");
    }

    /// Kernel whose per-element `query` always succeeds (models a kernel
    /// for which `probe` is valid), but whose `query_many` override
    /// incorrectly returns `Err` for a non-empty batch. Used to prove the
    /// `Err(_) if !probe_queries_succeed` guard in
    /// [`assert_query_many_length_invariant`] actually restricts the
    /// tolerated-`Err` branch to an *invalid* probe:
    /// `WrongLengthQueryManyKernel` above only ever exercises that guard
    /// with an invalid probe (its `query` always errors too), so a
    /// regression that dropped the `!probe_queries_succeed` condition —
    /// silently tolerating `Err` from `query_many` even when the probe is
    /// valid — would ship undetected without this fixture.
    struct ErroringQueryManyWithValidProbeKernel;

    impl GeometryKernel for ErroringQueryManyWithValidProbeKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed(STUB_MSG.into()))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            Ok(Value::Real(1.0))
        }

        fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            if queries.is_empty() {
                Ok(Vec::new())
            } else {
                Err(QueryError::QueryFailed(STUB_MSG.into()))
            }
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
    fn query_many_length_helper_catches_erroring_query_many_with_valid_probe() {
        // query(probe) succeeds, but query_many on a 2-element batch of
        // the same query wrongly returns Err — the guard must not
        // tolerate this, since the probe is valid.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_query_many_length_invariant(
                &ErroringQueryManyWithValidProbeKernel,
                GeometryHandleId(1),
            );
        }));
        assert_panic_contains(result, "query_many on 2 queries must return");
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

    impl_all_error_kernel!(UnstableExtractKernel, STUB_MSG, extra {
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
    });

    #[test]
    fn extract_determinism_helper_passes_conforming_and_catches_unstable() {
        // TestStubKernel inherits the trait default extract_*, which always
        // returns the same Err — must not panic.
        super::assert_extract_determinism(&mut TestStubKernel::new(), GeometryHandleId(1));

        // UnstableExtractKernel mints a fresh id every call — must panic.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_extract_determinism(&mut UnstableExtractKernel::new(), GeometryHandleId(1));
        }));
        assert_panic_contains(result, "extract_edges must be idempotent");
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
        assert_panic_contains(result, "execute on a dangling handle");
    }

    /// Which single handle-taking method (besides `execute`, which always
    /// errors) [`AcceptsDanglingOnlyAt`] accepts a dangling handle on.
    enum DanglingAcceptPoint {
        Query,
        Export,
        Tessellate,
    }

    /// Kernel that errors on `execute` and on every handle-taking method
    /// except the single one named by its [`DanglingAcceptPoint`], which
    /// it silently accepts (`Ok`) even for a dangling handle.
    /// `AcceptsDanglingKernel` above accepts on all four methods, so
    /// [`assert_dangling_handle_is_err`]'s negative self-test using it
    /// only ever reaches the `execute` panic arm — a bug in the
    /// query/export/tessellate detection would ship undetected. This
    /// fixture isolates each remaining branch: erroring everywhere but
    /// the one method under test forces
    /// [`assert_dangling_handle_is_err`] to walk past the earlier
    /// method(s) to reach it.
    struct AcceptsDanglingOnlyAt(DanglingAcceptPoint);

    impl GeometryKernel for AcceptsDanglingOnlyAt {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed(STUB_MSG.into()))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            if matches!(self.0, DanglingAcceptPoint::Query) {
                Ok(Value::Real(0.0))
            } else {
                Err(QueryError::QueryFailed(STUB_MSG.into()))
            }
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            if matches!(self.0, DanglingAcceptPoint::Export) {
                Ok(())
            } else {
                Err(ExportError::FormatError(STUB_MSG.into()))
            }
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            if matches!(self.0, DanglingAcceptPoint::Tessellate) {
                Ok(Mesh {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    normals: None,
                })
            } else {
                Err(TessError::TessellationFailed(STUB_MSG.into()))
            }
        }
    }

    #[test]
    fn dangling_is_err_helper_catches_accepting_kernel_on_query_export_tessellate() {
        // Isolate the `query` branch: execute errors, query wrongly accepts.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_dangling_handle_is_err(
                &mut AcceptsDanglingOnlyAt(DanglingAcceptPoint::Query),
                GeometryHandleId(999),
            );
        }));
        assert_panic_contains(result, "query on a dangling handle");

        // Isolate the `export` branch: execute/query error, export wrongly accepts.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_dangling_handle_is_err(
                &mut AcceptsDanglingOnlyAt(DanglingAcceptPoint::Export),
                GeometryHandleId(999),
            );
        }));
        assert_panic_contains(result, "export on a dangling handle");

        // Isolate the `tessellate` branch: execute/query/export error,
        // tessellate wrongly accepts.
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_dangling_handle_is_err(
                &mut AcceptsDanglingOnlyAt(DanglingAcceptPoint::Tessellate),
                GeometryHandleId(999),
            );
        }));
        assert_panic_contains(result, "tessellate on a dangling handle");
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
        assert_panic_contains(result, "OperationFailed");
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
            if let GeometryOp::Union { left, .. } = op
                && !self.handles.contains(left)
            {
                return Err(GeometryError::InvalidReference(*left));
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
        assert_panic_contains(result, "InvalidReference");
    }

    #[test]
    fn extract_succeeds_helper_passes_real_and_catches_broken_extraction() {
        let mut kernel = TestRealKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(10.0),
                depth: Value::Real(10.0),
            })
            .expect("TestRealKernel::execute(Box) must succeed to create a valid handle");

        // TestRealKernel's extract_* succeeds on a handle it created — must not panic.
        super::assert_extract_succeeds(&mut kernel, handle.id);

        // TestStubKernel's extract_* always errors, even for a handle the
        // caller treats as valid — must panic. Models a real kernel whose
        // topology extraction is broken or unimplemented, which
        // `assert_extract_determinism` alone would miss (two equal `Err`s
        // still look "idempotent").
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::assert_extract_succeeds(&mut TestStubKernel::new(), GeometryHandleId(1));
        }));
        assert_panic_contains(result, "extract_edges on a valid handle");
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
