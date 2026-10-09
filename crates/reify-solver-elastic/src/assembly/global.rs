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
//! dominates). Assembly is pattern-first and node-owned
//! ([`assemble_stiffness`]; PRD `docs/prds/v0_6/elastic-static-solver-performance.md`
//! §5 C12): each node builds its own rows of a canonical faer CSR matrix.

use std::num::NonZeroUsize;
use std::ops::{ControlFlow, Range};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use faer::sparse::{SparseRowMat, SymbolicSparseRowMat};

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
/// name the offending element in a contract violation) and is never a sort
/// key: the slice order alone fixes the order in which shared entries sum
/// (see [`assemble_stiffness`]).
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

/// Thread count for [`assemble_global_stiffness`]: `Deterministic` is one
/// thread, `Parallel { threads }` is `threads`. Both yield the same bits — the
/// mode sets parallelism only.
///
/// A compatibility shim: ξ (#8375) moves the remaining callers to
/// [`assemble_stiffness`], whose `NonZeroUsize` thread count cannot express
/// zero, and deletes this enum with the wrapper.
///
/// `Parallel { threads: 0 }` panics rather than falling back to one thread,
/// which would silently mask caller bugs (e.g. a misread config defaulting
/// `threads` to 0). The "tiny problems run single-threaded" policy lives in
/// the `ElasticOptions` resolution layer (PRD task #16), not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyMode {
    /// One thread.
    Deterministic,
    /// `threads` threads, the calling thread included. `threads` must be
    /// `>= 1`; passing `0` panics.
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
/// by [`assemble_stiffness`] to derive the global `D`). The global
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
/// [`assemble_stiffness`] calls this function internally under
/// `#[cfg(debug_assertions)]` and emits an `eprintln!` warning if
/// `count > 0`, so a forgotten BC/MPC stabilisation surfaces at assembly
/// rather than as a silently singular `K` in the linear solve.
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
/// [`assemble_stiffness`] — callers should validate the mesh before
/// calling this function as a pre-assembly check.
///
/// In debug builds, two `debug_assert!`s enforce these contracts eagerly:
/// one on non-empty connectivity and one on all node indices being in-bounds.
// G-allow: task #3293 (done) orphan-DOF detector; cfg(debug_assertions) emit consumer + detector/assembler-consistency pin (task #3293, done)
pub fn detect_orphan_dofs(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
) -> OrphanDofsSummary {
    if elements.is_empty() {
        return OrphanDofsSummary::default();
    }

    // Validate input contracts (matches assemble_stiffness's implicit
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
    // formula in assemble_stiffness. Derived inline with the per-node
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

/// Assemble the global stiffness `K` — `D·n_nodes` square, canonical CSR —
/// row by row: each node builds its own rows from the elements incident on it.
///
/// - `D = max(k_e.n_dofs / connectivity.len())` over `elements`, or `3` when
///   `elements` is empty. An element's local DOF `d_e·a + α` maps to global
///   DOF `D·connectivity[a] + α`, where `d_e = k_e.n_dofs / connectivity.len()`.
/// - Pattern: row `D·n + α` stores column `D·m + β` exactly when some element
///   containing both `n` and `m` has `d_e > max(α, β)`. Rows no element covers
///   — the orphan rotation rows of tet-only nodes in a mixed tet+shell mesh,
///   and every row of an untouched node — store nothing.
/// - Values: each stored entry is `+0.0` plus its contributions in
///   element-slice order, then local `(a, b)` order. That is the same entries
///   and the same bits as folding each element's `(a, α, b, β)` block into `K`
///   in slice order.
/// - The result is bitwise identical for every `threads`.
///
/// Rationale: PRD `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C12
/// and decision S6.
///
/// # Threads and cancellation
///
/// Rows are built in chunks of whole nodes, claimed from a shared queue by
/// `threads` threads; the calling thread is one of them, so `threads = 1`
/// spawns none.
/// - `poll` runs only on the calling thread, before each chunk it tries to
///   claim, and never before the contract checks below.
/// - Once `poll` returns `Break` it is not called again: chunks in progress
///   finish, no further chunk starts, and `Break(())` is returned.
/// - A panic on a spawned thread reaches the caller with its original payload.
///
/// # Panics
///
/// Before any work, if an element has empty connectivity, a `k_e.n_dofs` not
/// divisible by `connectivity.len()`, zero DOFs per node, or a node
/// `>= n_nodes`. Each message names the element id.
pub fn assemble_stiffness(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
    threads: NonZeroUsize,
    poll: &mut dyn FnMut() -> ControlFlow<()>,
) -> ControlFlow<(), SparseRowMat<usize, f64>> {
    let d = check_elements_and_derive_dofs_per_node(n_nodes, elements);
    // Zero release-mode cost; eprintln! because this crate has no logging
    // dependency (task 3293).
    #[cfg(debug_assertions)]
    {
        let orphans = detect_orphan_dofs(n_nodes, elements);
        if orphans.count > 0 {
            eprintln!(
                "[reify-solver-elastic] assemble_stiffness: {orphans}; \
                 apply Dirichlet BC (task 2917) or MPC tying (task 3020) to \
                 stabilise these DOFs before solving",
            );
        }
    }
    let rows = NodeOwnedRows::new(n_nodes, elements, d);
    let dim = d * n_nodes;

    let mut row_ptr = vec![0usize; dim + 1];
    let count_chunks: Vec<_> = node_chunks(n_nodes)
        .zip(row_ptr[1..].chunks_mut(d * NODES_PER_CHUNK))
        .collect();
    run_chunks(count_chunks, threads, poll, |(nodes, row_len)| {
        rows.count(nodes, row_len, &mut RowScratch::default());
    })?;
    for row in 0..dim {
        row_ptr[row + 1] += row_ptr[row];
    }

    let nnz = row_ptr[dim];
    let mut col_idx = vec![0usize; nnz];
    let mut val = vec![0.0; nnz];
    let mut fill_chunks = Vec::with_capacity(n_nodes.div_ceil(NODES_PER_CHUNK));
    let (mut cols_rest, mut vals_rest) = (&mut col_idx[..], &mut val[..]);
    for nodes in node_chunks(n_nodes) {
        let len = row_ptr[d * nodes.end] - row_ptr[d * nodes.start];
        let (cols, rest) = std::mem::take(&mut cols_rest).split_at_mut(len);
        cols_rest = rest;
        let (vals, rest) = std::mem::take(&mut vals_rest).split_at_mut(len);
        vals_rest = rest;
        fill_chunks.push((nodes, cols, vals));
    }
    run_chunks(fill_chunks, threads, poll, |(nodes, cols, vals)| {
        rows.fill(nodes, &row_ptr, cols, vals, &mut RowScratch::default());
    })?;

    ControlFlow::Continue(SparseRowMat::new(
        SymbolicSparseRowMat::new_checked(dim, dim, row_ptr, None, col_idx),
        val,
    ))
}

/// Nodes per work chunk: the unit of parallel work, and of cancellation
/// latency (one poll per chunk).
const NODES_PER_CHUNK: usize = 64;

/// `0..n_nodes` in consecutive ranges of [`NODES_PER_CHUNK`] nodes.
fn node_chunks(n_nodes: usize) -> impl Iterator<Item = Range<usize>> {
    (0..n_nodes)
        .step_by(NODES_PER_CHUNK)
        .map(move |first| first..(first + NODES_PER_CHUNK).min(n_nodes))
}

/// Run `work` on every chunk using `threads` threads, the calling thread
/// included; chunks are claimed in order from a shared queue. Only the calling
/// thread polls, before each attempt to claim. On `Break` no further chunk
/// starts, chunks in progress finish, and `Break(())` is returned. A worker's
/// panic is re-raised on the caller with its original payload.
fn run_chunks<C: Send>(
    chunks: Vec<C>,
    threads: NonZeroUsize,
    poll: &mut dyn FnMut() -> ControlFlow<()>,
    work: impl Fn(C) + Sync,
) -> ControlFlow<()> {
    let n_workers = threads.get().min(chunks.len()).saturating_sub(1);
    let queue = Mutex::new(chunks.into_iter());
    let claim = || queue.lock().unwrap_or_else(PoisonError::into_inner).next();
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..n_workers)
            .map(|_| {
                scope.spawn(|| {
                    while !cancelled.load(Ordering::Relaxed) {
                        let Some(chunk) = claim() else { break };
                        work(chunk);
                    }
                })
            })
            .collect();
        let outcome = loop {
            if poll().is_break() {
                cancelled.store(true, Ordering::Relaxed);
                break ControlFlow::Break(());
            }
            let Some(chunk) = claim() else {
                break ControlFlow::Continue(());
            };
            work(chunk);
        };
        for worker in workers {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
        outcome
    })
}

/// For each node, its `(element index, local index)` incidences in ascending
/// order — one entry per occurrence, so an element repeating a node appears
/// once per local index.
struct NodeIncidence {
    ptr: Vec<usize>,
    entries: Vec<(usize, usize)>,
}

impl NodeIncidence {
    /// Counting sort over `(element, local index)`; `elements` must already
    /// satisfy the node-range contract.
    fn new(n_nodes: usize, elements: &[AssemblyElement<'_>]) -> Self {
        let mut ptr = vec![0usize; n_nodes + 1];
        for element in elements {
            for &node in element.connectivity {
                ptr[node + 1] += 1;
            }
        }
        for node in 0..n_nodes {
            ptr[node + 1] += ptr[node];
        }
        let mut cursor = ptr[..n_nodes].to_vec();
        let mut entries = vec![(0, 0); ptr[n_nodes]];
        for (e, element) in elements.iter().enumerate() {
            for (a, &node) in element.connectivity.iter().enumerate() {
                entries[cursor[node]] = (e, a);
                cursor[node] += 1;
            }
        }
        Self { ptr, entries }
    }

    fn of(&self, node: usize) -> &[(usize, usize)] {
        &self.entries[self.ptr[node]..self.ptr[node + 1]]
    }
}

/// Per-node working buffers, reused across the nodes of one range.
#[derive(Default)]
struct RowScratch {
    /// `(neighbour node m, width w(n, m))`, ascending in `m`.
    neighbours: Vec<(usize, usize)>,
    /// `offsets[α·K + k]`: where neighbour `k`'s block starts within row
    /// `(n, α)`, for the `K` neighbours with `w > α`.
    offsets: Vec<usize>,
    /// Neighbour index of each local node of the element being scattered.
    local_to_neighbour: Vec<usize>,
}

/// The node-owned row builder behind [`assemble_stiffness`]. Every method
/// takes a node range and touches only that range's rows, so ranges are
/// independent of each other.
struct NodeOwnedRows<'s, 'e> {
    elements: &'s [AssemblyElement<'e>],
    incidence: NodeIncidence,
    /// Global DOFs per node `D`.
    d: usize,
}

impl<'s, 'e> NodeOwnedRows<'s, 'e> {
    fn new(n_nodes: usize, elements: &'s [AssemblyElement<'e>], d: usize) -> Self {
        Self {
            elements,
            incidence: NodeIncidence::new(n_nodes, elements),
            d,
        }
    }

    fn dofs_per_node(element: &AssemblyElement<'_>) -> usize {
        element.k_e.n_dofs / element.connectivity.len()
    }

    /// Sorted `(m, w(n, m))` over the nodes `m` sharing an element with
    /// `node`, where `w` is the largest `d_e` of the elements they share.
    fn neighbours(&self, node: usize, out: &mut Vec<(usize, usize)>) {
        out.clear();
        for &(e, _) in self.incidence.of(node) {
            let element = &self.elements[e];
            let d_e = Self::dofs_per_node(element);
            out.extend(element.connectivity.iter().map(|&m| (m, d_e)));
        }
        out.sort_unstable();
        out.dedup_by(|later, kept| {
            let same_node = later.0 == kept.0;
            if same_node {
                kept.1 = kept.1.max(later.1);
            }
            same_node
        });
    }

    /// Write the stored-entry count of every row of `nodes` into `row_len`,
    /// which holds exactly those `D·nodes.len()` rows.
    fn count(&self, nodes: Range<usize>, row_len: &mut [usize], scratch: &mut RowScratch) {
        let d = self.d;
        for (node, lens) in nodes.zip(row_len.chunks_exact_mut(d)) {
            self.neighbours(node, &mut scratch.neighbours);
            for (alpha, len) in lens.iter_mut().enumerate() {
                *len = scratch
                    .neighbours
                    .iter()
                    .filter(|&&(_, w)| w > alpha)
                    .map(|&(_, w)| w)
                    .sum();
            }
        }
    }

    /// Write the column indices and summed values of every row of `nodes`.
    /// `col_idx`/`val` hold exactly those rows' entries, starting at stored
    /// index `row_ptr[D·nodes.start]`; `row_ptr` is the full prefix sum.
    fn fill(
        &self,
        nodes: Range<usize>,
        row_ptr: &[usize],
        col_idx: &mut [usize],
        val: &mut [f64],
        scratch: &mut RowScratch,
    ) {
        let d = self.d;
        let base = row_ptr[d * nodes.start];
        for node in nodes {
            self.neighbours(node, &mut scratch.neighbours);
            let neighbours = &scratch.neighbours;
            let k_count = neighbours.len();
            let row_start = |alpha: usize| row_ptr[d * node + alpha] - base;

            scratch.offsets.clear();
            scratch.offsets.resize(d * k_count, 0);
            for alpha in 0..d {
                let start = row_start(alpha);
                let mut offset = 0;
                for (k, &(m, w)) in neighbours.iter().enumerate() {
                    if w > alpha {
                        scratch.offsets[alpha * k_count + k] = offset;
                        for (beta, col) in col_idx[start + offset..start + offset + w]
                            .iter_mut()
                            .enumerate()
                        {
                            *col = d * m + beta;
                        }
                        offset += w;
                    }
                }
                debug_assert_eq!(start + offset, row_start(alpha + 1), "row length mismatch");
            }

            for &(e, a) in self.incidence.of(node) {
                let element = &self.elements[e];
                let d_e = Self::dofs_per_node(element);
                scratch.local_to_neighbour.clear();
                scratch
                    .local_to_neighbour
                    .extend(element.connectivity.iter().map(|&m| {
                        neighbours
                            .binary_search_by_key(&m, |&(m, _)| m)
                            .expect("every node of an incident element is a neighbour")
                    }));
                for alpha in 0..d_e {
                    let start = row_start(alpha);
                    for (b, &k) in scratch.local_to_neighbour.iter().enumerate() {
                        let dst = start + scratch.offsets[alpha * k_count + k];
                        for beta in 0..d_e {
                            val[dst + beta] += element.k_e.get(d_e * a + alpha, d_e * b + beta);
                        }
                    }
                }
            }
        }
    }
}

/// Assemble the global stiffness `K` with [`assemble_stiffness`], which owns
/// the contract: shape, pattern, summation order and panics.
/// `AssemblyMode` only chooses the thread count — `Deterministic` is one
/// thread, `Parallel { threads }` is `threads` — so both give the same bits.
///
/// # Panics
///
/// - `AssemblyMode::Parallel { threads: 0 }`, checked first, even for an
///   empty `elements`: auto-fallback to single-threaded would silently mask
///   caller bugs (e.g. a misread config defaulting `threads` to 0). The
///   "tiny problems run single-threaded" policy lives in the `ElasticOptions`
///   resolution layer (PRD task #16), not here.
/// - Every contract panic of [`assemble_stiffness`].
pub fn assemble_global_stiffness(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
    mode: AssemblyMode,
) -> SparseRowMat<usize, f64> {
    let threads = match mode {
        AssemblyMode::Deterministic => NonZeroUsize::MIN,
        AssemblyMode::Parallel { threads } => NonZeroUsize::new(threads).unwrap_or_else(|| {
            panic!(
                "AssemblyMode::Parallel {{ threads: 0 }} is invalid: \
                 auto-fallback to single-threaded would silently mask \
                 caller bugs (e.g. a misread config defaulting threads to 0). \
                 Pass threads >= 1, or use AssemblyMode::Deterministic for \
                 single-threaded slice-order accumulation.",
            )
        }),
    };
    match assemble_stiffness(
        n_nodes,
        elements,
        threads,
        &mut || ControlFlow::Continue(()),
    ) {
        ControlFlow::Continue(k) => k,
        ControlFlow::Break(()) => unreachable!("a never-breaking poll cannot cancel assembly"),
    }
}

/// Enforce the per-element contract of [`assemble_stiffness`] and return the
/// global DOFs-per-node `D = max(k_e.n_dofs / connectivity.len())`, or `3`
/// for an empty slice (the v0.3 tet-only `3N × 3N` shape).
///
/// Unconditional `assert!` (not `debug_assert!`) per the project's Task-2544
/// contract-explicitness convention, mirrored in
/// `assembly/mod.rs::element_stiffness` and
/// `elements/mod.rs::ReferenceElement::jacobian`.
fn check_elements_and_derive_dofs_per_node(
    n_nodes: usize,
    elements: &[AssemblyElement<'_>],
) -> usize {
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
    elements
        .iter()
        .map(|e| e.k_e.n_dofs / e.connectivity.len())
        .max()
        .unwrap_or(3)
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
    /// Pins the empty-input contract through every `AssemblyMode` variant:
    /// the matrix is `3 * n_nodes` square (`D` defaults to 3) and stores
    /// nothing.
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
                "no elements ⇒ zero stored entries (mode = {mode:?})",
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
                        // for a single (a+b) pairing — and assembly sums
                        // contributions in element order, which here is
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

    /// Global K is symmetric within FP tolerance on the fan mesh from
    /// step-13.
    ///
    /// Per-element K_e is symmetric (pinned by Task 2915's tests, since
    /// `K_e = ∫ BᵀDB dV` with symmetric `D`). Assembly sums contributions
    /// in a fixed element order, so `K_global[i][j]` and `K_global[j][i]`
    /// are sums of mirror pairs of K_e entries. Every element scatters both
    /// `(a, b)` and `(b, a)` for every local pair, so the LSB of the sum order at `(i, j)` and `(j, i)` can
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
    /// re-substitutes the worker's message (e.g. `.expect(...)` on the join,
    /// a custom error type, or `unwrap_or_else(|_| panic!("..."))`) makes the
    /// "out of bounds" substring disappear and this test fail. Every element
    /// of a 137-tet chain shares a K_e with no data, so whichever of the
    /// eight threads fills a chunk first panics.
    #[test]
    #[should_panic(expected = "out of bounds")]
    fn worker_thread_panic_payload_propagates_to_caller() {
        let k_e = ElementStiffness {
            n_dofs: 12,
            data: vec![],
        };
        let conns: Vec<[usize; 4]> = (0..137).map(|i| [i, i + 1, i + 2, i + 3]).collect();
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(id, connectivity)| AssemblyElement {
                id,
                connectivity,
                k_e: &k_e,
            })
            .collect();
        let _ =
            assemble_global_stiffness(137 + 3, &elements, AssemblyMode::Parallel { threads: 8 });
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
    /// Pins that the D-agnostic scatter in `assemble_stiffness`
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
    /// Pins that the D-agnostic scatter handles the wedge's 6-node
    /// footprint (`d_e = 3`, `n_dofs = 18`) at identity connectivity.
    ///
    /// Pure-wedge mesh ⇒ `D = max(d_e) = 3`, so `K.nrows() == 18` and
    /// `K.ncols() == 18`. A regression that special-cases `n_local ∈ {4, 8, 10}`
    /// in the scatter's loop bounds would surface here.
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
    ///   shell touch translational DOFs. Contributions sum in element
    ///   order — tet first (it appears first in the slice), shell
    ///   second — so the bit-for-bit expected value is
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
        // shell (slice index 1); contributions sum left-to-right.
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
    ///   contributions sum left-to-right so the bit-for-bit expected value
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
        // a stale `local_a == 0` early-return in the scatter)
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
    /// Assembly sums each entry's contributions in fixed element order
    /// (PRD S6). The D-derived scatter takes the **full** `(a, α, b, β)` block
    /// for every element kind (not upper-triangle only), so
    /// `K_global[i][j]` and `K_global[j][i]` are sums of element-mirror
    /// pairs of entries coming from the same `K_e` matrices. The LSB
    /// of the element-order summation at `(i, j)` versus `(j, i)` can
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
    /// Each element scatters its full `(a, b)` block, both `(a, b)` and
    /// `(b, a)`, so `K_global[i][j]` and `K_global[j][i]` are sums of mirror
    /// pairs of the same K_e entries — the LSB of the element-order summation
    /// at `(i, j)` versus `(j, i)` can differ, but both reduce to the same
    /// value within the FP summation band.
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
    /// Interior nodes (e.g. node at
    /// position `(1/3, 1/3, 1/3)`) are shared by up to 8 hex cells × 6 tets
    /// = 48 elements, so the most-shared `(row, col)` pairs in the assembled
    /// K receive up to 48 contributions. Element geometries vary across the mesh
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

    // ─── assembly-level determinism guards ───────────────────────────────────

    /// 32 sequential `AssemblyMode::Deterministic` assemblies of the shared-DOF
    /// 3×3×3 box mesh produce bit-identical K (stored values AND sparsity
    /// pattern): K is a pure function of `(n_nodes, elements)`, whatever the
    /// load or scheduling.
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

    /// `AssemblyMode::Deterministic` on the shared-DOF 3×3×3 box mesh (up to
    /// 48 contributions per entry) is the encounter-order fold — same
    /// entries, same summation order, same bits (PRD S6).
    #[test]
    fn deterministic_assembly_matches_encounter_order_reference() {
        let mat = dimensionless_steel_like();
        let (n_nodes, ke_list, conns) = build_shared_dof_box_3x3x3(&mat);
        let elements = elements_of(&conns, &ke_list);

        let k_det = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);
        assert!(
            k_det.val().iter().any(|&v| v != 0.0),
            "K is all-zero — fixture may be degenerate",
        );
        assert_csr_bitwise_eq(
            &k_det,
            &encounter_order_reference(n_nodes, &elements),
            "Deterministic vs encounter-order reference",
        );
    }

    // ─── node-owned `assemble_stiffness` (PRD C12 / S6) ──────────────────────

    use crate::assembly::test_support::promote_tets_to_p2;

    /// splitmix64 finalizer — the constants of `boundary/dirichlet.rs`'s
    /// `mixed_sign_value`, so fixtures need no `rand` dependency.
    fn splitmix64(seed: u64) -> u64 {
        let mut h = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^ (h >> 31)
    }

    /// Deterministic pseudo-random value in `[-1, 1)`.
    fn signed_unit(seed: u64) -> f64 {
        (splitmix64(seed) >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }

    /// Fisher–Yates permutation of `0..n` driven by [`splitmix64`].
    fn splitmix_permutation(n: usize, seed: u64) -> Vec<usize> {
        let mut perm: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (splitmix64(seed ^ (i as u64).wrapping_mul(0xD1B5_4A32_D192_ED03))
                % (i as u64 + 1)) as usize;
            perm.swap(i, j);
        }
        perm
    }

    /// Kuhn-split `nx×ny×nz` box of unit cells whose node coordinates are
    /// each jittered by at most `0.1·h`, whose node ids are relabelled by a
    /// pseudo-random permutation, and whose element order is shuffled — so
    /// every node's incidence list is irregular in both size and order.
    fn unstructured_p1_box(
        nx: usize,
        ny: usize,
        nz: usize,
        seed: u64,
    ) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
        let (grid, conns) = box_p1_mesh_fixture(nx as f64, ny as f64, nz as f64, nx, ny, nz);
        let new_id = splitmix_permutation(grid.len(), seed);
        let mut nodes = vec![[0.0; 3]; grid.len()];
        for (old, p) in grid.iter().enumerate() {
            let jitter = |axis: u64| 0.1 * signed_unit(seed ^ ((4 * old as u64 + axis + 1) << 8));
            nodes[new_id[old]] = [p[0] + jitter(0), p[1] + jitter(1), p[2] + jitter(2)];
        }
        let order = splitmix_permutation(conns.len(), !seed);
        let shuffled = order
            .iter()
            .map(|&e| conns[e].map(|node| new_id[node]))
            .collect();
        (nodes, shuffled)
    }

    /// [`unstructured_p1_box`] promoted to 10-node P2 tets.
    fn unstructured_p2_box(
        nx: usize,
        ny: usize,
        nz: usize,
        seed: u64,
    ) -> (Vec<[f64; 3]>, Vec<[usize; 10]>) {
        let (nodes, conns) = unstructured_p1_box(nx, ny, nz, seed);
        promote_tets_to_p2(&nodes, &conns)
    }

    /// One `kernel` K_e per element of a single-family tet mesh.
    fn stiffnesses<const N: usize>(
        nodes: &[[f64; 3]],
        conns: &[[usize; N]],
        kernel: fn(&[[f64; 3]; N], &IsotropicElastic) -> ElementStiffness,
    ) -> Vec<ElementStiffness> {
        let mat = dimensionless_steel_like();
        conns
            .iter()
            .map(|conn| kernel(&conn.map(|node| nodes[node]), &mat))
            .collect()
    }

    /// Pair connectivities with their K_e, ids in slice order.
    fn elements_of<'a, C: AsRef<[usize]>>(
        conns: &'a [C],
        k_es: &'a [ElementStiffness],
    ) -> Vec<AssemblyElement<'a>> {
        assert_eq!(conns.len(), k_es.len(), "one K_e per connectivity");
        conns
            .iter()
            .zip(k_es)
            .enumerate()
            .map(|(id, (conn, k_e))| AssemblyElement {
                id,
                connectivity: conn.as_ref(),
                k_e,
            })
            .collect()
    }

    /// `copies` disjoint replicas of a P1 tet `[6k, 6k+1, 6k+2, 6k+3]`
    /// followed in the slice by a MITC3 shell `[6k, 6k+4, 6k+5]`. `D = 6`;
    /// nodes `6k+1..=6k+3` are tet-only, so their rotation rows are orphans.
    /// Returns `(n_nodes, conns, k_es)`.
    fn replicated_mixed_tet_shell(
        copies: usize,
    ) -> (usize, Vec<Vec<usize>>, Vec<ElementStiffness>) {
        let mat = dimensionless_steel_like();
        let k_e_tet = element_stiffness_p1(&UNIT_TET_P1, &mat);
        let k_e_shell = shell_element_stiffness(&UNIT_TRI, SHELL_T, &mat);
        let mut conns = Vec::with_capacity(2 * copies);
        let mut k_es = Vec::with_capacity(2 * copies);
        for k in 0..copies {
            conns.push(vec![6 * k, 6 * k + 1, 6 * k + 2, 6 * k + 3]);
            k_es.push(k_e_tet.clone());
            conns.push(vec![6 * k, 6 * k + 4, 6 * k + 5]);
            k_es.push(k_e_shell.clone());
        }
        (6 * copies, conns, k_es)
    }

    /// Dense `n_dofs × n_dofs` K_e whose entries span eight decades with
    /// either sign, so a reordered summation of shared entries changes bits.
    fn order_sensitive_stiffness(n_dofs: usize, seed: u64) -> ElementStiffness {
        let data = (0..n_dofs * n_dofs)
            .map(|i| {
                let s = seed.wrapping_add(i as u64);
                signed_unit(s) * 10f64.powi((splitmix64(!s) % 9) as i32 - 4)
            })
            .collect();
        ElementStiffness { n_dofs, data }
    }

    /// Executable definition of the summation contract (PRD S6): fold every
    /// element in slice order, each in `(a, α, b, β)` order, into a
    /// `BTreeMap` starting from `+0.0`, then emit canonical CSR. Written
    /// inline so it shares no code with the production scatter.
    fn encounter_order_reference(
        n_nodes: usize,
        elements: &[AssemblyElement<'_>],
    ) -> SparseRowMat<usize, f64> {
        let d = elements
            .iter()
            .map(|e| e.k_e.n_dofs / e.connectivity.len())
            .max()
            .unwrap_or(3);
        let mut fold: std::collections::BTreeMap<(usize, usize), f64> =
            std::collections::BTreeMap::new();
        for e in elements {
            let d_e = e.k_e.n_dofs / e.connectivity.len();
            for (a, &node_a) in e.connectivity.iter().enumerate() {
                for alpha in 0..d_e {
                    for (b, &node_b) in e.connectivity.iter().enumerate() {
                        for beta in 0..d_e {
                            *fold
                                .entry((d * node_a + alpha, d * node_b + beta))
                                .or_insert(0.0) += e.k_e.get(d_e * a + alpha, d_e * b + beta);
                        }
                    }
                }
            }
        }
        let dim = d * n_nodes;
        let mut row_ptr = vec![0usize; dim + 1];
        for &(row, _) in fold.keys() {
            row_ptr[row + 1] += 1;
        }
        for row in 0..dim {
            row_ptr[row + 1] += row_ptr[row];
        }
        let (col_idx, val): (Vec<usize>, Vec<f64>) =
            fold.into_iter().map(|((_, col), v)| (col, v)).unzip();
        SparseRowMat::new(
            SymbolicSparseRowMat::new_checked(dim, dim, row_ptr, None, col_idx),
            val,
        )
    }

    /// Assert two CSR matrices are identical: shape, `row_ptr`, `row_nnz`,
    /// `col_idx`, and the bits of every stored value. The failure names the
    /// first differing index (and its `(row, col)` for stored entries).
    fn assert_csr_bitwise_eq(
        actual: &SparseRowMat<usize, f64>,
        expected: &SparseRowMat<usize, f64>,
        label: &str,
    ) {
        assert_eq!(
            (actual.nrows(), actual.ncols()),
            (expected.nrows(), expected.ncols()),
            "{label}: shape differs",
        );
        let (a_sym, a_val) = actual.parts();
        let (e_sym, e_val) = expected.parts();
        assert_eq!(a_sym.row_nnz(), e_sym.row_nnz(), "{label}: row_nnz differs");
        let first_diff = |a: &[usize], e: &[usize]| {
            a.iter()
                .zip(e)
                .position(|(x, y)| x != y)
                .or((a.len() != e.len()).then(|| a.len().min(e.len())))
        };
        if let Some(i) = first_diff(a_sym.row_ptr(), e_sym.row_ptr()) {
            panic!(
                "{label}: row_ptr differs first at index {i}: actual {:?} vs expected {:?}",
                a_sym.row_ptr().get(i),
                e_sym.row_ptr().get(i),
            );
        }
        let row_of = |k: usize| e_sym.row_ptr().partition_point(|&p| p <= k) - 1;
        if let Some(k) = first_diff(a_sym.col_idx(), e_sym.col_idx()) {
            panic!(
                "{label}: col_idx differs first at stored index {k} (row {}): \
                 actual {:?} vs expected {:?}",
                row_of(k),
                a_sym.col_idx().get(k),
                e_sym.col_idx().get(k),
            );
        }
        assert_eq!(a_val.len(), e_val.len(), "{label}: value count differs");
        if let Some(k) = a_val
            .iter()
            .zip(e_val)
            .position(|(x, y)| x.to_bits() != y.to_bits())
        {
            panic!(
                "{label}: value differs first at stored index {k} = K[{}][{}]: \
                 actual {} vs expected {}",
                row_of(k),
                e_sym.col_idx()[k],
                a_val[k],
                e_val[k],
            );
        }
    }

    /// `assemble_stiffness` with a poll that never breaks.
    fn assemble_uncancelled(
        n_nodes: usize,
        elements: &[AssemblyElement<'_>],
        threads: usize,
    ) -> SparseRowMat<usize, f64> {
        let threads = NonZeroUsize::new(threads).expect("test thread count >= 1");
        match assemble_stiffness(
            n_nodes,
            elements,
            threads,
            &mut || ControlFlow::Continue(()),
        ) {
            ControlFlow::Continue(k) => k,
            ControlFlow::Break(()) => panic!("a never-breaking poll cannot cancel assembly"),
        }
    }

    /// The primitive reproduces the encounter-order fold entry for entry and
    /// bit for bit: on a heavily shared P1 box (up to 48 contributions per
    /// entry), an irregular P2 box, a mixed tet+shell mesh whose orphan
    /// rotation rows must be EMPTY (not stored zeros), and a degenerate
    /// element repeating a node, which pins the within-element `(a, b)` order.
    #[test]
    fn assemble_stiffness_matches_encounter_order_reference() {
        let mat = dimensionless_steel_like();
        let (box_n, box_k_es, box_conns) = build_shared_dof_box_3x3x3(&mat);

        let (p2_nodes, p2_conns) = unstructured_p2_box(2, 2, 2, 0x5EED_0002);
        let p2_k_es = stiffnesses(&p2_nodes, &p2_conns, element_stiffness_p2);

        let (mixed_n, mixed_conns, mixed_k_es) = replicated_mixed_tet_shell(4);

        let degenerate_conns = [[0usize, 1, 0, 2]];
        let degenerate_k_es = [order_sensitive_stiffness(12, 0xDE6E)];

        let fixtures = [
            (
                "shared-DOF 3×3×3 P1 box",
                box_n,
                elements_of(&box_conns, &box_k_es),
            ),
            (
                "unstructured P2 box",
                p2_nodes.len(),
                elements_of(&p2_conns, &p2_k_es),
            ),
            (
                "mixed tet+shell",
                mixed_n,
                elements_of(&mixed_conns, &mixed_k_es),
            ),
            (
                "repeated-node element",
                3,
                elements_of(&degenerate_conns, &degenerate_k_es),
            ),
        ];
        for (label, n_nodes, elements) in &fixtures {
            let k = assemble_uncancelled(*n_nodes, elements, 1);
            assert!(
                k.val().iter().any(|&v| v != 0.0),
                "{label}: K is all-zero — fixture is degenerate",
            );
            assert_csr_bitwise_eq(&k, &encounter_order_reference(*n_nodes, elements), label);
        }

        let k_mixed = assemble_uncancelled(mixed_n, &fixtures[2].2, 1);
        let row_ptr = k_mixed.symbolic().row_ptr();
        for replica in 0..4 {
            for node in 6 * replica + 1..=6 * replica + 3 {
                for axis in 3..6 {
                    let row = 6 * node + axis;
                    assert_eq!(
                        row_ptr[row],
                        row_ptr[row + 1],
                        "orphan row {row} (tet-only node {node}, axis {axis}) must store nothing",
                    );
                }
            }
        }
    }

    /// No elements ⇒ the `3N × 3N` shape (`D` defaults to 3) with nothing
    /// stored.
    #[test]
    fn assemble_stiffness_empty_elements_gives_3n_square_zero_nnz() {
        let k = assemble_uncancelled(4, &[], 1);
        assert_eq!((k.nrows(), k.ncols()), (12, 12));
        assert_eq!(k.compute_nnz(), 0);
        assert_eq!(k.symbolic().row_ptr(), &[0; 13]);
    }

    #[test]
    #[should_panic(expected = "empty connectivity")]
    fn assemble_stiffness_empty_connectivity_panics() {
        let k_e = ElementStiffness::zeros(3);
        let element = AssemblyElement {
            id: 7,
            connectivity: &[],
            k_e: &k_e,
        };
        let _ = assemble_uncancelled(1, &[element], 1);
    }

    #[test]
    #[should_panic(expected = "k_e.n_dofs (= 20) is not divisible by")]
    fn assemble_stiffness_non_divisible_n_dofs_panics() {
        let k_e = ElementStiffness::zeros(20);
        let conn = [0usize, 1, 2];
        let element = AssemblyElement {
            id: 13,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_uncancelled(3, &[element], 1);
    }

    #[test]
    #[should_panic(expected = "dofs_per_node = 0")]
    fn assemble_stiffness_zero_dofs_per_node_panics() {
        let k_e = ElementStiffness::zeros(0);
        let conn = [0usize];
        let element = AssemblyElement {
            id: 5,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_uncancelled(1, &[element], 1);
    }

    #[test]
    #[should_panic(expected = "AssemblyElement { id: 42 } references node 5 but n_nodes = 4")]
    fn assemble_stiffness_out_of_range_node_panics() {
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        let conn = [0usize, 1, 2, 5];
        let element = AssemblyElement {
            id: 42,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_uncancelled(4, &[element], 1);
    }

    /// Contract checks run before the first poll: a poll that would cancel
    /// immediately cannot mask a malformed mesh.
    #[test]
    #[should_panic(expected = "AssemblyElement { id: 42 } references node 5 but n_nodes = 4")]
    fn assemble_stiffness_contract_panics_precede_a_breaking_poll() {
        let k_e = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        let conn = [0usize, 1, 2, 5];
        let element = AssemblyElement {
            id: 42,
            connectivity: &conn,
            k_e: &k_e,
        };
        let _ = assemble_stiffness(4, &[element], NonZeroUsize::MIN, &mut || {
            ControlFlow::Break(())
        });
    }

    /// Run `assemble_stiffness` with a poll that counts its calls and breaks
    /// on call `break_on` (never, if `None`). Returns the result and the count.
    fn assemble_counting_polls(
        n_nodes: usize,
        elements: &[AssemblyElement<'_>],
        threads: usize,
        break_on: Option<usize>,
    ) -> (ControlFlow<(), SparseRowMat<usize, f64>>, usize) {
        let threads = NonZeroUsize::new(threads).expect("test thread count >= 1");
        let mut calls = 0;
        let result = assemble_stiffness(n_nodes, elements, threads, &mut || {
            calls += 1;
            if Some(calls) == break_on {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        (result, calls)
    }

    /// Minimum single-thread poll count for a fixture to count as spanning
    /// many work chunks, so that more threads genuinely split its rows.
    const MANY_CHUNKS_POLLS: usize = 16;

    /// BT7: `K` at `threads ∈ {2, 8}` is bitwise equal to `threads = 1` on a
    /// fixture that spans many work chunks. Returns the `threads = 1` matrix.
    fn assert_thread_invariant(
        label: &str,
        n_nodes: usize,
        elements: &[AssemblyElement<'_>],
    ) -> SparseRowMat<usize, f64> {
        let (_, polls) = assemble_counting_polls(n_nodes, elements, 1, None);
        assert!(
            polls >= MANY_CHUNKS_POLLS,
            "{label}: precondition — only {polls} polls at threads=1; \
             grow the fixture so threads > 1 split its rows",
        );
        let k1 = assemble_uncancelled(n_nodes, elements, 1);
        assert!(
            k1.val().iter().any(|&v| v != 0.0),
            "{label}: K is all-zero — fixture is degenerate",
        );
        for threads in [2, 8] {
            let k = assemble_uncancelled(n_nodes, elements, threads);
            assert_csr_bitwise_eq(&k, &k1, &format!("{label}: threads={threads} vs 1"));
        }
        k1
    }

    const BT7_P1_SEED: u64 = 0x5EED_0001;

    #[test]
    fn assemble_stiffness_bitwise_identical_across_threads_on_unstructured_p1_mesh() {
        let (nodes, conns) = unstructured_p1_box(8, 7, 6, BT7_P1_SEED);
        let k_es = stiffnesses(&nodes, &conns, element_stiffness_p1);
        assert_thread_invariant("unstructured P1", nodes.len(), &elements_of(&conns, &k_es));
    }

    #[test]
    fn assemble_stiffness_bitwise_identical_across_threads_on_unstructured_p2_mesh() {
        let (nodes, conns) = unstructured_p2_box(4, 3, 3, 0x5EED_0003);
        let k_es = stiffnesses(&nodes, &conns, element_stiffness_p2);
        assert_thread_invariant("unstructured P2", nodes.len(), &elements_of(&conns, &k_es));
    }

    /// Mixed `d_e` across chunk boundaries: thread-invariant, still equal to
    /// the encounter-order fold, orphan rows still empty.
    #[test]
    fn assemble_stiffness_bitwise_identical_across_threads_on_mixed_tet_shell_mesh() {
        let (n_nodes, conns, k_es) = replicated_mixed_tet_shell(96);
        let elements = elements_of(&conns, &k_es);
        let k1 = assert_thread_invariant("mixed tet+shell", n_nodes, &elements);
        assert_csr_bitwise_eq(
            &k1,
            &encounter_order_reference(n_nodes, &elements),
            "mixed tet+shell vs encounter-order reference",
        );
    }

    /// BT7b: a poll that breaks on its third call cancels assembly — `Break`
    /// carries no matrix — and is never called again.
    #[test]
    fn assemble_stiffness_returns_break_when_poll_breaks_on_third_call() {
        let (nodes, conns) = unstructured_p1_box(8, 7, 6, BT7_P1_SEED);
        let k_es = stiffnesses(&nodes, &conns, element_stiffness_p1);
        let elements = elements_of(&conns, &k_es);

        let (_, uncancelled_polls) = assemble_counting_polls(nodes.len(), &elements, 1, None);
        assert!(
            uncancelled_polls >= 3,
            "precondition — an uncancelled run polls only {uncancelled_polls} times",
        );

        let (result, calls) = assemble_counting_polls(nodes.len(), &elements, 1, Some(3));
        assert!(matches!(result, ControlFlow::Break(())), "expected Break");
        assert_eq!(calls, 3, "poll must not be called after it breaks");
    }

    #[test]
    fn assemble_stiffness_break_on_first_poll_returns_break_at_eight_threads() {
        let (nodes, conns) = unstructured_p1_box(8, 7, 6, BT7_P1_SEED);
        let k_es = stiffnesses(&nodes, &conns, element_stiffness_p1);
        let elements = elements_of(&conns, &k_es);

        let (result, calls) = assemble_counting_polls(nodes.len(), &elements, 8, Some(1));
        assert!(matches!(result, ControlFlow::Break(())), "expected Break");
        assert_eq!(calls, 1, "poll must not be called after it breaks");
    }

    /// A panic in whichever thread fills a chunk reaches the caller with its
    /// original payload: every element shares a K_e with no data, so the
    /// pattern pass succeeds and the value pass indexes out of bounds.
    #[test]
    #[should_panic(expected = "out of bounds")]
    fn assemble_stiffness_worker_panic_payload_propagates() {
        let (nodes, conns) = unstructured_p1_box(8, 7, 6, BT7_P1_SEED);
        let malformed = ElementStiffness {
            n_dofs: 12,
            data: vec![],
        };
        let elements: Vec<AssemblyElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(id, conn)| AssemblyElement {
                id,
                connectivity: conn,
                k_e: &malformed,
            })
            .collect();
        let _ = assemble_uncancelled(nodes.len(), &elements, 8);
    }

    /// `AssemblyMode` sets only parallelism (PRD S6): `Parallel { threads }`
    /// is bitwise equal to `Deterministic` and to the primitive at the same
    /// `threads`. Fixtures: the shared-DOF box (up to 48 contributions per
    /// entry), a 137-tet sliding-window chain (prime, so no thread count tiles
    /// it evenly), and the mixed tet+shell mesh.
    #[test]
    fn assemble_global_stiffness_parallel_bitwise_equals_deterministic_and_primitive() {
        let mat = dimensionless_steel_like();
        let (box_n, box_k_es, box_conns) = build_shared_dof_box_3x3x3(&mat);

        let chain_conns: Vec<[usize; 4]> = (0..137).map(|i| [i, i + 1, i + 2, i + 3]).collect();
        let chain_k_es = vec![element_stiffness_p1(&UNIT_TET_P1, &mat); chain_conns.len()];

        let (mixed_n, mixed_conns, mixed_k_es) = replicated_mixed_tet_shell(4);

        let fixtures = [
            (
                "shared-DOF 3×3×3 P1 box",
                box_n,
                elements_of(&box_conns, &box_k_es),
            ),
            (
                "137-tet chain",
                137 + 3,
                elements_of(&chain_conns, &chain_k_es),
            ),
            (
                "mixed tet+shell",
                mixed_n,
                elements_of(&mixed_conns, &mixed_k_es),
            ),
        ];
        for (label, n_nodes, elements) in &fixtures {
            let det = assemble_global_stiffness(*n_nodes, elements, AssemblyMode::Deterministic);
            for threads in [1, 2, 3, 5, 7, 8] {
                let par = assemble_global_stiffness(
                    *n_nodes,
                    elements,
                    AssemblyMode::Parallel { threads },
                );
                assert_csr_bitwise_eq(
                    &par,
                    &det,
                    &format!("{label}: Parallel {{ threads: {threads} }} vs Deterministic"),
                );
                assert_csr_bitwise_eq(
                    &assemble_uncancelled(*n_nodes, elements, threads),
                    &det,
                    &format!("{label}: assemble_stiffness at threads={threads} vs Deterministic"),
                );
            }
        }
    }
}
