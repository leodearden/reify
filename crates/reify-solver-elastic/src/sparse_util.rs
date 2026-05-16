//! Shared sparse-matrix helpers for the elastic solver kernel.
//!
//! This module collects small utilities that operate on faer's CSR
//! representation (`col_idx`, `row_ptr`, `vals` slices) and are reused across
//! multiple call sites — currently [`find_in_row`], which is shared between
//! the Dirichlet boundary-condition eliminator and the MPC row eliminator.
//!
//! # Invariants
//!
//! All helpers here assume faer's **soft invariant**: column indices within
//! each CSR row are sorted in ascending order.  Callers that build their `K`
//! via `faer::sparse::SparseRowMat::try_new_from_triplets` get this for free.
//! Violating the invariant causes silent wrong results (binary search finds a
//! spurious hit or misses a valid one); callers that cannot guarantee sortedness
//! must sort before calling.

/// Returns the absolute slot index in `col_idx` (and the matching `vals` slot)
/// for the stored entry at column `target` within CSR row `[start, end)`, or
/// `None` if the column is not stored.  Requires sorted column indices within
/// the row (faer `SymbolicSparseRowMat` soft invariant).
#[inline]
pub(crate) fn find_in_row(
    col_idx: &[usize],
    start: usize,
    end: usize,
    target: usize,
) -> Option<usize> {
    col_idx[start..end]
        .binary_search(&target)
        .ok()
        .map(|rel| start + rel)
}
