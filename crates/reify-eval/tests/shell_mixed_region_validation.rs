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

use reify_solver_elastic::{
    DirichletBc, IsotropicElastic, MpcRow, apply_dirichlet_row_elimination,
    apply_mpc_row_elimination, shell_element_stiffness_mitc3_plus,
};

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

/// Block hex-cell divisions in x.
const NX_B: usize = 2;
/// Block hex-cell divisions in y.
const NY_B: usize = 2;
/// Block hex-cell divisions in z.
const NZ_B: usize = 2;
/// Flexure triangle-cell divisions in x.
const NX_F: usize = 4;
/// Flexure triangle-cell divisions in y (must equal NY_B so y-nodes align).
const NY_F: usize = 2;

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

// ─── scaffolding helpers ──────────────────────────────────────────────────────

/// Build the flexure-on-block `CoupledMesh`.
///
/// Block: structured hex grid over x∈[0,Lb], y∈[0,W], z∈[-H/2,+H/2] with
/// NX_B×NY_B×NZ_B hex cells.  Each hex is split into 6 P1 tets via the
/// Freudenthal/Kuhn decomposition.
///
/// Flexure: flat triangulated plate in the z=0 plane over x∈[Lb,Lb+Lf],
/// y∈[0,W] with NX_F×NY_F quad cells each split into 2 CCW triangles.
/// Y-divisions match the block (NY_F==NY_B) so interface nodes align.
///
/// Shell nodes are appended after block nodes (global offset = n_block_nodes).
/// Interface: for each y-index j ∈ 0..=NY_F, the shell node at (is=0, js=j)
/// is tied to the block triple (i=NX_B, j, k) for k=0 (bot), k=1 (mid),
/// k=2 (top).
fn build_flexure_on_block_mesh() -> CoupledMesh {
    let n_bx = NX_B + 1; // 3 x-nodes in block
    let n_by = NY_B + 1; // 3 y-nodes in block
    let n_bz = NZ_B + 1; // 3 z-nodes in block

    // Flat closure: block node index (i,j,k)
    let block_node =
        |i: usize, j: usize, k: usize| -> usize { k * n_bx * n_by + j * n_bx + i };

    // ── Block nodes ──────────────────────────────────────────────────────────
    let mut nodes: Vec<[f64; 3]> = Vec::with_capacity(n_bx * n_by * n_bz);
    for k in 0..n_bz {
        for j in 0..n_by {
            for i in 0..n_bx {
                nodes.push([
                    LB * (i as f64) / (NX_B as f64),
                    W * (j as f64) / (NY_B as f64),
                    -H * 0.5 + H * (k as f64) / (NZ_B as f64),
                ]);
            }
        }
    }
    let n_block_nodes = nodes.len(); // 27

    // ── Tet connectivity: Freudenthal 6-tet decomposition per hex cell ────────
    //
    // Corners of hex cell (ii..ii+1, ji..ji+1, ki..ki+1) are labeled c[0..8]:
    //   c[0]=(0,0,0)  c[1]=(1,0,0)  c[2]=(0,1,0)  c[3]=(1,1,0)
    //   c[4]=(0,0,1)  c[5]=(1,0,1)  c[6]=(0,1,1)  c[7]=(1,1,1)
    //
    // The 6 Freudenthal tets (one per permutation of the Kuhn simplex, all
    // share corner c[0] and the main diagonal endpoint c[7]):
    //   [c0,c1,c3,c7] [c0,c1,c5,c7] [c0,c2,c3,c7]
    //   [c0,c2,c6,c7] [c0,c4,c5,c7] [c0,c4,c6,c7]
    //
    // `element_stiffness_p1` uses |det J| so negative-volume (left-handed)
    // tets still produce correct stiffness; the decomposition is consistent.
    let mut tet_conn: Vec<[usize; 4]> =
        Vec::with_capacity(NX_B * NY_B * NZ_B * 6);
    for ki in 0..NZ_B {
        for ji in 0..NY_B {
            for ii in 0..NX_B {
                let c = [
                    block_node(ii,     ji,     ki),     // c[0]
                    block_node(ii + 1, ji,     ki),     // c[1]
                    block_node(ii,     ji + 1, ki),     // c[2]
                    block_node(ii + 1, ji + 1, ki),     // c[3]
                    block_node(ii,     ji,     ki + 1), // c[4]
                    block_node(ii + 1, ji,     ki + 1), // c[5]
                    block_node(ii,     ji + 1, ki + 1), // c[6]
                    block_node(ii + 1, ji + 1, ki + 1), // c[7]
                ];
                tet_conn.push([c[0], c[1], c[3], c[7]]);
                tet_conn.push([c[0], c[1], c[5], c[7]]);
                tet_conn.push([c[0], c[2], c[3], c[7]]);
                tet_conn.push([c[0], c[2], c[6], c[7]]);
                tet_conn.push([c[0], c[4], c[5], c[7]]);
                tet_conn.push([c[0], c[4], c[6], c[7]]);
            }
        }
    }

    // ── Shell nodes ───────────────────────────────────────────────────────────
    let n_sx = NX_F + 1; // 5 x-nodes in shell
    let n_sy = NY_F + 1; // 3 y-nodes in shell
    let shell_base = n_block_nodes; // 27
    let shell_node = |is: usize, js: usize| -> usize { shell_base + js * n_sx + is };

    for js in 0..n_sy {
        for is in 0..n_sx {
            nodes.push([
                LB + LF * (is as f64) / (NX_F as f64),
                W * (js as f64) / (NY_F as f64),
                0.0,
            ]);
        }
    }
    let n_shell_nodes = nodes.len() - n_block_nodes; // 15

    // ── Shell connectivity: 2 CCW triangles per quad cell (normal = +z) ───────
    let mut shell_conn: Vec<[usize; 3]> = Vec::with_capacity(NX_F * NY_F * 2);
    for js in 0..NY_F {
        for is in 0..NX_F {
            let a = shell_node(is,     js);
            let b = shell_node(is + 1, js);
            let c = shell_node(is,     js + 1);
            let d = shell_node(is + 1, js + 1);
            // [a,b,d]: (b-a)×(d-a) = (+x)×(+x+y) = +z ✓
            shell_conn.push([a, b, d]);
            // [a,d,c]: (d-a)×(c-a) = (+x+y)×(+y) = +z ✓
            shell_conn.push([a, d, c]);
        }
    }

    // ── Interface tying sets ─────────────────────────────────────────────────
    //
    // For each y-row j ∈ 0..=NY_F (=NY_B), tie the shell node at (is=0, js=j)
    // to the block triple:
    //   bot = block_node(NX_B, j, 0)        z = -H/2
    //   mid = block_node(NX_B, j, NZ_B/2)   z =  0
    //   top = block_node(NX_B, j, NZ_B)     z = +H/2
    //
    // Y-coordinates match because NY_F == NY_B: W*j/NY_F == W*j/NY_B ∀ j.
    let mut interface_nodes: Vec<InterfaceNode> = Vec::with_capacity(n_sy);
    for j in 0..n_sy {
        interface_nodes.push(InterfaceNode {
            shell_node: shell_node(0, j),
            block_bot:  block_node(NX_B, j, 0),
            block_mid:  block_node(NX_B, j, NZ_B / 2),
            block_top:  block_node(NX_B, j, NZ_B),
        });
    }

    CoupledMesh {
        nodes,
        n_block_nodes,
        n_shell_nodes,
        tet_conn,
        shell_conn,
        interface_nodes,
    }
}

/// Solve the coupled flexure-on-block FEA system.
///
/// # Method
///
/// 1. Compute per-element stiffnesses: `element_stiffness_p1` for tets,
///    `shell_element_stiffness_mitc3_plus` for shell triangles.
/// 2. Assemble a FULLY-DENSE D·N × D·N `SparseRowMat` (all slots stored so
///    `apply_mpc_row_elimination`'s redistribution-target lookup never panics
///    on shell↔tet cross entries).
/// 3. Build 6 `MpcRow`s per interface node via
///    `MpcRow::shell_tet_tying(normal=[0,0,1], h=H)` and apply
///    `apply_mpc_row_elimination`.
/// 4. Pin orphan tet-rotation DOFs (axes 3,4,5 of every block node) plus the
///    caller-supplied `caller_bcs` via `apply_dirichlet_row_elimination`.
/// 5. Dense LU solve: `k.to_dense().partial_piv_lu()`.
///
/// # Returns
///
/// Displacement vector `u` of length `6·(n_block_nodes + n_shell_nodes)`.
/// For node `n`, DOFs are `u[6·n + 0..6]` = `[u_x, u_y, u_z, θ_x, θ_y, θ_z]`.
fn solve_flexure_on_block(
    mesh: &CoupledMesh,
    caller_bcs: &[DirichletBc],
    loads: &[(usize, f64)],
    mat: &IsotropicElastic,
) -> Vec<f64> {
    use faer::linalg::solvers::Solve;
    use faer::sparse::Triplet;
    use reify_solver_elastic::assembly::tet::element_stiffness_p1;

    let n_total = mesh.n_block_nodes + mesh.n_shell_nodes;
    let ndof = 6 * n_total;

    // ── 1. Per-element stiffnesses ────────────────────────────────────────────

    // Tet: 12-DOF P1 elements (d_e = 3, n_local = 4)
    let tet_ke: Vec<_> = mesh.tet_conn.iter().map(|c| {
        element_stiffness_p1(
            &[mesh.nodes[c[0]], mesh.nodes[c[1]], mesh.nodes[c[2]], mesh.nodes[c[3]]],
            mat,
        )
    }).collect();

    // Shell: 18-DOF MITC3+ elements (d_e = 6, n_local = 3)
    let shell_ke: Vec<_> = mesh.shell_conn.iter().map(|c| {
        shell_element_stiffness_mitc3_plus(
            &[mesh.nodes[c[0]], mesh.nodes[c[1]], mesh.nodes[c[2]]],
            T,
            mat,
        )
    }).collect();

    // ── 2. Assemble fully-dense K ─────────────────────────────────────────────
    //
    // Build dense_k[ndof²] first so every (row, col) slot is pre-allocated.
    // apply_mpc_row_elimination panics if any redistribution-target (row,col)
    // slot is absent; the shell↔tet cross entries are never emitted by element
    // assembly (no shared element), so a sparse assembly alone would miss them.
    // This mirrors the mpc.rs:1298 "fully dense" fixture.

    let mut dk = vec![0.0_f64; ndof * ndof];

    // Scatter tet elements (d_e=3 local → stride 6 in global D=6 numbering)
    for (conn, ke) in mesh.tet_conn.iter().zip(tet_ke.iter()) {
        for (a, &na) in conn.iter().enumerate() {
            for alpha in 0..3_usize {
                let row_g = 6 * na + alpha;
                let row_l = 3 * a + alpha;
                for (b, &nb) in conn.iter().enumerate() {
                    for beta in 0..3_usize {
                        let col_g = 6 * nb + beta;
                        let col_l = 3 * b + beta;
                        dk[row_g * ndof + col_g] += ke.data[row_l * ke.n_dofs + col_l];
                    }
                }
            }
        }
    }

    // Scatter shell elements (d_e=6 local → stride 6 in global D=6 numbering)
    for (conn, ke) in mesh.shell_conn.iter().zip(shell_ke.iter()) {
        for (a, &na) in conn.iter().enumerate() {
            for alpha in 0..6_usize {
                let row_g = 6 * na + alpha;
                let row_l = 6 * a + alpha;
                for (b, &nb) in conn.iter().enumerate() {
                    for beta in 0..6_usize {
                        let col_g = 6 * nb + beta;
                        let col_l = 6 * b + beta;
                        dk[row_g * ndof + col_g] += ke.data[row_l * ke.n_dofs + col_l];
                    }
                }
            }
        }
    }

    // Convert to fully-stored SparseRowMat (all ndof² slots present)
    let mut triplets: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(ndof * ndof);
    for i in 0..ndof {
        for j in 0..ndof {
            triplets.push(Triplet::new(i, j, dk[i * ndof + j]));
        }
    }
    let mut k =
        faer::sparse::SparseRowMat::try_new_from_triplets(ndof, ndof, &triplets).unwrap();

    // ── 3. Load vector ────────────────────────────────────────────────────────

    let mut f = vec![0.0_f64; ndof];
    for &(dof, val) in loads {
        f[dof] += val;
    }

    // ── 4. MPC row elimination ────────────────────────────────────────────────
    //
    // For each interface node: 6 MpcRows via shell_tet_tying(normal=[0,0,1], h=H).
    // For z-normal the 6 rows are:
    //   rows 0-2: displacement matching (pivot = shell_disp_dofs[a], a=0..2)
    //   row  3:   rotation a=0 (pivot = shell_rot_dofs[1] = 6·s+4)
    //   row  4:   rotation a=1 (pivot = shell_rot_dofs[0] = 6·s+3)
    //   row  5:   drilling fallback a=2 (pivot = tet_top_dofs[2] = 6·block_top+2)
    // All 18 pivots across the 3 interface nodes are distinct.

    let mut mpc_rows: Vec<MpcRow> = Vec::new();
    for iface in &mesh.interface_nodes {
        let s = iface.shell_node;
        let m = iface.block_mid;
        let t = iface.block_top;
        let b = iface.block_bot;
        let rows = MpcRow::shell_tet_tying(
            [6 * s + 0, 6 * s + 1, 6 * s + 2], // shell_disp_dofs
            [6 * s + 3, 6 * s + 4, 6 * s + 5], // shell_rot_dofs
            [6 * t + 0, 6 * t + 1, 6 * t + 2], // tet_top_dofs
            [6 * m + 0, 6 * m + 1, 6 * m + 2], // tet_mid_dofs
            [6 * b + 0, 6 * b + 1, 6 * b + 2], // tet_bot_dofs
            [0.0, 0.0, 1.0],                    // z-normal
            H,                                  // through-thickness separation
        );
        mpc_rows.extend(rows);
    }
    apply_mpc_row_elimination(&mut k, &mut f, &mpc_rows);

    // ── 5. Dirichlet BCs: orphan tet-rotation DOFs + caller BCs ──────────────
    //
    // Every block node n ∈ 0..n_block_nodes carries orphan rotation axes 3,4,5
    // in the D=6 global system (tet elements only fill axes 0,1,2). These are
    // structurally-zero diagonal rows that make K singular; pinning to 0 is
    // physically harmless (tets carry no rotational DOFs — those are numbering
    // artifacts only). This also does NOT overlap with MPC pivots: MPC pivots
    // are shell-node DOFs or tet-node axis-2 (drilling fallback), never
    // tet-node axes 3,4,5.

    let mut all_bcs: Vec<DirichletBc> = Vec::new();
    for n in 0..mesh.n_block_nodes {
        for axis in 3..6_usize {
            all_bcs.push(DirichletBc { dof: 6 * n + axis, value: 0.0 });
        }
    }
    all_bcs.extend_from_slice(caller_bcs);
    // Deduplicate (apply_dirichlet_row_elimination panics on duplicate DOFs)
    all_bcs.sort_by_key(|bc| bc.dof);
    all_bcs.dedup_by_key(|bc| bc.dof);
    apply_dirichlet_row_elimination(&mut k, &mut f, &all_bcs);

    // ── 6. Dense LU solve ─────────────────────────────────────────────────────
    //
    // MPC row-elimination produces a non-symmetric reduced system (pivot rows
    // hold the constraint equations, not stiffness rows) — CG (SPD-only) is
    // incorrect. Dense partial-pivot LU mirrors the mpc.rs:1339 pattern.

    let k_dense = k.to_dense();
    let plu = k_dense.partial_piv_lu();
    let mut rhs = faer::Mat::<f64>::from_fn(ndof, 1, |i, _| f[i]);
    plu.solve_in_place(&mut rhs);
    rhs.col_as_slice(0_usize).to_vec()
}

/// Return `(mesh, bcs, loads)` for the canonical cantilever configuration.
///
/// - **Boundary conditions**:
///   - Base clamp: all translation DOFs (axes 0,1,2) of block nodes on the
///     x=0 face are pinned to 0 (9 nodes × 3 DOFs = 27 BCs).
///   - Shell drilling: the MITC3+ flat-plate element carries zero stiffness
///     for the drilling rotation (θ_z = axis 5). Pinning it to 0 at all shell
///     nodes prevents a rank-deficient K (matches `shell_benchmarks.rs:1538`).
///
/// - **Loads**: unit point load P=1.0 in the +z direction distributed equally
///   across the ny_f+1 = 3 nodes on the flexure free edge (x=Lb+Lf).
///   Each free-edge node receives P/3 = 1/3 in the +z DOF.
///
/// The `_mat` parameter is accepted for API uniformity with future test helpers
/// that may parameterise loads by material constants.
fn cantilever_config(
    _mat: &IsotropicElastic,
) -> (CoupledMesh, Vec<DirichletBc>, Vec<(usize, f64)>) {
    let mesh = build_flexure_on_block_mesh();

    let n_bx = NX_B + 1; // 3
    let n_bz = NZ_B + 1; // 3
    let n_sx = NX_F + 1; // 5

    let block_node =
        |i: usize, j: usize, k: usize| -> usize { k * n_bx * (NY_B + 1) + j * n_bx + i };
    let shell_node =
        |is: usize, js: usize| -> usize { mesh.n_block_nodes + js * n_sx + is };

    let mut bcs: Vec<DirichletBc> = Vec::new();

    // ── Base clamp: block x=0 face (i=0, all j,k), translations only ─────────
    // (axes 3,4,5 for all block nodes are handled internally by
    // solve_flexure_on_block via orphan-DOF pinning)
    for k in 0..n_bz {
        for j in 0..(NY_B + 1) {
            let n = block_node(0, j, k);
            for axis in 0..3_usize {
                bcs.push(DirichletBc { dof: 6 * n + axis, value: 0.0 });
            }
        }
    }

    // ── Shell drilling: θ_z = 0 for all shell nodes ───────────────────────────
    // MITC3+ on a flat plate has zero drilling stiffness (K_shell[6s+5][6s+5]=0
    // for all s when all shell normals are parallel). Without this pin K is
    // rank-deficient. Matches the flat-plate precedent in shell_benchmarks.rs.
    for js in 0..(NY_F + 1) {
        for is in 0..(NX_F + 1) {
            let s = shell_node(is, js);
            bcs.push(DirichletBc { dof: 6 * s + 5, value: 0.0 });
        }
    }

    // ── Tip load: P=1.0 in +z, distributed over free-edge shell nodes ─────────
    // Free edge: is = NX_F (x = Lb + Lf), js = 0..=NY_F.
    // ny_f+1 = 3 nodes each receive P/(NY_F+1).
    let n_tip = NY_F + 1;
    let p_per_node = 1.0 / (n_tip as f64);
    let mut loads: Vec<(usize, f64)> = Vec::new();
    for js in 0..n_tip {
        let s = shell_node(NX_F, js);
        loads.push((6 * s + 2, p_per_node)); // axis 2 = u_z
    }

    (mesh, bcs, loads)
}

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
        youngs_modulus: E_MOD,
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
