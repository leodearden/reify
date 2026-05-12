# Audit: Multi-Kernel Geometry Dispatch

**PRD path:** `docs/prds/v0_2/multi-kernel.md`
**Auditor:** audit-multi-kernel
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 14

## Top concerns

- **Dispatch is built but unused at op execution.** `dispatcher::dispatch` exists, is well-tested in isolation, and ships with a `DispatchPlan` type — but the only production caller is the tolerance-budget side-channel (`compute_realization_tolerance_budget`). The actual op execution (`execute_realization_ops`) ignores the dispatcher entirely and routes every op through a single `&mut dyn GeometryKernel` that the engine picked once at startup via `pick_lexmin_brep_kernel`. The PRD's "per-operation basis" language is fiction at the op-execution seam.
- **No conversion edges are declared.** Zero production capability descriptors (OCCT/Manifold/Fidget/OpenVDB) declare any `Operation::Convert { from: X }` entry. The dispatcher's BFS expansion code is reachable only in tests, so the "B-rep → mesh → boolean → mesh" stack pattern the PRD highlights as the first integration motivation cannot actually be planned, let alone executed. Gmsh declares one Convert entry (surface→volume mesh) but that's for a separate v0.3 pathway.
- **Engine holds one kernel, not a set.** `Engine.geometry_kernel: Option<Box<dyn GeometryKernel>>`. There is no plumbing for routing distinct ops through distinct kernels, no per-handle ReprKind tracking (the handle's `repr` field is `Option<BRepKind>` — the B-rep sub-shape, not the kernel family), and no patchwork-assembly machinery. Many follow-on PRD claims ("`#kernel(...)` pragma override", project pin determinism) are downstream of structure that does not exist.
- **`#kernel(...)` pragma is parsed and dropped.** `module.kernel_pragma` is populated by the compiler but no engine code reads it. User-level kernel override is a fiction at the runtime seam.
- **`reify.toml` project pin parses but is not consumed.** `reify-config::Manifest` parses `[kernels]` table, validates kernel ids, supports the documented schema — but no other crate (engine, CLI, GUI) reads `Manifest::kernel_pins`. The PRD's "determinism follows from the pin" invariant has no runtime enforcement.

## Mechanisms

### M-001: `ReprKind` enum (BRep / Mesh / Sdf / Voxel)

- **State:** WIRED (with DRIFT)
- **Failure mode:** N/A (mechanism present); DRIFT note attached
- **Evidence:** `crates/reify-types/src/geometry.rs:100-115` defines the enum with **five** variants, not four — `VolumeMesh` was appended for v0.3 Gmsh surface→volume meshing. PRD §"Resolved design decisions" pins "four entries"; the as-built enum is five.
- **Blocks:** None directly; informational drift
- **Note:** Code carries a docblock acknowledging the extension is non-breaking and ordering-preserving, but the PRD itself was not amended. A reader trusting the PRD's "BRep | Mesh | Sdf | Voxel — four entries" line will not anticipate the `VolumeMesh` variant.

### M-002: `CapabilityDescriptor { supports: Vec<(Operation, ReprKind)> }`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/geometry.rs:275-308`. Feasibility-only, no `cost_hint` / `error_factor` per PRD. Each kernel adapter constructs one (OCCT register.rs:91, Manifold, Fidget register.rs:102, OpenVDB register.rs:100).
- **Blocks:** None
- **Note:** Shape matches PRD verbatim; `supports` predicate (geometry.rs:320) and `supports_any_repr` (line 346) are tested.

### M-003: Static linker-collection via `inventory::submit!` (compile-time registration)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/geometry.rs:409` (`inventory::collect!(KernelRegistration)`); OCCT submits in `crates/reify-kernel-occt/src/register.rs:184` gated `cfg(has_occt)`; Manifold/Fidget/OpenVDB submit in their respective `register.rs`. `reify-eval/src/kernel_registry.rs` materialises via `OnceLock` into a lex-ordered `BTreeMap`.
- **Blocks:** None
- **Note:** Resolves arch §16 open Q #8 as PRD claims. Duplicate-name and duplicate-`(op, repr)` checks fire via `debug_assert!` and `tracing::warn!`.

### M-004: Per-op kernel dispatch (PRD core claim: "select... on a per-operation basis")

- **State:** FICTION
- **Failure mode:** F6 (dispatch infrastructure leaned on but absent at the op-execution seam)
- **Evidence:** `crates/reify-eval/src/dispatcher.rs:383 dispatch(...)` exists and is well-tested for BFS plan ranking. But `engine_build.rs:1625 execute_realization_ops` calls `kernel.execute_with_history(&geom_op)` on a single `&mut dyn GeometryKernel` — there is NO call to `dispatch()` in this path. The sole production caller of `dispatch()` is `compute_realization_tolerance_budget` (engine_build.rs:1157), used only to compute conversion-stage count for tolerance-budget shrink. Module docblock (dispatcher.rs:33-41) explicitly acknowledges: "It does NOT yet wire dispatch into op execution in `geometry_ops.rs`; the kernel-registry mechanism + OCCT adapter migration that consumes [`dispatch`] is task 2642's responsibility." Engine docblock (engine_admin.rs:325-333) calls this out: "In v0.3 once additional adapters ship, the per-op dispatch decision moves into [`dispatcher::dispatch`]."
- **Blocks:** Stack pattern (M-014), patchwork pattern (M-015), Manifold for mesh Booleans (M-009), Fidget for SDFs (M-010), OpenVDB for voxels (M-011), `#kernel(...)` pragma override (M-016)
- **Note:** This is the central PRD claim. The infrastructure scaffold is in place; the actual switching never happens.

### M-005: Single engine kernel field (architectural shape)

- **State:** DRIFT
- **Failure mode:** F5 (shape mismatch between PRD and implementation)
- **Evidence:** `crates/reify-eval/src/lib.rs:291 geometry_kernel: Option<Box<dyn GeometryKernel>>`. `with_registered_kernel` (engine_admin.rs:374) picks ONE adapter via `pick_lexmin_brep_kernel` and stores it. The PRD describes a runtime that "selects among" multiple kernels per op — the implementation stores one and forwards everything to it.
- **Blocks:** M-004, M-014, M-015
- **Note:** PRD allows for v0.2 single-kernel scope per the "v0.2 single-kernel scope" note in engine_admin.rs docblock — but the PRD itself does NOT say v0.2 is single-kernel-only. The drift is between code (single-kernel "v0.3 will fix") and PRD prose (multi-kernel target).

### M-006: Cache key `(entity_id, repr_kind, tol: f64)` with partial-order tolerance lookup

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/realization_cache.rs:54-149`. `insert(entity, repr_kind, tol, val)` and `lookup(entity, repr_kind, tol)` use partial-order ("tighter satisfies looser") matching. Used in `execute_realization_ops` (engine_build.rs:1666) but only with `ReprKind::BRep` hard-coded.
- **Blocks:** Future multi-repr realization caching (M-014/M-015) needs the lookup callers to pass varying `repr_kind` — currently all callers pass `ReprKind::BRep` (engine_build.rs:1666; e.g. the `BUDGET_QUERY_TRIPLE_V02` hard-codes `BRep`).
- **Note:** Cache key shape matches PRD verbatim. Only one consumer call site exercises it, and that site pins `BRep`.

### M-007: Conversion-stage edges (`Convert { from: X }` entries in capability descriptors)

- **State:** FICTION
- **Failure mode:** F6 (BFS expansion present but no edges to expand over)
- **Evidence:** `dispatcher.rs:433-445` enumerates Convert entries during BFS expansion. Production descriptors: OCCT (`register.rs:93-136`) — zero Convert entries; Manifold — zero (docblock at `register.rs:23` explicitly calls this out as future work); Fidget (`register.rs:104-110`) — zero; OpenVDB (`register.rs:102-109`) — zero. Only gmsh declares a Convert entry (surface mesh → volume mesh, `crates/reify-kernel-gmsh/src/register.rs:96`), which is a v0.3 pathway. OCCT register.rs:26-34 documents the missing `(Convert { from: BRep }, Mesh)` entry as a v0.3 forward-compat plan.
- **Blocks:** M-009 (Manifold mesh Booleans — needs OCCT to tessellate BRep→Mesh), M-010 (Fidget SDF realization from `field def`), M-011 (OpenVDB voxel from imported), M-014, M-015
- **Note:** BFS algorithm is correct; nothing exercises it in production. Tests inject synthetic Convert edges via fixture descriptors (`dispatcher.rs:822 dispatch_single_conversion_chain`).

### M-008: Deterministic kernel selection via lex-min tie-break

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/kernel_registry.rs:133 pick_lexmin_kernel`, `pick_lexmin_brep_kernel` (line 182); dispatcher uses `BTreeMap` for ordered iteration (dispatcher.rs:382-422); `BTreeSet<ReprKind>` seed order (line 402); registry built once via `OnceLock` (line 68).
- **Blocks:** None
- **Note:** The "selection deterministic given pinned runtime configuration" PRD claim has structural support — though "pinned runtime configuration" itself (project pin) is FICTION at the consumer side (see M-013).

### M-009: Manifold integration (mesh Booleans, BRep→Mesh stack pattern)

- **State:** PARTIAL
- **Failure mode:** F6 (kernel exists, registers a descriptor, has working FFI for booleans+tessellate, but dispatch never selects it for an OCCT-input op)
- **Evidence:** `crates/reify-kernel-manifold/src/kernel.rs:152-178` implements boolean ops via `manifold3d` 0.1; `register.rs` declares `(BooleanUnion/Difference/Intersection, Mesh) × 3`. **No** `Convert { from: BRep }` edge exists, so dispatcher BFS cannot route OCCT-BRep input through OCCT-tessellate to Manifold-Mesh-boolean. Manifold's `query` and `export` are stub (`kernel.rs:50-52 STUB_MSG`). PRD's "first integration is Manifold" remains structurally untrue at runtime — Manifold can be instantiated but never selected for a user-visible op.
- **Blocks:** M-004 wiring; PRD §"Integration sequence" Manifold-first claim
- **Note:** `attribute_hook` impl is present but `propagate_attributes` is a `Discarded`+WARN stub (persistent-naming-v2 PRD task 9). The dispatcher integration tests use fixture descriptors with synthetic Convert edges, not the production capability tables.

### M-010: Fidget integration (`field def`-as-geometry SDF realization)

- **State:** PARTIAL
- **Failure mode:** F6 (kernel exists, supports SDF Booleans, but no path from `field def` to a Fidget-routed dispatch)
- **Evidence:** `crates/reify-kernel-fidget/src/kernel.rs:240-300` implements Sphere/Box/Union/Difference/Intersection via fidget tree construction; `register.rs:104-110` declares `(BooleanUnion/Difference/Intersection, Sdf) × 3`. No `Convert { from: ... }` entry. The `field def` evaluation path (`reify-compiler/src/compile_builder/fields_phase.rs`) does NOT route any expression into a Fidget realization — the PRD's "field def values with SDF semantics realize directly via Fidget rather than being meshed through OCCT" is unbacked. Search for any `field def` → kernel routing in `reify-eval` returns nothing relevant.
- **Blocks:** M-004 wiring; geometry-field bidirectionality (arch §10.6) is still B-rep-meshed
- **Note:** Fidget descriptor entries do not include any primitive constructors (no `(PrimitiveSphere, Sdf)`); even the bare primitives Fidget's `execute` handles are not dispatchable. The fidget kernel.rs handles Sphere/Box at execute-time but the descriptor only advertises Boolean ops.

### M-011: OpenVDB integration (voxel-octree stack; imported field source)

- **State:** PARTIAL
- **Failure mode:** F6 (kernel ships with stub `execute`, real impl gated on `cfg(has_openvdb)`; descriptor declares Booleans but the voxel realization path is absent)
- **Evidence:** `crates/reify-kernel-openvdb/src/kernel.rs:65-66 fn execute → STUB_MSG`. `kernel_real.rs` exists with real impl (`cfg(has_openvdb)`); `ingest.rs` provides imported-field-source ingestion as documented v0.2 follow-up (lib.rs:14). Descriptor declares `(BooleanUnion/Difference/Intersection, Voxel) × 3`. No `Convert { from: ... }` entry. The imported-field-source pathway (separate `imported-field-source.md` PRD) is the actual consumer; no dispatcher routing fires for voxel ops in v0.2.
- **Blocks:** M-004 wiring; arch §10.5 voxel-octree stack pattern
- **Note:** Cross-PRD breadcrumb to `imported-field-source-hdf5-csv.md`.

### M-012: Truck dropped from v0.2 (PRD explicit non-goal)

- **State:** WIRED (as a non-goal)
- **Failure mode:** N/A
- **Evidence:** No `reify-kernel-truck` crate exists. `reify-config/src/lib.rs:14` documents that Truck is rejected at the manifest parser. PRD §"Truck dropped from v0.2".
- **Blocks:** None
- **Note:** Cleanly absent as PRD intends.

### M-013: Project pin via `reify.toml [kernels]` table

- **State:** PARTIAL
- **Failure mode:** F2 (mechanism implemented at the parse boundary, unused at the consumption boundary)
- **Evidence:** `crates/reify-config/src/lib.rs:1-200` defines `Manifest::from_toml_str`, `KernelId`, `KernelPin`, `Manifest::kernel_pins` iterator. `kernel_name_consistency.rs` cross-checks adapter NAME constants against `KernelId::Display`. **But** a workspace-wide grep for `Manifest::` / `reify_config::Manifest` / `kernel_pins` outside `reify-config/` itself shows no consumers (`reify-eval`, `reify-cli`, `gui-tauri` don't read the kernel pins). The PRD's "determinism follows from the pin; the cache does not need to know about kernel versions because a version change forces a process restart" is unenforced — there is no code that compares the registry's actual `(name, version)` against the pinned set, refuses to start on mismatch, or even reads the manifest at startup.
- **Blocks:** Determinism contract claimed by PRD; downstream cache invariants
- **Note:** Schema is clean, fully tested at the parser layer. Pure consumer-side fiction.

### M-014: Stack pattern (B-rep → mesh → boolean → mesh → optional B-rep reconstruction)

- **State:** FICTION
- **Failure mode:** F6 (PRD names this as the motivating use case; nothing in code instantiates it)
- **Evidence:** No code in `reify-eval` or `reify-compiler` orchestrates a per-realization chain of differing reprs. The realization graph (`reify-eval/src/graph.rs RealizationNodeData`) carries `value_inputs`, `resolution_inputs`, `realization_inputs` — none of these distinguish or sequence by `ReprKind`. There is no "B-rep reconstruction" code path; mesh-to-BRep conversion is not implemented in any kernel.
- **Blocks:** PRD §"Sketch of approach" stack-pattern motivation; arch §10.5
- **Note:** Even with dispatcher wired and Convert edges declared (M-004, M-007), the stack pattern needs additional orchestration to thread heterogeneous handles through ordered ops — the engine's `execute_realization_ops` assumes one kernel handles all ops in a realization linearly.

### M-015: Patchwork pattern (heterogeneous reps in one assembly; spanning ops materialize)

- **State:** FICTION
- **Failure mode:** F6 (PRD claims "mostly already true — RealizationNode keying just needs to admit `repr_kind`"; the keying admits it on the cache side but assembly composition does not)
- **Evidence:** No "assembly" abstraction tracks per-component ReprKind; no "spanning operation" code path (visualization, interference checks) requests "compatible realizations on demand"; the GUI viewport tessellates via a single `kernel.tessellate(...)` call (`engine_build.rs:1461`). The `compute_tessellation_budgets` path hard-codes `BUDGET_QUERY_TRIPLE_V02 = (BooleanUnion, BRep, &[BRep])` — visualization is BRep-only.
- **Blocks:** Arch §10.5 patchwork; v0.3 FEA PRD's multi-rep stack-pattern claim
- **Note:** PRD's optimism ("mostly already true") understates the gap. Cache key shape is in place; nothing else is.

### M-016: `#kernel(...)` user-level pragma override

- **State:** PARTIAL
- **Failure mode:** F2 (parsed but unconsumed)
- **Evidence:** `crates/reify-compiler/src/module_pragmas.rs:682-758` parses `#kernel(<ident>)` into `module.kernel_pragma: Option<String>`; emits a Diagnostic::warning if v0.1 prose was expected and registers the chosen kernel name. Workspace grep for `kernel_pragma` outside the compiler/parser tests yields zero engine consumers (the only matches in `reify-eval` are absent). The PRD ("users can override via `#kernel(...)` pragma") has no runtime enforcement.
- **Blocks:** User-level override use case; PRD §"Sketch of approach"
- **Note:** Pragma is also gated against the v0.2 `KernelId` enum (occt/manifold/fidget/openvdb/gmsh) via the parser's accept-list (`module_pragmas.rs:41`), so misnamed kernels are rejected at compile time. But a valid name has no effect.

### M-017: Long-chain diagnostic (>2 stages and >500ms wall warns)

- **State:** PARTIAL
- **Failure mode:** F2 (predicate + diagnostic builder ready, no production caller)
- **Evidence:** `crates/reify-eval/src/dispatcher.rs:125 is_long_chain_realization`, `dispatcher.rs:179 long_chain_diagnostic`, env-var override resolver, `DiagnosticCode::LongChainRealization` typed code. Comprehensive test suite. Inline TODO at dispatcher.rs:150: "TODO(task-2642): wire this builder into the realization timing loop in `geometry_ops.rs`". `engine_build.rs::execute_realization_ops` does NOT call `is_long_chain_realization` — the diagnostic builder is dead-on-arrival in production. **And** without M-007 (Convert edges) and M-004 (per-op dispatch), there can't be a chain longer than zero stages in production anyway.
- **Blocks:** Operator visibility for the (currently impossible) long chains
- **Note:** Scaffolding-with-no-caller is the most-common shape in this PRD's audit.

### M-018: `KernelAttributeHook` cross-kernel attribute propagation

- **State:** PARTIAL
- **Failure mode:** F3 (one kernel exposes a hook; the body is a WARN stub)
- **Evidence:** `crates/reify-types/src/geometry.rs::KernelAttributeHook` trait. `crates/reify-eval/src/kernel_attribute_hook.rs propagate_via_kernel_attribute_hook` engine-side dispatcher exists and is tested. `ManifoldKernel::attribute_hook` returns `Some(self)` but `KernelAttributeHook::propagate_attributes` for Manifold returns `Ok(KernelAttributeOutcome::Discarded)` with a `tracing::warn!(reason="task_9_pending")` — the MeshGL walk is deferred to persistent-naming-v2 PRD task 9. Fidget/OpenVDB inherit `None` default (fall-through to computed selectors).
- **Blocks:** Persistent-naming-v2 task 9; selector resolution across kernel boundaries
- **Note:** Cross-PRD breadcrumb to `persistent-naming-v2.md` (not in this audit's scope).

## Cross-PRD breadcrumbs

- **`per-purpose-tolerance.md`** — owns the partial-order tolerance vocabulary that the cache key (M-006) uses, and shares the 500ms long-chain threshold with M-017.
- **`persistent-naming-v2.md` task 9** — owns the real `propagate_attributes` body for ManifoldKernel (M-018).
- **`imported-field-source.md` / `imported-field-source-hdf5-csv.md`** — owns the actual consumer for OpenVDB ingestion (M-011's "imported field source" rationale).
- **`structural-analysis-fea.md`** — depends on `VolumeMesh` ReprKind (M-001's drift, the v0.3 extension) for Gmsh surface→volume meshing.
- **`compute-node-infrastructure.md`** — adjacent dispatch story for stdlib `fn`-as-ComputeNode; analogous "infrastructure scaffold without runtime activation" pattern.
- **Arch §10.5 (stack/patchwork)** — the canonical document this PRD points at; the patterns themselves are unimplemented (M-014, M-015).
- **Arch §16 open Q #8** — PRD claims to resolve this via compile-time `inventory` registration; resolution is structurally correct (M-003).
