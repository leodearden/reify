// `node * 6 + axis` is the dominant DOF-index idiom in this file; allowing
// `+ 0` and `1 *` keeps the formula structure visible at every call site.
#![allow(clippy::identity_op)]

//! T22 integration test: mixed-region shell↔tet MPC coupling validation.
//!
//! PRD reference: `docs/prds/v0_4/structural-analysis-shells.md` §143 (T22).
//!
//! # Model
//!
//! A thin flat MITC3+ shell *flexure* (z=0 plane, x∈[Lb, Lb+Lf]) is
//! cantilevered off a thick solid P1-tet *block* (x∈[0, Lb]), coupled by
//! shell↔tet MPC tying with normal=[0,0,1] (z-normal canonical layout).
//!
//! Dimensions: Lb=1.0, Lf=2.0, W=1.0, H=1.0, t=0.05.
//! Material: E=1.0, ν=0.3 (dimensionless).
//! Block: nx=ny=nz=2 hex cells → Freudenthal-split into P1 tets; 3 z-layers.
//! Flexure: nx_f=4 divisions along x, ny_f=2 along y (y-nodes aligned to block).
//!
//! # Validation contracts
//!
//! 1. **MPC interface continuity** (exact to FP tol): `step-1`/`step-2`.
//! 2. **Rigid-body translation → zero stress everywhere**: `step-3`/`step-4`.
//! 3. **Tip deflection: beam-theory order-of-magnitude envelope**: `step-5`/`step-6`.
//! 4. **Interface stress bounded, no spurious concentration**: `step-7`/`step-8`.
//!
//! # Why not a tight published-reference benchmark
//!
//! Flat-facet MITC3/MITC3+ at coarse mesh, coupled through a compliant tet
//! block, has no achievable tight closed-form reference (see `shell_benchmarks.rs`
//! module doc: "smoke tests, NOT validated benchmarks"). The rigorous validation
//! weight is carried by first-principles-provable contracts 1 and 2, while
//! contracts 3 and 4 use beam-theory ORDER-OF-MAGNITUDE anchors with generous
//! margins matching the `shell_benchmarks.rs` convention.

use reify_solver_elastic::{DirichletBc, IsotropicElastic};

// ─── geometry / material constants ───────────────────────────────────────────

/// Block length along x.
const LB: f64 = 1.0;
/// Flexure length along x.
const LF: f64 = 2.0;
/// Width along y (shared by block and flexure).
const W: f64 = 1.0;
/// Block height (z-span: block occupies z∈[-H/2, +H/2]).
const H: f64 = 1.0;
/// Shell thickness (thin: t ≪ H ensures bending-dominated response).
const T: f64 = 0.05;
/// Young's modulus (dimensionless units).
const E_MOD: f64 = 1.0;
/// Poisson ratio.
const NU: f64 = 0.3;

// ─── shared data types ───────────────────────────────────────────────────────

/// One shell↔tet interface tying node set.
///
/// Represents the triple (bot/mid/top) of block nodes at x=Lb that a single
/// shell interface node is tied to via `MpcRow::shell_tet_tying`.
struct InterfaceNode {
    /// Global index of the shell node at the interface (x=Lb, z=0).
    shell_node: usize,
    /// Global index of the block node at z = −H/2 (bottom of through-thickness).
    block_bot: usize,
    /// Global index of the block node at z = 0 (mid-surface).
    block_mid: usize,
    /// Global index of the block node at z = +H/2 (top of through-thickness).
    block_top: usize,
}

/// Coupled mesh for the flexure-on-block validation model.
///
/// Block (tet) nodes are indexed 0..n_block_nodes; shell nodes follow at
/// n_block_nodes..n_block_nodes+n_shell_nodes. Global DOF for node `n` and
/// axis `a` is `6*n + a` (D=6 unified numbering throughout).
struct CoupledMesh {
    /// All node positions: block nodes first [0..n_block_nodes), then shell
    /// nodes [n_block_nodes..n_block_nodes+n_shell_nodes).
    nodes: Vec<[f64; 3]>,
    /// Count of block (P1-tet) nodes.
    n_block_nodes: usize,
    /// Count of shell (MITC3+) nodes.
    n_shell_nodes: usize,
    /// P1-tet element connectivity: each entry is [n0, n1, n2, n3] (global).
    tet_conn: Vec<[usize; 4]>,
    /// MITC3+ shell element connectivity: each entry is [n0, n1, n2] (global).
    shell_conn: Vec<[usize; 3]>,
    /// Interface tying node sets — one per y-row of block interface nodes.
    /// Each entry ties one shell node to the block's through-thickness triple.
    interface_nodes: Vec<InterfaceNode>,
}

// ─── helpers declared here; DEFINED in step-2 ────────────────────────────────

// NOTE: `build_flexure_on_block_mesh`, `solve_flexure_on_block`, and
// `cantilever_config` are NOT defined in this step (step-1).
// This makes the test file fail to compile — the intended RED state.
// Step-2 provides all implementations.

// ─── tests ───────────────────────────────────────────────────────────────────

/// Verify that every shell↔tet MPC displacement-tying constraint is satisfied
/// to floating-point tolerance after a coupled dense-LU solve.
///
/// # Validation contract (MPC interface continuity)
///
/// For each shell interface node S tied to block mid-node M, for all three
/// displacement axes a ∈ {0, 1, 2}:
///
/// ```text
/// |u[6·S + a] − u[6·M + a]| < 1e-9 · max(1, ‖u‖∞)
/// ```
///
/// This checks that the three displacement-matching MPC rows (the first 3 rows
/// of `MpcRow::shell_tet_tying`) are EXACTLY satisfied after the solve.
///
/// # Achievability
///
/// The MPC recovery identity is already pinned in `mpc.rs:1299–1358` at
/// `residual < 1e-9` on a comparable dense-LU-solved 15×15 system.  An
/// exact-arithmetic identical bound is achievable because:
/// - `apply_mpc_row_elimination` replaces the pivot row with the constraint
///   equation coefficients before the LU factorisation, so the solution is
///   computed in a space where the constraint IS the equation.
/// - Dense LU with full-pivot accumulates O(n² ε_mach) residual — for n ≈ 300
///   DOFs this is < 1e-12, well inside 1e-9.
///
/// # Configuration
///
/// Cantilever: block base (x=0 face) clamped, unit tip point load in +z on
/// the flexure free edge (x=Lb+Lf).
#[test]
fn coupled_interface_displacement_is_continuous_across_mpc() {
    let mat = IsotropicElastic {
        young_modulus: E_MOD,
        poisson_ratio: NU,
    };
    let (mesh, bcs, loads) = cantilever_config(&mat);
    let u = solve_flexure_on_block(&mesh, &bcs, &loads, &mat);

    let u_max = u.iter().copied().fold(0.0_f64, |a, x| a.max(x.abs()));
    let tol = 1e-9 * f64::max(1.0, u_max);

    for iface in &mesh.interface_nodes {
        let s = iface.shell_node;
        let m = iface.block_mid;
        for a in 0..3_usize {
            let u_shell = u[6 * s + a];
            let u_block = u[6 * m + a];
            assert!(
                (u_shell - u_block).abs() < tol,
                "MPC continuity violated at interface (shell={s}, block_mid={m}), \
                 axis={a}: u_shell={u_shell:.6e} u_block={u_block:.6e} \
                 |diff|={:.6e} tol={tol:.6e}",
                (u_shell - u_block).abs(),
            );
        }
    }
}
