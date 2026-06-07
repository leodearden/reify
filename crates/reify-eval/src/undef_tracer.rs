//! Undef-cause tracer: all-causes DAG walk (PRD undef-self-describing β, task 4322).
//!
//! Reconstructs the **complete, deduplicated** set of root [`UndefCause`]s for an
//! undef cell by following forward dependency edges through undef cells only,
//! cycle-guarded by a visited-cell set.
//!
//! # Design decisions (see plan.json)
//!
//! * **Free function `trace_undef_causes`**: takes explicit state (origins,
//!   dep_map, values, start) for synthetic-input unit-testing — no solver, no
//!   real eval required to exercise cycle / dedup / order invariants.
//! * **Thin `Engine::trace_undef_causes` wrapper** (in `engine_admin.rs`): the
//!   consumer-facing API for δ (CLI) / ε (GUI) / ζ (LSP).
//! * **Dedup by originating cell** (via the visited-cell set), NOT by
//!   `UndefCause` value — so two independent cells with an identical
//!   `SolveFailed{detail}` are both returned (PRD Q4).
//! * **Order-stability (B1)**: `DependencyMap::forward_reachable` returns cells
//!   sorted by `ValueCellId`; the result `Vec<UndefCause>` therefore inherits
//!   that order.

use std::collections::HashMap;

use reify_core::ValueCellId;
use reify_ir::{DeterminacyState, PersistentMap, UndefCause, Value};

use crate::deps::DependencyMap;

/// Reconstruct the complete set of root [`UndefCause`]s for `start`.
///
/// Walks forward dependencies from `start`, expanding only cells whose value is
/// undef (recurse predicate: `values.get(c) → v.is_undef()`; absent ⇒ treat as
/// undef per α's convention).  Collects each visited cell's recorded origin from
/// `origins`; cells with no recorded origin contribute nothing (they are
/// propagated undef cells — PRD A3).
///
/// # Deduplication
///
/// Dedup is by **originating cell** — each cell appears in the traversal at most
/// once (visited-set).  Two independent cells carrying an identical
/// `SolveFailed{detail}` are both returned.
///
/// # Order
///
/// Output is ordered by originating `ValueCellId` ascending, matching
/// `forward_reachable`'s sorted output (B1).
///
/// # Cycle safety
///
/// Cycles terminate via the visited-cell set inside `forward_reachable` (BT7).
pub fn trace_undef_causes(
    origins: &HashMap<ValueCellId, UndefCause>,
    dep_map: &DependencyMap,
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    start: &ValueCellId,
) -> Vec<UndefCause> {
    // STUB — returns empty vec so TDD test steps fail on assertion, not on missing symbol.
    // Replaced in step-4 (impl).
    let _ = (origins, dep_map, values, start);
    Vec::new()
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Tests added in step-3 (RED).
}
