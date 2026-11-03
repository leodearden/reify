// SPDX-License-Identifier: AGPL-3.0-or-later

//! Regression tests for task #5951 — a premature per-template
//! `Engine::redispatch_geometry_consuming_compute_nodes` pass strands a
//! geometry-consuming `@optimized` compute node.
//!
//! ## The defect these tests pin
//!
//! `redispatch_geometry_consuming_compute_nodes` is called once per template,
//! from inside the `for (t_idx, template) in module.templates.iter()` loop of
//! `Engine::build_with_geometry_output` in `engine_build.rs` — the shared
//! realization worker that `build()` and `realize_for_check()` both delegate
//! to, and whose call is mirrored in `build_snapshot`'s own template loop.
//! Each call scans **all** compute nodes in the graph — not just the current
//! template's.
//!
//! (Both of those were literal line pins until a rebase moved them 29 lines;
//! an earlier commit on this branch recorded them as verified-correct, which
//! no longer holds. They are symbol-anchored here so a rebase cannot re-stale
//! them — the same remedy that commit applied to the Phase-1 scan anchor
//! cited below.)
//!
//! Its Phase-1 candidate gate skips a node the pass has already redispatched.
//! The pass's sole write is what trips it, so one bad write ends the node's
//! redispatch — the defect below.
//!
//! When a template precedes the geometry-consuming one, the redispatch fires
//! during the *preceding* template's iteration, before the consumer's body has
//! realized. At that point `values` already holds a SYMBOLIC
//! `Value::GeometryHandle { kernel_handle: None, .. }` (minted by
//! `mint_symbolic_geometry_handles_into_values`), the Phase-2 gate matched
//! `Value::GeometryHandle { .. }` regardless of `kernel_handle`, and
//! `build_compute_realization_inputs` recorded a content-free handle. The gate
//! tripped, and the LATER — correct, post-hydration — pass for the consumer's
//! own template was skipped. The node kept the degraded first-dispatch
//! result, with **no diagnostic**: the `ReprKind::BRep` arm of
//! `project_realization_read_handle` is identity-only by design (PRD §4 D1) and
//! emits nothing.
//!
//! The gate is build-scoped (see `RedispatchPassState::redispatched`), so a
//! tripped gate strands the node for the rest of ONE build, not forever. A
//! content-free write is still wrong, which is what the content pair below
//! pins.
//!
//! ## Why this file is mock-kernel, not OCCT/gmsh
//!
//! The defect is decided *before* any realization executes, on a handle whose
//! `kernel_handle` is `None`. It is therefore kernel-independent, and the whole
//! contract is observable through `MockGeometryKernel` in milliseconds. The
//! real OCCT+gmsh acceptance path lives in
//! `tests/solve_elastic_static_body_e2e.rs`.
//!
//! ## Test shape
//!
//! The two PAIRS below compile the same module — one `@optimized` probe
//! consuming a `let body = box(..)` — and register a recording `ComputeFn` for
//! it. Each pair varies exactly one thing and asserts the same property in both
//! halves, so nothing but the varied factor can explain a difference.
//!
//! 1. **Ordering pair** — varies whether a no-op `structure` is declared AHEAD
//!    of the consumer. Both halves must see at least one probe invocation
//!    carrying a KERNEL-BACKED (`kernel_handle: Some(_)`) geometry arg, i.e.
//!    the post-hydration redispatch actually ran.
//! 2. **Content pair** — varies whether the kernel can tessellate. The write
//!    that trips the Phase-1 gate must only ever happen when the rebuilt
//!    `RealizationReadHandle`s carry real content: with a non-tessellating
//!    kernel the node's `realization_inputs` must stay EMPTY (still a
//!    candidate); with a tessellating one they must be recorded as before
//!    (so the guard cannot be satisfied by suppressing the redispatch
//!    wholesale).
//! 3. **Mixed-arg case** (not a pair — a single asymmetric fixture) — a
//!    two-geometry-arg probe where one body realizes and one is refused. Both
//!    the ordering and content contracts above are satisfied by fix (B)'s
//!    content guard ALONE, so neither pair distinguishes fix (A). This case
//!    does: the write legitimately happens, and the contract is about its
//!    MEMBERSHIP — the symbolic sibling's `realization_ref` must not be in it.
//! 4. **Warm surface** (#7332) — the same contracts across a cold `build()`,
//!    an `edit_param` and a warm `build_snapshot`, which reuses the graph
//!    without calling `eval()`.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use reify_ir::{ExportFormat, GeometryKernel, Value};

// ── recording probe ────────────────────────────────────────────────────────

/// One recorded `ComputeFn` invocation.
#[derive(Debug, Clone, Copy)]
struct ProbeRecord {
    /// Did any `value_inputs` entry arrive as a KERNEL-BACKED geometry handle
    /// (`Value::GeometryHandle { kernel_handle: Some(_), .. }`)?
    ///
    /// `false` covers both `Value::Undef` (the original in-`eval()` dispatch,
    /// before any geometry let has a realized handle) and the SYMBOLIC
    /// `kernel_handle: None` placeholder that the premature redispatch pass
    /// consumed.
    kernel_backed_arg: bool,
    /// `realization_inputs.len()` for this invocation.
    realization_inputs_len: usize,
    /// Did any `realization_inputs` handle carry real projected content
    /// (`RealizationReadHandle::content()` is `Some`)?
    any_realized_content: bool,
    /// [`content_fingerprint`] of the handed content, or `None` when no handle
    /// carried any.
    ///
    /// `any_realized_content` answers "was there content?"; this answers "was
    /// it the SAME content?". That second question is the only way a
    /// mock-kernel test can observe `RealizationProjectionStore::evict`: the
    /// store's key is `(realization_ref, content_hash)` and `content_hash` is a
    /// Debug render of the op IR, so it does not move when a parameter VALUE
    /// does — an un-evicted memo is re-served verbatim after an `edit_param`,
    /// and every OTHER field of this record reads identically whether the warm
    /// redispatch was handed the freshly re-projected content or the pre-edit
    /// one.
    content_fingerprint: Option<u64>,
}

/// Hash the GEOMETRY of the first content payload in `handles`; `None` when no
/// handle carries one.
///
/// Deliberately not a copy of `RealizationReadHandle::content_hash`: that field
/// is the projection store's own second key, and #7332's finding is precisely
/// that it does NOT move on a value-driven change. Fingerprinting it would
/// agree with a stale memo instead of catching one.
fn content_fingerprint(handles: &[reify_eval::RealizationReadHandle]) -> Option<u64> {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match handles.iter().find_map(|h| h.content())? {
        reify_eval::RealizedContent::SurfaceMesh(mesh) => {
            for v in &mesh.vertices {
                v.to_bits().hash(&mut hasher);
            }
            mesh.indices.hash(&mut hasher);
        }
        // No fixture here realizes an Sdf or a VolumeMesh. Hashing the
        // discriminant rather than panicking keeps the failure in the assertion
        // that READS the fingerprint — whose message explains what an unmoving
        // fingerprint means — rather than in this helper.
        other => std::mem::discriminant(other).hash(&mut hasher),
    }
    Some(hasher.finish())
}

// Per-thread capture slot. Each cargo test runs on its own thread and
// `Engine::build` is synchronous, so two tests in this binary cannot interleave
// records even when run in parallel. Each test clears the slot at entry.
thread_local! {
    static PROBE_RECORDS: RefCell<Vec<ProbeRecord>> = const { RefCell::new(Vec::new()) };
}

/// Recording [`reify_eval::ComputeFn`] for `@optimized("test::redispatch-order-probe")`.
///
/// Purity-preserving: it only reads the handed slices and appends one
/// [`ProbeRecord`] per invocation.
fn order_probe_fn(
    value_inputs: &[Value],
    realization_inputs: &[reify_eval::RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&reify_ir::OpaqueState>,
    _cancellation: &reify_eval::CancellationHandle,
) -> reify_eval::ComputeOutcome {
    let record = ProbeRecord {
        kernel_backed_arg: value_inputs.iter().any(|v| {
            matches!(
                v,
                Value::GeometryHandle {
                    kernel_handle: Some(_),
                    ..
                }
            )
        }),
        realization_inputs_len: realization_inputs.len(),
        any_realized_content: realization_inputs.iter().any(|h| h.content().is_some()),
        content_fingerprint: content_fingerprint(realization_inputs),
    };
    PROBE_RECORDS.with(|slot| slot.borrow_mut().push(record));
    reify_eval::ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// Reset the capture slot; call at the top of every test.
fn reset_records() {
    PROBE_RECORDS.with(|slot| slot.borrow_mut().clear());
}

/// Drain the recorded invocations.
fn recorded() -> Vec<ProbeRecord> {
    PROBE_RECORDS.with(|slot| slot.borrow().clone())
}

// ── fixtures ───────────────────────────────────────────────────────────────

/// `@optimized` target of the single-geometry-arg probe.
const ORDER_PROBE_TARGET: &str = "test::redispatch-order-probe";

/// `@optimized` target of the two-geometry-arg (MIXED) probe.
const MIXED_PROBE_TARGET: &str = "test::redispatch-mixed-arg-probe";

/// The geometry-consuming `@optimized` probe plus the consuming structure.
///
/// `order_probe` returns a NON-geometry type (`Int`) so `let probe =
/// order_probe(body)` becomes a *value cell* carrying a
/// `CompiledExprKind::UserFunctionCall` default-expr — which is exactly the
/// shape `redispatch_geometry_consuming_compute_nodes`'s Phase-1 scan matches
/// (its `// ── Phase 1: collect candidates` block in `engine_build.rs`, whose
/// `for (c_id, node_data) in state.snapshot.graph.compute_nodes.iter()` walk
/// skips any node this build has already redispatched, per
/// `RedispatchPassState::redispatched`). A
/// Geometry-typed consumer would lower to a separate realization and be
/// invisible to that scan.
const CONSUMER_STRUCTURE: &str = r#"
@optimized("test::redispatch-order-probe")
fn order_probe(g: Geometry) -> Int {
    0
}

structure OrderConsumer {
    param width : Length = 10mm

    let body = box(width, width, width)
    let probe = order_probe(body)
}
"#;

/// A no-op structure declared AHEAD of `OrderConsumer`. It owns no geometry and
/// no compute node — its only effect is to add one iteration to `build()`'s
/// per-template loop before the consumer's body realizes.
const LEADING_STRUCTURE: &str = r#"
structure Leading {
    param nominal : Real = 1.0
}
"#;

/// The WARM twin of [`CONSUMER_STRUCTURE`]: the same single-geometry-arg
/// `@optimized` consumer, but with an editable numeric LEAF in the body.
///
/// `CONSUMER_STRUCTURE` drives all three box extents from `width`. That is fine
/// for a single cold `build()`, but the warm tests re-drive `width` through
/// `edit_param` and want the smallest unambiguous change to the body's
/// realization, so only the X extent is parameterised here.
///
/// `order_probe` returns a NON-geometry type (`Int`) for the same reason
/// [`CONSUMER_STRUCTURE`] does: the consuming `let` must compile to a
/// `CompiledExprKind::UserFunctionCall` default-expr, which is the shape
/// `redispatch_geometry_consuming_compute_nodes`' Phase-1 scan matches.
///
/// Reuses [`ORDER_PROBE_TARGET`] rather than inventing a third target —
/// `Engine::register_compute_fn` panics on duplicate registration, and every
/// test here builds a fresh `Engine`, so sharing the target costs nothing.
///
/// Tests using this fixture append [`trailing_structures`]`(3)`: three no-op
/// structures declared AFTER `WarmConsumer`. That is what keeps the
/// "at most once per build" half of the warm pin non-vacuous — an
/// implementation that simply DELETED the Phase-1 candidate gate instead of
/// build-scoping it would re-dispatch once per trailing template, so the
/// recorded count would scale with the trailing count instead of staying at 1.
const WARM_CONSUMER_STRUCTURE: &str = r#"
@optimized("test::redispatch-order-probe")
fn order_probe(g: Geometry) -> Int {
    0
}

structure WarmConsumer {
    param width : Length = 10mm

    let body = box(width, 10mm, 10mm)
    let probe = order_probe(body)
}
"#;

/// A MIXED-arg `@optimized` probe: two `Geometry` params, one of which never
/// gets a kernel-backed handle.
///
/// `solid` realizes normally; `ghost` is a `sphere(..)`, which
/// [`KernelSwitches::start_refusing_spheres`] makes [`SwitchableKernel`] refuse
/// to execute, so its realization fails and its
/// value cell keeps the SYMBOLIC
/// `Value::GeometryHandle { kernel_handle: None, .. }` placeholder minted by
/// `mint_symbolic_geometry_handles_into_values`.
///
/// This is the one shape in which fix (A) — the `realization_probe_args`
/// downgrade in `redispatch_geometry_consuming_compute_nodes` — is NOT
/// subsumed by fix (B)'s content guard: the node has a hydrated arg, so
/// `.all(content().is_none())` is false and the write happens. Without the
/// downgrade the symbolic sibling's `realization_ref` rides along into
/// `realization_inputs` and into the compute cache key.
const MIXED_ARG_STRUCTURE: &str = r#"
@optimized("test::redispatch-mixed-arg-probe")
fn mixed_probe(a: Geometry, b: Geometry) -> Int {
    0
}

structure MixedConsumer {
    param width : Length = 10mm

    let solid = box(width, width, width)
    let ghost = sphere(width)
    let probe = mixed_probe(solid, ghost)
}
"#;

/// Shared, flippable control surface for [`SwitchableKernel`].
///
/// Cloned into the kernel the engine takes ownership of, so a test that no
/// longer holds the kernel can still read its counter and change its behaviour.
/// A cold-only test flips a switch before its one build. Every contract the
/// warm `build_snapshot` surface introduces has the shape "build 1 saw X,
/// build 2 sees Y", and only a flip BETWEEN builds can hold that shape.
#[derive(Clone, Default)]
struct KernelSwitches {
    refuse_spheres: Arc<AtomicBool>,
    refuse_tessellation: Arc<AtomicBool>,
    tessellate_calls: Arc<AtomicUsize>,
}

impl KernelSwitches {
    /// From the next `execute` on, refuse `sphere(..)`.
    ///
    /// A refused `execute` mints no handle, so the body's realization fails and
    /// `post_process_geometry_handle_cells` never hydrates its value cell: it
    /// keeps the symbolic `kernel_handle: None` placeholder. That is the only
    /// way to hold ONE arg of a multi-geometry `@optimized` node symbolic while
    /// a sibling arg is fully hydrated, within a single template.
    fn start_refusing_spheres(&self) {
        self.refuse_spheres.store(true, Ordering::SeqCst);
    }

    /// From the next `tessellate` on, fail.
    ///
    /// The body still REALIZES: its value cell hydrates to
    /// `kernel_handle: Some(_)`. But Part A's pre-tessellation in
    /// `redispatch_geometry_consuming_compute_nodes` cannot populate the
    /// projection store, so the realization stays at its `ReprKind::BRep`
    /// default and `project_realization_read_handle` takes the identity-only
    /// BRep arm: a `RealizationReadHandle` whose `content()` is `None`. A
    /// handle that IS kernel-backed but carries nothing real is the content
    /// half of the #5951 defect, isolated from template ordering.
    fn start_refusing_tessellation(&self) {
        self.refuse_tessellation.store(true, Ordering::SeqCst);
    }

    /// Every `tessellate` call so far, refused or not.
    fn tessellate_calls(&self) -> usize {
        self.tessellate_calls.load(Ordering::SeqCst)
    }
}

/// Metres of translation per tessellation serial. Large enough to survive the
/// `f32` round-trip through `Mesh::vertices`, small enough to keep the stamped
/// body near the origin.
const TESSELLATION_STAMP_STEP: f32 = 0.5;

/// The geometry kernel every non-default test here uses: a `MockGeometryKernel`
/// decorator with [`KernelSwitches`] refusals, a `tessellate` call counter, and
/// a MOVING tessellation.
///
/// A decorator, not a re-implementation: every trait method forwards to
/// `inner`, so `let body = box(..)` realizes exactly as it does under the plain
/// mock (same `GeometryHandleId` minting, same Wire/Solid repr classification,
/// same op recording), and an op added to the mock cannot diverge here
/// silently. Defined locally rather than in `reify-test-support` because
/// neither `FailingMockGeometryKernel` (whose `execute` fails, so no handle is
/// ever minted) nor `CountingMockKernel` covers realizes-but-cannot-tessellate.
///
/// `tessellate` TRANSLATES the mock's canned mesh by a per-call offset.
/// `MockGeometryKernel::tessellate` returns a CONSTANT `minimal_valid_mesh`
/// independent of handle and geometry, so two projections of the same body are
/// byte-identical, which would make a stale `RealizationProjectionStore` memo
/// invisible to any assertion a mock-kernel test could write. A rigid
/// translation leaves the mesh valid (same topology, same normals, same
/// welding) while making every call's output unique.
struct SwitchableKernel {
    inner: reify_test_support::MockGeometryKernel,
    switches: KernelSwitches,
}

impl SwitchableKernel {
    /// The boxed kernel, plus the switch handle that outlives the engine's
    /// ownership of it.
    fn boxed_with_switches() -> (Box<dyn GeometryKernel>, KernelSwitches) {
        let switches = KernelSwitches::default();
        let kernel = Self {
            inner: reify_test_support::MockGeometryKernel::new(),
            switches: switches.clone(),
        };
        (Box::new(kernel), switches)
    }
}

impl GeometryKernel for SwitchableKernel {
    fn execute(
        &mut self,
        op: &reify_ir::GeometryOp,
    ) -> Result<reify_ir::GeometryHandle, reify_ir::GeometryError> {
        if self.switches.refuse_spheres.load(Ordering::SeqCst)
            && matches!(op, reify_ir::GeometryOp::Sphere { .. })
        {
            return Err(reify_ir::GeometryError::OperationFailed(
                "SwitchableKernel refuses sphere(): the body stays symbolic so the \
                 consuming @optimized node has one hydrated and one unhydrated \
                 geometry arg"
                    .into(),
            ));
        }
        self.inner.execute(op)
    }

    fn query(
        &self,
        query: &reify_ir::GeometryQuery,
    ) -> Result<reify_ir::Value, reify_ir::QueryError> {
        self.inner.query(query)
    }

    fn export(
        &self,
        handle: reify_ir::GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), reify_ir::ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: reify_ir::GeometryHandleId,
        tolerance: f64,
    ) -> Result<reify_ir::Mesh, reify_ir::TessError> {
        let serial = self
            .switches
            .tessellate_calls
            .fetch_add(1, Ordering::SeqCst);
        if self.switches.refuse_tessellation.load(Ordering::SeqCst) {
            return Err(reify_ir::TessError::TessellationFailed(
                "SwitchableKernel refuses tessellation: the projection store \
                 stays empty so the realization projects to content: None"
                    .into(),
            ));
        }
        let mut mesh = self.inner.tessellate(handle, tolerance)?;
        let offset = serial as f32 * TESSELLATION_STAMP_STEP;
        for v in &mut mesh.vertices {
            *v += offset;
        }
        Ok(mesh)
    }
}

/// Build the module, register the probe, and run `build()` against `kernel`.
///
/// Returns the engine (so callers can inspect the post-build graph) and the
/// build diagnostics.
///
/// With `MockGeometryKernel`, `execute` accepts any op and mints a real
/// `GeometryHandleId`, so `let body = box(..)` realizes unconditionally and
/// `post_process_geometry_handle_cells` hydrates the cell with
/// `kernel_handle: Some(_)` — no OCCT required.
fn build_with_probe_using(
    source: &str,
    kernel: Box<dyn reify_ir::GeometryKernel>,
) -> (reify_eval::Engine, Vec<reify_core::Diagnostic>) {
    let (engine, _compiled, diagnostics) = build_with_probe_capturing_module(source, kernel);
    (engine, diagnostics)
}

/// [`build_with_probe_using`], additionally handing back the `CompiledModule`.
///
/// `build()` does not stash the module, but `build_snapshot(&module, ..)` needs
/// it a second time, so the WARM tests cannot use the two-value form. Both
/// helpers share this one body: there is a single place the probe targets get
/// registered and a single place the cold build runs.
fn build_with_probe_capturing_module(
    source: &str,
    kernel: Box<dyn reify_ir::GeometryKernel>,
) -> (
    reify_eval::Engine,
    reify_compiler::CompiledModule,
    Vec<reify_core::Diagnostic>,
) {
    let compiled = reify_test_support::parse_and_compile(source);
    let mut engine = reify_eval::Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(kernel),
    );
    engine.register_compute_fn(ORDER_PROBE_TARGET, order_probe_fn as reify_eval::ComputeFn);
    // Same recording fn under the mixed-arg target; only one of the two
    // fixtures is ever compiled per build, so the shared capture slot stays
    // unambiguous.
    engine.register_compute_fn(MIXED_PROBE_TARGET, order_probe_fn as reify_eval::ComputeFn);
    let diagnostics = engine.build(&compiled, ExportFormat::Step).diagnostics;
    (engine, compiled, diagnostics)
}

/// [`build_with_probe_using`] against the default tessellating mock kernel,
/// keeping only the diagnostics.
fn build_with_probe(source: &str) -> Vec<reify_core::Diagnostic> {
    build_with_probe_using(
        source,
        Box::new(reify_test_support::MockGeometryKernel::new()),
    )
    .1
}

/// The `realization_inputs` recorded on the probe's compute node in the
/// post-build snapshot graph.
///
/// This is the dependency record the redispatch pass writes, not the gate it
/// reads (see `RedispatchPassState::redispatched`). EMPTY means no pass has
/// written this node yet; non-empty means some build's pass did.
fn probe_node_realization_inputs(engine: &reify_eval::Engine) -> Vec<usize> {
    probe_node_realization_input_refs(engine, ORDER_PROBE_TARGET)
        .iter()
        .map(Vec::len)
        .collect()
}

/// The `realization_inputs` REFS recorded on every compute node dispatching to
/// `target`, one inner `Vec` per node.
///
/// [`probe_node_realization_inputs`] answers "did the pass write this node?";
/// this answers "with WHICH realizations?", which is what the mixed-arg case
/// needs — there the write legitimately happens and the contract is about the
/// membership of the recorded list.
fn probe_node_realization_input_refs(
    engine: &reify_eval::Engine,
    target: &str,
) -> Vec<Vec<reify_core::RealizationNodeId>> {
    let state = engine
        .eval_state()
        .expect("build() must leave an EvaluationState behind");
    state
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .filter(|(_, n)| n.target == target)
        .map(|(_, n)| n.realization_inputs.clone())
        .collect()
}

/// The `realization_ref` that the geometry let `<entity>.<name>` binds.
///
/// Read from `eval_state().snapshot.values`, which is a reliable source of the
/// REF and says nothing about hydration: `post_process_geometry_handle_cells`
/// writes the kernel-backed handle into `build()`'s LOCAL `values` map, not
/// into the snapshot, so every geometry cell in the snapshot still carries the
/// `kernel_handle: None` placeholder `mint_symbolic_geometry_handles_into_values`
/// minted — realized and unrealized bodies alike. Do not read `kernel_handle`
/// from here to decide whether a body hydrated; the probe records are the
/// observation point for that.
///
/// Panics unless the cell holds a `Value::GeometryHandle`.
fn geometry_cell_realization(
    engine: &reify_eval::Engine,
    entity: &str,
    name: &str,
) -> reify_core::RealizationNodeId {
    let state = engine
        .eval_state()
        .expect("build() must leave an EvaluationState behind");
    let cell = reify_core::ValueCellId::new(entity, name);
    match state.snapshot.values.get(&cell) {
        Some((
            Value::GeometryHandle {
                realization_ref, ..
            },
            _,
        )) => realization_ref.clone(),
        other => panic!("`{entity}.{name}` must hold a Value::GeometryHandle, got {other:?}"),
    }
}

/// Shared assertion: the post-hydration redispatch must have delivered a
/// kernel-backed geometry arg to the probe at least once.
fn assert_probe_saw_kernel_backed_body(case: &str, records: &[ProbeRecord]) {
    assert!(
        !records.is_empty(),
        "{case}: the @optimized probe must be invoked at least once; \
         it was never dispatched (records: {records:?})"
    );
    // Diagnostic aid only (NOT the assertion): count invocations that were
    // handed non-empty `realization_inputs` carrying nothing real. Each such
    // invocation corresponds to a write that tripped the Phase-1 gate while
    // delivering no content — the signature of the #5951 strand.
    // `assert_no_contentless_write` turns this into a first-class assertion;
    // here it just makes the failure legible.
    let contentless_writes = records
        .iter()
        .filter(|r| r.realization_inputs_len > 0 && !r.any_realized_content)
        .count();
    assert!(
        records.iter().any(|r| r.kernel_backed_arg),
        "{case}: at least one probe invocation must receive a KERNEL-BACKED \
         `Value::GeometryHandle {{ kernel_handle: Some(_), .. }}` — i.e. the \
         post-hydration `redispatch_geometry_consuming_compute_nodes` pass must \
         actually run for this node. Every recorded invocation carried an \
         unhydrated arg instead, which is the task #5951 strand: a premature \
         per-template redispatch consumed the SYMBOLIC \
         (`kernel_handle: None`) handle and tripped the Phase-1 candidate \
         gate, so the correct later pass in this build was skipped. \
         {contentless_writes} of \
         {} invocation(s) were handed non-empty realization_inputs carrying no \
         realized content. Records: {records:?}",
        records.len(),
    );
}

/// Shared assertion for #5951's content guard, on every surface that can reach
/// it: no `@optimized` invocation may be handed non-empty `realization_inputs`
/// in which every `RealizationReadHandle::content()` is `None`.
///
/// One predicate, one failure message, one place a future change to #5951's
/// contract has to land. Asserted from the cold `build()` case and — since
/// #7332 made the warm `build_snapshot` surface reachable — from its warm twin
/// too.
fn assert_no_contentless_write(case: &str, records: &[ProbeRecord]) {
    let contentless: Vec<_> = records
        .iter()
        .filter(|r| r.realization_inputs_len > 0 && !r.any_realized_content)
        .collect();
    assert!(
        contentless.is_empty(),
        "{case}: no probe invocation may be handed non-empty \
         `realization_inputs` in which every `RealizationReadHandle::content()` \
         is None. Such a write delivers nothing to the trampoline yet still ends \
         the node's redispatch for the rest of that build, stranding it on its \
         degraded first-dispatch result — SILENTLY, because the `ReprKind::BRep` \
         arm of `project_realization_read_handle` is identity-only by design \
         (PRD §4 D1) and `degrade_projection`'s Warning is never reached (task \
         #5951 fix (B)). Since #7332 that strand lasts one build instead of \
         forever, which narrows the door but does not close it. Offending \
         invocations: {contentless:?} of all records: {records:?}"
    );
}

/// Shared assertion for #5951's end-of-build report: a node the content guard
/// can never rescue must be named exactly ONCE per build — per NODE, not per
/// per-template redispatch call, and never zero times.
///
/// Asserted from the cold `build()` case and the warm `build_snapshot` one.
/// Both fixtures carry trailing templates, so "once" is a real claim rather
/// than an artefact of there being only one call.
fn assert_one_strand_warning(case: &str, diagnostics: &[reify_core::Diagnostic]) {
    let strand_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("projected no content")
        })
        .collect();
    assert_eq!(
        strand_warnings.len(),
        1,
        "{case}: exactly ONE Warning must report the stranded node — the \
         degradation must be visible, and it must be reported per node. Zero \
         means it is as silent as the pre-#5951 defect was; more than one means \
         it is reported per per-template redispatch call. Got: {strand_warnings:?}"
    );
    assert!(
        strand_warnings[0].message.contains(ORDER_PROBE_TARGET),
        "{case}: the Warning must name the @optimized target so the user can \
         find the degraded node: {:?}",
        strand_warnings[0]
    );
}

// ── tests ──────────────────────────────────────────────────────────────────

/// Task #5951 (RED before the fix): a single no-op structure declared ahead of
/// the geometry-consuming structure must NOT strand the compute node.
///
/// `Leading` owns no geometry and no compute node — it contributes nothing but
/// one extra iteration of `build()`'s per-template loop. That is enough to make
/// the redispatch fire while `OrderConsumer`'s body is still symbolic.
#[test]
fn redispatch_reaches_kernel_backed_body_when_a_template_precedes_the_consumer() {
    reset_records();
    let source = format!("{LEADING_STRUCTURE}{CONSUMER_STRUCTURE}");
    let diagnostics = build_with_probe(&source);

    // The strand is SILENT by design (the `ReprKind::BRep` projection arm emits
    // no diagnostic), so a diagnostic assertion cannot detect it. Pin the
    // silence so a future reader does not look for an error that never comes.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "leading-template case must build without Error diagnostics \
         (the defect is silent, not diagnosed): {errors:?}"
    );

    assert_probe_saw_kernel_backed_body("leading-template case", &recorded());
}

/// Control (GREEN both before and after the fix): the byte-identical module
/// WITHOUT the leading structure.
///
/// `OrderConsumer` is template index 0, so the FIRST redispatch call already
/// runs after its own `post_process_geometry_handle_cells` hydration and sees a
/// kernel-backed handle. Pairing this with the test above isolates template
/// ordering as the sole variable — same body, same probe, same kernel, same
/// assertion.
#[test]
fn redispatch_reaches_kernel_backed_body_when_the_consumer_is_the_first_template() {
    reset_records();
    let diagnostics = build_with_probe(CONSUMER_STRUCTURE);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "control case must build without Error diagnostics: {errors:?}"
    );

    assert_probe_saw_kernel_backed_body("control (no leading template)", &recorded());
}

/// Task #5951 (RED after fix (A), GREEN after fix (B)): the Phase-1 latch must
/// never be tripped by a write that carried nothing real.
///
/// Fix (A) filters SYMBOLIC handles, but a KERNEL-BACKED handle whose
/// realization projects to no content slips straight through it. The kernel
/// here realizes the body for real (`kernel_handle: Some(_)`) and then fails
/// `tessellate`, so Part A's pre-tessellation cannot populate the projection
/// store and `project_realization_read_handle` returns a handle with
/// `content(): None`.
///
/// Before fix (B), `redispatch_geometry_consuming_compute_nodes` still wrote
/// those content-free inputs onto the node and still re-dispatched, tripping
/// the one-shot `realization_inputs.is_empty()` candidate gate. The node was
/// then permanently excluded from every later pass — including one that could
/// have delivered real content — and, per the BRep arm's identity-only
/// contract (PRD §4 D1), nothing anywhere reported it.
///
/// Note this case carries NO leading structure: the consumer is template 0.
/// It isolates the content half of the defect from the ordering half.
#[test]
fn latch_is_not_tripped_when_the_rebuilt_inputs_carry_no_realized_content() {
    reset_records();
    let (kernel, switches) = SwitchableKernel::boxed_with_switches();
    switches.start_refusing_tessellation();
    let (engine, diagnostics) = build_with_probe_using(CONSUMER_STRUCTURE, kernel);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a kernel that cannot tessellate must not fail the build outright — \
         the degradation this test pins is silent, not diagnosed: {errors:?}"
    );

    let records = recorded();
    assert!(
        !records.is_empty(),
        "the @optimized probe must be invoked at least once; it was never \
         dispatched (records: {records:?})"
    );

    // (a) Observable at the trampoline: the same `realization_inputs` that get
    //     written onto the node are the ones handed to `run_compute_dispatch`,
    //     so a content-free write is visible here.
    assert_no_contentless_write("cold build, non-tessellating kernel", &records);

    // (b) Observable in the post-build graph: the candidate gate is untripped,
    //     so a later pass — a later template's iteration, a later build tick —
    //     can still do the real work.
    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![0],
        "the probe's compute node must still have EMPTY `realization_inputs` \
         after a build that could not project any content, so it remains a \
         redispatch candidate. A non-empty value here means the contentless \
         write went through: the Phase-1 gate skips this node for the rest of \
         the build."
    );
}

/// Positive control for fix (B) (GREEN before and after): when the projection
/// DOES yield real content, the write must still happen.
///
/// Same module, same probe — only the kernel differs. `MockGeometryKernel`
/// tessellates successfully, so Part A populates the projection store, the
/// rebuilt handle carries `Some(RealizedContent::SurfaceMesh(..))`, and the
/// node is legitimately dispatched with real inputs and legitimately latched.
///
/// Pairing this with the test above pins fix (B) as a guard on *contentless*
/// writes specifically — not a blanket suppression that would stop the
/// redispatch from ever recording its inputs.
#[test]
fn latch_is_tripped_normally_when_the_rebuilt_inputs_carry_realized_content() {
    reset_records();
    let (engine, _diagnostics) = build_with_probe_using(
        CONSUMER_STRUCTURE,
        Box::new(reify_test_support::MockGeometryKernel::new()),
    );

    let records = recorded();
    assert!(
        records
            .iter()
            .any(|r| r.realization_inputs_len > 0 && r.any_realized_content),
        "at least one probe invocation must be handed `realization_inputs` \
         carrying real projected content — otherwise fix (B) has over-fired \
         and suppressed the legitimate redispatch write. Records: {records:?}"
    );

    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![1],
        "a redispatch that DID deliver content must record its \
         `realization_inputs` on the node as before"
    );
}

/// Task #5951 fix (A), pinned on the one shape where fix (B) does NOT subsume
/// it: a MIXED-arg node — one hydrated geometry arg, one still symbolic.
///
/// Fix (A) is two things at the same site: the Phase-2 gate narrowed to
/// `kernel_handle: Some(_)`, and the `realization_probe_args` downgrade fed to
/// `build_compute_realization_inputs`. In every SINGLE-geometry-arg case fix
/// (B)'s content guard reaches the same verdict for free — a symbolic handle
/// projects to `content: None`, so `.all(content().is_none())` is true and the
/// write is skipped anyway. That is why the four tests above stay green with
/// fix (A) reverted, and why this case exists.
///
/// Here `solid` hydrates and `ghost` does not, so:
///   * the Phase-2 gate passes on `solid` (a kernel-backed arg IS present),
///   * `solid` projects to real content, so the content guard does NOT fire,
///   * and the write happens — legitimately.
///
/// The question fix (A) answers is what that write CONTAINS.
/// `build_compute_realization_inputs` matches `Value::GeometryHandle { .. }`
/// regardless of `kernel_handle`, so on RAW `arg_values` it records `ghost`'s
/// `realization_ref` too: a content-free entry in the node's
/// `realization_inputs`, which is a dependency edge for freshness and a term in
/// the compute cache key (`compute_cache_key(node, graph)`), keyed on a
/// realization that never produced anything. The downgrade keeps it out.
#[test]
fn a_symbolic_sibling_arg_is_kept_out_of_realization_inputs() {
    reset_records();
    let (kernel, switches) = SwitchableKernel::boxed_with_switches();
    switches.start_refusing_spheres();
    let (engine, diagnostics) = build_with_probe_using(MIXED_ARG_STRUCTURE, kernel);

    let solid_ref = geometry_cell_realization(&engine, "MixedConsumer", "solid");
    let ghost_ref = geometry_cell_realization(&engine, "MixedConsumer", "ghost");
    assert_ne!(
        solid_ref, ghost_ref,
        "the two geometry lets must bind DISTINCT realizations"
    );

    // Premise, asserted rather than assumed: `ghost` never realized, so it can
    // only be carrying the symbolic placeholder. If a future change makes
    // `sphere()` succeed under this kernel — or makes a refused realization
    // hydrate to a stub `Some(handle)` — this fixture stops exercising the
    // mixed case and the conclusion below would hold vacuously.
    assert!(
        diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error
                && d.message.contains("SwitchableKernel refuses sphere()")),
        "`ghost`'s realization must be REFUSED by the kernel — that refusal is \
         what keeps its handle symbolic while its sibling hydrates. \
         Diagnostics: {diagnostics:?}"
    );

    let per_node = probe_node_realization_input_refs(&engine, MIXED_PROBE_TARGET);
    assert_eq!(
        per_node.len(),
        1,
        "exactly one compute node must dispatch to `{MIXED_PROBE_TARGET}`; \
         got {per_node:?}"
    );
    let inputs = &per_node[0];

    assert!(
        !inputs.contains(&ghost_ref),
        "the SYMBOLIC sibling's realization `{ghost_ref:?}` must NOT appear in \
         the node's `realization_inputs`. It projects to no content, so \
         recording it adds a freshness edge and a compute-cache-key term for a \
         realization that produced nothing — task #5951 fix (A): \
         `redispatch_geometry_consuming_compute_nodes` probes \
         `build_compute_realization_inputs` through `realization_probe_args`, \
         which downgrades `kernel_handle: None` handles to `Value::Undef`, \
         exactly as the two `@optimized` dispatch sites in `engine_eval.rs` do. \
         Recorded inputs: {inputs:?}"
    );
    assert_eq!(
        inputs,
        &vec![solid_ref.clone()],
        "the hydrated body `{solid_ref:?}` — and only it — must be recorded"
    );

    // And the write really did happen: this is not the content guard firing,
    // and `solid` really did hydrate. Pairing this with the assertion above
    // makes the case specific to fix (A) — fix (B) is inert here.
    let records = recorded();
    assert!(
        records.iter().any(|r| r.kernel_backed_arg
            && r.realization_inputs_len == 1
            && r.any_realized_content),
        "the probe must be dispatched with the one realized body's handle and \
         its projected content — otherwise either `solid` never hydrated (so \
         the Phase-2 gate never opened) or fix (B)'s content guard fired, and \
         this case is not exercising fix (A) at all. Records: {records:?}"
    );
}

/// Trailing no-op structures, `count` of them, declared AFTER the consumer.
///
/// Each adds one iteration to `build()`'s per-template loop, and therefore one
/// re-entry of `redispatch_geometry_consuming_compute_nodes` for a node the
/// content guard has left a candidate.
fn trailing_structures(count: usize) -> String {
    (0..count)
        .map(|i| format!("\nstructure Trailing{i} {{\n    param nominal : Real = 1.0\n}}\n"))
        .collect()
}

/// Amendment to #5951's content guard: keeping a node a redispatch candidate
/// must not make Part A's pre-tessellation cost scale with the number of
/// templates declared after it.
///
/// The guard's whole point is that a non-projectable node stays a candidate so
/// a later pass can still do the real work. The pass, though, is re-entered
/// once per template and scans ALL compute nodes, so the *unmitigated* shape of
/// that decision is a failing `kernel.tessellate()` re-attempted once per
/// trailing template — measured at 1 / 2 / 4 attempts for 0 / 1 / 3 trailing
/// structures. On this mock that is free; on the real OCCT/gmsh path a failing
/// tessellation of a pathological body is not, and it is paid on every build
/// tick.
///
/// `tessellate` is a pure function of `(realization, content_hash)`, so a
/// failure cannot become a success later in the SAME build. `RedispatchPassState`
/// negative-caches it. The invariant asserted here is the one that matters and
/// is robust to any constant baseline: the count does not GROW with the number
/// of trailing templates.
#[test]
fn a_failed_pre_tessellation_is_not_retried_once_per_trailing_template() {
    let mut counts = Vec::new();
    for trailing in [0usize, 1, 3] {
        reset_records();
        let (kernel, switches) = SwitchableKernel::boxed_with_switches();
        switches.start_refusing_tessellation();
        let source = format!("{CONSUMER_STRUCTURE}{}", trailing_structures(trailing));
        let (_engine, _diagnostics) = build_with_probe_using(&source, kernel);
        counts.push((trailing, switches.tessellate_calls()));
    }

    let baseline = counts[0].1;
    assert!(
        baseline > 0,
        "the fixture must reach Part A's pre-tessellation at least once, \
         otherwise this test cannot observe the retry it is guarding against \
         (counts: {counts:?})"
    );
    for &(trailing, calls) in &counts {
        assert_eq!(
            calls, baseline,
            "`kernel.tessellate()` was attempted {calls} times with {trailing} \
             trailing template(s) but {baseline} time(s) with none. The content \
             guard leaves the node a redispatch candidate, so the pass is \
             re-entered once per trailing template; the negative cache in \
             `RedispatchPassState` must make the failed pre-tessellation \
             idempotent within a build. All counts: {counts:?}"
        );
    }
}

/// Amendment to #5951's content guard: a node the guard could never rescue must
/// not end the build silently.
///
/// The guard converts a permanent silent strand into a *retried* one — but for
/// a body that can never project content (this kernel's shape, or a real body
/// OCCT cannot tessellate) no later pass ever succeeds, and the end state is
/// the pre-fix one: a node stranded on its degraded first-dispatch result. The
/// original defect's defining property is that NOTHING reported it — the
/// `ReprKind::BRep` arm of `project_realization_read_handle` is identity-only
/// by design (PRD §4 D1), so `degrade_projection`'s own Warning is never
/// reached. Recoverable-in-principle is not the same as visible.
///
/// One Warning per stranded NODE per build — not one per per-template call —
/// so adding trailing templates must not multiply it.
#[test]
fn a_permanently_stranded_node_is_reported_once_at_end_of_build() {
    for trailing in [0usize, 3] {
        reset_records();
        let source = format!("{CONSUMER_STRUCTURE}{}", trailing_structures(trailing));
        let (kernel, switches) = SwitchableKernel::boxed_with_switches();
        switches.start_refusing_tessellation();
        let (engine, diagnostics) = build_with_probe_using(&source, kernel);

        // Still a candidate — i.e. genuinely the stranded shape this warns about.
        assert_eq!(
            probe_node_realization_inputs(&engine),
            vec![0],
            "precondition: the content guard must have left the node a candidate"
        );

        assert_one_strand_warning(
            &format!("cold build, {trailing} trailing template(s)"),
            &diagnostics,
        );
    }
}

// ── task #7332: the WARM (`build_snapshot`) surface ────────────────────────

/// Task #7332: a warm `build_snapshot` following an `edit_param` must
/// re-dispatch the geometry-consuming `@optimized` node exactly once, and hand
/// it freshly projected content.
///
/// The mock-kernel, always-runs twin of `morph_arm_e2e.rs`'s gmsh-gated
/// `e2e_non_structural_tick_morphs_and_preserves_connectivity`, which pins the
/// same contract on the real OCCT+gmsh VolumeMesh path.
///
/// `build()` reaches the pass on a fresh graph every time, because `eval()`
/// installs one. `build_snapshot` never calls `eval()` and reuses
/// `eval_state.snapshot.graph`, so only a build-scoped redispatch signal (see
/// `RedispatchPassState::redispatched`) keeps the cold build's redispatch from
/// closing the warm one.
///
/// That the warm surface really re-executes realizations against the edited
/// param is pinned independently by
/// `interactive_edit_loop.rs::edit_param_then_build_snapshot_updates_geometry`
/// on this same `MockGeometryKernel`; premise (b) in the shared body
/// re-asserts it locally so a failure of the pin cannot be blamed on a dead
/// geometry path.
#[test]
fn a_warm_build_snapshot_after_edit_param_re_dispatches_the_geometry_consuming_node() {
    assert_warm_build_snapshot_re_dispatches_with_fresh_content(
        "consumer first, 3 trailing templates",
        &format!("{WARM_CONSUMER_STRUCTURE}{}", trailing_structures(3)),
    );
}

/// The warm twin of
/// `redispatch_reaches_kernel_backed_body_when_a_template_precedes_the_consumer`:
/// #5951's original shape, a template declared AHEAD of the consumer, on the
/// warm `build_snapshot` surface.
///
/// The leading template's pass runs while the consumer's body is still
/// symbolic. It must leave no build-scoped residue that outlives the body's
/// hydration: neither a negative-cached pre-tessellation that would refuse the
/// consumer's own pass, nor a contentless skip that would report a strand that
/// never happened.
#[test]
fn a_warm_build_snapshot_re_dispatches_when_a_template_precedes_the_consumer() {
    assert_warm_build_snapshot_re_dispatches_with_fresh_content(
        "leading template, 3 trailing templates",
        &format!(
            "{LEADING_STRUCTURE}{WARM_CONSUMER_STRUCTURE}{}",
            trailing_structures(3)
        ),
    );
}

/// The shared body of the two warm re-dispatch tests above: a cold `build()`,
/// `edit_param(WarmConsumer.width)`, then a warm `build_snapshot`, against the
/// stamping [`SwitchableKernel`]. `source` must declare
/// [`WARM_CONSUMER_STRUCTURE`] and three trailing templates.
fn assert_warm_build_snapshot_re_dispatches_with_fresh_content(case: &str, source: &str) {
    reset_records();
    // The STAMPING kernel, not the plain mock: the eviction pin at the bottom
    // needs two projections of the same body to be distinguishable, and the
    // plain mock's `tessellate` returns a constant mesh. Every assertion above
    // that pin reads identically under either kernel.
    let (kernel, _switches) = SwitchableKernel::boxed_with_switches();
    let (mut engine, compiled, diagnostics) = build_with_probe_capturing_module(source, kernel);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{case}: premise: the cold build must succeed, or nothing below is about the \
         warm surface: {errors:?}"
    );

    // ── Premise (a): the node really IS a geometry-consuming redispatch
    //    candidate, AND the cold build dispatched it at most once.
    //
    // This doubles as the counterfactual guard on the FIX's shape. The fixture
    // carries three trailing templates, and the pass is re-entered once per
    // template scanning ALL compute nodes. An implementation that made the warm
    // pin below pass by simply DELETING the Phase-1 gate — rather than
    // build-scoping it — would re-dispatch on every trailing template too, and
    // this count would read 5 (1 original + 4 per-template calls), not 2.
    let cold = recorded();
    assert_eq!(
        cold.len(),
        2,
        "{case}: premise: the cold build() must dispatch the probe EXACTLY twice — the \
         original `Value::Undef`-bodied dispatch, then ONE post-hydration \
         redispatch that the candidate gate then stops repeating for the three \
         trailing templates. A count of 5 means the gate was removed rather \
         than build-scoped. Records: {cold:?}"
    );
    let last = cold.last().expect("cold.len() == 2 was just asserted");
    assert!(
        last.kernel_backed_arg && last.any_realized_content && last.realization_inputs_len == 1,
        "{case}: premise: the cold build's LAST probe invocation must be the \
         post-hydration redispatch — a KERNEL-BACKED geometry arg, exactly one \
         `realization_inputs` entry, and real projected content. Without that \
         this node is not a geometry-consuming redispatch candidate at all and \
         the warm pin below would hold vacuously. Last record: {last:?}, all: \
         {cold:?}"
    );

    // Non-structural tick: 10mm → 12mm on the body's one parameterised extent.
    engine
        .edit_param(
            reify_core::ValueCellId::new("WarmConsumer", "width"),
            Value::length(0.012),
        )
        .expect("edit_param must succeed against the WarmConsumer.width Length param");

    // Drop the cold records: everything below is attributable to
    // `build_snapshot` alone, not to `edit_param`'s own incremental re-eval.
    reset_records();
    let build = engine
        .build_snapshot(&compiled, ExportFormat::Step)
        .expect("build_snapshot must return a BuildResult after edit_param");

    // ── Premise (b): the warm surface really re-executed the realization and
    //    re-stamped a kernel-backed handle. Without this, a zero warm dispatch
    //    count below could mean "geometry path is dead", not "gate is shut".
    assert!(
        build.geometry_output.is_some(),
        "{case}: premise: the warm build_snapshot must produce geometry output — it is \
         what proves the body was re-realized and re-exported against the edited \
         param, so the pin below is about the CANDIDATE GATE and not about a \
         dead geometry path"
    );

    // ── THE PIN (#7332) ────────────────────────────────────────────────────
    let warm = recorded();
    assert!(
        !warm.is_empty(),
        "{case}: #7332: the warm `build_snapshot` must dispatch the \
         geometry-consuming `@optimized` node at least once, and dispatched it \
         ZERO times: the cold build's redispatch closed the warm one. \
         `build_snapshot` reuses the graph and never calls `eval()`, so a \
         redispatch-once signal that is not build-scoped (see \
         `RedispatchPassState::redispatched`) shuts the node out of every later \
         build."
    );
    let warm_dispatch = warm
        .iter()
        .find(|r| r.kernel_backed_arg && r.any_realized_content && r.realization_inputs_len == 1);
    assert!(
        warm_dispatch.is_some(),
        "{case}: #7332: the warm `build_snapshot`'s redispatch must deliver the SAME \
         shape the cold one did — a KERNEL-BACKED `Value::GeometryHandle`, \
         exactly one `realization_inputs` entry, and real projected content — \
         otherwise the node was re-admitted as a candidate but handed nothing \
         the `@optimized` fn can use, which is a re-dispatch in name only. \
         Warm records: {warm:?}"
    );
    let warm_dispatch = warm_dispatch.expect("`warm_dispatch.is_some()` was just asserted");

    // ── THE EVICTION PIN (#7332's second half) ─────────────────────────────
    //
    // Re-admitting the node is necessary but not sufficient: it must be handed
    // the body as it is NOW, not the body as it was before `edit_param`.
    // `RealizationProjectionStore` is keyed `(realization_ref, content_hash)`,
    // and `content_hash` is a Debug render of the op IR — it provably does not
    // move on a value-driven change — so without
    // `invalidate_realization_dirty_cone`'s eviction the post-edit lookup hits
    // the PRE-edit entry verbatim and the redispatch re-consumes stale
    // geometry. Every other field of a `ProbeRecord` is identical either way,
    // which is why the fingerprint exists.
    //
    // This is the always-runs twin of `morph_arm_e2e.rs`'s x-extent assertion,
    // which is `#[cfg(has_gmsh)]` AND soft-returns without system OCCT — and
    // reify pins no OCCT version and preflights none, so that test can stop
    // running silently.
    let cold_fingerprint = last.content_fingerprint.expect(
        "premise: the cold build's post-hydration redispatch carried content \
         (`any_realized_content` was just asserted), so it must have a \
         fingerprint",
    );
    assert_eq!(
        warm.iter()
            .filter(|r| r.content_fingerprint.is_some())
            .count(),
        1,
        "{case}: premise: exactly one warm invocation may carry content, or \
         `warm_dispatch` below is not the one this pin is about. Warm records: \
         {warm:?}"
    );
    assert_ne!(
        warm_dispatch.content_fingerprint,
        Some(cold_fingerprint),
        "{case}: #7332: the warm redispatch must be handed FRESHLY PROJECTED content, \
         and was handed byte-identical content to the cold build's. The \
         `RealizationProjectionStore` memo for this realization survived \
         `edit_param`: its second key is the STATIC `content_hash`, which does \
         not move when a parameter value does, so the post-edit lookup re-hits \
         the pre-edit entry. `invalidate_realization_dirty_cone` must \
         `RealizationProjectionStore::evict` every realization whose evaluated \
         input cone moved. Cold: {cold_fingerprint:?}, warm: {warm_dispatch:?}"
    );

    // ── No phantom strand ──────────────────────────────────────────────────
    //
    // A pass that ran while the body was still symbolic (the leading-template
    // case) must not have recorded a contentless skip that the consumer's own
    // pass then failed to clear. The node was rescued, so it is not stranded.
    let strand_warnings: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("projected no content")
        })
        .collect();
    assert!(
        strand_warnings.is_empty(),
        "{case}: the warm build_snapshot re-dispatched the node with real \
         content, so it must not also report it as stranded: \
         {strand_warnings:?}"
    );
}

/// Task #7332 opened a SECOND way into
/// `redispatch_geometry_consuming_compute_nodes` — the warm `build_snapshot`
/// surface — and #5951's content guard must hold on it exactly as it does on
/// the cold one.
///
/// `latch_is_not_tripped_when_the_rebuilt_inputs_carry_no_realized_content`
/// above proves the guard on a single cold `build()`. A warm rebuild re-admits
/// the node, so the guard is re-evaluated against a freshly re-realized body on
/// every tick — and a kernel that realizes but cannot tessellate keeps
/// projecting to `content: None` forever.
///
/// The three assertions are the cold twin's two, restated across both builds,
/// plus the end-of-build report, which `build_snapshot`'s own template loop
/// must emit too.
#[test]
fn a_warm_rebuild_still_refuses_contentless_inputs() {
    reset_records();
    let source = format!("{WARM_CONSUMER_STRUCTURE}{}", trailing_structures(3));
    let (kernel, switches) = SwitchableKernel::boxed_with_switches();
    switches.start_refusing_tessellation();
    let (mut engine, compiled, _cold_diagnostics) =
        build_with_probe_capturing_module(&source, kernel);

    engine
        .edit_param(
            reify_core::ValueCellId::new("WarmConsumer", "width"),
            Value::length(0.012),
        )
        .expect("edit_param must succeed against the WarmConsumer.width Length param");

    let warm = engine
        .build_snapshot(&compiled, ExportFormat::Step)
        .expect("build_snapshot must return a BuildResult after edit_param");

    // (a) #5951's door, over records from BOTH builds — same predicate as the
    //     cold twin.
    assert_no_contentless_write(
        "cold build + warm build_snapshot, non-tessellating kernel",
        &recorded(),
    );

    // (b) Still a candidate after the warm build, so a later tick — a repaired
    //     kernel, a re-realized body — can still do the real work.
    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![0],
        "the probe's compute node must STILL have empty `realization_inputs` \
         after a warm rebuild that could not project any content. A non-empty \
         value here means the warm path wrote content-free inputs, which is \
         #5951's door reopened on the surface #7332 just made reachable."
    );

    // (c) The degradation must not be silent on the warm surface either — and
    //     must be reported once per NODE, not once per trailing template. The
    //     fixture carries three, so a per-template report would read 4.
    assert_one_strand_warning(
        "warm build_snapshot, 3 trailing template(s)",
        &warm.diagnostics,
    );
}

/// Task #7332 (amendment): a node that tessellated on an EARLIER build and can
/// project nothing on THIS one must still be reported as stranded.
///
/// `a_warm_rebuild_still_refuses_contentless_inputs` above cannot
/// reach this: it uses a kernel that never tessellates, on either build, so the
/// probe's `realization_inputs` never become non-empty and
/// `emit_contentless_redispatch_warnings`' filter is never exercised in its
/// non-empty state. Flipping the refusal BETWEEN the two builds is what reaches
/// it — build 1 tessellates and writes the dependency record, build 2 cannot,
/// so the content guard strands the node while build 1's record still stands.
///
/// So the filter cannot ask `realization_inputs.is_empty()` (see
/// `RedispatchPassState::redispatched`). A filter that does reds this test at
/// ZERO warnings: the node stranded on a degraded result for a whole build with
/// no diagnostic, which is #5951's root complaint on the warm surface.
#[test]
fn a_warm_rebuild_that_loses_tessellation_still_reports_the_strand() {
    reset_records();
    let source = format!("{WARM_CONSUMER_STRUCTURE}{}", trailing_structures(3));
    let (kernel, switches) = SwitchableKernel::boxed_with_switches();
    let (mut engine, compiled, _cold_diagnostics) =
        build_with_probe_capturing_module(&source, kernel);

    // ── Premise: the COLD build really did tessellate and write the record.
    //    Without this the warm build has nothing non-empty to be dropped by,
    //    and the test degenerates into the existing never-tessellates case.
    let cold = recorded();
    assert!(
        cold.iter()
            .any(|r| r.kernel_backed_arg && r.any_realized_content),
        "premise: the cold build must complete a real post-hydration \
         redispatch, or there is no earlier-build record for the warm build's \
         report to be wrongly filtered by. Cold records: {cold:?}"
    );
    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![1],
        "premise: the cold build's write must stand on the node — NON-EMPTY \
         `realization_inputs` is exactly what the pre-amendment filter read as \
         `not stranded`, so an empty value here would make the warm assertion \
         below pass for the wrong reason"
    );

    engine
        .edit_param(
            reify_core::ValueCellId::new("WarmConsumer", "width"),
            Value::length(0.012),
        )
        .expect("edit_param must succeed against the WarmConsumer.width Length param");

    // The kernel loses tessellation for the warm build only.
    switches.start_refusing_tessellation();

    reset_records();
    let warm = engine
        .build_snapshot(&compiled, ExportFormat::Step)
        .expect("build_snapshot must return a BuildResult after edit_param");

    // The strand itself: re-admitted as a candidate, handed nothing.
    assert_no_contentless_write("warm build_snapshot after losing tessellation", &recorded());

    // ── THE PIN ────────────────────────────────────────────────────────────
    assert_one_strand_warning(
        "warm build_snapshot after losing tessellation, 3 trailing template(s)",
        &warm.diagnostics,
    );

    // And the record from build 1 really was still standing while that warning
    // was emitted — i.e. the assertion above was carried by the build-scoped
    // predicate and not by an incidentally-empty `realization_inputs`.
    assert_eq!(
        probe_node_realization_inputs(&engine),
        vec![1],
        "the earlier build's dependency record must survive a warm build that \
         could not project content: the record is the Realization→Compute \
         reverse edge, and dropping it would under-evict on a later \
         `edit_param`"
    );
}

/// Task #7332 (amendment): a warm redispatch must not NARROW the node's
/// `realization_inputs`.
///
/// The pass writes the record on every build, and the incoming set comes from
/// `realization_probe_args`, which DROPS any arg whose handle is symbolic. So a
/// REPLACING write would narrow a mixed-arg node's record from [solid, ghost]
/// to [solid] the first time `ghost` fails to realize. `deps.rs` builds reverse
/// edge #10 from this field, so a later `edit_param` moving `ghost` would then
/// under-evict this compute node. The write unions instead (see
/// `RedispatchPassState::redispatched`).
///
/// `a_symbolic_sibling_arg_is_kept_out_of_realization_inputs` is the cold twin
/// and cannot see this: there `ghost` is refused on the ONLY build, so the
/// record is [solid] from birth and narrowing has nothing to narrow from.
#[test]
fn a_warm_rebuild_must_not_narrow_realization_inputs() {
    reset_records();
    let (kernel, switches) = SwitchableKernel::boxed_with_switches();
    let (mut engine, compiled, _cold_diagnostics) =
        build_with_probe_capturing_module(MIXED_ARG_STRUCTURE, kernel);

    let solid_ref = geometry_cell_realization(&engine, "MixedConsumer", "solid");
    let ghost_ref = geometry_cell_realization(&engine, "MixedConsumer", "ghost");
    assert_ne!(
        solid_ref, ghost_ref,
        "the two geometry lets must bind DISTINCT realizations"
    );

    // ── Premise: with spheres still allowed, BOTH args hydrate, so the cold
    //    build records both realizations. This is the state the warm build can
    //    narrow away from; without it the test pins nothing.
    let cold_inputs = probe_node_realization_input_refs(&engine, MIXED_PROBE_TARGET);
    assert_eq!(
        cold_inputs.len(),
        1,
        "exactly one compute node must dispatch to `{MIXED_PROBE_TARGET}`; \
         got {cold_inputs:?}"
    );
    assert!(
        cold_inputs[0].contains(&solid_ref) && cold_inputs[0].contains(&ghost_ref),
        "premise: with `sphere()` still accepted, BOTH geometry args hydrate \
         and both realizations must be recorded — that two-entry record is \
         what the warm build must not narrow. Recorded: {:?}",
        cold_inputs[0]
    );

    engine
        .edit_param(
            reify_core::ValueCellId::new("MixedConsumer", "width"),
            Value::length(0.012),
        )
        .expect("edit_param must succeed against the MixedConsumer.width Length param");

    // `ghost` stops realizing for the warm build only, so the warm probe set
    // is [solid] alone.
    switches.start_refusing_spheres();

    reset_records();
    engine
        .build_snapshot(&compiled, ExportFormat::Step)
        .expect("build_snapshot must return a BuildResult after edit_param");

    // Premise: the warm build really did re-dispatch and really did write —
    // `solid` still carries content, so #5951's content guard is inert here.
    let warm = recorded();
    assert!(
        warm.iter()
            .any(|r| r.kernel_backed_arg && r.any_realized_content),
        "premise: the warm `build_snapshot` must reach the write — `solid` is \
         still hydrated, so the content guard cannot fire. Without a write \
         there is no narrowing to observe. Warm records: {warm:?}"
    );

    // ── THE PIN ────────────────────────────────────────────────────────────
    let warm_inputs = probe_node_realization_input_refs(&engine, MIXED_PROBE_TARGET);
    assert_eq!(
        warm_inputs.len(),
        1,
        "still exactly one node: {warm_inputs:?}"
    );
    assert!(
        warm_inputs[0].contains(&ghost_ref),
        "the newly-symbolic sibling's realization `{ghost_ref:?}` must SURVIVE \
         in `realization_inputs` across the warm rebuild. The pass's write \
         unions the probed set into the record rather than replacing it, \
         because the record is the Realization→Compute reverse edge \
         (`deps.rs` edge #10) and `realization_probe_args` drops symbolic args \
         — so a replacing write would delete the edge for the very realization \
         that just stopped resolving, and a later `edit_param` moving it would \
         under-evict this node. Recorded: {:?}",
        warm_inputs[0]
    );
    assert!(
        warm_inputs[0].contains(&solid_ref),
        "the still-hydrated body `{solid_ref:?}` must remain recorded: {:?}",
        warm_inputs[0]
    );
    assert_eq!(
        warm_inputs[0].len(),
        2,
        "the union must not DUPLICATE the re-probed `{solid_ref:?}` — \
         `compute_cache_key` debug-asserts that `realization_inputs` carries no \
         duplicate `RealizationNodeId`. Recorded: {:?}",
        warm_inputs[0]
    );
}
