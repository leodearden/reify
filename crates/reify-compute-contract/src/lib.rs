//! Pure `ComputeFn` contract value types for Reify — OCCT-free foundation crate.
//!
//! Extracted from `reify-eval` (task A / #4934) so that `reify-eval` AND future
//! non-OCCT evaluation engines (e.g. `reify-eval-fea`, task B) can depend on the
//! compute-node contract's value types without pulling in OCCT.
//!
//! `reify-eval` still re-exports the compute-dispatch types (`ComputeFn`,
//! `ComputeOutcome`, `DispatchError`, `RealizationReadHandle`,
//! `RealizedContent`, `StructuredComputeDetail`) at its crate root. The
//! FEA-result cluster (`ElasticResult`, `ShellChannels`,
//! `AposterioriEstimate`, `max_deflection_magnitude`) is consumed from this
//! crate directly.
//!
//! See `docs/prds/v0_3/compute-node-contract.md` for the compute-node contract
//! spec and the task A plan (`.task/plan.json`) for the extraction rationale
//! (in particular the orphan-rule analysis that pulls `ElasticResult` and its
//! supporting f64-slab codec into this crate alongside the dispatch types).

mod cancellation;
pub use cancellation::CancellationHandle;

mod dispatch;
pub use dispatch::{ComputeFn, ComputeOutcome, DispatchError, StructuredComputeDetail};

mod realization;
pub use realization::{RealizationReadHandle, RealizedContent};

mod elastic_result;
pub use elastic_result::{
    check_f64_vec_len, max_deflection_magnitude, read_f64_slab, write_f64_slab,
    AposterioriEstimate, ElasticResult, ShellChannels, MAX_F64_ELEMENTS,
};
#[cfg(any(test, target_endian = "big"))]
pub use elastic_result::decode_f64_slab_from_le_bytes;
