//! Global sparse-matrix assembly for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #9 and v0.4
//! `docs/prds/v0_4/structural-analysis-shells.md` task T11. This module
//! scatters per-element [`crate::assembly::ElementStiffness`] dense matrices
//! into a global sparse stiffness matrix `K` of size `D·N × D·N`, where
//! `N` is the global node count and `D` is the global DOFs-per-node count
//! (derived as `max` over the per-element `k_e.n_dofs / connectivity.len()`,
//! defaulting to `3` for empty inputs). Tet-only meshes ⇒ `D = 3`,
//! pure-shell meshes ⇒ `D = 6`, mixed tet+shell meshes ⇒ `D = 6` (shell
//! dominates). Uses `faer-rs` CSR triplet builders.

use faer::sparse::{SparseRowMat, Triplet};

use super::ElementStiffness;

/// Maximum number of `(node, axis)` pairs stored in
/// [`OrphanDofsSummary::examples`].
///
/// Keeps the struct small on large meshes — production models can have 10K+
/// tet-only nodes in a mixed tet+shell system (30K+ orphan pairs). 16 entries
/// are enough to identify a clustered pattern in a debug log line; callers
/// needing full enumeration can compute the orphan set directly from the
/// element slice. [`OrphanDofsSummary::count`] always reflects the true total
/// regardless of this cap.
const MAX_EXAMPLES: usize = 16;

/// One element's contribution to the global system.
///
/// `connectivity` lists the global node IDs of the element's local nodes in
/// the same order as the rows/columns of `k_e`. The per-element
/// DOFs-per-node count `d_e` is derived as `k_e.n_dofs / connectivity.len()`
/// (which must divide evenly): tet ⇒ `d_e = 3`, MITC3 shell ⇒ `d_e = 6`.
/// The local DOF index `d_e * a + α` (axis/component
/// `α ∈ {0, ..., d_e - 1}`) maps to global DOF
/// `D * connectivity[a] + α`, where `D` is the global DOFs-per-node count
/// `max(d_e)` over all elements (see module docs).
///
/// The `id` field is descriptive metadata used in panic messages (e.g. to
/// name the offending element in a contract violation) and is *not* used
/// internally as a sort key in any [`AssemblyMode`]. Callers requiring a
/// canonical iteration order in [`AssemblyMode::Deterministic`] must sort
/// the slice themselves before passing it in.
pub struct AssemblyElement<'a> {
    /// Element ID (descriptive metadata; surfaces in panic messages).
    pub id: usize,
    /// Global node IDs — `k_e.n_dofs % connectivity.len() == 0` is required,
    /// and the derived per-element DOFs-per-node `k_e.n_dofs / connectivity.len()`
    /// is the element's DOF stride (3 for tet, 6 for MITC3 shell).
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
/// concatenated in **`handles`-vector order (== chunk-iteration order ==
/// slice order)** before being handed to faer. This gives bit-stability
/// for any *fixed* thread count, but the
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
    /// Multi-threaded scatter with fixed-handle-order merge. `threads`
    /// must be `>= 1`; passing `0` panics.
    Parallel {
        /// Worker thread count.
        threads: usize,
    },
}

/// Summary returned by [`detect_orphan_dofs`] describing nodes whose DOF
/// coverage falls short of the global `D` in a mixed-element mesh.
///
/// An *orphan DOF* at node `n`, axis `α` is a `(D·n + α)` row/col in the
/// global stiffness matrix `K` that receives **no nonzero contribution from
/// any element** because `α >= d_e_max_local(n)` (the highest per-element
/// DOF count touching that node). In a pure-tet mesh every node has
/// `d_e_max_local = 3 = D`, so there are no orphans. In a mixed tet+shell
/// mesh (`D = 6`) tet-only nodes have `d_e_max_local = 3 < 6`, leaving axes
/// 3, 4, 5 as orphan zeros — the signature of a singular K unless a BC or
/// MPC layer (task 2917 / task 3020) clamps them.
///
/// This is the diagnostic surface added in task 3293. Call
/// [`detect_orphan_dofs`] before or after assembly to check whether the mesh
/// configuration requires BC/MPC stabilisation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrphanDofsSummary {
    /// Total number of orphan `(node, axis)` pairs across the whole mesh.
    /// This is the true count even when `examples` is truncated to
    /// `MAX_EXAMPLES`.
    pub count: usize,
    /// First up to `MAX_EXAMPLES` representative orphan `(node, axis)` pairs,
    /// sorted ascending by `(node, axis)`. When `examples.len() < count` the
    /// list is truncated; callers that need the full set can derive it
    /// directly from the element slice.
    pub examples: Vec<(usize, usize)>,
}

impl std::fmt::Display for OrphanDofsSummary {
    /// Single-line summary suitable for `eprintln!` and grep.
    ///
    /// Format (non-empty, no truncation — all examples listed explicitly):
    /// `orphan DOFs: count=9, examples=[(1, 3), (1, 4), (1, 5), (2, 3), (2, 4), (2, 5), (3, 3), (3, 4), (3, 5)]`
    ///
    /// Format (truncated, `examples.len() < count` — all `MAX_EXAMPLES`
    /// entries listed explicitly, then a trailing parenthetical):
    /// `orphan DOFs: count=24, examples=[(1, 3), (1, 4), (1, 5), (2, 3), (2, 4), (2, 5), (3, 3), (3, 4), (3, 5), (4, 3), (4, 4), (4, 5), (5, 3), (5, 4), (5, 5), (6, 3)] (first 16 of 24)`
    ///
    /// Note: there is no `...` ellipsis inside the brackets — all stored
    /// examples are emitted verbatim. The parenthetical `(first N of M)` only
    /// appears when the list is truncated (`examples.len() < count`).
    ///
    /// Format (empty / no orphans):
    /// `orphan DOFs: count=0, examples=[]`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "orphan DOFs: count={}, examples=[", self.count)?;
        for (i, &(node, axis)) in self.examples.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "({node}, {axis})")?;
        }
        write!(f, "]")?;
        if self.examples.len() < self.count {
            write!(
                f,
                " (first {} of {})",
                self.examples.len(),
                self.count,
            )?;
        }
        Ok(())
    }
}

/// Detect orphan DOFs in a mesh described by `elements`, returning a summary
/// of all `(node, axis)` pairs whose row/col in the global stiffness matrix
/// would be structurally zero because no touching element covers that axis.
///
/// ## Semantics
///
/// Computes, for each node `n` in `0..n_nodes`:
///
/// ```text
/// d_e_max_local(n) = max(d_e_e  for elements e where n in e.connectivity)
/// ```
///
/// with `d_e_e = e.k_e.n_dofs / e.connectivity.len()` (the same formula used
/// by [`assemble_global_stiffness`] to derive the global `D`). The global
/// `D = max(d_e_max_local)` over all elements. Empty `elements` returns an
/// empty summary (`count=0`) without computing `D` (see early return below).
///
/// Node `n` carries orphan DOFs at axes `α ∈ [d_e_max_local(n)..D)` **only
/// if the node is touched** (`d_e_max_local(n) > 0`). Completely-untouched
/// nodes (never in any element's connectivity) have `d_e_max_local = 0`
/// and are not reported — their empty rows/cols are a mesh-completeness
/// issue, not a DOF-coverage violation.
///
/// ## In debug builds
///
/// [`assemble_global_stiffness`] calls this function internally under
/// `#[cfg(debug_assertions)]` and emits an `eprintln!` warning if
/// `count > 0`. See the design note at the assembly boundary.
///
/// ## Task marker
///
/// Diagnostic surface added per task 3293. See also task 2917 (Dirichlet BC)
/// and task 3020 (MPC tying) for the stabilisation layers that suppress
/// singular-K failures from orphan DOFs.
///
/// # Panics
///
/// Panics (index out of bounds) if any node index in an element's connectivity
/// is `>= n_nodes`. Panics (divide by zero) if any element's connectivity is
/// empty (`e.connectivity.len() == 0`). These contracts match those of
/// [`assemble_global_stiffness`] — callers should validate the mesh before
/// calling this function as a pre-assembly check.
///
/// In debug builds, two `debug_assert!`s enforce these contracts eagerly:
/// one on non-empty connectivity and one on all node indices being in-bounds.
// G-allow: task #3293 orphan-DOF detector; cfg(debug_assertions) emit consumer + detector/assembler-consistency pin (task #3293)
pub fn detect_orphan_dofs(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
) -> OrphanDofsSummary {
    if elements.is_empty() {
        return OrphanDofsSummary::default();
    }

    // Validate input contracts (matches assemble_global_stiffness's implicit
    // contract): every element must have non-empty connectivity, and every
    // referenced node must be in-bounds for n_nodes.
    #[cfg(debug_assertions)]
    for e in elements {
        debug_assert!(
            !e.connectivity.is_empty(),
            "detect_orphan_dofs: element {} has empty connectivity",
            e.id
        );
        debug_assert!(
            e.connectivity.iter().all(|&n| n < n_nodes),
            "detect_orphan_dofs: element {} has a node index >= n_nodes ({})",
            e.id,
            n_nodes
        );
    }

    // Global DOFs-per-node D = max(d_e_e) over all elements, mirroring the
    // formula in assemble_global_stiffness. Derived inline with the per-node
    // aggregation below; the early return above guarantees elements is non-empty
    // here so d_global is always set to at least the first element's d_e.
    let mut d_global: usize = 0;
    // Build per-node d_e_max_local: the highest d_e of any element touching
    // that node. Nodes never mentioned in any connectivity stay 0 (untouched).
    let mut d_max_local = vec![0usize; n_nodes];
    for e in elements {
        let d_e = e.k_e.n_dofs / e.connectivity.len();
        if d_e > d_global {
            d_global = d_e;
        }
        for &node in e.connectivity {
            if d_e > d_max_local[node] {
                d_max_local[node] = d_e;
            }
        }
    }

    // Count orphan (node, axis) pairs and collect up to MAX_EXAMPLES of them.
    // Nodes that ARE touched (d_max_local > 0) but whose best-covering
    // element stops short of D contribute (d_global - d_local) orphan axes.
    // Both loops are ascending so examples are naturally sorted by (node, axis).
    // count always reflects the true total; push is gated by the cap.
    let mut count = 0usize;
    let mut examples: Vec<(usize, usize)> = Vec::new();
    for (node, &d_local) in d_max_local.iter().enumerate() {
        if d_local > 0 && d_local < d_global {
            for axis in d_local..d_global {
                count += 1;
                if examples.len() < MAX_EXAMPLES {
                    examples.push((node, axis));
                }
            }
        }
    }

    OrphanDofsSummary { count, examples }
}

/// Scatter per-element stiffness matrices into a global `D·N × D·N` sparse
/// stiffness matrix, where `D` is the global DOFs-per-node count derived
/// from the element slice (see below) and `N = n_nodes`.
///
/// `n_nodes` is the global node count; the returned matrix has
/// `D * n_nodes` rows and columns, where:
///
/// - `D = max(k_e.n_dofs / connectivity.len())` over all elements, with
///   `unwrap_or(3)` so empty inputs yield the v0.3 tet-only `3N × 3N`
///   shape. This rule produces:
///     - pure-tet meshes ⇒ `D = 3` (backward-compatible),
///     - pure-shell meshes ⇒ `D = 6`,
///     - mixed tet + shell meshes ⇒ `D = 6` (shell dominates).
///
/// `elements` is the slice of element contributions (see
/// [`AssemblyElement`]); each contribution emits a full dense `(a, b, α, β)`
/// block of `d_e² · n_local²` triplets (where `d_e = k_e.n_dofs / n_local`
/// is the element's own DOFs-per-node count), and faer's CSR builder sums
/// duplicates that share a `(row, col)` pair.
///
/// Tet-only nodes in a `D = 6` mixed-element global system carry orphan
/// rotation rows/cols of zero (rows/cols `D·n + 3..D·n + 6` at any node
/// touched only by tets). Stabilising those orphan DOFs is the
/// downstream BC/MPC layers' job (Dirichlet auto-clamp on rotations,
/// shell-tet tying via `crate::mpc`); the assembler ships them as zeros.
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
/// - `k_e.n_dofs % connectivity.len() != 0` for any element — `k_e.n_dofs`
///   must be divisible by `connectivity.len()` so the per-element DOFs-per-node
///   count `d_e = k_e.n_dofs / connectivity.len()` is an integer; otherwise
///   the `(D·conn[a]+α, D·conn[b]+β)` mapping is ill-defined. The panic
///   message names the offending element id, observed `connectivity.len()`,
///   observed `k_e.n_dofs`, and the divisibility remainder.
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
        let n_local = element.connectivity.len();
        assert!(
            n_local > 0,
            "AssemblyElement {{ id: {} }} has empty connectivity \
             (k_e.n_dofs = {}); cannot derive dofs_per_node from zero nodes",
            element.id,
            element.k_e.n_dofs,
        );
        let remainder = element.k_e.n_dofs % n_local;
        assert!(
            remainder == 0,
            "AssemblyElement {{ id: {} }} has connectivity.len() = {} \
             but k_e.n_dofs (= {}) is not divisible by connectivity.len() \
             (remainder = {}); expected k_e.n_dofs to be a multiple of \
             connectivity.len() so the per-element dofs_per_node is an integer",
            element.id,
            n_local,
            element.k_e.n_dofs,
            remainder,
        );
        let dofs_per_node = element.k_e.n_dofs / n_local;
        assert!(
            dofs_per_node >= 1,
            "AssemblyElement {{ id: {} }} has connectivity.len() = {} \
             but k_e.n_dofs = {} ⇒ dofs_per_node = {} < 1; element kernels \
             must emit at least one DOF per node",
            element.id,
            n_local,
            element.k_e.n_dofs,
            dofs_per_node,
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

    // Global DOFs-per-node `D` is the maximum over all per-element
    // `d_e = k_e.n_dofs / connectivity.len()`. Empty inputs default to
    // `3` so the v0.3 tet-only `3N × 3N` empty-mesh shape is preserved
    // bit-for-bit. Pure-tet meshes ⇒ `D = 3`. Pure-shell meshes ⇒
    // `D = 6`. Mixed tet+shell meshes ⇒ `D = 6` (shell dominates;
    // tet-only nodes carry orphan rotation rows/cols of zero, which
    // downstream BC/MPC layers handle).
    //
    // Design note (task 3293): when `D > min(d_e)` (mixed tet+shell),
    // nodes touched only by lower-`d_e` elements end up with structurally
    // zero rows/cols at the extra DOFs (`α >= d_e_max_local(n)` for that
    // node). Stabilisation is provided by Shells T10's MPC tying (task
    // 3020, landed) plus the Dirichlet rotation auto-clamp (task 2917,
    // landed); the contract owner is the BC/MPC layer.
    //
    // Diagnostic surface: `detect_orphan_dofs(n_nodes, elements)` returns
    // an `OrphanDofsSummary { count, examples }` describing any such
    // under-covered (node, axis) pairs. In debug builds this function
    // calls `detect_orphan_dofs` internally and emits a single `eprintln!`
    // warning if `count > 0`, so forgetting BC/MPC stabilisation surfaces
    // at the assembly boundary rather than as a silent singular K in the
    // linear solve.
    let n_dofs_per_node: usize = elements
        .iter()
        .map(|e| e.k_e.n_dofs / e.connectivity.len())
        .max()
        .unwrap_or(3);

    // Mode-specific dispatch. The deterministic arm exercises the shared
    // `emit_element_triplets` scatter primitive in slice order; the
    // parallel arm partitions into `threads` chunks via
    // `std::thread::scope` and merges per-thread Vecs in spawn order.
    //
    // Each element emits a full dense block of `d_e² · n_local²` triplets
    // where `d_e = k_e.n_dofs / n_local` is the element's per-node DOF
    // count (3 for tet, 6 for MITC3 shell). Worked examples: P1 tet
    // ⇒ 3² · 4² = 144; P2 tet ⇒ 3² · 10² = 900; MITC3 shell
    // ⇒ 6² · 3² = 324. Pre-sizing both the merged accumulator and
    // per-thread local Vecs to the exact triplet count avoids the
    // O(log N) reallocs `Vec::new()` would walk through on the FEA hot
    // path — for ~10K P1 elements at 144 triplets each (24 B per
    // `Triplet<usize, usize, f64>`), that's ~34 MB of allocator churn
    // the worker would otherwise perform per chunk.
    let total_triplets: usize = elements
        .iter()
        .map(|e| {
            let dpn = e.k_e.n_dofs / e.connectivity.len();
            dpn * dpn * e.connectivity.len() * e.connectivity.len()
        })
        .sum();
    let triplets: Vec<Triplet<usize, usize, f64>> = match mode {
        AssemblyMode::Deterministic => {
            let mut acc = Vec::with_capacity(total_triplets);
            for element in elements {
                emit_element_triplets(element, n_dofs_per_node, &mut acc);
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
            // **The merge order is `handles` insertion order
            // (== chunk-iteration order == slice order).** Concretely:
            //
            //   (a) `elements.chunks(chunk_size)` is called once with a
            //       stable chunk size, so the chunk-iteration order is
            //       deterministic and matches `elements`'s slice order.
            //   (b) Threads spawn sequentially in chunk-iteration order,
            //       so handle slot `t` in the `handles` Vec always
            //       corresponds to the worker for chunk `t` — regardless
            //       of what OS thread id that worker was assigned.
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
                            .map(|e| {
                                let dpn = e.k_e.n_dofs / e.connectivity.len();
                                dpn * dpn * e.connectivity.len() * e.connectivity.len()
                            })
                            .sum();
                        let mut local: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(cap);
                        for element in chunk {
                            emit_element_triplets(element, n_dofs_per_node, &mut local);
                        }
                        local
                    }));
                }
                let mut acc: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(total_triplets);
                for h in handles {
                    // Joining in handle-vector order = spawn order =
                    // chunk-iteration order in `elements`. A worker
                    // panic is forwarded to the caller via
                    // `resume_unwind` rather than being swallowed by
                    // `.expect(...)`: `expect` would format the boxed
                    // `Any` payload as `Any { .. }` (losing the
                    // original panic text and backtrace location),
                    // whereas `resume_unwind` propagates the original
                    // payload intact so the caller sees the worker's
                    // exact panic message (e.g. "index out of bounds").
                    // Per the Task-2544 contract-explicitness
                    // convention: make the contract explicit in
                    // production code rather than relying on test
                    // coverage. Mirrors the `run_with_deadlock_timeout`
                    // pattern in `reify-test-support::mocks`.
                    match h.join() {
                        Ok(local) => acc.extend(local),
                        Err(payload) => std::panic::resume_unwind(payload),
                    }
                }
                acc
            })
        }
    };
    // faer 0.24's `try_new_from_triplets` **sums duplicate `(row, col)`
    // entries in encounter order**. We rely on this contract: when two
    // (or more) elements share a DOF pair, each element emits its own
    // triplet, and the accumulated `K_global[i][j]` is the sum of all
    // contributions in slice-iteration order.
    //
    // The contract is pinned by the `faer_sums_duplicate_triplets_in_encounter_order`
    // unit test below, which seeds five duplicate triplets whose left-fold
    // (encounter-order) and pairwise-tree-reduction sums diverge above
    // the LSB — so a faer upgrade that switches summation strategy
    // surfaces immediately rather than silently invalidating the
    // parallel-mode determinism contract. Faer's own `test_from_indices`
    // (sparse/mod.rs:280-326) demonstrates the same behavior on values
    // that happen to be sum-order-invariant; our canary uses values that
    // are not, so it would also catch a regression that survives faer's
    // own test suite.
    //
    // If a future faer version regresses this (e.g. overwrites instead of
    // summing), the fix is to switch the local helper to a pre-merge pass
    // that sums in a `BTreeMap<(row, col), f64>` keyed by the canonical
    // `(row, col)` order. step-5's `two_p1_elements_sharing_face_*` test
    // would also surface the regression.
    // Debug-only orphan-DOF diagnostic. Zero release-mode cost; matches the
    // #[cfg(debug_assertions)] gating idiom in mpc.rs:172-213. Uses eprintln!
    // rather than tracing/log because reify-solver-elastic has no logging
    // dependency (task 3293, design decision 3).
    #[cfg(debug_assertions)]
    {
        let orphans = detect_orphan_dofs(n_nodes, elements);
        if orphans.count > 0 {
            eprintln!(
                "[reify-solver-elastic] assemble_global_stiffness: {orphans}; \
                 apply Dirichlet BC (task 2917) or MPC tying (task 3020) to \
                 stabilise these DOFs before solving",
            );
        }
    }

    let dim = n_dofs_per_node * n_nodes;
    SparseRowMat::try_new_from_triplets(dim, dim, &triplets).expect(
        "triplets within declared n_dofs_per_node*n_nodes dims \
         (per-element bounds enforced upstream)",
    )
}

/// Emit one dense `d_e² · n_local²` block of triplets for `element` and
/// append to `out`. `d_e = element.k_e.n_dofs / n_local` is the element's
/// per-node DOF count (3 for tet, 6 for MITC3 shell). `n_dofs_per_node`
/// is the **global** per-node DOF count (`D` in module docs); it sets the
/// global row/col stride `D · conn[a] + α` independent of `d_e`. When
/// `D > d_e` (e.g. tet element in a 6-DOF/node mixed system, `D = 6`,
/// `d_e = 3`), the element fills only the first `d_e` rows/cols at each
/// node-pair block and the remaining `D − d_e` rows/cols stay zero
/// (orphan DOFs at tet-only nodes in a `D = 6` system).
///
/// # Worked examples (per-element local stride `d_e` × global stride `D`)
///
/// - **P1 tet in pure-tet mesh** (`d_e = 3`, `n_local = 4`, `D = 3`):
///   emits `9 · 16 = 144` triplets at `(3·conn[a]+α, 3·conn[b]+β)`.
/// - **P2 tet in pure-tet mesh** (`d_e = 3`, `n_local = 10`, `D = 3`):
///   emits `9 · 100 = 900` triplets at `(3·conn[a]+α, 3·conn[b]+β)`.
/// - **MITC3 shell in pure-shell mesh** (`d_e = 6`, `n_local = 3`,
///   `D = 6`): emits `36 · 9 = 324` triplets at
///   `(6·conn[a]+α, 6·conn[b]+β)`.
/// - **P1 tet in mixed tet+shell mesh** (`d_e = 3`, `n_local = 4`,
///   `D = 6`): emits the same `144` triplets at
///   `(6·conn[a]+α, 6·conn[b]+β)` for `α, β ∈ 0..3` — leaves
///   `α, β ∈ 3..6` rows/cols **unstored** at the touched nodes,
///   producing the orphan rotation rows/cols densified to zero.
/// - **MITC3 shell in mixed tet+shell mesh** (`d_e = 6`, `n_local = 3`,
///   `D = 6`): identical to the pure-shell case (the local and global
///   strides agree). Both translation and rotation DOFs land at
///   `(6·conn[a]+α, 6·conn[b]+β)` for `α, β ∈ 0..6`.
///
/// The emission order is the C-style row-major nesting
/// `for a in 0..n_local { for α in 0..d_e { for b in 0..n_local { for β in 0..d_e } } }` —
/// chosen so the within-block `(row, col)` sequence is monotonic, which
/// gives faer's duplicate-summation step a stable input ordering and
/// matches the row-major layout of [`ElementStiffness::data`] (one
/// sequential read per output triplet).
///
/// # Sparsity contract
///
/// Explicit-zero entries (`K_e[i][j] == 0.0`) are emitted unconditionally —
/// this helper does not zero-prune. For current dense isotropic-elastic K_e
/// producers there are no structural zeros, so this is a no-op. Callers
/// feeding K_e with structural zeros (e.g. anisotropic or shell-coupled
/// stiffness blocks) and requiring sparse-storage savings must pre-prune K_e
/// before calling; storing wasted explicit-zero entries downstream in CSR is
/// the caller's responsibility, not this helper's. The current behavior is
/// intentional — zero-pruning here would couple the helper to K_e's sparsity
/// pattern without benefiting current consumers.
///
/// # N-agnostic / D-agnostic contract
///
/// The loop bounds derive from `n_local = element.connectivity.len()`,
/// `d_e = element.k_e.n_dofs / n_local`, and the global `n_dofs_per_node`
/// stride exclusively — there is no hardcoded `4` (P1 node count) or `10`
/// (P2 node count) or `3` (tet axes per node) or `6` (shell DOFs per node)
/// anywhere in the body. The `assemble_global_stiffness` entry-point's
/// per-element contract check (`k_e.n_dofs % connectivity.len() == 0`) is
/// the only coupling between `n_local` and `k_e`, so this primitive
/// accepts any element shape whose K_e satisfies that invariant. Pinned by
/// `single_p2_element_identity_connectivity_matches_k_e_bit_for_bit` (n=10,
/// d=3) and `single_shell_18dof_element_identity_connectivity_matches_k_e_bit_for_bit`
/// (n=3, d=6).
fn emit_element_triplets(
    element: &AssemblyElement<'_>,
    n_dofs_per_node: usize,
    out: &mut Vec<Triplet<usize, usize, f64>>,
) {
    let n_local = element.connectivity.len();
    let local_dofs_per_node = element.k_e.n_dofs / n_local;
    // Iteration order is `(a, α, b, β)` so row = D*conn[a]+α stays fixed
    // for the inner `(b, β)` sweep — that's the row-major traversal of
    // both the local k_e block and the global K row, minimizing cache
    // pressure and giving faer monotonically non-decreasing rows for
    // duplicate-summation stability.
    for a in 0..n_local {
        let row_node = element.connectivity[a];
        for alpha in 0..local_dofs_per_node {
            let row = n_dofs_per_node * row_node + alpha;
            let local_row = local_dofs_per_node * a + alpha;
            for b in 0..n_local {
                let col_node = element.connectivity[b];
                for beta in 0..local_dofs_per_node {
                    let col = n_dofs_per_node * col_node + beta;
                    let local_col = local_dofs_per_node * b + beta;
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
    use crate::assembly::hex::element_stiffness_hex_p1;
    use crate::assembly::test_support::{
        dimensionless_steel_like, scaled_p2_phys_nodes, scaled_unit_hex_phys_nodes,
        scaled_unit_wedge_phys_nodes,
    };
    use crate::assembly::tet::{element_stiffness_p1, element_stiffness_p2};
    use crate::assembly::wedge::element_stiffness_wedge_p1;
    use crate::constitutive::IsotropicElastic;
    use crate::shell_assembly::shell_element_stiffness;

    /// Canonical 4-node P1 phys layout (unit reference tet).
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    /// Canonical 3-node phys layout (unit reference triangle in the xy-plane).
    /// Mirrors the `UNIT_TRI` constant in `shell_assembly.rs::tests` so shell
    /// K_e instances built here match those built there for the shell-assembly
    /// regression tests.
    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    /// Canonical shell thickness for the mixed-element fixtures. 0.05 matches
    /// the value used throughout `shell_assembly.rs::tests`.
    const SHELL_T: f64 = 0.05;

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
    ///
    /// Exercised through every `AssemblyMode` variant. The parallel arm's
    /// `chunks(chunk_size)` over an empty slice yields zero chunks, so zero
    /// worker threads spawn — but the dim/nnz contract still holds. A
    /// regression that, say, panics on `elements.len().div_ceil(threads)` for
    /// `threads > 0` and `elements.len() == 0` would surface here.
    #[test]
    fn empty_elements_returns_zero_3n_by_3n_sparse_matrix() {
        let n_nodes = 4;
        for mode in [
            AssemblyMode::Deterministic,
            AssemblyMode::Parallel { threads: 1 },
        ] {
            let k = assemble_global_stiffness(n_nodes, &[], mode);
            assert_eq!(k.nrows(), 3 * n_nodes, "mode = {mode:?}");
            assert_eq!(k.ncols(), 3 * n_nodes, "mode = {mode:?}");
            assert_eq!(
                k.compute_nnz(),
                0,
                "no triplets ⇒ zero stored entries (mode = {mode:?})",
            );
        }
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

    /// `k_e.n_dofs` not divisible by `connectivity.len()` panics with a
    /// descriptive message containing both `"k_e.n_dofs"` (so the existing
    /// `mismatched_connectivity_length_and_k_e_n_dofs_panics` substring
    /// match still locks the same code path) and `"divisible by"` (so the
    /// new contract's intent — "the per-element DOFs-per-node must be
    /// integer" — is named explicitly in the message).
    ///
    /// Uses a synthetic `ElementStiffness::zeros(20)` (3 nodes, 20 DOFs ⇒
    /// 20 % 3 = 2, not divisible). `ElementStiffness::zeros` is `pub` so
    /// the test does not depend on a real element kernel; the panic fires
    /// in the entry-point's per-element divisibility assertion before any
    /// emission happens.
    #[test]
    #[should_panic(expected = "k_e.n_dofs (= 20) is not divisible by")]
    fn non_divisible_n_dofs_per_node_panics_with_descriptive_message() {
        let k_e = ElementStiffness::zeros(20);
        let conn = [0usize, 1, 2]; // 3 nodes — 20 % 3 = 2 ≠ 0.
        let element = AssemblyElement {
            id: 13,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_global_stiffness(3, &[element], AssemblyMode::Deterministic);
    }

    /// Pins the `n_local > 0` guard: empty connectivity slice ⇒ panic.
    #[test]
    #[should_panic(expected = "empty connectivity")]
    fn empty_connectivity_panics() {
        let k_e = ElementStiffness::zeros(3);
        let element = AssemblyElement {
            id: 7,
            connectivity: &[],
            k_e: &k_e,
        };
        let _ = assemble_global_stiffness(1, &[element], AssemblyMode::Deterministic);
    }

    /// Pins the `dofs_per_node >= 1` guard: zero-DOF kernel ⇒ panic.
    #[test]
    #[should_panic(expected = "dofs_per_node = 0")]
    fn zero_dofs_per_node_panics() {
        let k_e = ElementStiffness::zeros(0);
        let conn = [0usize]; // n_local = 1; 0 % 1 = 0 (guard 2 passes); 0 / 1 = 0 (guard 3 fires).
        let element = AssemblyElement {
            id: 5,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_global_stiffness(1, &[element], AssemblyMode::Deterministic);
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
        let conns: [[usize; 4]; 4] = [[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11], [12, 13, 14, 15]];
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
        let par1 =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 1 });
        let par2 =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 2 });
        let par4 =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 4 });
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
        let conns: [[usize; 4]; 4] = [[0, 1, 2, 3], [0, 4, 5, 6], [0, 7, 8, 9], [0, 10, 11, 12]];
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
        let par_a =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 4 });
        let par_b =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 4 });

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

    /// `Parallel { threads: 2 }` and `Parallel { threads: 4 }` produce
    /// tolerance-equivalent results on a shared-DOF mesh.
    ///
    /// The `AssemblyMode::Parallel` doc comment claims cross-thread-count
    /// drift on shared-DOF meshes is bounded by `O(ulp · max|K_e[i][j]|)` —
    /// but
    /// `parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`
    /// only exercises Deterministic vs `Parallel { threads: 4 }`. This test
    /// pins the cross-thread-count claim directly: two parallel runs at
    /// **different** thread counts must agree within the same tolerance.
    ///
    /// Mesh: same fan-around-central-node mesh as step-13. Two thread
    /// counts that produce different chunk partitions:
    /// - `threads = 2` ⇒ `chunk_size = ceil(4 / 2) = 2` ⇒ chunks
    ///   `[e0, e1]`, `[e2, e3]`. Two workers, each emits 2 elements'
    ///   triplets, then merge.
    /// - `threads = 4` ⇒ `chunk_size = ceil(4 / 4) = 1` ⇒ chunks
    ///   `[e0]`, `[e1]`, `[e2]`, `[e3]`. Four workers, each emits 1
    ///   element's triplets, then merge.
    ///
    /// In our current implementation both flatten to the same triplet
    /// sequence (slice order, since chunks tile slice order) and faer
    /// sums in encounter order, so today's output is bit-equal across
    /// the two thread counts. The test asserts the *looser* tolerance
    /// contract because (a) it matches what the docstring guarantees,
    /// (b) it leaves implementation flexibility for a future load-balanced
    /// chunker that might reorder accumulation, and (c) bit-equality
    /// would be an over-fit — the right contract for FEA is
    /// tolerance-equivalence, not bit-stability across thread counts.
    #[test]
    fn parallel_mode_cross_thread_count_tolerance_equivalent_on_shared_dof_mesh() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);

        // Same fan-around-central-node mesh as step-13.
        let conns: [[usize; 4]; 4] = [[0, 1, 2, 3], [0, 4, 5, 6], [0, 7, 8, 9], [0, 10, 11, 12]];
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

        let par2 =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 2 });
        let par4 =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 4 });

        let dim = 3 * n_nodes;
        for i in 0..dim {
            for j in 0..dim {
                let p2 = read(&par2, i, j);
                let p4 = read(&par4, i, j);
                let tol = 1e-12 * p4.abs().max(1.0);
                let delta = (p2 - p4).abs();
                assert!(
                    delta < tol,
                    "K_par2[{i}][{j}] = {p2} but K_par4[{i}][{j}] = {p4}; \
                     |Δ| = {delta} ≥ tol = {tol}",
                );
            }
        }
    }

    /// Partition-edge correctness of `Parallel` across thread counts on a
    /// 137-element chain mesh.
    ///
    /// # What this tests
    ///
    /// `assemble_global_stiffness` with `AssemblyMode::Parallel { threads }`
    /// partitions the element slice into `threads` chunks of size up to
    /// `ceil(N / threads)` (via `elements.chunks(ceil(N / threads))`), assigns
    /// one chunk per worker thread, and merges the local `Vec<Triplet>` in
    /// handle-vector (== chunk-iteration == slice) order. The merge order is
    /// deterministic for any fixed `threads`, but the per-thread accumulation
    /// boundary shifts with `threads`, so shared-DOF sums may differ at the
    /// LSB level across thread counts.
    ///
    /// This test asserts tolerance-equivalence (|K_par − K_det| < 1e-12 ·
    /// |K_det|.max(1.0)) between every `Parallel { threads }` result and the
    /// `Deterministic` baseline for `threads ∈ {1, 2, 3, 5, 7, 8}`.
    ///
    /// # Why N = 137 (prime)
    ///
    /// 137 is prime. For every thread count `t > 1` in the sweep, `ceil(137 / t)`
    /// produces a tail chunk strictly smaller than the leading chunks; `t = 1`
    /// is included as a no-partition baseline. This means partition-edge effects
    /// (shared-DOF sums that land on a chunk boundary) are exercised for every
    /// `t > 1` — unlike with 4 elements, where thread counts 1, 2, 4 all evenly
    /// tile the slice. Explicit chunk shapes:
    ///
    /// - threads=1 → 1 chunk of 137
    /// - threads=2 → chunks (69, 68)
    /// - threads=3 → chunks (46, 46, 45)
    /// - threads=5 → chunks (28, 28, 28, 28, 25)
    /// - threads=7 → chunks (20, 20, 20, 20, 20, 20, 17)
    /// - threads=8 → chunks (18, 18, 18, 18, 18, 18, 18, 11)
    ///
    /// # Chain mesh shape and shared-DOF rationale
    ///
    /// Tet `i` has connectivity `[i, i+1, i+2, i+3]` (sliding-window).
    /// Each tet shares a 3-node face with the previous tet, so shared-DOF
    /// accumulation occurs at every interior face. FP non-associativity from
    /// chunk reordering would surface in exactly these shared-DOF entries.
    /// `n_nodes = 140`, `dim = 420`; total triplets per assembly ≈ 19 728.
    ///
    /// A single shared `k_e` (computed from `UNIT_TET_P1` and
    /// `dimensionless_steel_like()`) is reused for all 137 elements.
    /// The 137 distinct connectivities exercise all DOF-mapping paths; K_e
    /// numerics stay in O(1) range for readable failure messages.
    ///
    /// # Complement to existing fan-mesh tests
    ///
    /// `parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`
    /// and `parallel_mode_cross_thread_count_tolerance_equivalent_on_shared_dof_mesh`
    /// use a 4-element fan mesh with thread counts in {1, 2, 4}. For that mesh
    /// size, `chunk_size = ceil(4 / t)` always evenly tiles the 4 elements —
    /// no tail chunk exists — so those tests cannot surface partition-edge bugs.
    /// This test fills that gap with a workload where uneven partitioning
    /// actually occurs for every thread count in the sweep.
    #[test]
    fn parallel_mode_chain_mesh_tolerance_equivalent_to_deterministic_across_thread_counts() {
        const N_ELEMENTS: usize = 137;
        let n_nodes = N_ELEMENTS + 3; // 140

        let k_e = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        assert_eq!(
            k_e.n_dofs, 12,
            "P1 tet must have 12 DOFs (4 nodes × 3 axes)"
        );

        // Sliding-window connectivity: tet i → [i, i+1, i+2, i+3].
        // Each consecutive pair shares a 3-node face, so every interior face
        // is a shared-DOF accumulation site.
        let conns: Vec<[usize; 4]> = (0..N_ELEMENTS).map(|i| [i, i + 1, i + 2, i + 3]).collect();
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
        assert_eq!(det.nrows(), 3 * n_nodes, "det rows must equal 3 * n_nodes");
        assert_eq!(det.ncols(), 3 * n_nodes, "det cols must equal 3 * n_nodes");

        let dim = 3 * n_nodes;

        // Track the worst-offending (threads, i, j) pair across the full sweep so
        // the failure message names the entry with the largest |Δ|/tol ratio, not
        // merely the first one encountered in row-major order.
        struct Worst {
            threads: usize,
            i: usize,
            j: usize,
            p: f64,
            d: f64,
            delta: f64,
            tol: f64,
        }
        let mut worst: Option<Worst> = None;

        for threads in [1_usize, 2, 3, 5, 7, 8] {
            let par =
                assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads });
            for i in 0..dim {
                for j in 0..dim {
                    let d = read(&det, i, j);
                    let p = read(&par, i, j);
                    let tol = 1e-12 * d.abs().max(1.0);
                    let delta = (p - d).abs();
                    if delta >= tol {
                        let is_worse = worst.as_ref().is_none_or(|w| delta / tol > w.delta / w.tol);
                        if is_worse {
                            worst = Some(Worst {
                                threads,
                                i,
                                j,
                                p,
                                d,
                                delta,
                                tol,
                            });
                        }
                    }
                }
            }
        }
        if let Some(Worst {
            threads,
            i,
            j,
            p,
            d,
            delta,
            tol,
        }) = worst
        {
            panic!(
                "worst-offender across sweep: threads={threads} K_par[{i}][{j}] = {p} \
                 but K_det[{i}][{j}] = {d}; |Δ| = {delta} ≥ tol = {tol} \
                 (ratio = {:.1}×)",
                delta / tol
            );
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
        let conns: [[usize; 4]; 4] = [[0, 1, 2, 3], [0, 4, 5, 6], [0, 7, 8, 9], [0, 10, 11, 12]];
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
        // Iterate the upper triangle only — `(i, j)` and `(j, i)` describe
        // the same unordered pair, so checking `j in i..dim` halves the loop
        // count from `dim²` to `dim·(dim+1)/2` without any coverage loss.
        for i in 0..dim {
            for j in i..dim {
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

    /// Regression guard for the panic-propagation contract: any change that
    /// re-substitutes the worker's message (e.g. reverting to `.expect(...)`,
    /// wrapping with a custom error type, or `unwrap_or_else(|_| panic!("..."))`)
    /// will cause the "out of bounds" substring to disappear and this test to
    /// fail — surfacing the regression immediately rather than silently burying
    /// it.
    #[test]
    #[should_panic(expected = "out of bounds")]
    fn worker_thread_panic_payload_propagates_to_caller() {
        use crate::assembly::ElementStiffness;
        let k_e = ElementStiffness {
            n_dofs: 12,
            data: vec![],
        };
        let connectivity = [0usize, 1, 2, 3];
        let element = AssemblyElement {
            id: 99,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        // threads: 1 — one worker, one panic, deterministic. Higher thread
        // counts add no coverage of the propagation path and risk timing noise.
        let _ = assemble_global_stiffness(4, &[element], AssemblyMode::Parallel { threads: 1 });
    }

    /// Faer's `try_new_from_triplets` sums duplicate `(row, col)` entries
    /// in **encounter order** — the contract `assemble_global_stiffness`'s
    /// parallel-mode determinism depends on. A faer minor-version bump
    /// that switches internally to e.g. tree-pairwise reduction (without
    /// breaking faer's own internal `test_from_indices` fixture, whose
    /// values happen to be sum-order-invariant) would silently invalidate
    /// the bit-stability claim pinned by
    /// `parallel_mode_bit_equal_to_deterministic_on_disjoint_mesh` and the
    /// fixed-thread-count back-to-back determinism check in
    /// `parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`.
    ///
    /// Five duplicate triplets at `(0, 0)` whose left-fold (encounter-order)
    /// sum and pairwise-tree-reduction sum diverge well above the LSB:
    ///
    /// ```text
    /// values:           [1e20, 1.0, -1e20, 1.0, 1.0]
    /// encounter-fold:   ((((1e20 + 1.0) + -1e20) + 1.0) + 1.0)
    ///                 = ((1e20 + -1e20) + 1.0) + 1.0
    ///                 = (0.0 + 1.0) + 1.0
    ///                 = 2.0
    /// pairwise-tree:    ((1e20 + 1.0) + (-1e20 + 1.0)) + 1.0
    ///                 = (1e20 + -1e20) + 1.0
    ///                 = 0.0 + 1.0
    ///                 = 1.0
    /// ```
    ///
    /// (`1.0` is below the half-ulp of `1e20 ≈ 2^66`, so `1e20 + 1.0` and
    /// `-1e20 + 1.0` round back to `±1e20` exactly.) Any non-`2.0` result
    /// means faer's summation order has changed and the assembly
    /// determinism contract must be re-validated.
    #[test]
    fn faer_sums_duplicate_triplets_in_encounter_order() {
        let triplets = [
            Triplet::new(0usize, 0usize, 1e20),
            Triplet::new(0, 0, 1.0),
            Triplet::new(0, 0, -1e20),
            Triplet::new(0, 0, 1.0),
            Triplet::new(0, 0, 1.0),
        ];
        let k = SparseRowMat::try_new_from_triplets(1, 1, &triplets)
            .expect("1x1 input within declared dims");
        let v = read(&k, 0, 0);
        assert_eq!(
            v.to_bits(),
            2.0_f64.to_bits(),
            "faer no longer sums duplicates in encounter order: got {v}, expected 2.0. \
             This breaks the parallel-mode determinism contract pinned by \
             `parallel_mode_bit_equal_to_deterministic_on_disjoint_mesh` and the \
             tolerance-equivalence claim in \
             `parallel_mode_tolerance_equivalent_to_deterministic_on_shared_dof_mesh`. \
             Re-validate before bumping the faer dep.",
        );
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

    /// Single P1 hex element with identity connectivity `[0..8]` → K_global
    /// equals K_e bit-for-bit at every entry.
    ///
    /// Pins that the D-agnostic emission loop in `emit_element_triplets`
    /// handles `n_local = 8`, `d_e = 3`. Identity connectivity ⇒ no
    /// FP-summation reordering ⇒ bit-equality is achievable, not just
    /// tolerance-equality. The 24×24 dim assertion is the first regression
    /// pin for the hex element kind in `assemble_global_stiffness`.
    ///
    /// Pure-hex mesh ⇒ `D = max(d_e) = 3` (NOT 6 — shell isn't present),
    /// so `K.nrows() == 24` and `K.ncols() == 24`, not 48.
    #[test]
    fn single_p1_hex_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_hex_phys_nodes(1.0);
        let k_e = element_stiffness_hex_p1(&phys, &mat);
        assert_eq!(k_e.n_dofs, 24);

        let connectivity: [usize; 8] = std::array::from_fn(|i| i);
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(8, &[element], AssemblyMode::Deterministic);

        // Pure-hex mesh ⇒ D = 3 (NOT 6), so dim = 3 · 8 = 24 (NOT 48).
        assert_eq!(
            k.nrows(),
            24,
            "pure-hex mesh must derive D = 3, giving 3·8 = 24 rows (not 6·8 = 48)",
        );
        assert_eq!(
            k.ncols(),
            24,
            "pure-hex mesh must derive D = 3, giving 3·8 = 24 cols (not 6·8 = 48)",
        );

        // Bit-equality: identity connectivity ⇒ each entry has exactly one
        // contributing triplet.
        for i in 0..24 {
            for j in 0..24 {
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

    /// Single P1 wedge element with identity connectivity `[0..6]` → K_global
    /// equals K_e bit-for-bit at every entry.
    ///
    /// First regression test for `n_local = 6` in `assemble_global_stiffness`.
    /// Pins that the D-agnostic emission loop handles the wedge's 6-node
    /// footprint (`d_e = 3`, `n_dofs = 18`) at identity connectivity.
    ///
    /// Pure-wedge mesh ⇒ `D = max(d_e) = 3`, so `K.nrows() == 18` and
    /// `K.ncols() == 18`. A regression that special-cases `n_local ∈ {4, 8, 10}`
    /// in `emit_element_triplets`'s loop bounds would surface here.
    #[test]
    fn single_p1_wedge_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let phys = scaled_unit_wedge_phys_nodes(1.0);
        let k_e = element_stiffness_wedge_p1(&phys, &mat);
        assert_eq!(k_e.n_dofs, 18);

        let connectivity: [usize; 6] = std::array::from_fn(|i| i);
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(6, &[element], AssemblyMode::Deterministic);

        assert_eq!(
            k.nrows(),
            18,
            "pure-wedge mesh must derive D = 3, giving 3·6 = 18 rows",
        );
        assert_eq!(
            k.ncols(),
            18,
            "pure-wedge mesh must derive D = 3, giving 3·6 = 18 cols",
        );

        for i in 0..18 {
            for j in 0..18 {
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

    /// Existing P2-only mesh assembles with `D = 3` under the new
    /// max-over-elements DOFs-per-node derivation — i.e. pure-tet meshes
    /// keep their v0.3 `3 · n_nodes` global dim, *not* a 6-DOF/node shape.
    ///
    /// Re-runs the same identity-connectivity invariant as
    /// `single_p2_element_identity_connectivity_matches_k_e_bit_for_bit`
    /// but additionally asserts `K.nrows() == 30` and `K.ncols() == 30`
    /// (= `3 · 10`, not `6 · 10 = 60`). Locks the design decision: the
    /// global D is `max(d_e)` rather than e.g. `max(d_e, 6)` or
    /// `unwrap_or(6)` (typo guards). A future regression that, say,
    /// flips the `unwrap_or(3)` default to `unwrap_or(6)` would surface
    /// here as a 60-row matrix.
    #[test]
    fn existing_p2_global_assembly_pattern_unchanged_under_d_derived_loop() {
        let mat = dimensionless_steel_like();
        let phys = scaled_p2_phys_nodes(1.0);
        let k_e = element_stiffness_p2(&phys, &mat);
        assert_eq!(k_e.n_dofs, 30);

        let connectivity: [usize; 10] = std::array::from_fn(|i| i);
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(10, &[element], AssemblyMode::Deterministic);

        // Pure-P2 mesh ⇒ max(d_e) = 3 ⇒ dim = 30 (NOT 60).
        assert_eq!(
            k.nrows(),
            30,
            "pure-P2 mesh must derive D = 3, giving 3·10 = 30 rows (not 6·10 = 60)",
        );
        assert_eq!(
            k.ncols(),
            30,
            "pure-P2 mesh must derive D = 3, giving 3·10 = 30 cols (not 6·10 = 60)",
        );

        // Reassert bit-equality at every entry.
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

    /// One P1 tet sharing only node 0 with one MITC3 shell → unified
    /// 6-DOF/node global K with the right per-node-pair contributions.
    ///
    /// Mesh: tet on connectivity `[0, 1, 2, 3]`, shell on connectivity
    /// `[0, 4, 5]`. The two elements share *only* node 0. `n_nodes = 6`,
    /// expected global dim `6 · 6 = 36`.
    ///
    /// Pinning strategy (each assertion locks a different contract):
    /// - **dim**: `D = 6` derives from `max(d_e)` = `max(3, 6)` = 6.
    /// - **node 0 displacement block (rows/cols 0..3)**: both tet and
    ///   shell touch translational DOFs. faer sums duplicates in encounter
    ///   order — tet emits first (it appears first in the slice), shell
    ///   emits second — so the bit-for-bit expected value is
    ///   `K_e_tet[α][β] + K_e_shell[α][β]` summed in that order.
    /// - **node 0 rotation block (rows/cols 3..6)**: only the shell has
    ///   rotation DOFs; the tet's 3-DOF/node emission stops at α < 3, so
    ///   nothing lands at α ∈ [3, 6). Bit-equal to `K_e_shell[3+α][3+β]`.
    /// - **tet-only nodes 1..4**: rotation DOFs (`6n + 3..6n + 6`) read
    ///   as `0.0` exactly — the shell does not touch these nodes, the
    ///   tet emits no rotation DOFs, so the orphan rotation rows/cols
    ///   are unstored and densify to zero.
    /// - **shell-only nodes 4, 5**: all 6 DOFs/node match
    ///   `K_e_shell[6·a_local + α][6·b_local + β]` exactly (only shell
    ///   contributes; encounter order has just one summand).
    #[test]
    fn mixed_tet_and_shell_share_node_assembles_into_unified_6dof_per_node_global_k() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        assert_eq!(k_e_tet.n_dofs, 12);
        assert_eq!(k_e_shell.n_dofs, 18);

        let conn_tet = [0usize, 1, 2, 3];
        let conn_shell = [0usize, 4, 5];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
        ];
        let n_nodes = 6;
        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // (i) dim = 6 · n_nodes (max(d_e) = max(3, 6) = 6).
        assert_eq!(k.nrows(), 36);
        assert_eq!(k.ncols(), 36);

        // (ii) Node 0 displacement-displacement block (α, β ∈ 0..3): both
        // elements contribute. Encounter order is tet (slice index 0) then
        // shell (slice index 1); faer sums duplicates left-to-right.
        for alpha in 0..3 {
            for beta in 0..3 {
                let i = alpha;
                let j = beta;
                let actual = read(&k, i, j);
                let expected = k_e_tet.get(alpha, beta) + k_e_shell.get(alpha, beta);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] (node 0 disp-disp): \
                     actual = {actual}, expected = K_tet+K_shell = {expected}",
                );
            }
        }

        // (iii) Node 0 rotation-rotation block (α, β ∈ 3..6 in K_e_shell;
        // global rows/cols 3..6 because n_dofs_per_node = 6 and node = 0).
        for alpha in 0..3 {
            for beta in 0..3 {
                let i = 3 + alpha; // rows 3..6
                let j = 3 + beta; // cols 3..6
                let actual = read(&k, i, j);
                // Local shell index for node 0's rotation DOFs is 3 + α.
                let expected = k_e_shell.get(3 + alpha, 3 + beta);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] (node 0 rot-rot): \
                     actual = {actual}, expected = K_shell[{}][{}] = {expected}",
                    3 + alpha,
                    3 + beta,
                );
            }
        }

        // (iv) Tet-only nodes 1..=3 (touched by tet only; tet emits 3
        // DOFs/node so rotation DOFs at these nodes are orphan zeros).
        // Node 4 and 5 are shell-only (handled in (v)).
        for tet_only_node in 1..=3 {
            for alpha in 3..6 {
                for j in 0..36 {
                    let i = 6 * tet_only_node + alpha;
                    let actual = read(&k, i, j);
                    assert_eq!(
                        actual, 0.0,
                        "K_global[{i}][{j}] (orphan rotation row at tet-only node {tet_only_node}): \
                         expected 0.0, got {actual}",
                    );
                }
            }
        }

        // (v) Shell-only nodes 4 and 5: all 6×6 DOF blocks match
        // `K_e_shell[6·a_local + α][6·b_local + β]` exactly. Local index
        // 0 = global node 0 (skipped — covered above), local 1 = node 4,
        // local 2 = node 5.
        let shell_node_to_local = |gn: usize| -> Option<usize> {
            match gn {
                0 => Some(0),
                4 => Some(1),
                5 => Some(2),
                _ => None,
            }
        };
        for &gn_a in &[4usize, 5] {
            let la = shell_node_to_local(gn_a).unwrap();
            for &gn_b in &[4usize, 5] {
                let lb = shell_node_to_local(gn_b).unwrap();
                for alpha in 0..6 {
                    for beta in 0..6 {
                        let i = 6 * gn_a + alpha;
                        let j = 6 * gn_b + beta;
                        let actual = read(&k, i, j);
                        let expected = k_e_shell.get(6 * la + alpha, 6 * lb + beta);
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "K_global[{i}][{j}] (shell-only node-pair (gn={gn_a}, gn={gn_b}), \
                             local ({la}, {lb}), α={alpha} β={beta}): \
                             actual = {actual}, expected = {expected}",
                        );
                    }
                }
            }
        }
    }

    /// Parallel mode is tolerance-equivalent to deterministic mode on a
    /// 4×-replicated mixed-element (tet + shell-sharing-node) mesh.
    ///
    /// Mesh: four disjoint copies of the step-3 fixture (one P1 tet
    /// `[6k, 6k+1, 6k+2, 6k+3]` plus one MITC3 shell `[6k, 6k+4, 6k+5]`,
    /// for k ∈ {0, 1, 2, 3}). 8 elements total, 24 nodes, dim = `6 · 24
    /// = 144`. The eight-element slice gives `Parallel { threads: 4 }`'s
    /// `chunk_size = ceil(8 / 4) = 2` two elements per worker; each
    /// worker handles one tet + one shell from its own disjoint copy.
    ///
    /// Pins three contracts that step-2's generalisation must preserve:
    ///
    /// 1. **Per-thread Vec capacity formula** uses
    ///    `Σ d_e² · n_local²` (P1 tet: 144, MITC3 shell: 324) — not the
    ///    old hardcoded `9 · n_local²` (which would over-allocate
    ///    by 3× for tets but under-allocate by 4× for shells, masking
    ///    correctness via Vec growth but breaking the pre-sized invariant
    ///    the docstring claims).
    /// 2. **Mixed `d_e` slice in a single chunk** — when one chunk
    ///    contains both a tet (`d_e = 3`) and a shell (`d_e = 6`),
    ///    `emit_element_triplets` must derive `d_e` per-element rather
    ///    than reading some chunk-level constant.
    /// 3. **Slice-order merge survives mixed-element emission** —
    ///    `K_par` matches `K_det` to 1e-12 relative+absolute, the same
    ///    band the existing pure-tet shared-DOF tests pin.
    #[test]
    fn mixed_mesh_parallel_mode_tolerance_equivalent_to_deterministic() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);

        // Four disjoint replicas: replica k owns nodes 6k..6k+6.
        // Slice ordering interleaves tet, shell, tet, shell, ... so a
        // chunk_size of 2 puts (tet_k, shell_k) into worker k.
        let conns_tet: Vec<[usize; 4]> = (0..4)
            .map(|k| [6 * k, 6 * k + 1, 6 * k + 2, 6 * k + 3])
            .collect();
        let conns_shell: Vec<[usize; 3]> = (0..4).map(|k| [6 * k, 6 * k + 4, 6 * k + 5]).collect();

        let mut elements: Vec<AssemblyElement<'_>> = Vec::with_capacity(8);
        for k in 0..4usize {
            elements.push(AssemblyElement {
                id: 2 * k,
                connectivity: &conns_tet[k],
                k_e: &k_e_tet,
            });
            elements.push(AssemblyElement {
                id: 2 * k + 1,
                connectivity: &conns_shell[k],
                k_e: &k_e_shell,
            });
        }
        let n_nodes = 24;
        let dim = 6 * n_nodes; // 144

        let det = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let par =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Parallel { threads: 4 });

        assert_eq!(det.nrows(), dim);
        assert_eq!(det.ncols(), dim);
        assert_eq!(par.nrows(), dim);
        assert_eq!(par.ncols(), dim);

        for i in 0..dim {
            for j in 0..dim {
                let d = read(&det, i, j);
                let p = read(&par, i, j);
                let tol = 1e-12 * d.abs().max(1.0);
                let delta = (p - d).abs();
                assert!(
                    delta < tol,
                    "K_par[{i}][{j}] = {p} but K_det[{i}][{j}] = {d}; \
                     |Δ| = {delta} ≥ tol = {tol}",
                );
            }
        }
    }

    /// Single 18-DOF MITC3 shell element with identity connectivity
    /// `[0, 1, 2]` → `K_global` equals `K_e` bit-for-bit at every entry.
    ///
    /// Pins the D-agnostic generalisation of the scatter loop: the shell
    /// element ships 6 DOFs per node (3 translations + 3 rotations) rather
    /// than the 3 DOFs of a tet, so the per-element divisibility derivation
    /// `dofs_per_node = k_e.n_dofs / connectivity.len()` must yield 6, and
    /// the global matrix dim must be `6 * n_nodes = 18`. Identity
    /// connectivity makes the DOF mapping degenerate to identity, so
    /// bit-equality is achievable with no FP-summation reordering.
    #[test]
    fn single_shell_18dof_element_identity_connectivity_matches_k_e_bit_for_bit() {
        let mat = dimensionless_steel_like();
        let k_e = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        assert_eq!(k_e.n_dofs, 18);

        let connectivity = [0usize, 1, 2];
        let element = AssemblyElement {
            id: 0,
            connectivity: &connectivity,
            k_e: &k_e,
        };
        let k = assemble_global_stiffness(3, &[element], AssemblyMode::Deterministic);
        // 6 DOFs/node × 3 nodes = 18.
        assert_eq!(k.nrows(), 18);
        assert_eq!(k.ncols(), 18);

        for i in 0..18 {
            for j in 0..18 {
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

    /// One P1 tet, one P1 hex, and one P1 wedge sharing only node 0 →
    /// unified 3-DOF/node global K with the right per-node-pair contributions.
    ///
    /// Mesh:
    /// - tet on `[0, 1, 2, 3]` (P1, 12 DOFs)
    /// - hex on `[0, 4, 5, 6, 7, 8, 9, 10]` (P1, 24 DOFs)
    /// - wedge on `[0, 11, 12, 13, 14, 15]` (P1, 18 DOFs)
    ///
    /// All three elements share *only* node 0. `n_nodes = 16`, expected
    /// global dim `3 · 16 = 48`. All three have `d_e = 3`, so the global
    /// `D = max(d_e) = 3` (NOT 6).
    ///
    /// Pinning strategy:
    /// - **dim**: pure-volume-element mesh ⇒ D = 3, dim = 48 (NOT 96).
    /// - **node 0 displacement block (rows/cols 0..3)**: all three elements
    ///   contribute. Encounter order matches slice order tet → hex → wedge;
    ///   faer sums duplicates left-to-right so the bit-for-bit expected value
    ///   is `K_e_tet[α][β] + K_e_hex[α][β] + K_e_wedge[α][β]` summed in that
    ///   order. A three-summand left-fold has stable bit-equality with the
    ///   `expected` construction order.
    /// - **exclusive nodes**: tet-only (1..4), hex-only (4..11), wedge-only
    ///   (11..16) — each 3×3 self-block matches the corresponding
    ///   K_e[3·a_local..3·a_local+3, 3·b_local..3·b_local+3] bit-for-bit
    ///   (only one summand from the owning element).
    /// - **shared-to-exclusive cross-blocks**: pair the shared node 0 with
    ///   one element-exclusive partner per kind (gn ∈ {1, 4, 11}). Each
    ///   cross-block is single-summand (only the owning element touches the
    ///   exclusive partner), so bit-equality with the per-element K_e sub-
    ///   block holds. Both `(0, partner)` and `(partner, 0)` directions are
    ///   checked — a regression that miscomputed the dest-row/col when the
    ///   first connectivity entry is shared but the second is exclusive
    ///   would slip past the (a)/(c)/(d)/(e) self-block cells.
    #[test]
    fn mixed_tet_hex_wedge_sharing_node_assembles_into_unified_3dof_per_node_global_k() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_hex = element_stiffness_hex_p1(&scaled_unit_hex_phys_nodes(1.0), &mat);
        let k_e_wedge = element_stiffness_wedge_p1(&scaled_unit_wedge_phys_nodes(1.0), &mat);
        assert_eq!(k_e_tet.n_dofs, 12);
        assert_eq!(k_e_hex.n_dofs, 24);
        assert_eq!(k_e_wedge.n_dofs, 18);

        let conn_tet = [0usize, 1, 2, 3];
        let conn_hex = [0usize, 4, 5, 6, 7, 8, 9, 10];
        let conn_wedge = [0usize, 11, 12, 13, 14, 15];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_hex,
                k_e: &k_e_hex,
            },
            AssemblyElement {
                id: 2,
                connectivity: &conn_wedge,
                k_e: &k_e_wedge,
            },
        ];
        let n_nodes = 16;
        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // (a) dim = 3 · n_nodes (max(d_e) = 3, NOT 6).
        let dim = 3 * n_nodes;
        assert_eq!(k.nrows(), dim);
        assert_eq!(k.ncols(), dim);

        // (b) Node 0's displacement-displacement block (α, β ∈ 0..3): all
        // three elements contribute, summed in tet → hex → wedge encounter
        // order. Bit-equal to a three-summand left-fold of the K_e's
        // [0..3, 0..3] sub-blocks.
        for alpha in 0..3 {
            for beta in 0..3 {
                let i = alpha;
                let j = beta;
                let actual = read(&k, i, j);
                // Three-summand left-fold in slice order:
                //   ((K_tet[α,β] + K_hex[α,β]) + K_wedge[α,β])
                let expected = k_e_tet.get(alpha, beta)
                    + k_e_hex.get(alpha, beta)
                    + k_e_wedge.get(alpha, beta);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "K_global[{i}][{j}] (node 0 shared block): \
                     actual = {actual}, expected = K_tet+K_hex+K_wedge = {expected}",
                );
            }
        }

        // (c) tet-exclusive nodes (1..=3): 3×3 self-blocks match
        // K_e_tet[3·a_local..3·a_local+3, 3·b_local..3·b_local+3] bit-for-bit.
        // Local indices: global node 1 = local 1, node 2 = local 2, node 3 = local 3.
        for &gn_a in &[1usize, 2, 3] {
            let la = gn_a; // identity local-from-global on tet-only nodes
            for &gn_b in &[1usize, 2, 3] {
                let lb = gn_b;
                for alpha in 0..3 {
                    for beta in 0..3 {
                        let i = 3 * gn_a + alpha;
                        let j = 3 * gn_b + beta;
                        let actual = read(&k, i, j);
                        let expected = k_e_tet.get(3 * la + alpha, 3 * lb + beta);
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "K_global[{i}][{j}] (tet-only node-pair gn=({gn_a}, {gn_b}), \
                             local ({la}, {lb}), α={alpha} β={beta}): \
                             actual = {actual}, expected = {expected}",
                        );
                    }
                }
            }
        }

        // (d) hex-exclusive nodes (4..=10): 3×3 self-blocks match
        // K_e_hex[3·a_local..3·a_local+3, 3·b_local..3·b_local+3] bit-for-bit.
        // Local indices: global node 4 = local 1, 5 = local 2, ..., 10 = local 7.
        for &gn_a in &[4usize, 5, 6, 7, 8, 9, 10] {
            let la = gn_a - 3; // global 4..=10 → local 1..=7
            for &gn_b in &[4usize, 5, 6, 7, 8, 9, 10] {
                let lb = gn_b - 3;
                for alpha in 0..3 {
                    for beta in 0..3 {
                        let i = 3 * gn_a + alpha;
                        let j = 3 * gn_b + beta;
                        let actual = read(&k, i, j);
                        let expected = k_e_hex.get(3 * la + alpha, 3 * lb + beta);
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "K_global[{i}][{j}] (hex-only node-pair gn=({gn_a}, {gn_b}), \
                             local ({la}, {lb}), α={alpha} β={beta}): \
                             actual = {actual}, expected = {expected}",
                        );
                    }
                }
            }
        }

        // (e) wedge-exclusive nodes (11..=15): 3×3 self-blocks match
        // K_e_wedge[3·a_local..3·a_local+3, 3·b_local..3·b_local+3] bit-for-bit.
        // Local indices: global node 11 = local 1, ..., 15 = local 5.
        for &gn_a in &[11usize, 12, 13, 14, 15] {
            let la = gn_a - 10; // global 11..=15 → local 1..=5
            for &gn_b in &[11usize, 12, 13, 14, 15] {
                let lb = gn_b - 10;
                for alpha in 0..3 {
                    for beta in 0..3 {
                        let i = 3 * gn_a + alpha;
                        let j = 3 * gn_b + beta;
                        let actual = read(&k, i, j);
                        let expected = k_e_wedge.get(3 * la + alpha, 3 * lb + beta);
                        assert_eq!(
                            actual.to_bits(),
                            expected.to_bits(),
                            "K_global[{i}][{j}] (wedge-only node-pair gn=({gn_a}, {gn_b}), \
                             local ({la}, {lb}), α={alpha} β={beta}): \
                             actual = {actual}, expected = {expected}",
                        );
                    }
                }
            }
        }

        // (f) Cross-blocks pairing the shared node (gn=0) with one
        // element-exclusive partner per element kind: gn=1 (tet-only),
        // gn=4 (hex-only), gn=11 (wedge-only). Each pair is owned by
        // exactly one element (the non-shared partner is exclusive to
        // it), so the cross-block is a single-summand triplet — bit-
        // equality with `K_e[3·la..3·la+3, 3·lb..3·lb+3]` still works.
        //
        // Both orderings are checked: `(gn=0, gn=partner)` exercises the
        // dest-row from the *shared* node and dest-col from the exclusive
        // partner, and `(gn=partner, gn=0)` exercises the mirror direction.
        // A regression that miscomputed the dest-row when the *first*
        // connectivity entry is shared but the second is exclusive (e.g.
        // a stale `local_a == 0` early-return in `emit_element_triplets`)
        // would not be caught by the (a)/(c)/(d)/(e) self-block pinned
        // cells alone — this section closes that gap.
        for (gn_partner, k_e, local_partner, kind) in [
            (1usize, &k_e_tet, 1usize, "tet"),
            (4usize, &k_e_hex, 1usize, "hex"),
            (11usize, &k_e_wedge, 1usize, "wedge"),
        ] {
            // Direction 1: K_global[3·0..3·0+3, 3·gn_partner..3·gn_partner+3]
            //   = K_e[3·0..3·0+3, 3·local_partner..3·local_partner+3]
            for alpha in 0..3 {
                for beta in 0..3 {
                    let i = alpha; // 3 · 0 + alpha
                    let j = 3 * gn_partner + beta;
                    let actual = read(&k, i, j);
                    let expected = k_e.get(alpha, 3 * local_partner + beta);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "K_global[{i}][{j}] ({kind} cross-block (gn=0, gn={gn_partner}), \
                         local (0, {local_partner}), α={alpha} β={beta}): \
                         actual = {actual}, expected = {expected}",
                    );
                }
            }
            // Direction 2: K_global[3·gn_partner..3·gn_partner+3, 3·0..3·0+3]
            //   = K_e[3·local_partner..3·local_partner+3, 3·0..3·0+3]
            for alpha in 0..3 {
                for beta in 0..3 {
                    let i = 3 * gn_partner + alpha;
                    let j = beta; // 3 · 0 + beta
                    let actual = read(&k, i, j);
                    let expected = k_e.get(3 * local_partner + alpha, beta);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "K_global[{i}][{j}] ({kind} cross-block (gn={gn_partner}, gn=0), \
                         local ({local_partner}, 0), α={alpha} β={beta}): \
                         actual = {actual}, expected = {expected}",
                    );
                }
            }
        }
    }

    /// Mixed-element global K is symmetric within the same FP tolerance
    /// band the existing tet-only `global_k_is_symmetric_within_fp_tolerance`
    /// pins.
    ///
    /// Mesh: a small fan rooted at node 0, mixing one P1 tet `[0,1,2,3]`
    /// with two MITC3 shells `[0,4,5]` and `[0,6,7]`. `n_nodes = 8`,
    /// global dim = `6 · 8 = 48`. Node 0 is multi-coupled across all three
    /// elements so the duplicate-triplet summation path runs on a node
    /// where contributions from two different `dofs_per_node` (3 from
    /// tet, 6 from each shell) overlap on displacement DOFs.
    ///
    /// **Why mixed-mesh symmetry follows from per-kind symmetry +
    /// full-block emission** (mirrors the rationale block on
    /// `global_k_is_symmetric_within_fp_tolerance`):
    /// `element_stiffness_p1` ships a symmetric `K_e_tet`
    /// (Task 2915 / `tet::tests::p1_element_stiffness_is_symmetric...`)
    /// and `shell_element_stiffness` ships a symmetric `K_e_shell`
    /// (Task 3014 / `shell_element_stiffness_is_symmetric_within_fp_tolerance`).
    /// faer's `try_new_from_triplets` sums duplicate `(row, col)` entries
    /// in fixed encounter order — pinned by
    /// `faer_sums_duplicate_triplets_in_encounter_order`. The D-derived
    /// emission loop in step-2 emits the **full** `(a, α, b, β)` block
    /// for every element kind (not upper-triangle only), so
    /// `K_global[i][j]` and `K_global[j][i]` are sums of element-mirror
    /// pairs of triplets coming from the same `K_e` matrices. The LSB
    /// of the encounter-order summation at `(i, j)` versus `(j, i)` can
    /// differ, but both sides reduce to the same value within the FP
    /// summation band the tet-only case pins. A regression that breaks
    /// the full-block invariant for **either** kind (e.g. half-block
    /// emission for shells while keeping full blocks for tets) surfaces
    /// here even when the per-kind symmetry tests stay green.
    ///
    /// Tolerance `1e-9 · max(|K[i][j]|, |K[j][i]|, 1)`, identical to the
    /// pure-tet symmetry test. Iterates the upper triangle (`j in i..dim`)
    /// — `(i, j)` and `(j, i)` describe the same unordered pair so the
    /// halved loop loses no coverage.
    #[test]
    fn mixed_mesh_global_k_is_symmetric_within_fp_tolerance() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);

        let conn_tet = [0usize, 1, 2, 3];
        let conn_shell_a = [0usize, 4, 5];
        let conn_shell_b = [0usize, 6, 7];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_shell_a,
                k_e: &k_e_shell,
            },
            AssemblyElement {
                id: 2,
                connectivity: &conn_shell_b,
                k_e: &k_e_shell,
            },
        ];
        let n_nodes = 8;
        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // Mixed mesh ⇒ max(d_e) = 6 ⇒ global dim = 6 · 8 = 48.
        let dim = 6 * n_nodes;
        assert_eq!(k.nrows(), dim);
        assert_eq!(k.ncols(), dim);

        for i in 0..dim {
            for j in i..dim {
                let kij = read(&k, i, j);
                let kji = read(&k, j, i);
                let tol = 1e-9 * kij.abs().max(kji.abs()).max(1.0);
                let delta = (kij - kji).abs();
                assert!(
                    delta <= tol,
                    "mixed-mesh K[{i}][{j}] = {kij}, K[{j}][{i}] = {kji}; \
                     |Δ| = {delta} > tol = {tol}",
                );
            }
        }
    }

    /// Global K assembled from a tet+hex+wedge fan mesh (same shape as
    /// `mixed_tet_hex_wedge_sharing_node_assembles_into_unified_3dof_per_node_global_k`)
    /// is symmetric within FP tolerance.
    ///
    /// Per-kind K_e symmetry is already pinned upstream:
    /// - tet via Task 2915 / `tet::tests::p1_element_stiffness_is_symmetric_...`
    /// - hex / wedge via Task 2985 / `run_element_stiffness_tests` block-(b)
    ///   symmetry check.
    ///
    /// The full-block emission of `emit_element_triplets` (both `(a, b)` and
    /// `(b, a)` triplets) combined with faer's stable duplicate-summation
    /// order means `K_global[i][j]` and `K_global[j][i]` are sums of
    /// mirror-pair triplets from the same K_e — the LSB of the encounter-
    /// order summation at `(i, j)` versus `(j, i)` can differ, but both
    /// reduce to the same value within the FP summation band.
    ///
    /// Mirrors `mixed_mesh_global_k_is_symmetric_within_fp_tolerance` (which
    /// pins tet+shell symmetry); this test extends the contract to
    /// tet+hex+wedge. A regression that breaks the full-block invariant
    /// for any of the three volume kernels surfaces here even when the
    /// per-kind symmetry tests stay green.
    ///
    /// Tolerance `1e-9 · max(|K[i][j]|, |K[j][i]|, 1)` matches the existing
    /// pure-tet symmetry test.
    #[test]
    fn mixed_tet_hex_wedge_global_k_is_symmetric_within_fp_tolerance() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_hex = element_stiffness_hex_p1(&scaled_unit_hex_phys_nodes(1.0), &mat);
        let k_e_wedge = element_stiffness_wedge_p1(&scaled_unit_wedge_phys_nodes(1.0), &mat);

        // Same fan mesh as step-17. All three elements share node 0.
        let conn_tet = [0usize, 1, 2, 3];
        let conn_hex = [0usize, 4, 5, 6, 7, 8, 9, 10];
        let conn_wedge = [0usize, 11, 12, 13, 14, 15];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_hex,
                k_e: &k_e_hex,
            },
            AssemblyElement {
                id: 2,
                connectivity: &conn_wedge,
                k_e: &k_e_wedge,
            },
        ];
        let n_nodes = 16;
        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // Pure-volume mesh ⇒ max(d_e) = 3 ⇒ dim = 48.
        let dim = 3 * n_nodes;
        assert_eq!(k.nrows(), dim);
        assert_eq!(k.ncols(), dim);

        // Iterate upper triangle only — (i, j) and (j, i) describe the same
        // unordered pair, so j in i..dim halves the loop count without any
        // coverage loss.
        for i in 0..dim {
            for j in i..dim {
                let kij = read(&k, i, j);
                let kji = read(&k, j, i);
                let tol = 1e-9 * kij.abs().max(kji.abs()).max(1.0);
                let delta = (kij - kji).abs();
                assert!(
                    delta <= tol,
                    "tet+hex+wedge mesh K[{i}][{j}] = {kij}, K[{j}][{i}] = {kji}; \
                     |Δ| = {delta} > tol = {tol}",
                );
            }
        }
    }

    /// Empty `elements` slice → `OrphanDofsSummary` with count=0 and
    /// examples empty.
    ///
    /// Pins the empty-input contract for `detect_orphan_dofs`, paralleling
    /// `empty_elements_returns_zero_3n_by_3n_sparse_matrix` for
    /// `assemble_global_stiffness`.
    #[test]
    fn detect_orphan_dofs_empty_elements_returns_zero_summary() {
        let summary = detect_orphan_dofs(4, &[]);
        assert_eq!(summary.count, 0);
        assert!(summary.examples.is_empty());
    }

    /// Single P1 tet on `[0,1,2,3]` → no orphan DOFs.
    ///
    /// In a pure-tet mesh `D = 3` and every node has `d_e_max_local = 3 = D`,
    /// so no axis at any node is orphaned. Pins that `detect_orphan_dofs`
    /// reports zero orphans for a homogeneous-D mesh (no false positives).
    #[test]
    fn detect_orphan_dofs_pure_tet_mesh_reports_zero_orphans() {
        let mat = dimensionless_steel_like();
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let conn = [0usize, 1, 2, 3];
        let elements = [AssemblyElement {
            id: 0,
            connectivity: &conn,
            k_e: &k_e,
        }];
        let summary = detect_orphan_dofs(4, &elements);
        assert_eq!(summary.count, 0);
        assert!(summary.examples.is_empty());
    }

    /// Every reported orphan `(node, α)` has a structurally zero row and column
    /// in the assembled global K.
    ///
    /// Builds the mixed tet+shell fixture, assembles K with
    /// `assemble_global_stiffness`, then calls `detect_orphan_dofs` and
    /// verifies that for every `(node, α)` in `examples`, the entire row
    /// `D*node + α` and column `D*node + α` in K are zero.
    ///
    /// Pins that the detector's output matches the assembler's actual emission
    /// pattern: a regression in either (e.g. wrong d_e formula, wrong axis
    /// mapping) surfaces here rather than silently producing a wrong matrix.
    #[test]
    fn detect_orphan_dofs_consistent_with_assemble_global_stiffness_emission() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        let conn_tet = [0usize, 1, 2, 3];
        let conn_shell = [0usize, 4, 5];
        let n_nodes = 6usize;
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
        ];

        let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let summary = detect_orphan_dofs(n_nodes, &elements);

        // D = 6; dim = 6 * n_nodes = 36.
        let d_global = 6usize;
        let dim = d_global * n_nodes;

        assert_eq!(k.nrows(), dim);
        assert_eq!(k.ncols(), dim);

        // Precondition: the fixture's orphan count must fit within MAX_EXAMPLES
        // so that every orphan is covered by summary.examples. If the fixture
        // ever grows past the cap this assert fires loudly rather than silently
        // weakening the loop below.
        assert!(
            summary.examples.len() == summary.count,
            "examples truncated: stored {} of {} total orphans — \
             either raise MAX_EXAMPLES or use a smaller fixture",
            summary.examples.len(),
            summary.count,
        );

        // For every reported orphan (node, α) the entire DOF row and column
        // must be zero.
        for &(node, axis) in &summary.examples {
            let dof = d_global * node + axis;
            for j in 0..dim {
                let row_val = read(&k, dof, j);
                assert_eq!(
                    row_val,
                    0.0,
                    "K[{dof}][{j}] (orphan row for node={node}, axis={axis}) \
                     should be 0.0, got {row_val}",
                );
            }
            for i in 0..dim {
                let col_val = read(&k, i, dof);
                assert_eq!(
                    col_val,
                    0.0,
                    "K[{i}][{dof}] (orphan col for node={node}, axis={axis}) \
                     should be 0.0, got {col_val}",
                );
            }
        }
    }

    /// `Display` for `OrphanDofsSummary` emits a single-line diagnostic string.
    ///
    /// Pins:
    /// 1. No newlines in the output (`!result.contains('\n')`).
    /// 2. The string contains `count=9` so it is grep-able.
    /// 3. The first three `(node, axis)` pairs `(1, 3)`, `(1, 4)`, `(1, 5)`
    ///    appear literally so a reader can spot offending nodes.
    /// 4. The empty summary (`count=0`) also formats correctly.
    #[test]
    fn orphan_dofs_summary_display_emits_single_line_diagnostic() {
        // Non-empty case: use the mixed-mesh fixture (count=9, full examples).
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        let conn_tet = [0usize, 1, 2, 3];
        let conn_shell = [0usize, 4, 5];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
        ];
        let summary = detect_orphan_dofs(6, &elements);
        let s = format!("{}", summary);

        // Single line.
        assert!(!s.contains('\n'), "Display must not contain newlines; got: {s:?}");

        // Must name the count.
        assert!(
            s.contains("count=9"),
            "Display must contain 'count=9'; got: {s:?}",
        );

        // Must contain the first three canonical (node, axis) pairs.
        assert!(
            s.contains("(1, 3)"),
            "Display must contain '(1, 3)'; got: {s:?}",
        );
        assert!(
            s.contains("(1, 4)"),
            "Display must contain '(1, 4)'; got: {s:?}",
        );
        assert!(
            s.contains("(1, 5)"),
            "Display must contain '(1, 5)'; got: {s:?}",
        );

        // Empty summary is also well-defined.
        let empty = OrphanDofsSummary::default();
        let se = format!("{}", empty);
        assert!(!se.contains('\n'), "empty Display must not contain newlines");
        assert!(
            se.contains("count=0"),
            "empty Display must contain 'count=0'; got: {se:?}",
        );
    }

    /// `Display` for `OrphanDofsSummary` in the truncated regime
    /// (`examples.len() < count`) lists all stored entries verbatim — no
    /// `...` ellipsis — followed by a trailing parenthetical.
    ///
    /// Pins:
    /// 1. Single line: `!s.contains('\n')`.
    /// 2. Names the true (untruncated) count: `s.contains("count=24")`.
    /// 3. Every one of the 16 stored `(node, axis)` pairs appears literally as
    ///    a substring in the formatted output.
    /// 4. Trailing parenthetical: `s.ends_with("] (first 16 of 24)")`.
    #[test]
    fn orphan_dofs_summary_display_truncated_form_lists_all_stored_examples_verbatim() {
        // Truncating fixture: shell on [0,9,10] + two P1 tets on [1,2,3,4] and
        // [5,6,7,8], n_nodes=11. D=6 (shell). Tet-only nodes {1..=8}: 8 nodes ×
        // 3 orphan axes {3,4,5} = 24 total, capped to MAX_EXAMPLES=16 stored.
        let mat = dimensionless_steel_like();
        let k_e_tet1 = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_tet2 = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);

        let n_nodes = 11;
        let conn_shell = [0usize, 9, 10];
        let conn_tet1 = [1usize, 2, 3, 4];
        let conn_tet2 = [5usize, 6, 7, 8];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_tet1,
                k_e: &k_e_tet1,
            },
            AssemblyElement {
                id: 2,
                connectivity: &conn_tet2,
                k_e: &k_e_tet2,
            },
        ];
        let summary = detect_orphan_dofs(n_nodes, &elements);
        assert_eq!(summary.count, 24, "fixture must produce 24 orphans");
        assert_eq!(summary.examples.len(), 16, "fixture must be truncated to 16");

        let s = format!("{}", summary);

        // Pin 1: single line.
        assert!(!s.contains('\n'), "Display must not contain newlines; got: {s:?}");

        // Pin 2: names the true (untruncated) count.
        assert!(s.contains("count=24"), "Display must contain 'count=24'; got: {s:?}");

        // Pin 3: every one of the 16 stored (node, axis) pairs appears literally.
        // First 16 sorted pairs: nodes 1..=5 fully (3 axes each = 15 entries)
        // + node 6 axis 3 (16th entry).
        let expected_first_16: Vec<(usize, usize)> = (1usize..=6)
            .flat_map(|n| {
                if n < 6 {
                    (3usize..6).map(|a| (n, a)).collect::<Vec<_>>()
                } else {
                    vec![(n, 3)]
                }
            })
            .collect();
        for (node, axis) in &expected_first_16 {
            let pair = format!("({node}, {axis})");
            assert!(
                s.contains(&pair),
                "Display must contain '{pair}'; got: {s:?}",
            );
        }

        // Pin 4: trailing parenthetical indicates truncation with exact counts.
        assert!(
            s.ends_with("] (first 16 of 24)"),
            "Display must end with '] (first 16 of 24)'; got: {s:?}",
        );
    }

    /// Mesh with > MAX_EXAMPLES orphan pairs → count is the true total but
    /// examples is capped at MAX_EXAMPLES.
    ///
    /// Fixture: one shell on `[0, n_nodes-2, n_nodes-1]` + 2 disjoint P1 tets
    /// on `[1,2,3,4]` and `[5,6,7,8]`. D=6 (shell dominates). Tet-only nodes
    /// are 1..=8 minus the shell nodes (n_nodes-2, n_nodes-1 are shell-only,
    /// node 0 is shared). So tet-only = {1,2,3,4,5,6,7,8}: 8 nodes × 3 orphan
    /// axes {3,4,5} = 24 orphans > MAX_EXAMPLES=16.
    ///
    /// Asserts `count == 24`, `examples.len() == 16`, and that examples == first
    /// 16 entries of the full sorted list `[(1,3),(1,4),(1,5),(2,3),...,(6,3)]`.
    #[test]
    fn detect_orphan_dofs_caps_examples_at_max_examples_constant() {
        let mat = dimensionless_steel_like();
        let k_e_tet1 = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_tet2 = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);

        // n_nodes: 0..=8 for 9 nodes (0=shared, 1..=4=tet1-only,
        // 5..=8=tet2-only; node 7 and 8 are shell-only → but
        // we want shell_conn=[0,7,8], tet1=[1,2,3,4], tet2=[5,6,7,8])
        // Actually let's make it cleaner: shell on [0,9,10], tet1 on [1,2,3,4],
        // tet2 on [5,6,7,8]. That gives tet-only={1..8}: 8 nodes × 3 axes = 24.
        let n_nodes = 11;
        let conn_shell = [0usize, 9, 10];
        let conn_tet1 = [1usize, 2, 3, 4];
        let conn_tet2 = [5usize, 6, 7, 8];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_tet1,
                k_e: &k_e_tet1,
            },
            AssemblyElement {
                id: 2,
                connectivity: &conn_tet2,
                k_e: &k_e_tet2,
            },
        ];
        let summary = detect_orphan_dofs(n_nodes, &elements);

        // 8 tet-only nodes × 3 orphan axes = 24 total.
        assert_eq!(summary.count, 24, "true total should be 24");
        // examples capped at MAX_EXAMPLES=16.
        assert_eq!(summary.examples.len(), 16, "examples should be capped at 16");

        // First 16 sorted (node, axis) pairs: nodes 1..=5 fully represented
        // plus nodes 6 axis 3 (16th entry).
        let expected_first_16: Vec<(usize, usize)> = (1usize..=6)
            .flat_map(|n| {
                if n < 6 {
                    // nodes 1..5: all 3 axes (3,4,5) → 5 nodes × 3 = 15 entries
                    (3usize..6).map(|a| (n, a)).collect::<Vec<_>>()
                } else {
                    // node 6: only axis 3 (the 16th entry)
                    vec![(n, 3)]
                }
            })
            .collect();
        assert_eq!(
            summary.examples,
            expected_first_16,
            "examples should be the first 16 (node,axis) pairs sorted ascending",
        );
    }

    /// Mixed tet+shell mesh → tet-only nodes 1,2,3 report 3 orphan rotation
    /// axes each; node 0 (shared) and shell-only nodes 4,5 have no orphans.
    ///
    /// Fixture: P1 tet on `[0,1,2,3]` + MITC3 shell on `[0,4,5]`, n_nodes=6.
    /// D=6 (shell dominates). Nodes 1,2,3 are tet-only (d_e_max_local=3 < 6),
    /// contributing 3 orphan axes each → count=9. Node 0 is shared (d_e_max_local=6=D,
    /// no orphans). Nodes 4,5 are shell-only (d_e_max_local=6=D, no orphans).
    ///
    /// Also asserts the sorted `examples` list equals the first-9 canonical
    /// `(node, axis)` pairs.
    #[test]
    fn detect_orphan_dofs_mixed_tet_shell_reports_tet_only_node_rotation_dofs() {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        let conn_tet = [0usize, 1, 2, 3];
        let conn_shell = [0usize, 4, 5];
        let elements = [
            AssemblyElement {
                id: 0,
                connectivity: &conn_tet,
                k_e: &k_e_tet,
            },
            AssemblyElement {
                id: 1,
                connectivity: &conn_shell,
                k_e: &k_e_shell,
            },
        ];
        let summary = detect_orphan_dofs(6, &elements);

        // Nodes 1,2,3 are tet-only (d_e_max_local=3 < D=6), each contributing
        // 3 orphan axes {3,4,5} → total 9.
        assert_eq!(summary.count, 9, "expected 9 orphan (node,axis) pairs");

        // Full example list (9 < MAX_EXAMPLES=16, so no truncation).
        let expected_examples = vec![
            (1, 3),
            (1, 4),
            (1, 5),
            (2, 3),
            (2, 4),
            (2, 5),
            (3, 3),
            (3, 4),
            (3, 5),
        ];
        assert_eq!(
            summary.examples,
            expected_examples,
            "examples should list all 9 orphan (node,axis) pairs sorted by (node, axis)",
        );
    }

    // ─── prereq-2: shared-DOF box-mesh fixture ────────────────────────────────

    /// Split a hex cell into 6 P1 tets via the Kuhn triangulation.
    ///
    /// Identical to the helper in `tests/determinism.rs` (copied per the
    /// established per-test-file pattern). All 6 tets share the main diagonal
    /// from `c[0]` to `c[6]`.
    ///
    /// Corner ordering:
    /// ```text
    /// c[0]=(ix,iy,iz)     c[4]=(ix,iy,iz+1)
    /// c[1]=(ix+1,iy,iz)   c[5]=(ix+1,iy,iz+1)
    /// c[2]=(ix+1,iy+1,iz) c[6]=(ix+1,iy+1,iz+1)
    /// c[3]=(ix,iy+1,iz)   c[7]=(ix,iy+1,iz+1)
    /// ```
    fn kuhn_split_hex_to_six_tets_fixture(c: [usize; 8]) -> [[usize; 4]; 6] {
        [
            [c[0], c[1], c[2], c[6]], // σ=(x,y,z)
            [c[0], c[1], c[5], c[6]], // σ=(x,z,y)
            [c[0], c[3], c[2], c[6]], // σ=(y,x,z)
            [c[0], c[3], c[7], c[6]], // σ=(y,z,x)
            [c[0], c[4], c[5], c[6]], // σ=(z,x,y)
            [c[0], c[4], c[7], c[6]], // σ=(z,y,x)
        ]
    }

    /// Build a `[0,Lx]×[0,Ly]×[0,Lz]` structured P1 tet mesh with
    /// `nx×ny×nz` hex cells, each Kuhn-split into 6 tets.
    ///
    /// Node indexing: `iz*(ny+1)*(nx+1) + iy*(nx+1) + ix`.
    /// Identical to `box_p1_mesh` in `tests/determinism.rs`.
    fn box_p1_mesh_fixture(
        lx: f64,
        ly: f64,
        lz: f64,
        nx: usize,
        ny: usize,
        nz: usize,
    ) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let nnx = nx + 1;
        let nny = ny + 1;
        let nnz = nz + 1;

        let mut nodes = Vec::with_capacity(nnx * nny * nnz);
        for iz in 0..nnz {
            for iy in 0..nny {
                for ix in 0..nnx {
                    nodes.push([
                        ix as f64 * lx / nx as f64,
                        iy as f64 * ly / ny as f64,
                        iz as f64 * lz / nz as f64,
                    ]);
                }
            }
        }

        let node_idx =
            |ix: usize, iy: usize, iz: usize| iz * nny * nnx + iy * nnx + ix;

        let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
        for iz in 0..nz {
            for iy in 0..ny {
                for ix in 0..nx {
                    let c = [
                        node_idx(ix, iy, iz),
                        node_idx(ix + 1, iy, iz),
                        node_idx(ix + 1, iy + 1, iz),
                        node_idx(ix, iy + 1, iz),
                        node_idx(ix, iy, iz + 1),
                        node_idx(ix + 1, iy, iz + 1),
                        node_idx(ix + 1, iy + 1, iz + 1),
                        node_idx(ix, iy + 1, iz + 1),
                    ];
                    for tet in kuhn_split_hex_to_six_tets_fixture(c) {
                        connectivity.push(tet);
                    }
                }
            }
        }

        (nodes, connectivity)
    }

    /// Build the shared-DOF 3×3×3 box-mesh fixture for assembly-determinism
    /// unit tests.
    ///
    /// Geometry: `[0,1]×[0,1]×[0,1]`, `3×3×3` hex cells →
    /// `4×4×4 = 64` nodes, `6×27 = 162` P1 tet elements.
    ///
    /// Total triplets: `162 × 144 = 23 328`. Interior nodes (e.g. node at
    /// position `(1/3, 1/3, 1/3)`) are shared by up to 8 hex cells × 6 tets
    /// = 48 elements, so the most-shared `(row, col)` pairs in the assembled
    /// K receive up to 48 duplicate contributions — well into faer's
    /// large-array argsort / quicksort regime (above the ~20-element
    /// insertion-sort threshold). Element geometries vary across the mesh
    /// (the 6 Kuhn-split tets per hex cell have distinct shapes), so
    /// contributions to shared `(row, col)` pairs carry distinct values —
    /// FP non-associativity from different summation orders produces
    /// observable K differences.
    ///
    /// Returns `(n_nodes, ke_list, conns)`.
    fn build_shared_dof_box_3x3x3(
        mat: &IsotropicElastic,
    ) -> (usize, Vec<ElementStiffness>, Vec<[usize; 4]>) {
        let (nodes, conns) = box_p1_mesh_fixture(1.0, 1.0, 1.0, 3, 3, 3);
        let n_nodes = nodes.len(); // 4×4×4 = 64
        assert_eq!(conns.len(), 6 * 27, "expected 162 elements for 3×3×3 hex mesh");

        let ke_list: Vec<ElementStiffness> = conns
            .iter()
            .map(|conn| {
                let phys: [[f64; 3]; 4] = [
                    nodes[conn[0]],
                    nodes[conn[1]],
                    nodes[conn[2]],
                    nodes[conn[3]],
                ];
                element_stiffness_p1(&phys, mat)
            })
            .collect();

        (n_nodes, ke_list, conns)
    }

    // ─── step-1 (RED): assembly-level determinism guards ─────────────────────

    /// 32 sequential `AssemblyMode::Deterministic` assemblies of the shared-DOF
    /// 3×3×3 box mesh produce bit-identical K (stored values AND sparsity
    /// pattern).
    ///
    /// **Before the BTreeMap fix** (step-2): faer's `sort_unstable` is
    /// deterministic for the same in-memory input in sequential execution, so
    /// this test will PASS even without the fix.  Under concurrent CPU load,
    /// however, different heap-allocation patterns change the physical layout
    /// of the triplet `Vec`, which can cause `sort_unstable` to produce
    /// different orderings for equal-key `(row, col)` triplets → different K
    /// matrices → diverging CG iteration counts (the observed harness flake).
    ///
    /// **After the BTreeMap fix**: K is a pure reify-owned left-fold over
    /// elements in encounter order, guaranteed bit-identical across ANY
    /// execution environment (loaded or not, any thread count, any OS scheduler
    /// decision). The test encodes this cross-environment invariant.
    ///
    /// Non-trivial-K sanity check: asserts at least one stored value is
    /// non-zero, so a degenerate zero-K can't make the test pass spuriously.
    #[test]
    fn deterministic_assembly_bit_identical_across_repeats() {
        let mat = dimensionless_steel_like();
        let (n_nodes, ke_list, conns) = build_shared_dof_box_3x3x3(&mat);

        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .zip(ke_list.iter())
            .enumerate()
            .map(|(i, (conn, ke))| AssemblyElement {
                id: i,
                connectivity: conn.as_slice(),
                k_e: ke,
            })
            .collect();

        // Reference assembly (repeat 0).
        let reference =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        let (ref_sym, ref_vals) = reference.parts();

        // Sanity: K is non-trivial.
        assert!(
            ref_vals.iter().any(|&v| v != 0.0),
            "reference K is all-zero — fixture may be degenerate",
        );

        // 31 further repeats must be bit-identical.
        for repeat in 1..32_usize {
            let k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
            let (sym, vals) = k.parts();

            // Sparsity pattern.
            assert_eq!(
                sym.row_ptr(),
                ref_sym.row_ptr(),
                "repeat {repeat}: row_ptr differs",
            );
            assert_eq!(
                sym.col_idx(),
                ref_sym.col_idx(),
                "repeat {repeat}: col_idx differs",
            );

            // Stored values must be bit-identical.
            assert_eq!(
                vals.len(),
                ref_vals.len(),
                "repeat {repeat}: nnz count differs",
            );
            for (j, (&a, &b)) in vals.iter().zip(ref_vals.iter()).enumerate() {
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "repeat {repeat}: vals[{j}] = {a} differs from reference {b}",
                );
            }
        }
    }

    /// `AssemblyMode::Deterministic` on the shared-DOF 3×3×3 box mesh produces
    /// K bit-identical to the encounter-order BTreeMap reference at every
    /// `(row, col)` entry.
    ///
    /// **Reference construction**: independently accumulate the same elements
    /// into a `BTreeMap<(usize,usize),f64>` in slice/encounter order,
    /// mirroring `emit_element_triplets`' `(a,α,b,β)` emission —
    /// `reference[(row,col)] = Σ K_e[a*d+α][b*d+β]` summed over all elements
    /// contributing to `(row, col)` in the order they appear in the element
    /// slice. This is the reify-owned left-fold that the BTreeMap fix
    /// guarantees.
    ///
    /// **Before the BTreeMap fix** (step-2): faer's `sort_unstable` on 23 328
    /// triplets operates in the large-array quicksort regime (NOT insertion
    /// sort), which is **not stable** — equal-key triplets may land in any
    /// relative order. For the 3×3×3 mesh, interior nodes receive up to 48
    /// contributions per `(row,col)` pair; if quicksort reorders them
    /// differently from encounter order, the resulting K[i][j] sum will differ
    /// from the reference due to FP non-associativity (distinct element
    /// geometries → distinct contribution values → order-dependent sums).
    ///
    /// Expected: **FAIL** with a `to_bits()` mismatch at a shared-DOF entry
    /// (confirms faer's sort does not preserve encounter order for large meshes,
    /// establishing the RED baseline for the BTreeMap fix).
    ///
    /// **After the BTreeMap fix**: K_det IS the encounter-order BTreeMap fold,
    /// so this assertion is guaranteed to pass bit-for-bit.
    #[test]
    fn deterministic_assembly_matches_encounter_order_reference() {
        let mat = dimensionless_steel_like();
        let (n_nodes, ke_list, conns) = build_shared_dof_box_3x3x3(&mat);

        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .zip(ke_list.iter())
            .enumerate()
            .map(|(i, (conn, ke))| AssemblyElement {
                id: i,
                connectivity: conn.as_slice(),
                k_e: ke,
            })
            .collect();

        // Assemble via the Deterministic path (currently delegates dedup to faer).
        let k_det =
            assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

        // Build the encounter-order BTreeMap reference.
        // Uses emit_element_triplets (the production scatter primitive) to
        // ensure the reference mirrors exactly the same (a,α,b,β) emission
        // order, then folds contributions in slice order.
        let n_dofs_per_node = 3usize; // pure P1 tet mesh → D = 3
        let mut reference: std::collections::BTreeMap<(usize, usize), f64> =
            std::collections::BTreeMap::new();
        let mut tmp: Vec<Triplet<usize, usize, f64>> = Vec::new();
        for element in &elements {
            tmp.clear();
            emit_element_triplets(element, n_dofs_per_node, &mut tmp);
            for t in &tmp {
                *reference.entry((t.row, t.col)).or_insert(0.0) += t.val;
            }
        }

        // Sanity: reference is non-trivial.
        assert!(
            reference.values().any(|&v| v != 0.0),
            "encounter-order reference is all-zero — fixture may be degenerate",
        );

        // Every (row, col) in the reference must match K_det bit-for-bit.
        // A mismatch at any shared-DOF entry means faer's sort reordered equal-
        // key triplets differently from encounter order — the RED confirmation.
        let dim = 3 * n_nodes;
        for row in 0..dim {
            for col in 0..dim {
                let ref_val = reference.get(&(row, col)).copied().unwrap_or(0.0);
                let det_val = read(&k_det, row, col);
                assert_eq!(
                    det_val.to_bits(),
                    ref_val.to_bits(),
                    "K_det[{row}][{col}] = {det_val} differs from encounter-order \
                     reference = {ref_val}; \
                     faer sort reordered equal-key triplets at this shared DOF",
                );
            }
        }
    }
}
