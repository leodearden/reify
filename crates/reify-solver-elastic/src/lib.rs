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
//!     // Task 4068: degenerate-shell substrate public surface
//!     shell_element_stiffness_degenerate, Director, ShellRefCoord3, directors_from_facets,
//!     IsotropicElastic,
//!     ShellStress,
//!     ShellElementStress, shell_element_stress,
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
//!     RefinementDemand, AdvanceDecision,
//!     coarse_pass_tuning, refinement_pass_tuning, near_constraint_boundary, should_refine,
//!     SweepElementTarget, Mesh2d, Mesh2dReport, ProfileBoundary, Mesh2dOptions, Mesh2dError,
//!     compute_quad_skew, recombine_quality_ok, auto_mesh_size_from_boundary,
//!     mesh_swept_profile_2d,
//!     // Task 2988: sweep step public surface
//!     SweepParams, SweptMesh3d, SweptConnectivity, SweepError, ThroughThicknessSweepWarning,
//!     sweep_2d_mesh_to_3d, derive_layer_count, check_sweep_through_thickness,
//!     // Task 2996: Z-Z error indicator surface
//!     ZzIndicator, compute_zz_indicator,
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
//! // Degenerate-shell substrate smoke test (Task 4068): the element consumes
//! // explicit per-node directors + the 3D (ξ,η,ζ) reference coordinate, and the
//! // neighbour-averaged facet-normal fallback supplies directors when no
//! // extraction normals exist. A flat facet with +z directors reduces to flat
//! // MITC3+, so the element matrix is the same 18×18 container.
//! let _ = ShellRefCoord3::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
//! let dirs: Vec<Director> = directors_from_facets(&nodes, &[[0, 1, 2]]);
//! assert_eq!(dirs.len(), 3);
//! let k_deg = shell_element_stiffness_degenerate(&nodes, &[dirs[0], dirs[1], dirs[2]], &[0.05; 3], &mat);
//! assert_eq!(k_deg.n_dofs, 18);
//! assert_eq!(k_deg.data.len(), 324);
//!
//! // ShellStress smoke test (T16): use a non-trivial value so a regression where
//! // one channel is left default would surface here.
//! let field = reify_ir::Value::Real(1.0);
//! let ss = ShellStress::homogeneous(field.clone());
//! assert_eq!(ss.top, field, "homogeneous: top must equal input");
//! assert_eq!(ss.mid, field, "homogeneous: mid must equal input");
//! assert_eq!(ss.bottom, field, "homogeneous: bottom must equal input");
//!
//! // T7 smoke tests: local_to_global orthonormality + shell_element_stress API typecheck.
//! let frame_mat: [[f64; 3]; 3] = build_shell_frame(&nodes).local_to_global();
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
//! // shell_element_stress public-API typecheck; zero-DOF behaviour is covered by
//! // shell_result::tests::shell_element_stress_zero_dofs_yields_all_zero_stress.
//! let _: fn(&[[f64; 3]; 3], f64, &IsotropicElastic, &[f64; 18]) -> ShellElementStress = shell_element_stress;
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
//! let _ = FaceOrder::P1Quad;
//! let mut f_smoke = vec![0.0_f64; 12];
//! apply_point_load(&mut f_smoke, 0, [1.0, 2.0, 3.0]);
//! assert_eq!(f_smoke[0], 1.0, "apply_point_load smoke: f[0]");
//! assert_eq!(f_smoke[1], 2.0, "apply_point_load smoke: f[1]");
//! assert_eq!(f_smoke[2], 3.0, "apply_point_load smoke: f[2]");
//!
//! // Task 2986: pin FaceOrder::P1Quad dispatches through apply_traction_load.
//! // Zero traction ⇒ result is exactly the input (no behavioral commitment
//! // beyond compile-and-dispatch). API-surface check matching the P1Tri /
//! // P2Tri pattern.
//! let mut f_quad_smoke = vec![0.0_f64; 12];
//! let quad_face_phys: [[f64; 3]; 4] = [
//!     [-1.0, -1.0, 0.0],
//!     [1.0, -1.0, 0.0],
//!     [1.0, 1.0, 0.0],
//!     [-1.0, 1.0, 0.0],
//! ];
//! apply_traction_load(
//!     &mut f_quad_smoke,
//!     FaceOrder::P1Quad,
//!     &[0_usize, 1, 2, 3],
//!     &quad_face_phys,
//!     [0.0, 0.0, 0.0],
//! );
//! for v in &f_quad_smoke {
//!     assert_eq!(*v, 0.0, "zero traction P1Quad smoke must leave f exactly 0.0");
//! }
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
//! // The import block above asserts key progressive types and helpers compile.
//! // TerminationReason is also re-exported from the crate root; it is covered
//! // transitively via AdvanceDecision in the should_refine pin below rather
//! // than directly in the import block.  These fn-signature pins catch renames
//! // or signature changes at compile time.
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
//! // The default-value contract for `recombine_skew_threshold` is pinned
//! // by `mesher::tests::mesh2d_options_default_matches_spec`; we only check
//! // here that `Mesh2dOptions::default()` is reachable / typed correctly.
//! let _opts_2987: Mesh2dOptions = Mesh2dOptions::default();
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
//!
//! // Task 2988: sweep step — behavioral smoke test.
//! // Variant/field/signature pins live in `sweep::surface_pins` and `sweep::tests`.
//! let base2d = Mesh2d::Triangle {
//!     vertices: vec![0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0],
//!     indices: vec![0_u32, 1, 2],
//! };
//! let swept = sweep_2d_mesh_to_3d(
//!     &base2d,
//!     &SweepParams::Extrude { axis: [0.0_f64, 0.0, 1.0], length: 1.0_f64 },
//!     1,
//! ).unwrap();
//! assert_eq!(swept.layers, 1);
//! assert_eq!(swept.vertices.len(), 18); // 2 planes × 3 verts × 3 coords
//!
//! // Task 2996: Z-Z error indicator surface pin.
//! // Struct-literal + function-item signature pins; behaviour covered by
//! // `error_estimator::tests`. A rename or removal of either the type or
//! // the function trips this doctest at compile time.
//! let _zz = ZzIndicator {
//!     per_element: vec![0.5_f64],
//!     global_relative_energy_error: 0.05_f64,
//! };
//! let _: fn(
//!     &[StressElement<'_>],
//!     &reify_ir::VolumeMesh,
//!     &IsotropicElastic,
//! ) -> ZzIndicator = compute_zz_indicator;
//!
//! // Task 3293: orphan-DOF diagnostic surface — crate-root re-export smoke pin.
//! // Pins that OrphanDofsSummary and detect_orphan_dofs are discoverable from
//! // the crate root.
//! use reify_solver_elastic::{OrphanDofsSummary, detect_orphan_dofs};
//! let _: OrphanDofsSummary = OrphanDofsSummary::default();
//! let s = detect_orphan_dofs(0, &[]);
//! assert_eq!(s.count, 0);
//!
//! // Task 3778: foundation β — minimal usage example for the
//! // per-element field-aware assembly entry point. The four sibling
//! // signatures (P1, P2, hex P1, wedge P1) and the trait-object-safety
//! // check are pinned in `tests/assembly_anisotropic.rs::_signature_pin_*`
//! // alongside the behavioural rows, so this doctest stays a usage
//! // example rather than an API-surface duplicator.
//! use reify_solver_elastic::{
//!     AnisotropicMaterial, ConstantField, element_stiffness_p1_with_field,
//! };
//! let iso_3778 = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
//! let identity_3778: [[f64; 3]; 3] =
//!     [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
//! let field_3778 = ConstantField {
//!     material: AnisotropicMaterial::from_law(&iso_3778, identity_3778),
//! };
//! let unit_tet_3778: [[f64; 3]; 4] = [
//!     [0.0, 0.0, 0.0],
//!     [1.0, 0.0, 0.0],
//!     [0.0, 1.0, 0.0],
//!     [0.0, 0.0, 1.0],
//! ];
//! let k_3778 = element_stiffness_p1_with_field(&unit_tet_3778, &field_3778);
//! assert_eq!(k_3778.n_dofs, 12);
//!
//! // Task 3795: Tensegrity T1b free-standing form-find — crate-root surface
//! // pin. Behaviour is covered by `form_find_free::tests` and the integration
//! // golden `tests/tensegrity_t1b_form_find_free.rs`; this block only pins that
//! // the public surface (ForceDensitySpec / FreeFormResult / FreeFormError /
//! // form_find_free) is reachable and callable from the crate root.
//! use reify_solver_elastic::{
//!     ForceDensitySpec, FreeFormError, FreeFormResult, form_find_free,
//! };
//! // Function-item signature pin: a renamed / removed re-export (or a changed
//! // signature) trips this at doctest-compile time. `MemberKind` is shared with
//! // the anchored T1a kernel and already re-exported.
//! let _: fn(
//!     &[[f64; 3]],
//!     &[(usize, usize)],
//!     &[reify_solver_elastic::MemberKind],
//!     &ForceDensitySpec,
//! ) -> Result<FreeFormResult, FreeFormError> = form_find_free;
//! // Both spec variants — a renamed field or removed variant trips the doctest.
//! let _ = ForceDensitySpec::Explicit(vec![-1.0_f64]);
//! let _ = ForceDensitySpec::GroupRatios {
//!     group_ids: vec![0_usize],
//!     seed_ratios: vec![-1.0_f64],
//!     reference_group: 0_usize,
//! };
//! // Tiny behavioural smoke on the up-front guard: a members/kinds/q length
//! // disagreement is a clean DimensionMismatch (a typed error, never a panic).
//! let dim_err = form_find_free(
//!     &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
//!     &[(0, 1)],
//!     &[reify_solver_elastic::MemberKind::Cable],
//!     &ForceDensitySpec::Explicit(vec![1.0, 2.0]), // 2 densities for 1 member
//! );
//! assert_eq!(dim_err.unwrap_err(), FreeFormError::DimensionMismatch);
//! ```

pub mod assembly;
pub mod boundary;
pub mod buckling_kernel;
pub mod constitutive;
pub mod eigensolve;
pub mod elements;
pub mod error_estimator;
// Task 3794: Tensegrity T1a — anchored Force-Density form-finding kernel.
pub mod form_find;
// Task 3795: Tensegrity T1b — free-standing Force-Density form-finding kernel.
pub mod form_find_free;
pub mod geometric_stiffness;
pub mod interpolation;
// Task 3868: κ — additive joint-stiffness kernel (PRD compliant-joints-flexures.md §7.2).
pub mod joint_stiffness;
pub mod mass_matrix;
pub mod material_field;
pub mod math;
// Task 4066: P2-tet consistent mass-matrix kernel (closed-form degree-4-exact
// barycentric integration) — the missing primitive for P2 modal analysis.
pub mod p2_tet;
pub mod mesher;
pub mod mpc;
pub mod progressive;
pub mod resample;
pub mod result;
pub mod shell_assembly;
pub mod shell_boundary;
pub mod shell_kinematics;
pub mod shell_result;
// Task 3594/δ: flat-plate MITC3 cantilever shell driver (PRD
// shell-extract-engine-bridge.md §3/§5/§7). Neutral-types kernel driver behind
// the reify-eval shell-solve glue.
pub mod shell_solve;
pub mod solver;
pub(crate) mod sparse_util;
pub mod sweep;
pub mod volume_refine;
pub mod warm_state;
// Unconditional `WarmStartableRegistration` submission for NodeKind::Compute
// — see module docs and PRD §5 B5 / I-3 (M-013 fix).
mod warm_register;

pub use assembly::{
    AssemblyElement, AssemblyMode, BarSection, ElementOrder, ElementStiffness, OrphanDofsSummary,
    assemble_global_stiffness, detect_orphan_dofs, element_stiffness, element_stiffness_bar_p1,
    hex::element_stiffness_hex_p1, wedge::element_stiffness_wedge_p1,
    // Task 3778: foundation β — field-aware assembly entry points.
    element_stiffness_hex_p1_with_field, element_stiffness_p1_with_field,
    element_stiffness_p2_with_field, element_stiffness_wedge_p1_with_field,
};
pub use boundary::{
    DirichletBc, FaceOrder, apply_body_force, apply_dirichlet_row_elimination, apply_point_load,
    apply_traction_load,
};
pub use constitutive::{
    ConstitutiveLaw, IsotropicElastic, OrthotropicMaterial, TransverseIsotropicMaterial,
    rotate_voigt,
};
// Task 3778: foundation β — `AnisotropicMaterial` evaluated value bridging
// `ConstitutiveLaw` + frame into a single `Copy` 6×6 + 3×3 record, plus the
// `MaterialField` trait, `ConstantField` constant-lift, and `DiscreteCellField`
// cell-indexed field. Assembly `_with_field` entry points are re-exported in
// step-16.
pub use material_field::{AnisotropicMaterial, ConstantField, DiscreteCellField, MaterialField};
pub use elements::{
    Jacobian, QuadraturePoint, ReferenceCoord, ReferenceElement,
    degenerate_shell::{Director, ShellRefCoord3, directors_from_facets},
    hex_p1::HexP1,
    mitc3_plus::{Mitc3Plus, ShellReferenceCoord, TyingPoint},
    tet_p1::TetP1,
    tet_p2::TetP2,
    wedge_p1::WedgeP1,
};
pub use interpolation::{
    LocatableTet, TetSpatialIndex, barycentric_p1, interpolate_p1_at_point, locate_element_p1,
    point_in_tet_p1,
};
pub use mpc::{MpcRow, apply_mpc_row_elimination};
pub use progressive::{
    AdvanceDecision, PartialElasticResult, PassTuning, ProgressiveOptions, RefinementDemand,
    TerminationReason, coarse_pass_tuning, near_constraint_boundary, refinement_pass_tuning,
    should_refine,
};
// Task 4084: FEA result-model α — Regular3D Sampled Field population.
// GridSpec + resample_*_to_grid compose the 2920 P1-tet primitives
// (barycentric_p1, recover_nodal_stress_p1, tet_volume_p1) into a
// reify_ir::SampledField for displacement (stride 3) and stress (stride 9).
// resample_multi_nodal_to_grid amortises point-location across multiple
// fields on the same geometry (displacement + stress in one pass).
pub use resample::{GridSpec, resample_multi_nodal_to_grid, resample_nodal_to_grid};
pub use result::{
    StressElement, element_stress_p1, element_stress_p2, recover_nodal_stress_p1, tet_volume_p1,
};
pub use shell_assembly::{
    ShellFrame, build_shell_frame, plane_stress_d, shell_element_stiffness,
    shell_element_stiffness_degenerate, shell_element_stiffness_degenerate_ans,
    shell_element_stiffness_mitc3_plus,
};
pub use shell_boundary::{SupportBodyKind, SupportCompatibility, SupportKind, build_support_bcs};
pub use shell_kinematics::{ShellKinematics, shell_kinematics};
pub use shell_result::{
    ShellElementStress, ShellStress, flatten_shell_channels, shell_element_stress,
};
// Task 3594/δ: flat-plate MITC3 cantilever shell driver (neutral types only).
pub use shell_solve::{FlatPlateShellSolve, solve_flat_plate_shell};
// Task 2996: Z-Z error indicator — kernel-layer a-posteriori error estimator.
// PRD: docs/prds/v0_4/a-posteriori-error-estimation.md, Task decomposition #1.
pub use error_estimator::{ZzIndicator, compute_zz_indicator};
// Task 3451: buckling eigensolver kernel — shift-invert Lanczos + dense fallback.
// Task 3882: generic shift-invert Lanczos over arbitrary SPD operator pairs.
// PRD: docs/prds/v0_5/buckling-eigensolver.md §5 / §13 phase 2 task β.
pub use eigensolve::{
    EigenSolverOptions, EigenSolverResult, MetricOp, SparseMetricOp, SparseStiffnessOp,
    StiffnessOp, lanczos_shift_invert, solve_eigen_dense, solve_eigen_shift_invert,
};
// Task 3453: buckling-kernel orchestrator — pre-stress → K_g → eigensolve → mode-shape.
// PRD: docs/prds/v0_5/buckling-eigensolver.md §13 task δ.
pub use buckling_kernel::{
    BucklingKernelOptions, BucklingKernelResult, Mode,
    solve_buckling_kernel, solve_buckling_kernel_p2,
};
// Task 3452: P1-tet K_g element kernel + global assembly + shell/hex/wedge stubs.
// PRD: docs/prds/v0_5/buckling-eigensolver.md §13 task γ.
// Task 3797: T3a bar/cable K_g element kernel + per-member tangent stiffness.
pub use geometric_stiffness::{
    InitialStress3, bar_tangent_stiffness, geometric_element_stiffness_bar_p1,
    geometric_element_stiffness_hex_p1, geometric_element_stiffness_shell,
    geometric_element_stiffness_tet_p1, geometric_element_stiffness_tet_p2,
    geometric_element_stiffness_wedge_p1,
};
// Task 3818: P1-tet consistent mass-matrix element kernel; reuses
// `assemble_global_stiffness` for the global scatter (the assembler treats
// `k_e` opaquely — K vs K_g vs M).
// PRD: docs/prds/v0_3/modal-analysis.md §10 Phase 1 task δ.
pub use mass_matrix::consistent_element_mass_tet_p1;
// Task 4066: P2-tet consistent mass-matrix element kernel (closed-form
// degree-4-exact barycentric integration). Pairs with the P2 stiffness
// (`element_stiffness` at `ElementOrder::P2`) for the modal eigenproblem
// `K φ = λ M φ`; assembled via the same `assemble_global_stiffness` scatter.
pub use p2_tet::consistent_element_mass_tet_p2;
pub use solver::{
    CgIterationControl, CgResult, CgSolverOptions, SolverMode, solve_cg, solve_cg_warm,
    solve_cg_with_progress,
};
pub use warm_state::{CgWarmState, solve_cg_with_warm_state};
// Task 2987: 2D cross-section meshing surface for the hex/wedge swept-body
// pipeline. Re-export the typed orchestrator (`mesh_swept_profile_2d`), its
// input/output types, and the pure quality + auto-size helpers so callers
// (task 2988 sweep step, task 2989 eval-side wiring) can reach them via
// `reify_solver_elastic::*` without descending into the `mesher` module.
pub use mesher::{
    Mesh2d, Mesh2dError, Mesh2dOptions, Mesh2dReport, ProfileBoundary, SweepElementTarget,
    auto_mesh_size_from_boundary, compute_quad_skew, mesh_swept_profile_2d, recombine_quality_ok,
};
// Task 2988: sweep step — 2D mesh × K layers → 3D wedge/hex connectivity.
// PRD reference: docs/prds/v0_3/hex-wedge-meshing.md task #7.
// Downstream consumers:
//   - PRD task #8 (volume-mesh integration wraps SweptMesh3d → VolumeMesh)
//   - PRD task #9 (ElasticOptions wiring: derive_layer_count from mesh_size)
pub use sweep::{
    SweepError, SweepParams, SweptConnectivity, SweptMesh3d, ThroughThicknessSweepWarning,
    check_sweep_through_thickness, derive_layer_count, sweep_2d_mesh_to_3d,
};
// Task 2999: a-posteriori volume mesh refinement driven by per-element size
// hints (PRD docs/prds/v0_4/a-posteriori-error-estimation.md task #2).
//
// `project_per_element_sizes_to_vertices` is intentionally NOT re-exported:
// it is `pub(crate)` because its caller-validation contract (`size_hints.len()
// == n_elements`, see `volume_refine::refine_with_size_field` lines 161-166)
// is enforced by the orchestrator, not by the projector itself. External
// callers cannot misuse it with a short slice.
pub use volume_refine::{RefineError, refine_with_size_field};
// Task 3868: κ — additive joint-stiffness kernel.
// PRD compliant-joints-flexures.md §7.2: each spring-loaded joint contributes
// K[dof,dof] += k to the global stiffness matrix; empty contributions → rigid
// joint (zero addition), preserving existing modal-analysis behaviour.
//
// # Usage example
//
// ```
// use reify_solver_elastic::{JointStiffness, add_joint_stiffness};
// use faer::sparse::{SparseRowMat, Triplet};
//
// // 2×2 K = [[5, 0], [0, 0]] with (1,1) absent
// let trips: Vec<Triplet<usize, usize, f64>> = vec![Triplet::new(0, 0, 5.0)];
// let k = SparseRowMat::try_new_from_triplets(2, 2, &trips).unwrap();
//
// // Add a spring of stiffness 3.0 at DOF 1 (structurally absent → created).
// let k2 = add_joint_stiffness(&k, &[JointStiffness { dof: 1, stiffness: 3.0 }]);
//
// // K[0,0] unchanged; K[1,1] created as 3.0.
// let sym = k2.symbolic();
// let get = |r, c| {
//     let cols = sym.col_idx_of_row_raw(r);
//     let vals = k2.val_of_row(r);
//     cols.iter().zip(vals.iter()).find(|(&col, _)| col == c).map(|(_, &v)| v).unwrap_or(0.0)
// };
// assert_eq!(get(0, 0), 5.0);
// assert_eq!(get(1, 1), 3.0);
// ```
pub use joint_stiffness::{JointStiffness, add_joint_stiffness};
// Task 3794: Tensegrity T1a — anchored Force-Density form-finding kernel.
// PRD: docs/prds/v0_6/tensegrity-structures.md §4. Pure numeric kernel behind
// the `solver::form_find` ComputeNode target; the Value-cracking trampoline
// lives in reify-eval's compute_targets/form_find.rs.
pub use form_find::{FormFindError, FormFindSolve, MemberKind, form_find_anchored};
// Task 3795: Tensegrity T1b — free-standing Force-Density form-finding kernel.
// PRD: docs/prds/v0_6/tensegrity-structures.md Tier-1 leaf T1b. Eigenvalue /
// null-space q search via faer; kernel-only (no .ri / stdlib / trampoline
// wiring in this task, per plan.json design_decisions).
pub use form_find_free::{ForceDensitySpec, FreeFormError, FreeFormResult, form_find_free};
