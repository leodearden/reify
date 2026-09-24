//! Shared sparse-matrix helpers for the elastic solver kernel.
//!
//! This module collects small utilities that operate on faer's CSR
//! representation (`col_idx`, `row_ptr`, `vals` slices):
//!
//! - [`find_in_row`] locates one stored entry of a row by binary search; the
//!   MPC row eliminator uses it.
//! - [`ColumnSlots`] indexes the stored entries of chosen columns; the
//!   Dirichlet boundary-condition eliminator uses it to walk a constrained
//!   column without scanning every row.
//!
//! # Invariants
//!
//! [`find_in_row`] assumes faer's **soft invariant**: column indices within
//! each CSR row are sorted in ascending order.  Callers that build their `K`
//! via `faer::sparse::SparseRowMat::try_new_from_triplets` get this for free.
//! Violating the invariant causes silent wrong results (binary search finds a
//! spurious hit or misses a valid one); callers that cannot guarantee sortedness
//! must sort before calling.  [`ColumnSlots`] does not depend on it.

use faer::sparse::SymbolicSparseRowMatRef;

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

/// One stored entry of a CSR column: its `row`, and its `slot` — the ABSOLUTE
/// index into the matrix's `col_idx` and value arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnEntry {
    pub(crate) row: usize,
    pub(crate) slot: usize,
}

/// Column-wise index of a CSR pattern, built only for the columns a caller
/// chooses: the column access a row-major matrix lacks, without mirroring the
/// whole matrix.
///
/// [`column`](Self::column) lists every stored entry of a chosen column, rows
/// ascending, each as a [`ColumnEntry`] with an absolute slot. An unchosen
/// column reads as empty. Building costs O(nnz + ncols) time and
/// O(ncols + stored entries of the chosen columns) memory, and does not depend
/// on sorted `col_idx`.
pub(crate) struct ColumnSlots {
    /// `col_start[c]..col_start[c + 1]` is column `c`'s range in `entries`.
    col_start: Vec<usize>,
    entries: Vec<ColumnEntry>,
}

impl ColumnSlots {
    /// Indexes the stored entries of `columns` in `pattern`. Choosing a column
    /// more than once indexes it once.
    ///
    /// # Panics
    ///
    /// If any chosen column is `>= pattern.ncols()`.
    pub(crate) fn new(
        pattern: SymbolicSparseRowMatRef<'_, usize>,
        columns: impl IntoIterator<Item = usize>,
    ) -> Self {
        let ncols = pattern.ncols();
        let mut chosen = vec![false; ncols];
        for col in columns {
            assert!(
                col < ncols,
                "ColumnSlots: column {col} is out of range for a pattern with {ncols} columns",
            );
            chosen[col] = true;
        }

        let col_idx = pattern.col_idx();
        let mut col_start = vec![0; ncols + 1];
        for row in 0..pattern.nrows() {
            for &col in &col_idx[pattern.row_range(row)] {
                if chosen[col] {
                    col_start[col + 1] += 1;
                }
            }
        }
        for col in 0..ncols {
            col_start[col + 1] += col_start[col];
        }

        let mut cursor = col_start[..ncols].to_vec();
        let mut entries = vec![ColumnEntry { row: 0, slot: 0 }; col_start[ncols]];
        for row in 0..pattern.nrows() {
            for slot in pattern.row_range(row) {
                let col = col_idx[slot];
                if chosen[col] {
                    entries[cursor[col]] = ColumnEntry { row, slot };
                    cursor[col] += 1;
                }
            }
        }
        Self { col_start, entries }
    }

    /// The stored entries of column `col`, rows ascending; empty when `col`
    /// was not chosen or stores nothing.
    pub(crate) fn column(&self, col: usize) -> &[ColumnEntry] {
        &self.entries[self.col_start[col]..self.col_start[col + 1]]
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnEntry, ColumnSlots, find_in_row};
    use faer::sparse::SymbolicSparseRowMat;

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

    // ColumnSlots — 4×4 pattern shared by several tests, the same layout as
    // the Dirichlet row-range-boundaries test:
    //   row 0 → slots 0..2 (cols [0, 2])
    //   row 1 → slots 2..4 (cols [1, 3])
    //   row 2 → slots 4..7 (cols [0, 2, 3])
    //   row 3 → slots 7..9 (cols [2, 3])
    fn boundaries_pattern() -> SymbolicSparseRowMat<usize> {
        SymbolicSparseRowMat::<usize>::new_checked(
            4,
            4,
            vec![0, 2, 4, 7, 9],
            None,
            vec![0, 2, 1, 3, 0, 2, 3, 2, 3],
        )
    }

    fn entry(row: usize, slot: usize) -> ColumnEntry {
        ColumnEntry { row, slot }
    }

    // (i) Every stored entry of a chosen column, as ABSOLUTE slots, rows
    //     ascending. A row-relative slot would report K[3][2] as {3, 0}; row 1
    //     stores no column 2 and must be skipped.
    #[test]
    fn chosen_columns_list_every_stored_entry_as_absolute_slots_rows_ascending() {
        let pattern = boundaries_pattern();
        let columns = ColumnSlots::new(pattern.as_ref(), [1, 2, 3]);
        assert_eq!(columns.column(2), [entry(0, 1), entry(2, 5), entry(3, 7)]);
        assert_eq!(columns.column(3), [entry(1, 3), entry(2, 6), entry(3, 8)]);
        assert_eq!(columns.column(1), [entry(1, 2)]);
    }

    // (j) Memory is spent only on requested columns: columns 0 and 3 both
    //     store entries, but only column 2 was chosen.
    #[test]
    fn unchosen_column_reads_as_empty() {
        let pattern = boundaries_pattern();
        let columns = ColumnSlots::new(pattern.as_ref(), [2]);
        assert!(columns.column(0).is_empty(), "{:?}", columns.column(0));
        assert!(columns.column(3).is_empty(), "{:?}", columns.column(3));
    }

    // (k) A chosen column with no stored entry — the Dirichlet
    //     missing-diagonal case — reads as empty.
    #[test]
    fn chosen_column_without_stored_entries_is_empty() {
        let pattern =
            SymbolicSparseRowMat::<usize>::new_checked(3, 3, vec![0, 1, 2, 2], None, vec![0, 1]);
        let columns = ColumnSlots::new(pattern.as_ref(), [2]);
        assert!(columns.column(2).is_empty(), "{:?}", columns.column(2));
    }

    // (l) Choosing a column twice indexes it once — no doubled entries.
    #[test]
    fn choosing_a_column_twice_indexes_it_once() {
        let pattern = boundaries_pattern();
        let once = ColumnSlots::new(pattern.as_ref(), [2]);
        let twice = ColumnSlots::new(pattern.as_ref(), [2, 2]);
        assert_eq!(twice.column(2), once.column(2));
    }

    // (m) Unlike find_in_row, the index does not depend on sorted col_idx:
    //     row 0 stores cols [2, 0], and every entry still carries its true
    //     slot, rows ascending.
    #[test]
    fn rows_stay_ascending_when_col_idx_is_unsorted_within_rows() {
        //   row 0 → slots 0..2 (cols [2, 0], out of order)
        //   row 1 → slots 2..4 (cols [0, 1])
        //   row 2 → slot  4    (cols [2])
        let pattern = SymbolicSparseRowMat::<usize>::new_unsorted_checked(
            3,
            3,
            vec![0, 2, 4, 5],
            None,
            vec![2, 0, 0, 1, 2],
        );
        let columns = ColumnSlots::new(pattern.as_ref(), [0, 2]);
        assert_eq!(columns.column(0), [entry(0, 1), entry(1, 2)]);
        assert_eq!(columns.column(2), [entry(0, 0), entry(2, 4)]);
    }

    // (n) A chosen column outside the pattern panics descriptively rather
    //     than with a bare index-out-of-bounds.
    #[test]
    #[should_panic(expected = "out of range")]
    fn out_of_range_column_panics() {
        let pattern = boundaries_pattern();
        let _ = ColumnSlots::new(pattern.as_ref(), [4]);
    }
}
