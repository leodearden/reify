//! Dense Householder linear algebra under the trust-region solve — task #6675
//! (solver-unification η), PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md`.
//!
//! Two primitives, pinned by their observable outputs only:
//! - [`RowSpaceQr`]: an ORDERED rank-revealing factorization that takes rows in
//!   input order and reports which of them widened the span, with an orthonormal
//!   basis of what is left over (the null space);
//! - [`least_squares_min_norm`]: the least-squares solution with no component in
//!   that null space.
//!
//! Exactness basis: Householder reflectors are orthogonal to working precision, so
//! orthonormality and annihilation hold to a few ulp on these O(1) fixtures; the
//! `1e-12` bars below leave thousands of ulp of headroom.

use reify_constraints::linalg::{RankTolerance, RowSpaceQr, least_squares_min_norm};

const TOL: RankTolerance = RankTolerance {
    independence: 1e-8,
    zero_row: 1e-9,
};

const EXACT: f64 = 1e-12;

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

/// Which pushes widened the span, in push order.
fn contributions(dim: usize, rows: &[Vec<f64>], tolerance: RankTolerance) -> Vec<bool> {
    let mut qr = RowSpaceQr::new(dim, tolerance);
    rows.iter().map(|row| qr.push(row)).collect()
}

fn qr_of(dim: usize, rows: &[Vec<f64>]) -> RowSpaceQr {
    let mut qr = RowSpaceQr::new(dim, TOL);
    qr.push_rows(rows);
    qr
}

fn assert_orthonormal(vectors: &[Vec<f64>]) {
    for (i, a) in vectors.iter().enumerate() {
        for (j, b) in vectors.iter().enumerate() {
            let expected = if i == j { 1.0 } else { 0.0 };
            let got = dot(a, b);
            assert!((got - expected).abs() <= EXACT, "z{i}·z{j} = {got}");
        }
    }
}

fn assert_annihilated(rows: &[Vec<f64>], null_space: &[Vec<f64>]) {
    for (i, row) in rows.iter().enumerate() {
        for (j, z) in null_space.iter().enumerate() {
            let got = dot(row, z);
            assert!(
                got.abs() <= EXACT * norm(row).max(1.0),
                "row {i} · z{j} = {got}"
            );
        }
    }
}

/// `Aᵀ(Ax − b)`.
fn normal_residual(rows: &[Vec<f64>], rhs: &[f64], x: &[f64]) -> Vec<f64> {
    let mut g = vec![0.0; x.len()];
    for (row, b) in rows.iter().zip(rhs) {
        let r = dot(row, x) - b;
        for (gk, ak) in g.iter_mut().zip(row) {
            *gk += ak * r;
        }
    }
    g
}

fn frobenius(rows: &[Vec<f64>]) -> f64 {
    rows.iter().map(|r| dot(r, r)).sum::<f64>().sqrt()
}

// ── (a) Ordered rank-revealing row-space factorization ───────────────────────

#[test]
fn each_push_reports_whether_it_widened_the_span() {
    let rows = vec![
        vec![1.0, 0.0, 0.0],
        vec![0.0, 2.0, 0.0],
        vec![3.0, -4.0, 0.0], // a combination of the first two
        vec![0.0, 0.0, 0.0],  // numerically zero
        vec![1.0, 1.0, 5.0],
    ];
    assert_eq!(
        contributions(3, &rows, TOL),
        vec![true, true, false, false, true]
    );
    assert_eq!(qr_of(3, &rows).rank(), 3);
}

#[test]
fn push_rows_counts_the_new_directions() {
    let mut qr = RowSpaceQr::new(4, TOL);
    let first = vec![vec![1.0, 0.0, 0.0, 0.0], vec![0.0, 1.0, 1.0, 0.0]];
    assert_eq!(qr.push_rows(&first), 2);
    // One repeat of the span, one new direction.
    let second = vec![vec![2.0, 3.0, 3.0, 0.0], vec![0.0, 0.0, 0.0, 7.0]];
    assert_eq!(qr.push_rows(&second), 1);
    assert_eq!(qr.rank(), 3);
}

#[test]
fn a_row_below_the_zero_floor_is_skipped() {
    let rows = vec![vec![1e-12, 0.0, 0.0], vec![0.0, 0.0, 1e-10]];
    assert_eq!(contributions(3, &rows, TOL), vec![false, false]);
    assert_eq!(qr_of(3, &rows).rank(), 0);
}

#[test]
fn a_nearly_parallel_row_is_judged_on_its_normalized_remainder() {
    let below = vec![vec![1.0, 0.0, 0.0], vec![1.0, 1e-10, 0.0]];
    assert_eq!(contributions(3, &below, TOL), vec![true, false]);

    let above = vec![vec![1.0, 0.0, 0.0], vec![1.0, 1e-6, 0.0]];
    assert_eq!(contributions(3, &above, TOL), vec![true, true]);
}

#[test]
fn the_verdicts_ignore_a_common_positive_row_scaling() {
    let rows = vec![
        vec![1.0, 2.0, 0.0, -1.0],
        vec![2.0, 4.0, 0.0, -2.0 + 1e-10],
        vec![0.0, 1.0, 1.0, 0.0],
        vec![1.0, 3.0, 1.0, -1.0],
        vec![0.5, 0.0, 0.0, 0.25],
    ];
    let base = contributions(4, &rows, TOL);
    assert_eq!(base, vec![true, false, true, false, true]);
    for k in [1024.0, 1e3, 1e-3] {
        let scaled: Vec<Vec<f64>> = rows
            .iter()
            .map(|r| r.iter().map(|v| v * k).collect())
            .collect();
        assert_eq!(contributions(4, &scaled, TOL), base, "k = {k}");
    }
}

#[test]
fn the_first_row_of_a_span_is_the_one_kept() {
    // Order matters for WHICH row drives, not for how many do.
    let a = vec![1.0, 1.0, 0.0];
    let b = vec![2.0, 2.0, 0.0];
    let c = vec![0.0, 0.0, 1.0];
    assert_eq!(
        contributions(3, &[a.clone(), b.clone(), c.clone()], TOL),
        vec![true, false, true]
    );
    assert_eq!(contributions(3, &[b, a, c], TOL), vec![true, false, true]);
}

// ── (b) Null space ───────────────────────────────────────────────────────────

#[test]
fn null_space_is_an_orthonormal_complement_of_the_pushed_rows() {
    let rows = vec![
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        vec![1.0, -1.0, 0.0, 0.0, 0.0, 0.0], // dependent
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        vec![0.0, 0.0, 0.5, 0.25, 0.0, 0.0],
        vec![0.0, 0.0, 1.0, 0.5, 0.0, 2.0], // dependent
    ];
    let qr = qr_of(6, &rows);
    assert_eq!(qr.rank(), 4);
    let z = qr.null_space();
    assert_eq!(z.len(), 2);
    assert!(z.iter().all(|v| v.len() == 6));
    assert_orthonormal(&z);
    assert_annihilated(&rows, &z);
}

#[test]
fn null_space_of_nothing_is_the_whole_space_and_of_full_rank_is_empty() {
    let empty = RowSpaceQr::new(3, TOL);
    assert_eq!(empty.rank(), 0);
    let z = empty.null_space();
    assert_eq!(z.len(), 3);
    assert_orthonormal(&z);

    let full = qr_of(
        3,
        &[
            vec![2.0, 1.0, 0.0],
            vec![1.0, 3.0, 1.0],
            vec![0.0, 1.0, 4.0],
        ],
    );
    assert_eq!(full.rank(), 3);
    assert!(full.null_space().is_empty());
}

#[test]
fn null_space_of_a_skewed_rank_two_set_in_five_dimensions() {
    let rows = vec![
        vec![0.3, -1.2, 0.7, 2.0, 0.1],
        vec![1.1, 0.4, -0.6, 0.0, 0.9],
        vec![1.4, -0.8, 0.1, 2.0, 1.0], // sum of the two
    ];
    let qr = qr_of(5, &rows);
    assert_eq!(qr.rank(), 2);
    let z = qr.null_space();
    assert_eq!(z.len(), 3);
    assert_orthonormal(&z);
    assert_annihilated(&rows, &z);
}

// ── (c) Minimum-norm least squares ───────────────────────────────────────────

#[test]
fn full_rank_square_system_is_solved_exactly() {
    let rows = vec![
        vec![2.0, 1.0, 0.0],
        vec![1.0, 3.0, 1.0],
        vec![0.0, 1.0, 4.0],
    ];
    let expected = [1.0, -2.0, 3.0];
    let rhs: Vec<f64> = rows.iter().map(|r| dot(r, &expected)).collect();
    let solution = least_squares_min_norm(&rows, &rhs, TOL);
    assert_eq!(solution.rank, 3);
    assert!(solution.null_space.is_empty());
    for (got, want) in solution.x.iter().zip(expected) {
        assert!(
            (got - want).abs() <= EXACT * norm(&expected),
            "{solution:?}"
        );
    }
}

#[test]
fn rank_deficient_consistent_system_leaves_the_null_direction_at_zero() {
    let rows = vec![
        vec![1.0, 0.0, 0.0],
        vec![2.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
    ];
    let rhs = [3.0, 6.0, -1.0];
    let solution = least_squares_min_norm(&rows, &rhs, TOL);
    for (row, b) in rows.iter().zip(rhs) {
        assert!(
            (dot(row, &solution.x) - b).abs() <= EXACT * 6.0,
            "{solution:?}"
        );
    }
    assert!(solution.x[2].abs() <= EXACT, "{solution:?}");
    assert_eq!(solution.rank, 2);
    assert_eq!(solution.null_space.len(), 1);
    assert!((solution.null_space[0][2].abs() - 1.0).abs() <= EXACT);
}

#[test]
fn inconsistent_overdetermined_system_returns_the_least_squares_minimizer() {
    let rows = vec![
        vec![1.0, 0.0],
        vec![0.0, 1.0],
        vec![1.0, 1.0],
        vec![1.0, -2.0],
    ];
    let rhs = [1.0, 1.0, 0.0, 0.5];
    let solution = least_squares_min_norm(&rows, &rhs, TOL);
    let g = normal_residual(&rows, &rhs, &solution.x);
    assert!(
        norm(&g) <= EXACT * frobenius(&rows) * norm(&rhs),
        "Aᵀ(Ax − b) = {g:?}"
    );
    assert_eq!(solution.rank, 2);
}

#[test]
fn inconsistent_rank_deficient_system_is_minimal_and_minimum_norm() {
    let rows = vec![
        vec![1.0, 0.0, 0.0, 1.0],
        vec![1.0, 0.0, 0.0, 1.0],
        vec![0.0, 2.0, 0.0, 0.0],
    ];
    let rhs = [1.0, 3.0, 4.0];
    let solution = least_squares_min_norm(&rows, &rhs, TOL);
    let g = normal_residual(&rows, &rhs, &solution.x);
    assert!(
        norm(&g) <= EXACT * frobenius(&rows) * norm(&rhs),
        "Aᵀ(Ax − b) = {g:?}"
    );
    for z in &solution.null_space {
        assert!(dot(z, &solution.x).abs() <= EXACT, "{solution:?}");
    }
    assert_eq!(solution.rank, 2);
    assert_eq!(solution.null_space.len(), 2);
}

#[test]
fn least_squares_reports_the_rank_and_null_space_of_its_rows() {
    let rows = vec![
        vec![0.3, -1.2, 0.7, 2.0, 0.1, 0.0],
        vec![1.1, 0.4, -0.6, 0.0, 0.9, 0.0],
        vec![1.4, -0.8, 0.1, 2.0, 1.0, 0.0],
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 3.0],
    ];
    let rhs = [1.0, -1.0, 0.0, 2.0];
    let solution = least_squares_min_norm(&rows, &rhs, TOL);
    let qr = qr_of(6, &rows);
    assert_eq!(solution.rank, qr.rank());
    assert_eq!(solution.null_space, qr.null_space());
}

#[test]
fn only_zero_rows_give_the_zero_solution() {
    let rows: Vec<Vec<f64>> = vec![vec![0.0, 0.0]];
    let solution = least_squares_min_norm(&rows, &[5.0], TOL);
    assert_eq!(solution.x, vec![0.0, 0.0]);
    assert_eq!(solution.rank, 0);
    assert_eq!(solution.null_space.len(), 2);
}
