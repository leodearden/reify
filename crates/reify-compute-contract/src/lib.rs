//! Pure `ComputeFn` contract value types for Reify — OCCT-free foundation crate.
//!
//! Extracted from `reify-eval` (task A / #4934) so that `reify-eval` AND future
//! non-OCCT evaluation engines (e.g. `reify-eval-fea`, task B) can depend on the
//! compute-node contract's value types without pulling in OCCT.
//!
//! `reify-eval` re-exports every type this crate defines at its original public
//! paths (crate-root AND originating module), so downstream crates continue to
//! reference `reify_eval::ComputeFn` etc. unchanged — this crate is an internal
//! extraction, not a new public surface for external consumers to adopt directly.
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
