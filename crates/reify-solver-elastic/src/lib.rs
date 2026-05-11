//! `reify-solver-elastic` — Linear-elastostatic FEA solver kernel for Reify.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/structural-analysis-fea.md` task #7. This crate ships
//! the reference-element primitives (P1/P2 tetrahedra, P1 hex, P1 wedge:
//! shape functions, gradients, Gauss quadrature, reference→physical Jacobian)
//! used by the later assembly/CG/etc. tasks (PRD tasks #8–#15).
//! P1 hex shipped per `docs/prds/v0_3/hex-wedge-meshing.md` task #2.
//! P1 wedge shipped per `docs/prds/v0_3/hex-wedge-meshing.md` task #3.
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
//!     element_stiffness_hex_p1, element_stiffness_wedge_p1,
//!     Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement, TetP1, TetP2, HexP1, WedgeP1,
//!     Mitc3Plus, ShellReferenceCoord, TyingPoint,
//!     ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness,
//!     IsotropicElastic,
//!     ShellStress,
//!     ShellElementStress, shell_element_frame, shell_element_stress,
//!     DirichletBc, apply_dirichlet_row_elimination,
//!     FaceOrder, apply_body_force, apply_point_load, apply_traction_load,
//!     SupportKind, SupportBodyKind, SupportCompatibility, build_support_bcs,
//!     MpcRow, apply_mpc_row_elimination,
//!     solve_cg, solve_cg_warm, CgSolverOptions, CgResult, SolverMode,
//!     CgWarmState, solve_cg_with_warm_state,
//!     barycentric_p1, point_in_tet_p1, interpolate_p1_at_point,
//!     locate_element_p1, LocatableTet,
//!     StressElement, element_stress_p1, recover_nodal_stress_p1, tet_volume_p1,
//!     ProgressiveOptions, PartialElasticResult, PassTuning,
//!     RefinementDemand, TerminationReason, AdvanceDecision,
//!     coarse_pass_tuning, refinement_pass_tuning, near_constraint_boundary, should_refine,
//!     SweepElementTarget, Mesh2d, Mesh2dReport, ProfileBoundary, Mesh2dOptions, Mesh2dError,
//!     compute_quad_skew, recombine_quality_ok, auto_mesh_size_from_boundary,
//!     mesh_swept_profile_2d,
//! };
//!
//! let _: TetP1 = TetP1;
//! let _: TetP2 = TetP2;
//! let _: HexP1 = HexP1;
//! let _: WedgeP1 = WedgeP1;
//! assert_eq!(<TetP1 as ReferenceElement>::N_NODES, 4);
//! assert_eq!(<TetP2 as ReferenceElement>::N_NODES, 10);
//! assert_eq!(<HexP1 as ReferenceElement>::N_NODES, 8);
//! assert_eq!(HexP1.quad_points().len(), 8);
//! assert_eq!(<WedgeP1 as ReferenceElement>::N_NODES, 6);
//! assert_eq!(WedgeP1.quad_points().len(), 6);
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
//!
//! // Task 2921: warm-state plumbing smoke test — exercises solve_cg_warm
//! // (None-shim equivalence vs solve_cg), solve_cg_with_warm_state (producer
//! // wrapper), and CgWarmState OpaqueState round-trip on the 1×1 identity.
//! let cg_opts_2921 = CgSolverOptions::default();
//! let cold_via_solve_cg = solve_cg(&k_cg, &f_cg, cg_opts_2921.clone(), SolverMode::Deterministic);
//! let cold_via_warm = solve_cg_warm(&k_cg, &f_cg, None, cg_opts_2921.clone(), SolverMode::Deterministic);
//! assert_eq!(
//!     cold_via_solve_cg.u, cold_via_warm.u,
//!     "solve_cg_warm(None) must match solve_cg on 1×1 identity"
//! );
//! let (_r, fresh) = solve_cg_with_warm_state(
//!     &k_cg, &f_cg, None, cg_opts_2921, SolverMode::Deterministic,
//! );
//! assert_eq!(fresh.u.len(), 1, "fresh warm state u must have length 1");
//! let opaque = fresh.into_opaque_state();
//! let restored = CgWarmState::from_opaque_state(opaque).expect("downcast");
//! assert_eq!(restored.u.len(), 1, "OpaqueState round-trip must preserve u length");
//!
//! // Task 2920: result-interpolation smoke tests — exercise tet_volume_p1,
//! // element_stress_p1, point_in_tet_p1, and locate_element_p1 from the
//! // crate root. A regression that breaks any of the four re-exports would
//! // fail at the doctest compile step.
//! //
//! // Role: API-surface check, NOT a behavioural test. Each call below
//! // exists to pin one re-exported symbol's name + signature; the
//! // behavioural assertions duplicate cases already tested in
//! // `interpolation::tests` / `result::tests`. Future hands should not
//! // grow this section with new behavioural assertions — add them as
//! // unit tests in the owning module instead.
//! let unit_tet: [[f64; 3]; 4] = [
//!     [0.0, 0.0, 0.0],
//!     [1.0, 0.0, 0.0],
//!     [0.0, 1.0, 0.0],
//!     [0.0, 0.0, 1.0],
//! ];
//! let v = tet_volume_p1(&unit_tet);
//! assert!((v - 1.0 / 6.0).abs() < 1e-12, "tet_volume_p1 unit = {v}");
//!
//! let mat_2920 = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
//! let sigma = element_stress_p1(&unit_tet, &mat_2920, &[0.0_f64; 12]);
//! for i in 0..3 {
//!     for j in 0..3 {
//!         assert_eq!(sigma[i][j], 0.0, "zero-u σ[{i}][{j}] must be 0.0");
//!     }
//! }
//!
//! assert!(
//!     point_in_tet_p1(&unit_tet, [0.25, 0.25, 0.25], 1e-9),
//!     "centroid must be inside the unit tet",
//! );
//! let elements = [LocatableTet { phys_nodes: &unit_tet }];
//! assert_eq!(
//!     locate_element_p1(&elements, [0.25, 0.25, 0.25], 1e-9),
//!     Some(0),
//!     "single-element locate must return Some(0) at the centroid",
//! );
//!
//! // Task 2923: progressive-solve framework smoke pin.
//! // The import block above already asserts all progressive re-exports compile;
//! // these one-shot constructions confirm renames or removals trip this doctest.
//! let _ = ProgressiveOptions::default();
//! let _ = PassTuning { mesh_tol: 0.0, cg_tol: 0.0 };
//! let _ = PartialElasticResult {
//!     displacement: vec![], stress: vec![], max_von_mises: 0.0,
//!     converged: false, iterations: 0,
//! };
//! let _ = (RefinementDemand::None, TerminationReason::BudgetExhausted);
//! let _ = AdvanceDecision::Continue(PassTuning { mesh_tol: 0.0, cg_tol: 0.0 });
//! let _: fn(&ProgressiveOptions) -> PassTuning = coarse_pass_tuning;
//! let _: fn(&ProgressiveOptions, usize) -> PassTuning = refinement_pass_tuning;
//! let _: fn(&PartialElasticResult, &ProgressiveOptions) -> bool = near_constraint_boundary;
//! let _: fn(&ProgressiveOptions, usize, &PartialElasticResult, RefinementDemand) -> AdvanceDecision = should_refine;
//!
//! // Task 2987: 2D meshing public surface — pin SweepElementTarget,
//! // Mesh2d/Mesh2dReport/Mesh2dOptions/Mesh2dError, ProfileBoundary, and
//! // the four helpers/orchestrator are discoverable from the crate root.
//! // Behaviour is covered by `mesher::tests` + the integration tests under
//! // `tests/mesh_swept_profile_2d_tests.rs`; this block is purely an
//! // API-surface check that mirrors the precedent set by hex_p1 / wedge_p1.
//! let _: SweepElementTarget = SweepElementTarget::HexPreferred;
//! let _: SweepElementTarget = SweepElementTarget::WedgeOnly;
//! let _ = ProfileBoundary {
//!     outer: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
//!     holes: vec![],
//! };
//! let opts_2987 = Mesh2dOptions::default();
//! assert_eq!(
//!     opts_2987.recombine_skew_threshold,
//!     std::f64::consts::FRAC_PI_4,
//!     "Mesh2dOptions::default().recombine_skew_threshold must be \u{3c0}/4",
//! );
//! // Construct each variant of Mesh2d and Mesh2dError so a renamed field
//! // or removed variant trips this doctest at compile time.
//! let _ = Mesh2d::Triangle { vertices: vec![0.0_f32; 6], indices: vec![0_u32, 1, 2] };
//! let _ = Mesh2d::Quad { vertices: vec![0.0_f32; 8], indices: vec![0_u32, 1, 2, 3] };
//! let _ = Mesh2dReport {
//!     mesh: Mesh2d::Triangle { vertices: vec![0.0_f32; 6], indices: vec![0_u32, 1, 2] },
//!     recombine_attempted: false,
//!     recombine_quality_ok: true,
//! };
//! let _ = Mesh2dError::EmptyBoundary;
//! let _ = Mesh2dError::DegenerateBoundary;
//! let _ = Mesh2dError::GmshUnavailable;
//! // Pin the four function items by their full signatures.
//! let _: fn(&[[f64; 2]; 4]) -> f64 = compute_quad_skew;
//! let _: fn(&[f32], &[u32], f64) -> bool = recombine_quality_ok;
//! let _: fn(&ProfileBoundary, f64) -> f64 = auto_mesh_size_from_boundary;
//! let _: fn(
//!     &ProfileBoundary,
//!     SweepElementTarget,
//!     &Mesh2dOptions,
//! ) -> Result<Mesh2dReport, Mesh2dError> = mesh_swept_profile_2d;
//! ```

pub mod assembly;
pub mod boundary;
pub mod constitutive;
pub mod elements;
pub mod interpolation;
pub mod mesher;
pub mod mpc;
pub mod progressive;
pub mod result;
pub mod shell_assembly;
pub mod shell_boundary;
pub mod shell_result;
pub mod solver;
pub mod warm_state;

pub use assembly::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness, assemble_global_stiffness,
    element_stiffness,
    hex::element_stiffness_hex_p1,
    wedge::element_stiffness_wedge_p1,
};
pub use boundary::{
    DirichletBc, FaceOrder, apply_body_force, apply_dirichlet_row_elimination, apply_point_load,
    apply_traction_load,
};
pub use constitutive::IsotropicElastic;
pub use mpc::{MpcRow, apply_mpc_row_elimination};
pub use progressive::{
    AdvanceDecision, PartialElasticResult, PassTuning, ProgressiveOptions, RefinementDemand,
    TerminationReason, coarse_pass_tuning, near_constraint_boundary, refinement_pass_tuning,
    should_refine,
};
pub use elements::{
    Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement,
    hex_p1::HexP1,
    mitc3_plus::{Mitc3Plus, ShellReferenceCoord, TyingPoint},
    tet_p1::TetP1,
    tet_p2::TetP2,
    wedge_p1::WedgeP1,
};
pub use shell_assembly::{ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness};
pub use shell_boundary::{
    SupportBodyKind, SupportCompatibility, SupportKind, build_support_bcs,
};
pub use shell_result::{
    ShellElementStress, ShellStress, shell_element_frame, shell_element_stress,
};
pub use interpolation::{
    LocatableTet, barycentric_p1, interpolate_p1_at_point, locate_element_p1, point_in_tet_p1,
};
pub use result::{StressElement, element_stress_p1, recover_nodal_stress_p1, tet_volume_p1};
pub use solver::{CgResult, CgSolverOptions, SolverMode, solve_cg, solve_cg_warm};
pub use warm_state::{CgWarmState, solve_cg_with_warm_state};
// Task 2987: 2D cross-section meshing surface for the hex/wedge swept-body
// pipeline. Re-export the typed orchestrator (`mesh_swept_profile_2d`), its
// input/output types, and the pure quality + auto-size helpers so callers
// (task 2988 sweep step, task 2989 eval-side wiring) can reach them via
// `reify_solver_elastic::*` without descending into the `mesher` module.
pub use mesher::{
    auto_mesh_size_from_boundary, compute_quad_skew, mesh_swept_profile_2d, recombine_quality_ok,
    Mesh2d, Mesh2dError, Mesh2dOptions, Mesh2dReport, ProfileBoundary, SweepElementTarget,
};
