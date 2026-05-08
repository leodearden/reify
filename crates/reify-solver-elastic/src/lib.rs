//! `reify-solver-elastic` — Linear-elastostatic FEA solver kernel for Reify.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` task #7. This crate ships
//! the reference-element primitives (P1 and P2 tetrahedra: shape functions,
//! gradients, Gauss quadrature, reference→physical Jacobian) used by the
//! later assembly/CG/etc. tasks (PRD tasks #8–#15).
//!
//! # v0.3 scope
//!
//! Skeleton + reference elements only. The following are explicitly out of
//! scope for this crate at this stage and are tracked elsewhere:
//!
//! - faer-rs / sparse-matrix wiring → PRD task #9.
//! - Inverse Jacobian J⁻ᵀ for physical-gradient mapping → PRD task #8
//!   (stiffness assembly is the consumer).
//! - `@optimized` registration / engine wiring → PRD task #16.
//! - 11-point quadrature rule for curved-Jacobian P2 → deferred to v0.4+;
//!   our straight-edge P2 elements have a constant Jacobian, so the
//!   4-point Stroud rule is exact for stiffness.
//! - Bridging the stdlib-side `ElementOrder` enum (in
//!   `crates/reify-compiler/stdlib/solver_elastic.ri`) to the Rust solver
//!   types → PRD task #16's job.
//!
//! # Re-export smoke test
//!
//! ```
//! use reify_solver_elastic::{
//!     Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement, TetP1, TetP2, HexP1,
//!     Mitc3Plus, ShellReferenceCoord, TyingPoint,
//!     ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness,
//!     IsotropicElastic,
//!     ShellStress,
//!     ShellElementStress, shell_element_frame, shell_element_stress,
//! };
//!
//! let _: TetP1 = TetP1;
//! let _: TetP2 = TetP2;
//! let _: HexP1 = HexP1;
//! assert_eq!(<TetP1 as ReferenceElement>::N_NODES, 4);
//! assert_eq!(<TetP2 as ReferenceElement>::N_NODES, 10);
//! assert_eq!(<HexP1 as ReferenceElement>::N_NODES, 8);
//! assert_eq!(HexP1.quad_points().len(), 8);
//! let _ = QuadraturePoint {
//!     coord: ReferenceCoord::new(0.25, 0.25, 0.25),
//!     weight: 1.0 / 6.0,
//! };
//! let _ = Jacobian::from_matrix([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
//!
//! let _: Mitc3Plus = Mitc3Plus;
//! assert_eq!(Mitc3Plus::N_NODES, 3);
//! assert_eq!(Mitc3Plus::N_DOFS, 18);
//! assert_eq!(Mitc3Plus::N_TYING_POINTS, 3);
//! let _ = ShellReferenceCoord::new(1.0 / 3.0, 1.0 / 3.0);
//! let _: &[TyingPoint] = Mitc3Plus.tying_points();
//!
//! // Shell-assembly smoke tests (T6).
//! let nodes = [[0.0_f64; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
//! let frame: ShellFrame = build_shell_frame(&nodes);
//! assert!((frame.area - 0.5).abs() < 1e-12, "area = {}", frame.area);
//! let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
//! let _d = plane_stress_d(&mat);
//! let k = shell_element_stiffness(&nodes, 0.05, &mat);
//! assert_eq!(k.n_dofs, 18);
//! assert_eq!(k.data.len(), 324);
//!
//! // ShellStress smoke test (T16): use a non-trivial value so a regression where
//! // one channel is left default would surface here.
//! let field = reify_types::Value::Real(1.0);
//! let ss = ShellStress::homogeneous(field.clone());
//! assert_eq!(ss.top, field, "homogeneous: top must equal input");
//! assert_eq!(ss.mid, field, "homogeneous: mid must equal input");
//! assert_eq!(ss.bottom, field, "homogeneous: bottom must equal input");
//!
//! // T7 smoke tests: shell_element_frame orthonormality + shell_element_stress zero-DOF regression.
//! let frame_mat: [[f64; 3]; 3] = shell_element_frame(&nodes);
//! // All three rows of the local-to-global rotation matrix must have unit norm.
//! for i in 0..3 {
//!     let norm_sq = frame_mat[i][0]*frame_mat[i][0]
//!         + frame_mat[i][1]*frame_mat[i][1]
//!         + frame_mat[i][2]*frame_mat[i][2];
//!     assert!((norm_sq - 1.0).abs() < 1e-12,
//!         "frame_mat row {i} norm² = {norm_sq}, expected 1.0");
//! }
//! // Off-diagonal Gram entry: rows 0 and 1 must be orthogonal.
//! let gram_01 = frame_mat[0][0]*frame_mat[1][0]
//!     + frame_mat[0][1]*frame_mat[1][1]
//!     + frame_mat[0][2]*frame_mat[1][2];
//! assert!(gram_01.abs() < 1e-12, "frame_mat rows 0·1 = {gram_01}, expected 0.0");
//! // Zero DOFs → all stress components must be exactly 0.0 (regression guard).
//! let ses: ShellElementStress = shell_element_stress(&nodes, 0.05, &mat, &[0.0_f64; 18]);
//! assert_eq!(ses.top[0][0], 0.0, "zero-DOF top σ_xx must be 0.0");
//! assert_eq!(ses.mid[0][0], 0.0, "zero-DOF mid σ_xx must be 0.0");
//! assert_eq!(ses.bottom[0][0], 0.0, "zero-DOF bottom σ_xx must be 0.0");
//! ```

pub mod assembly;
pub mod constitutive;
pub mod elements;
pub mod shell_assembly;
pub mod shell_result;

pub use assembly::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness, assemble_global_stiffness,
    element_stiffness,
};
pub use constitutive::IsotropicElastic;
pub use elements::{
    Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement, tet_p1::TetP1, tet_p2::TetP2,
    mitc3_plus::{Mitc3Plus, ShellReferenceCoord, TyingPoint},
};
pub use shell_assembly::{ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness};
pub use shell_result::{ShellStress, ShellElementStress, shell_element_frame, shell_element_stress};
