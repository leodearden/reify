//! `MpcRow`'s public-seam contract (solver-crates-layering D10 / C6, BT8).
//!
//! Every `MpcRow` a consumer can hold satisfies one invariant:
//! `dofs.len() == coeffs.len() > 0`, `coeffs[0]` finite and non-zero, and
//! `rhs` finite. This binary sees only what another crate sees: the two
//! constructors `MpcRow::new` and `MpcRow::shell_tet_tying`, and the read
//! accessors `dofs()`, `coeffs()` and `rhs()`.

use reify_solver_elastic::MpcRow;

#[test]
#[should_panic(expected = "pivot")]
fn mpc_row_new_rejects_a_zero_pivot_coefficient() {
    let _ = MpcRow::new(vec![3, 7], vec![0.0, 1.0], 0.0);
}

#[test]
#[should_panic(expected = "dofs.len()")]
fn mpc_row_new_rejects_a_dofs_coeffs_length_mismatch() {
    let _ = MpcRow::new(vec![1, 2], vec![1.0], 0.0);
}

#[test]
#[should_panic(expected = "at least one")]
fn mpc_row_new_rejects_empty_dofs() {
    let _ = MpcRow::new(vec![], vec![], 0.0);
}

/// NaN passes the `!= 0.0` pivot guard (`NaN != 0.0` is true under IEEE 754),
/// so the `is_finite` guard is the one that must reject it.
#[test]
#[should_panic(expected = "finite")]
fn mpc_row_new_rejects_a_nan_pivot() {
    let _ = MpcRow::new(vec![1], vec![f64::NAN], 0.0);
}

#[test]
#[should_panic(expected = "rhs")]
fn mpc_row_new_rejects_a_non_finite_rhs() {
    let _ = MpcRow::new(vec![1], vec![1.0], f64::INFINITY);
}

#[test]
fn mpc_row_accessors_return_the_constructed_values() {
    let row = MpcRow::new(vec![3, 7, 11], vec![1.0, -0.5, 0.5], 0.25);
    assert_eq!(row.dofs(), [3, 7, 11]);
    assert_eq!(row.coeffs(), [1.0, -0.5, 0.5]);
    assert_eq!(row.rhs().to_bits(), 0.25_f64.to_bits());
}

/// Axis-aligned normals reach the drilling-axis fallback and the two-term
/// rotation row; oblique normals reach the four-term rotation row.
#[test]
fn every_shell_tet_tying_row_satisfies_the_mpc_row_invariant() {
    let inv_sqrt3 = 1.0 / 3.0_f64.sqrt();
    let normals: [[f64; 3]; 6] = [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.6, 0.8, 0.0],
        [0.0, 0.6, 0.8],
        [inv_sqrt3, inv_sqrt3, inv_sqrt3],
    ];
    for normal in normals {
        for thickness in [0.05, 2.0] {
            let rows = MpcRow::shell_tet_tying(
                [0, 1, 2],
                [3, 4, 5],
                [6, 7, 8],
                [9, 10, 11],
                [12, 13, 14],
                normal,
                thickness,
            );
            for (index, row) in rows.iter().enumerate() {
                let at = format!("normal {normal:?}, thickness {thickness}, row {index}");
                assert_eq!(
                    row.dofs().len(),
                    row.coeffs().len(),
                    "{at}: dofs and coeffs differ in length"
                );
                assert!(!row.dofs().is_empty(), "{at}: no DOFs");
                let pivot = row.coeffs()[0];
                assert!(
                    pivot != 0.0 && pivot.is_finite(),
                    "{at}: pivot coefficient {pivot} is zero or non-finite"
                );
                assert!(
                    row.rhs().is_finite(),
                    "{at}: rhs {} is not finite",
                    row.rhs()
                );
            }
        }
    }
}
