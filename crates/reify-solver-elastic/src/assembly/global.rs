//! Global sparse-matrix assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #9. This module
//! scatters per-element [`crate::assembly::ElementStiffness`] dense matrices
//! into a global sparse stiffness matrix `K` of size `3N × 3N` (N = global
//! node count) using `faer-rs` CSR triplet builders.

use faer::sparse::{SparseRowMat, Triplet};

use super::ElementStiffness;

/// One element's contribution to the global system.
///
/// `connectivity` lists the global node IDs of the element's local nodes in
/// the same order as the rows/columns of `k_e` — that is, the local DOF index
/// `3 * a + α` (axis `α ∈ {0, 1, 2}`) maps to global DOF
/// `3 * connectivity[a] + α`.
///
/// The `id` field is descriptive metadata used in panic messages (e.g. to
/// name the offending element in a contract violation) and is *not* used
/// internally as a sort key in any [`AssemblyMode`]. Callers requiring a
/// canonical iteration order in [`AssemblyMode::Deterministic`] must sort
/// the slice themselves before passing it in.
pub struct AssemblyElement<'a> {
    /// Element ID (descriptive metadata; surfaces in panic messages).
    pub id: usize,
    /// Global node IDs — `connectivity.len() * 3 == k_e.n_dofs`.
    pub connectivity: &'a [usize],
    /// Per-element stiffness matrix.
    pub k_e: &'a ElementStiffness,
}

/// How [`assemble_global_stiffness`] iterates over `elements` when scattering
/// per-element triplets into the global system.
///
/// # `Deterministic`
///
/// Single-threaded, slice-order accumulation. The triplet emission order is
/// exactly the iteration order of the input slice. faer's CSR builder sums
/// duplicate `(row, col)` pairs in the order it encounters them, so the
/// global `K[i][j]` summation order is fully determined by the slice's
/// iteration order. Bit-stable across runs **and across machines**.
///
/// # `Parallel { threads }`
///
/// Multi-threaded scatter via `std::thread::scope`. The element slice is
/// partitioned into `threads` chunks; each thread accumulates a local
/// `Vec<Triplet>` in slice order; after join the per-thread Vecs are
/// concatenated in **thread-spawn order (0, 1, 2, …)** before being handed
/// to faer. This gives bit-stability for any *fixed* thread count, but the
/// summation order — and hence the LSB of shared-DOF sums — varies across
/// thread counts. Cross-thread-count equivalence is bounded by
/// `O(ulp · max|K_e[i][j]|)`, far below the FEA tolerance band.
///
/// `Parallel { threads: 0 }` is rejected with a panic at function entry —
/// auto-falling-back to single-threaded would silently mask caller bugs
/// (e.g. a misread config defaulting `threads` to 0). The "tiny problems
/// fall back to single-threaded under 10K DOF" policy lives in the
/// `ElasticOptions` resolution layer (PRD task #16), not in this primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyMode {
    /// Single-threaded, slice-order accumulation.
    Deterministic,
    /// Multi-threaded scatter with fixed-thread-id-order merge. `threads`
    /// must be `>= 1`; passing `0` panics.
    Parallel {
        /// Worker thread count.
        threads: usize,
    },
}

/// Scatter per-element stiffness matrices into a global `3N × 3N` sparse
/// stiffness matrix.
///
/// `n_nodes` is the global node count; the returned matrix has
/// `3 * n_nodes` rows and columns. `elements` is the slice of element
/// contributions (see [`AssemblyElement`]); each contribution emits a full
/// dense `(a, b, α, β)` block of `9 · k_e.n_local²` triplets, and faer's
/// CSR builder sums duplicates that share a `(row, col)` pair.
///
/// # Symmetry
///
/// `K_global` inherits the symmetry of the per-element `K_e`. For
/// `IsotropicElastic` materials `K_e = ∫ BᵀDB dV` is symmetric by
/// construction (Task 2915 pins this via unit tests). The emission loop
/// in `emit_element_triplets` emits the **full dense `(a, b, α, β)` block**
/// — both `(a, b)` and `(b, a)` for every local pair — rather than just
/// the upper triangle. Combined with faer's stable duplicate-summation
/// order, this means `|K_global[i][j] − K_global[j][i]|` is bounded by
/// `O(ulp · max|K_e[i][j]|)` for any input — far below
/// `1e-9 · max(|K[i][j]|, |K[j][i]|, 1)`, the tolerance pinned by
/// `global_k_is_symmetric_within_fp_tolerance`.
///
/// Why full-block instead of upper-triangle-only: emitting only the upper
/// triangle would shift the mirror-and-sum bookkeeping onto callers (or
/// onto a separate post-pass), and would couple `assemble_global_stiffness`
/// to the property "`K_e` is symmetric" — a property of the constitutive
/// law, not a hard contract on the input. The `9 · n_local²` triplet
/// emission per element is dominated by the constitutive-tensor pre-pass
/// (PRD task #8) anyway, so the 2× emission cost is invisible at the
/// pipeline level. See `design_decisions[2]` in the plan for the full
/// rationale.
///
/// # Panics
///
/// - `AssemblyMode::Parallel { threads: 0 }` — auto-fallback to
///   single-threaded would silently mask caller bugs (e.g. a misread config
///   defaulting `threads` to 0); the panic surfaces them at the call site.
///   The "tiny problems run single-threaded" policy lives in the
///   `ElasticOptions` resolution layer (PRD task #16), not in this primitive.
/// - `connectivity.len() * 3 != k_e.n_dofs` for any element — the per-element
///   DOF count must agree with the connectivity, otherwise the
///   `(3·conn[a]+α, 3·conn[b]+β)` mapping is ill-defined. The panic message
///   names the offending element id.
/// - `connectivity[i] >= n_nodes` for any element — out-of-range global node
///   ID would translate to an out-of-range DOF row/col index. The panic
///   message names the offending element id and node id.
///
/// See [`AssemblyMode`] for the iteration / merge contract per mode.
pub fn assemble_global_stiffness(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
    mode: AssemblyMode,
) -> SparseRowMat<usize, f64> {
    // Public-surface contract checks. Unconditional `assert!` (not
    // `debug_assert!`) per the project's Task-2544 contract-explicitness
    // convention, mirrored in `assembly/mod.rs::element_stiffness` and
    // `elements/mod.rs::ReferenceElement::jacobian`.
    //
    // Threads check first, before per-element checks: a `Parallel { threads: 0 }`
    // call with an empty `elements` slice should still panic, surfacing the
    // caller bug regardless of mesh size.
    if let AssemblyMode::Parallel { threads } = mode {
        assert!(
            threads != 0,
            "AssemblyMode::Parallel {{ threads: 0 }} is invalid: \
             auto-fallback to single-threaded would silently mask \
             caller bugs (e.g. a misread config defaulting threads to 0). \
             Pass threads >= 1, or use AssemblyMode::Deterministic for \
             single-threaded slice-order accumulation.",
        );
    }
    for element in elements {
        assert_eq!(
            element.connectivity.len() * 3,
            element.k_e.n_dofs,
            "AssemblyElement {{ id: {} }} has connectivity.len() = {} \
             but k_e.n_dofs = {}; expected connectivity.len() * 3 == k_e.n_dofs",
            element.id,
            element.connectivity.len(),
            element.k_e.n_dofs,
        );
        for &node in element.connectivity {
            assert!(
                node < n_nodes,
                "AssemblyElement {{ id: {} }} references node {} \
                 but n_nodes = {} (valid range is 0..{})",
                element.id,
                node,
                n_nodes,
                n_nodes,
            );
        }
    }

    // Mode-specific dispatch. The deterministic arm exercises the shared
    // `emit_element_triplets` scatter primitive in slice order; the
    // parallel arm partitions into `threads` chunks via
    // `std::thread::scope` and merges per-thread Vecs in spawn order.
    //
    // Each element emits a full dense block of `9 · n_local²` triplets
    // (3 axes × 3 axes × n_local² local DOF pairs). Pre-sizing both the
    // merged accumulator and per-thread local Vecs to the exact triplet
    // count avoids the O(log N) reallocs `Vec::new()` would walk through
    // on the FEA hot path — for ~10K P1 elements at 144 triplets each
    // (24 B per `Triplet<usize, usize, f64>`), that's ~34 MB of
    // allocator churn the worker would otherwise perform per chunk.
    let total_triplets: usize = elements
        .iter()
        .map(|e| 9 * e.connectivity.len() * e.connectivity.len())
        .sum();
    let triplets: Vec<Triplet<usize, usize, f64>> = match mode {
        AssemblyMode::Deterministic => {
            let mut acc = Vec::with_capacity(total_triplets);
            for element in elements {
                emit_element_triplets(element, &mut acc);
            }
            acc
        }
        AssemblyMode::Parallel { threads } => {
            // Partition `elements` into `threads` chunks via
            // `chunks(div_ceil(...))`, so chunk i goes to thread i with
            // at most one short tail chunk. `.max(1)` clamps the chunk
            // size: `[].chunks(0)` would panic, but `[].chunks(1)`
            // yields zero chunks ⇒ zero spawned threads ⇒ empty triplet
            // Vec, the right behavior for the empty-elements case.
            //
            // When `elements.len() < threads`, only `elements.len()`
            // threads spawn (one per non-empty chunk); the requested
            // `threads` count is an upper bound, not a lower bound.
            //
            // # Determinism contract
            //
            // **The merge order is the thread spawn order, which is also
            // the thread-id order.** Concretely:
            //
            //   (a) `elements.chunks(chunk_size)` is called once with a
            //       stable chunk size, so the chunk-iteration order is
            //       deterministic and matches `elements`'s slice order.
            //   (b) Threads spawn sequentially in chunk-iteration order,
            //       so the thread-id `t` for the worker handling chunk
            //       `t` is fixed.
            //   (c) `handles[t].join()` is called in `t`-ascending order,
            //       and `acc.extend(...)` appends each worker's local
            //       Vec in that order — preserving thread-spawn order
            //       in the merged Vec.
            //
            // Reordering the spawn loop, switching to a non-stable chunk
            // dispatch (e.g. work-stealing), or joining handles in any
            // order other than spawn order would break the
            // fixed-thread-count bit-stability contract pinned by
            // `parallel_mode_bit_equal_to_deterministic_on_disjoint_mesh`
            // (step-11) and the back-to-back determinism check in
            // `parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`
            // (step-13). See PRD `docs/prds/v0_3/structural-analysis-fea.md`
            // task #9 for the user-facing contract.
            let chunk_size = elements.len().div_ceil(threads).max(1);
            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(threads);
                for chunk in elements.chunks(chunk_size) {
                    handles.push(s.spawn(move || {
                        // Pre-size to the exact per-chunk triplet count;
                        // see the rationale on `total_triplets` above.
                        let cap: usize = chunk
                            .iter()
                            .map(|e| 9 * e.connectivity.len() * e.connectivity.len())
                            .sum();
                        let mut local: Vec<Triplet<usize, usize, f64>> =
                            Vec::with_capacity(cap);
                        for element in chunk {
                            emit_element_triplets(element, &mut local);
                        }
                        local
                    }));
                }
                let mut acc: Vec<Triplet<usize, usize, f64>> =
                    Vec::with_capacity(total_triplets);
                for h in handles {
                    // Joining in handle-vector order = spawn order =
                    // chunk-iteration order in `elements`. A worker
                    // panic propagates via `expect`; per the project's
                    // contract-explicitness convention, we surface the
                    // worker panic at the caller rather than swallowing
                    // it into a generic error.
                    acc.extend(h.join().expect("global-assembly worker thread panicked"));
                }
                acc
            })
        }
    };
    // faer 0.24's `try_new_from_triplets` **sums duplicate `(row, col)`
    // entries in encounter order**. We rely on this contract: when two
    // (or more) elements share a DOF pair, each element emits its own
    // triplet, and the accumulated `K_global[i][j]` is the sum of all
    // contributions in slice-iteration order. Verified by faer's own
    // `test_from_indices` (sparse/mod.rs:280-326), which asserts
    // `mat.val() == &[1.0 + 3.0, ..., 6.0 + 7.0]` after seeding two
    // duplicate `(0,0)` and `(3,3)` triplets — the assertion uses the
    // unevaluated `1.0 + 3.0` form, which is bit-exact for those values
    // but documents the encounter-order sum contract.
    //
    // If a future faer version regresses this (e.g. overwrites instead of
    // summing), the fix is to switch the local helper to a pre-merge pass
    // that sums in a `BTreeMap<(row, col), f64>` keyed by the canonical
    // `(row, col)` order. step-5's `two_p1_elements_sharing_face_*` test
    // would surface the regression.
    SparseRowMat::try_new_from_triplets(3 * n_nodes, 3 * n_nodes, &triplets)
        .expect("triplets within declared 3*n_nodes dims (per-element bounds enforced upstream)")
}

/// Emit one dense `9 · n_local²` block of triplets for `element` and append
/// to `out`. The emission order is the C-style row-major nesting
/// `for a in 0..n_local { for α in 0..3 { for b in 0..n_local { for β in 0..3 } } }` —
/// chosen so the within-block `(row, col)` sequence is monotonic, which
/// gives faer's duplicate-summation step a stable input ordering and
/// matches the row-major layout of [`ElementStiffness::data`] (one
/// sequential read per output triplet).
///
/// # N-agnostic contract
///
/// The loop bounds derive from `n_local = element.connectivity.len()` and
/// the constant `3` (axes per node) exclusively — there is no hardcoded
/// `4` (P1 node count) or `10` (P2 node count) anywhere in the body. The
/// `assemble_global_stiffness` entry-point's per-element contract check
/// (`connectivity.len() * 3 == k_e.n_dofs`) is the only coupling between
/// `n_local` and `k_e`, so this primitive accepts any element shape whose
/// K_e satisfies that invariant. Pinned by the
/// `single_p2_element_identity_connectivity_matches_k_e_bit_for_bit` test.
fn emit_element_triplets(
    element: &AssemblyElement<'_>,
    out: &mut Vec<Triplet<usize, usize, f64>>,
) {
    let n_local = element.connectivity.len();
    // Iteration order is `(a, α, b, β)` so row = 3*conn[a]+α stays fixed
    // for the inner `(b, β)` sweep — that's the row-major traversal of
    // both the local k_e block and the global K row, minimizing cache
    // pressure and giving faer monotonically non-decreasing rows for
    // duplicate-summation stability.
    for a in 0..n_local {
        let row_node = element.connectivity[a];
        for alpha in 0..3 {
            let row = 3 * row_node + alpha;
            let local_row = 3 * a + alpha;
            for b in 0..n_local {
                let col_node = element.connectivity[b];
                for beta in 0..3 {
                    let col = 3 * col_node + beta;
                    let local_col = 3 * b + beta;
                    let val = element.k_e.get(local_row, local_col);
                    out.push(Triplet::new(row, col, val));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::tet::{element_stiffness_p1, element_stiffness_p2};
    use crate::assembly::test_support::scaled_p2_phys_nodes;
    use crate::constitutive::IsotropicElastic;

    /// Steel-like dimensionless material reused across the global-assembly
    /// tests. Mirrors the convention from `assembly::tests::dimensionless_steel_like`
    /// and `tet::tests::dimensionless_steel_like` so K_e numerics stay in
    /// O(1) range for human-readable failure messages.
    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Read entry `(i, j)` of a `SparseRowMat<usize, f64>` as a plain `f64`,
    /// returning `0.0` if the entry is not stored. Lets test code densify
    /// the global K with one read per `(row, col)` regardless of whether
    /// the assembly path bothered to store explicit zero entries.
    fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
        k.get(i, j).copied().unwrap_or(0.0)
    }

    /// Empty `elements` slice → `3N × 3N` all-zero sparse matrix.
    ///
    /// Pins the empty-input contract: the function returns a matrix whose
    /// dimensions match `3 * n_nodes`, and whose stored-entry count is zero
    /// (faer's CSR builder must accept a zero-triplet input cleanly).
    #[test]
    fn empty_elements_returns_zero_3n_by_3n_sparse_matrix() {
        // Compile-only construction of both `AssemblyMode` variants so a
        // future regression that drops one of the variants surfaces here.
        let _det = AssemblyMode::Deterministic;
        let _par = AssemblyMode::Parallel { threads: 1 };

        let n_nodes = 4;
        let k = assemble_global_stiffness(n_nodes, &[], AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 3 * n_nodes);
        assert_eq!(k.ncols(), 3 * n_nodes);
        assert_eq!(k.compute_nnz(), 0, "no triplets ⇒ zero stored entries");
    }

    /// Build a P1 K_e at the unit reference tet for a stiffer-or-softer
    /// material. We reuse the unit-tet geometry so the only difference
    /// between two K_e instances is the linear `E` scaling — making the
    /// per-element contributions visually distinguishable in failure
    /// messages while keeping the geometry trivial.
    fn k_e_p1_with_youngs_modulus(youngs_modulus: f64) -> ElementStiffness {
        let mat = IsotropicElastic {
            youngs_modulus,
            poisson_ratio: 0.3,
        };
        element_stiffness_p1(&UNIT_TET_P1, &mat)
    }

    /// Single P1 element with identity connectivity `[0,1,2,3]` → K_global
    /// equals K_e bit-for-bit at every entry.
    ///
    /// Pins the DOF-mapping rule:
    /// `K_global[3*conn[a]+α][3*conn[b]+β] = K_e[3*a+α][3*b+β]`. With
    /// identity connectivity the rule degenerates to identity, so the
    /// densified 12×12 must match K_e exactly. A future regression that
    /// transposes the row/col mapping (or shifts axis-major vs node-major
    /// indexing) will surface here.
    #[test]
    fn single_p1_element_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        let connectivity = [0usize, 1, 2, 3];
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(4, &[element], AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 12);
        assert_eq!(k.ncols(), 12);

        for i in 0..12 {
            for j in 0..12 {
                let actual = read(&k, i, j);
                let expected = k_e.get(i, j);
                // Bit-for-bit: identity mapping ⇒ no FP-summation reordering.
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] = {actual} but K_e[{i}][{j}] = {expected}",
                );
            }
        }
    }

    /// Two adjacent P1 elements sharing the face {1, 2, 3}; shared-DOF
    /// entries sum, exclusive-DOF entries pass through unchanged.
    ///
    /// Element 0 uses connectivity `[0,1,2,3]` (identity-mapped), element 1
    /// uses `[1,2,3,4]` (shifted by +1 from local). Two distinguishable
    /// materials (`E=1.0` vs `E=2.0`) keep K_e0 and K_e1 per-entry visually
    /// distinct in failure messages — they differ by a strict `2.0` factor.
    /// The mesh has `n_nodes = 5 ⇒ K_global is 15 × 15`.
    ///
    /// Three independent assertion blocks cover the three contribution
    /// patterns:
    /// - Both DOFs anchored to node 0 (or any pair where one node is 0):
    ///   only element 0 contributes.
    /// - Both DOFs anchored to node 4 (or any pair where one node is 4):
    ///   only element 1 contributes.
    /// - Both DOFs anchored to nodes {1, 2, 3}: both elements contribute,
    ///   summed in element-iteration order.
    ///
    /// Pinning the per-element-mapping equation in three separate blocks —
    /// rather than re-implementing the production scatter as a check —
    /// catches a regression that, say, swaps the local-DOF index direction
    /// for one element only.
    #[test]
    fn two_p1_elements_sharing_face_accumulate_at_shared_dofs() {
        let k_e0 = k_e_p1_with_youngs_modulus(1.0);
        let k_e1 = k_e_p1_with_youngs_modulus(2.0);
        assert_eq!(k_e0.n_dofs, 12);
        assert_eq!(k_e1.n_dofs, 12);

        let conn0 = [0usize, 1, 2, 3];
        let conn1 = [1usize, 2, 3, 4];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn0,
                k_e: &k_e0,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn1,
                k_e: &k_e1,
            },
        ];
        let n_nodes = 5;
        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 15);
        assert_eq!(k.ncols(), 15);

        // Helper: K_e0 contributes at (i, j) iff both nodes are in conn0
        // (nodes 0..=3); local index in element 0 = global node (identity).
        // K_e1 contributes iff both nodes are in conn1 (nodes 1..=4); local
        // index = global node - 1.
        let in_e0 = |node: usize| node <= 3;
        let in_e1 = |node: usize| (1..=4).contains(&node);

        for node_a in 0..n_nodes {
            for node_b in 0..n_nodes {
                for alpha in 0..3 {
                    for beta in 0..3 {
                        let i = 3 * node_a + alpha;
                        let j = 3 * node_b + beta;
                        let mut expected = 0.0_f64;
                        if in_e0(node_a) && in_e0(node_b) {
                            expected += k_e0.get(3 * node_a + alpha, 3 * node_b + beta);
                        }
                        if in_e1(node_a) && in_e1(node_b) {
                            // element 1's local indexing shifts by -1.
                            expected += k_e1.get(3 * (node_a - 1) + alpha, 3 * (node_b - 1) + beta);
                        }
                        let actual = read(&k, i, j);
                        // Two-summand FP add is order-independent in IEEE754
                        // for a single (a+b) pairing — and faer iterates
                        // duplicates in encounter order, which here is
                        // element 0 then element 1 (matches our `expected`
                        // construction). Bit-equality is achievable.
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "K_global[{i}][{j}] (node_a={node_a}, node_b={node_b}, \
                             alpha={alpha}, beta={beta}): actual={actual}, expected={expected}",
                        );
                    }
                }
            }
        }
    }

    /// Mismatched `connectivity.len()` and `k_e.n_dofs` panics with a
    /// descriptive message.
    ///
    /// 4-node connectivity paired with a 30-DOF P2 K_e — `4 * 3 = 12 ≠ 30`,
    /// so the per-element contract `connectivity.len() * 3 == k_e.n_dofs`
    /// is violated. The panic message must mention the offending element id
    /// and both observed dimensions to make debugging single-element
    /// failures in a 100K-element mesh tractable.
    #[test]
    #[should_panic(expected = "k_e.n_dofs")]
    fn mismatched_connectivity_length_and_k_e_n_dofs_panics() {
        let mat = dimensionless_steel_like();
        let phys = scaled_p2_phys_nodes(1.0);
        let k_e_p2 = element_stiffness_p2(&phys, &mat);
        assert_eq!(k_e_p2.n_dofs, 30);

        let conn = [0usize, 1, 2, 3]; // 4 nodes — incompatible with 30 DOFs.
        let element = AssemblyElement {
            id: 7,
            connectivity: &conn,
            k_e: &k_e_p2,
        };
        let _ = assemble_global_stiffness(10, &[element], AssemblyMode::Deterministic);
    }

    /// Out-of-range connectivity entry (`>= n_nodes`) panics with a
    /// descriptive message naming the offending element id and node id.
    #[test]
    #[should_panic(expected = "AssemblyElement")]
    fn out_of_range_connectivity_entry_panics() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let conn = [0usize, 1, 2, 5]; // node 5 ≥ n_nodes = 4.
        let element = AssemblyElement {
            id: 42,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_global_stiffness(4, &[element], AssemblyMode::Deterministic);
    }

    /// `AssemblyMode::Parallel { threads: 0 }` panics rather than
    /// auto-falling-back to single-threaded.
    ///
    /// Per the design decision pinned in plan.json: auto-fallback masks
    /// caller bugs (e.g. `ElasticOptions.threads` defaulting to 0 from a
    /// misread config); the policy that "tiny problems run
    /// single-threaded" lives in PRD task #16's `ElasticOptions`
    /// resolution layer, not in this primitive.
    #[test]
    #[should_panic(expected = "AssemblyMode::Parallel")]
    fn parallel_mode_with_zero_threads_panics() {
        let _ = assemble_global_stiffness(4, &[], AssemblyMode::Parallel { threads: 0 });
    }

    /// Parallel mode produces the bit-identical dense matrix as deterministic
    /// mode on a 4-element disjoint-tet mesh, for thread counts 1, 2, and 4.
    ///
    /// The mesh has no shared nodes (4 tets, each on 4 fresh nodes for
    /// `n_nodes = 16`), so every global DOF receives at most one element's
    /// contribution. Without duplicate summation there is no FP
    /// non-associativity, so bit-equality is achievable across any
    /// partition / merge order — this is the strongest possible bit-stability
    /// claim, applicable to *any* thread count, not just to a fixed thread
    /// count.
    ///
    /// Pinning all four results to bit-identical means a future regression
    /// that introduces an out-of-order accumulation in the parallel path
    /// (even on disjoint meshes) surfaces here. Step-13 covers the
    /// shared-DOF case where bit-equality across threads is impossible and
    /// only tolerance-equivalence holds.
    #[test]
    fn parallel_mode_bit_equal_to_deterministic_on_disjoint_mesh() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        // 4 disjoint tets, each on its own block of 4 nodes.
        let conns: [[usize; 4]; 4] =
            [[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11], [12, 13, 14, 15]];
        let n_nodes = 16;
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, c)| AssemblyElement {
                id: i,
                connectivity: c,
                k_e: &k_e,
            })
            .collect();

        let det = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let par1 = assemble_global_stiffness(
            n_nodes,
            &elements,
            AssemblyMode::Parallel { threads: 1 },
        );
        let par2 = assemble_global_stiffness(
            n_nodes,
            &elements,
            AssemblyMode::Parallel { threads: 2 },
        );
        let par4 = assemble_global_stiffness(
            n_nodes,
            &elements,
            AssemblyMode::Parallel { threads: 4 },
        );
        assert_eq!(det.nrows(), 3 * n_nodes);
        assert_eq!(det.ncols(), 3 * n_nodes);

        for i in 0..3 * n_nodes {
            for j in 0..3 * n_nodes {
                let d = read(&det, i, j);
                let p1 = read(&par1, i, j);
                let p2 = read(&par2, i, j);
                let p4 = read(&par4, i, j);
                // Bit-equality across all four — disjoint mesh ⇒ no
                // duplicate summation ⇒ no FP non-associativity.
                assert_eq!(
                    d.to_bits(),
                    p1.to_bits(),
                    "K[{i}][{j}] det={d} != par1={p1}",
                );
                assert_eq!(
                    d.to_bits(),
                    p2.to_bits(),
                    "K[{i}][{j}] det={d} != par2={p2}",
                );
                assert_eq!(
                    d.to_bits(),
                    p4.to_bits(),
                    "K[{i}][{j}] det={d} != par4={p4}",
                );
            }
        }
    }

    /// Parallel mode is tolerance-equivalent to deterministic mode on a
    /// shared-DOF mesh, and bit-stable across back-to-back invocations at
    /// a fixed thread count.
    ///
    /// Mesh: 4 tets fanning around central node 0 (`n_nodes = 13`,
    /// connectivity `[0,1,2,3]`, `[0,4,5,6]`, `[0,7,8,9]`, `[0,10,11,12]`).
    /// Node 0 is shared across all 4 elements ⇒ the (0..3, 0..3) DOF block
    /// receives a 4-way summation, surfacing any FP non-associativity that
    /// a different parallel summation order would introduce.
    ///
    /// Two assertions:
    /// 1. **Tolerance-equivalence**: for every `(i, j)`,
    ///    `|K_par[i][j] − K_det[i][j]| < 1e-12 * max(1, |K_det[i][j]|)`.
    ///    Strict bit-equality is not required across modes (different
    ///    summation order can perturb the LSB). Our implementation
    ///    happens to merge in slice order so bit-equality holds today,
    ///    but the test pins the tolerance contract — the PRD only
    ///    requires the FP delta be far below physical tolerance.
    /// 2. **Fixed-thread-count bit-stability**: two back-to-back
    ///    invocations with `Parallel { threads: 4 }` on the same input
    ///    produce bit-identical output. This is the determinism contract
    ///    the PRD pins: at a fixed thread count, the assembly is
    ///    reproducible bit-for-bit.
    #[test]
    fn parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        assert_eq!(k_e.n_dofs, 12);

        // 4 tets fanning around central node 0.
        let conns: [[usize; 4]; 4] = [
            [0, 1, 2, 3],
            [0, 4, 5, 6],
            [0, 7, 8, 9],
            [0, 10, 11, 12],
        ];
        let n_nodes = 13;
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, c)| AssemblyElement {
                id: i,
                connectivity: c,
                k_e: &k_e,
            })
            .collect();

        let det = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let par_a = assemble_global_stiffness(
            n_nodes,
            &elements,
            AssemblyMode::Parallel { threads: 4 },
        );
        let par_b = assemble_global_stiffness(
            n_nodes,
            &elements,
            AssemblyMode::Parallel { threads: 4 },
        );

        let dim = 3 * n_nodes;
        assert_eq!(det.nrows(), dim);
        assert_eq!(par_a.nrows(), dim);

        for i in 0..dim {
            for j in 0..dim {
                let d = read(&det, i, j);
                let pa = read(&par_a, i, j);
                let pb = read(&par_b, i, j);

                // (1) Tolerance-equivalence: parallel ≈ deterministic
                // within a relative-or-absolute tolerance of 1e-12. The
                // "max(1, |d|)" form covers both the small-magnitude
                // (absolute) and large-magnitude (relative) regimes
                // without a special-case branch.
                let tol = 1e-12 * d.abs().max(1.0);
                let delta = (pa - d).abs();
                assert!(
                    delta < tol,
                    "K_par[{i}][{j}] = {pa} but K_det[{i}][{j}] = {d}; \
                     |Δ| = {delta} ≥ tol = {tol}",
                );

                // (2) Fixed-thread-count bit-stability: par_a == par_b
                // bit-for-bit. This is the determinism contract the PRD
                // pins.
                assert_eq!(
                    pa.to_bits(),
                    pb.to_bits(),
                    "back-to-back Parallel {{ threads: 4 }} not bit-stable at \
                     [{i}][{j}]: par_a={pa}, par_b={pb}",
                );
            }
        }
    }

    /// Global K is symmetric within FP tolerance on the fan mesh from
    /// step-13.
    ///
    /// Per-element K_e is symmetric (pinned by Task 2915's tests, since
    /// `K_e = ∫ BᵀDB dV` with symmetric `D`). faer's CSR-from-triplets
    /// sums duplicates in a fixed encounter order, so `K_global[i][j]`
    /// and `K_global[j][i]` are sums of mirror pairs of triplets. The
    /// emission loop emits both `(a, b)` and `(b, a)` for every local
    /// pair, so the LSB of the sum order at `(i, j)` and `(j, i)` can
    /// differ — but both must equal each other within FP tolerance.
    ///
    /// Mesh: 4 P1 tets fanning around node 0, `n_nodes = 13` (same as
    /// step-13). Multiple K_e contributions land at shared DOFs, so a
    /// regression that, say, accidentally makes the (a, b) and (b, a)
    /// emission paths drift apart (e.g. by emitting upper-triangle only
    /// for one element and full block for another) surfaces here.
    ///
    /// Tolerance is `1e-9 * max(|K[i][j]|, |K[j][i]|, 1)` — generous
    /// enough that any reasonable summation order satisfies it, tight
    /// enough that an algorithmic asymmetry (e.g. dropping triplets,
    /// half-block emission) trips the assertion.
    #[test]
    fn global_k_is_symmetric_within_fp_tolerance() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);

        // Same fan-around-central-node mesh as step-13.
        let conns: [[usize; 4]; 4] = [
            [0, 1, 2, 3],
            [0, 4, 5, 6],
            [0, 7, 8, 9],
            [0, 10, 11, 12],
        ];
        let n_nodes = 13;
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, c)| AssemblyElement {
                id: i,
                connectivity: c,
                k_e: &k_e,
            })
            .collect();

        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let dim = 3 * n_nodes;
        for i in 0..dim {
            for j in 0..dim {
                let kij = read(&k, i, j);
                let kji = read(&k, j, i);
                let tol = 1e-9 * kij.abs().max(kji.abs()).max(1.0);
                let delta = (kij - kji).abs();
                assert!(
                    delta <= tol,
                    "K[{i}][{j}] = {kij}, K[{j}][{i}] = {kji}; |Δ| = {delta} > tol = {tol}",
                );
            }
        }
    }

    /// Single P2 element with identity connectivity `[0..10]` → K_global
    /// equals K_e bit-for-bit at every entry.
    ///
    /// Pins the contract that the scatter loop is generic on
    /// `connectivity.len()`. `connectivity.len() = 10` and `k_e.n_dofs = 30`
    /// are asserted explicitly so a future regression that special-cases
    /// 4-node elements (e.g. hardcodes `n_local = 4` somewhere in the
    /// emission loop) surfaces as a 30×30 mismatch here rather than being
    /// silently ignored. Densification and bit-equality follow the same
    /// approach as the P1 test (identity connectivity ⇒ no FP-summation
    /// reordering ⇒ bit-equality is achievable, not just tolerance-equality).
    #[test]
    fn single_p2_element_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let phys = scaled_p2_phys_nodes(1.0);
        let k_e = element_stiffness_p2(&phys, &mat);
        assert_eq!(k_e.n_dofs, 30);

        let connectivity: [usize; 10] = std::array::from_fn(|i| i);
        assert_eq!(connectivity.len(), 10);
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(10, &[element], AssemblyMode::Deterministic);
        assert_eq!(k.nrows(), 30);
        assert_eq!(k.ncols(), 30);

        for i in 0..30 {
            for j in 0..30 {
                let actual = read(&k, i, j);
                let expected = k_e.get(i, j);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] = {actual} but K_e[{i}][{j}] = {expected}",
                );
            }
        }
    }
}
