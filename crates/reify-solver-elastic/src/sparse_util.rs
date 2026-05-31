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

#[cfg(test)]
mod tests {
    use super::find_in_row;

    // col_idx slice shared by several tests: columns 10, 20, 30, 40, 50
    // The full row occupies slots [2, 7) — i.e. start=2, end=7.
    fn sample() -> ([usize; 9], usize, usize) {
        ([0, 0, 10, 20, 30, 40, 50, 0, 0], 2, 7)
    }

    // (a) target at the first slot of the row → absolute index == start
    #[test]
    fn target_at_start_returns_start() {
        let (col_idx, start, end) = sample();
        assert_eq!(find_in_row(&col_idx, start, end, 10), Some(2));
    }

    // (b) target in the middle → absolute offset, NOT relative
    //     Catches a regression where the impl returns the relative index instead of start+rel.
    #[test]
    fn target_in_middle_returns_absolute_offset() {
        let (col_idx, start, end) = sample();
        // Column 30 is at col_idx[4]; relative index = 2, absolute = start(2) + 2 = 4.
        assert_eq!(find_in_row(&col_idx, start, end, 30), Some(4));
    }

    // (c) target at the last slot of the row → absolute index == end - 1
    #[test]
    fn target_at_end_returns_end_minus_one() {
        let (col_idx, start, end) = sample();
        // Column 50 is at col_idx[6] == end - 1 = 6.
        assert_eq!(find_in_row(&col_idx, start, end, 50), Some(6));
    }

    // (d) target less than every column in the row → None
    #[test]
    fn target_less_than_all_returns_none() {
        let (col_idx, start, end) = sample();
        assert_eq!(find_in_row(&col_idx, start, end, 5), None);
    }

    // (e) target between two stored columns (absent) → None
    #[test]
    fn target_between_columns_returns_none() {
        let (col_idx, start, end) = sample();
        assert_eq!(find_in_row(&col_idx, start, end, 25), None);
    }

    // (f) target greater than every column in the row → None
    #[test]
    fn target_greater_than_all_returns_none() {
        let (col_idx, start, end) = sample();
        assert_eq!(find_in_row(&col_idx, start, end, 99), None);
    }

    // (g) empty row (start == end) → None regardless of target
    #[test]
    fn empty_row_returns_none() {
        let col_idx: &[usize] = &[10, 20, 30];
        assert_eq!(find_in_row(col_idx, 1, 1, 20), None);
    }

    // (h) single-element row — hit and miss
    #[test]
    fn single_element_row_hit() {
        let col_idx: &[usize] = &[0, 42, 0];
        // Row spans [1, 2); target 42 is present at absolute index 1.
        assert_eq!(find_in_row(col_idx, 1, 2, 42), Some(1));
    }

    #[test]
    fn single_element_row_miss() {
        let col_idx: &[usize] = &[0, 42, 0];
        assert_eq!(find_in_row(col_idx, 1, 2, 7), None);
    }
}
