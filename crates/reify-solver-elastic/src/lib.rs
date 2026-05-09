//! `reify-solver-elastic` — Linear-elastostatic FEA solver kernel for Reify.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` task #7. This crate ships
//! the reference-element primitives (P1/P2 tetrahedra and P1 hex: shape
//! functions, gradients, Gauss quadrature, reference→physical Jacobian)
//! used by the later assembly/CG/etc. tasks (PRD tasks #8–#15).
//! P1 hex shipped per `docs/prds/v0_3/hex-wedge-meshing.md` task #2.
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
//!     DirichletBc, apply_dirichlet_row_elimination,
//!     FaceOrder, apply_body_force, apply_point_load, apply_traction_load,
//!     SupportKind, SupportBodyKind, SupportCompatibility, build_support_bcs,
//!     MpcRow, apply_mpc_row_elimination,
//!     solve_cg, CgSolverOptions, CgResult, SolverMode,
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
//!
//! // DirichletBc smoke test (T2917): construct, clone, and verify round-trip.
//! let bc = DirichletBc { dof: 0, value: 0.0 };
//! assert_eq!(bc.clone(), bc, "DirichletBc must round-trip through Clone");
//! // Verify apply_dirichlet_row_elimination is callable (empty bcs = no-op).
//! let _ = apply_dirichlet_row_elimination;
//!
//! // Neumann BC smoke tests (T2918): verify public surface is callable.
//! let _ = FaceOrder::P1Tri;
//! let _ = FaceOrder::P2Tri;
//! let mut f_smoke = vec![0.0_f64; 12];
//! apply_point_load(&mut f_smoke, 0, [1.0, 2.0, 3.0]);
//! assert_eq!(f_smoke[0], 1.0, "apply_point_load smoke: f[0]");
//! assert_eq!(f_smoke[1], 2.0, "apply_point_load smoke: f[1]");
//! assert_eq!(f_smoke[2], 3.0, "apply_point_load smoke: f[2]");
//!
//! // Shell BC smoke tests (T8): verify public surface is callable.
//! let _ = SupportKind::Fixed;
//! let _ = SupportKind::Pinned;
//! let _ = SupportBodyKind::Shell;
//! let _ = SupportBodyKind::Tet;
//! let _ = SupportCompatibility::Ok;
//! let _ = SupportCompatibility::PinnedOnTetEquivalentToFixed;
//! let (bcs_t8, compat_t8) = build_support_bcs(&[0], SupportKind::Fixed, SupportBodyKind::Shell);
//! assert_eq!(bcs_t8.len(), 6, "FixedSupport on shell node 0 → 6 BCs");
//! assert_eq!(compat_t8, SupportCompatibility::Ok, "Fixed on Shell → Ok compat");
//!
//! // MpcRow re-export smoke test (T11 / Task 3021): pin that the type is
//! // discoverable from the crate root for downstream consumers (Task 3020 will
//! // add construction methods on this type). The struct-literal init is the
//! // surface check; the dofs/coeffs equal-length invariant is T10's
//! // constructor's job, not a literal-vs-literal len comparison here.
//! let _row = MpcRow { dofs: vec![0, 6], coeffs: vec![1.0, -1.0], rhs: 0.0 };
//!
//! // T10 smoke tests (Task 3020): pin apply_mpc_row_elimination and
//! // MpcRow::shell_tet_tying are discoverable from the crate root.
//! let _ = apply_mpc_row_elimination;
//! let _rows = MpcRow::shell_tet_tying(
//!     [0, 1, 2], [3, 4, 5], [6, 7, 8], [9, 10, 11], [12, 13, 14],
//!     [0.0, 0.0, 1.0], 0.05,
//! );
//! assert_eq!(_rows.len(), 6, "shell_tet_tying must produce 6 constraint rows");
//!
//! // Task 2919: CG solver smoke test — exercises solve_cg, CgSolverOptions,
//! // CgResult, and SolverMode from the crate root. A regression that renames
//! // CgResult.u or removes Default from CgSolverOptions will trip this doctest.
//! let cg_triplet = faer::sparse::Triplet::new(0_usize, 0_usize, 1.0_f64);
//! let k_cg = faer::sparse::SparseRowMat::try_new_from_triplets(1, 1, &[cg_triplet]).unwrap();
//! let f_cg = [3.0_f64];
//! let cg_opts = CgSolverOptions::default();
//! let cg_result: CgResult = solve_cg(&k_cg, &f_cg, cg_opts, SolverMode::Deterministic);
//! assert!(cg_result.converged, "1×1 identity CG must converge");
//! assert_eq!(cg_result.u.len(), 1, "CgResult.u must have length 1");
//! assert!((cg_result.u[0] - 3.0).abs() < 1e-9, "u[0] = {}", cg_result.u[0]);
//! ```

pub mod assembly;
pub mod boundary;
pub mod constitutive;
pub mod elements;
pub mod mpc;
pub mod shell_assembly;
pub mod shell_boundary;
pub mod shell_result;
pub mod solver;

pub use assembly::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness, assemble_global_stiffness,
    element_stiffness,
};
pub use boundary::{
    DirichletBc, FaceOrder, apply_body_force, apply_dirichlet_row_elimination, apply_point_load,
    apply_traction_load,
};
pub use constitutive::IsotropicElastic;
pub use mpc::{MpcRow, apply_mpc_row_elimination};
pub use elements::{
    Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement,
    hex_p1::HexP1,
    mitc3_plus::{Mitc3Plus, ShellReferenceCoord, TyingPoint},
    tet_p1::TetP1,
    tet_p2::TetP2,
};
pub use shell_assembly::{ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness};
pub use shell_boundary::{
    SupportBodyKind, SupportCompatibility, SupportKind, build_support_bcs,
};
pub use shell_result::{
    ShellElementStress, ShellStress, shell_element_frame, shell_element_stress,
};
pub use solver::{CgResult, CgSolverOptions, SolverMode, solve_cg};
