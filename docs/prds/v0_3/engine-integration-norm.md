# Engine-integration norm

Status: normative + enforceable contract. Authored 2026-05-12 in interactive `/prd` session. Resolves cluster C-14 / gap **GR-017** per `docs/architecture-audit/gap-register.md`.

This document is the **norm** for the family of seams where producer crates (`reify-mesh-morph`, `reify-shell-extract`, `reify-kernel-*`, `reify-solver-elastic`, future kernels) plug into the `reify-eval` engine. It catalogs the seams, specifies per-seam intake disciplines, and gives `/prd`'s G1 a concrete checklist so the audit's "library callable in isolation but no engine consumer" pattern (8+ PRDs in cluster C-14) stops recurring.

This PRD is companion to:
- `docs/prds/v0_3/compute-node-contract.md` (owns one specific seam — ComputeNode dispatch).
- `docs/prds/v0_3/multi-kernel-phase-3.md` (owns the kernel-capability dispatcher seam and Convert-edge inventory).
- `scripts/audit-orphan-producers.sh` (the G-tool — code-side detector for Type-A producer-orphans). The norm is the forward-facing side (PRD-author tells the seam where the kernel plugs in); G is the backward-facing side (detects when nothing plugs in). Together they form a two-sided gate.

The audit's dominant failure mode — "incomplete/ill-formed implementation chain" (`preferences_implementation_chain_naming`) — is what this norm is designed to prevent for **engine-integration** specifically, the way `compute-node-contract.md` prevents it for **ComputeNode dispatch** specifically. Resolution mode is approach **B + H** per `preferences_implementation_chain_portfolio`: a contract document (§3 catalog + §4 consumer policy) with vertical-slice DAG (§12) and boundary-test sketch (§8).

## §0 — Purpose and supersession

No prior PRD prose is superseded. This document **adds** a normative artifact that prior and future PRDs should be read against. The seam catalog (§3) is a snapshot of the v0.3 corpus of engine seams; §6 names the F-infra (queued) and G-tool (shipped) infrastructure that consume and maintain the catalog.

The 8+ PRDs in cluster C-14 (mesh-morphing, structural-analysis-shells, hex-wedge-meshing, multi-kernel, freshness-4-variant, structural-analysis-fea — see GR-017 evidence list) are **grandfathered** per §9. Resolution happens task-by-task as natural occasion arises (e.g. mesh-morph wiring under CN-contract §8 task κ; shell-extract bridge under the in-flight GR-021 PRD).

## §1 — GR-017 summary

Cluster C-14 of the 2026-05-12 audit found six kernel-module surfaces shipped with no engine caller:

| Surface | State | Where ships | Where consumer should live |
|---|---|---|---|
| `reify-mesh-morph` (full crate) | FICTION at engine seam | `crates/reify-mesh-morph/` | `engine_build.rs::dispatch_volume_mesh` morph branch (absent) |
| `reify-shell-extract` (mid-surface mesh + segmentation + per-vertex thickness) | FICTION at engine seam | `crates/reify-shell-extract/` | `reify-solver-elastic` + GUI IPC bridge (absent) |
| `dispatcher::dispatch` (multi-kernel BFS planner) | FICTION at op-execute seam | `reify-eval/src/dispatcher.rs:383` | `execute_realization_ops` (calls `kernel.execute_with_history` directly, never `dispatch`) |
| `propagate_freshness_only` | PARTIAL — implementation + tests, no production caller | `reify-eval/src/freshness_walk.rs:50` | edit handlers (`engine_edit.rs::edit_param` / `edit_source`) |
| `dispatch_volume_mesh` | PARTIAL — 8-case truth-table dispatcher, `#[allow(dead_code)]`, no live caller | `reify-eval/src/engine_build.rs:2403` | `execute_realization_ops` for `ReprKind::VolumeMesh` outputs |
| `mesh_surface_to_volume_with_diagnostics` (Gmsh tet fall-back) | PARTIAL — kernel-side fn, no eval-side binding | `reify-kernel-gmsh/src/mesh_volume.rs:161` | `dispatch_volume_mesh` tet-fall-back closure (currently `FnOnce` placeholder) |

All evidence file:line citations from `docs/architecture-audit/findings/` (mesh-morphing M-012–M-018, multi-kernel M-004/M-014/M-015, freshness-4-variant M-013, hex-wedge-meshing M-017/M-018, structural-analysis-shells M-018). The pattern repeats with shape "audit M-NNN found `<symbol>` at `<file:line>` has zero non-test callers across the workspace."

Phase-3 synthesis §5d names the systemic effect: **inversion of expected PRD ordering** — code lands ahead of engine seams. The norm "every PRD decomposition includes its engine-integration phase" is the durable preventative.

## §2 — The norm

> **A kernel module is not "integrated" until a named engine seam wires it into a user-observable path.**

Three terms:

- **Engine seam** — a specific dispatch / walk / registration point in `reify-eval` (or the engine constructor) where producer modules plug in. The full catalog is §3.
- **User-observable path** — a chain that terminates at a user signal — CLI output, viewport state change, LSP behavior, error diagnostic, stdlib `.ri` example that runs in CI. Same definition as `/prd`'s G2 (see `references/gates.md`).
- **Named** — identifiable at PRD-resolve time as a specific call site (file:line) or dispatch entry (target string), in the implementing crate.

A PRD that introduces a kernel module MUST, at PRD-resolve time, state in its prose:

1. **Which engine seam it plugs into** — citing the §3 entry by name (e.g. "§3.2 realization-kind dispatch", "§3.4 ComputeNode dispatch via CN-contract §6").
2. **The named consumer call site** — file path + function name where the seam invokes the producer (e.g. `engine_build.rs::dispatch_volume_mesh` morph branch).
3. **The user-observable signal** — the same artifact `/prd`'s G2 demands for leaf tasks (`feedback_task_chain_user_observable`).

If the seam itself does not exist yet (e.g. the seam is being introduced by another in-flight PRD like CN-contract or multi-kernel Phase 3), the consumer-side PRD declares its dependency on the seam-owning PRD per `/prd`'s G4 (cross-PRD seam ownership).

If the seam catalog (§3) lacks an entry the PRD needs, the PRD's resolution adds a new §3 entry as part of its scope (see §13 question 2).

## §3 — Engine seam catalog (the contract)

Eight seams in the v0.3 engine, plus one deprecated entry. Each row cites the dispatch-point file:line; the registration mechanism; the kinds of mechanism that plug in; and the contract-owning PRD (if any).

### §3.1 — Operation-execute seam (geometry kernel call)

| | |
|---|---|
| **Call site** | `Engine::execute_realization_ops` — `crates/reify-eval/src/engine_build.rs:582, 831` |
| **Invoked as** | `kernel.execute_with_history(&geom_op)` on the engine's `&mut dyn GeometryKernel` |
| **Registration** | Construction-time via `Engine::with_registered_kernel` (`engine_admin.rs:374`) which calls `pick_lexmin_brep_kernel` (`kernel_registry.rs:182`) over `inventory::submit!`-collected adapters; one kernel slot per Engine in v0.3 |
| **Plug-ins** | `GeometryKernel` trait impls (OCCT, Manifold, Fidget, OpenVDB, Gmsh, future kernels) |
| **Contract owner** | `multi-kernel-phase-3.md` (GR-020 / cluster C-18) — current v0.3 single-kernel scope is acknowledged drift toward the multi-kernel target |

### §3.2 — Realization-kind dispatch seam

| | |
|---|---|
| **Call site** | Pattern: per-`ReprKind` output, switch to a specialized producer. Today the only instance is `dispatch_volume_mesh` (`engine_build.rs:2403`, `pub(crate)` `#[allow(dead_code)]`, **no live caller**). Future instances may exist for mid-surface mesh extraction, voxel ingestion, etc. |
| **Invoked from** | Should be invoked from `execute_realization_ops` when output `ReprKind` requires specialization (volume mesh: tet / hex / wedge / morph). Currently the call edge is absent (cluster C-14 evidence). |
| **Registration** | Code-side; truth-table or trait-dispatch in the dispatcher body. No external registry. |
| **Plug-ins** | Per-realization-kind specialized producers: Gmsh tet mesher, hex/wedge sweep producer, `reify-mesh-morph::morph`, future `reify-shell-extract::extract_mid_surface_mesh` |
| **Contract owner** | **This PRD** (no upstream owner today). New §3.2 entries are added by the PRD introducing the realization kind. |

This is the seam shape **most C-14 evidence converges on**: the dispatcher exists with `#[allow(dead_code)]`, the specialized producer ships in its own crate, the realization stage doesn't call the dispatcher.

### §3.3 — Multi-kernel dispatch / conversion-planning seam

| | |
|---|---|
| **Call site** | `dispatcher::dispatch` — `crates/reify-eval/src/dispatcher.rs:383`. Current production caller: `compute_realization_tolerance_budget` (`engine_build.rs:1157`) only, for stage-count probing. Not yet called from `execute_realization_ops` (audit multi-kernel M-004 evidence). |
| **Registration** | Capability descriptors via `inventory::submit!` at kernel-crate load time; collected through `kernel_registry.rs` (`pick_lexmin_brep_kernel`, `OnceLock`-built registry); `BTreeMap` ordering for determinism. |
| **Plug-ins** | Capability declarations per kernel crate (`(Operation, ReprKind)` supports tuples + `Convert { from: ReprKind }` edges). Convert-edge inventory is `multi-kernel-phase-3.md` §2. |
| **Contract owner** | `multi-kernel-phase-3.md` (GR-020 / cluster C-18). |

Distinct from §3.1: §3.1 is "which kernel handles the next op"; §3.3 is "plan a BFS path across (op, ReprKind) edges including conversions."

### §3.4 — ComputeNode dispatch seam

| | |
|---|---|
| **Call site** | `Engine::insert_compute_node` — `crates/reify-eval/src/graph.rs:522`; lowered from `eval_user_function_call` (`reify-expr/src/lib.rs:719`) when `CompiledFunction.optimized_target` is `Some(target)`. |
| **Registration** | Per-Engine via `Engine::register_compute_fn(target, ComputeFn)` (introduced by CN-contract §4; landing under CN-contract §8 task γ). Convention: each crate exposes `pub fn register_compute_fns(engine: &mut Engine)` called at engine construction. |
| **Plug-ins** | Stdlib `fn`s annotated `@optimized("target::name")` paired with a Rust trampoline. The annotation is the lowering trigger; the registry is the dispatch table. |
| **Contract owner** | **`compute-node-contract.md`** (GR-002 / cluster C-02). All normative content for this seam (cancellation, pending lifecycle, OpaqueState, consumer policy, trampoline signature) is **owned by CN-contract**. This PRD's §3.4 is a listing entry only. |

The CN-contract is the gold-standard exemplar of B+H for a single seam (see CN-contract §6 Consumer policy: "Origin does not enter the rule"; threshold ≥~50 ms; per-feature disposition table). This PRD's §4 is the corresponding cross-seam meta-policy.

### §3.5 — Constraint-solver seam

| | |
|---|---|
| **Call site** | Solver dispatch by name from constraint-evaluation paths (kinematic constraint solver invocation; specific call sites vary). |
| **Registration** | Per-Engine via `Engine::register_solver(name, Box<dyn ConstraintSolver>)` — `engine_admin.rs:497`. Setter pattern: `Engine::with_solver(...)` for the default slot (`engine_admin.rs:480`). |
| **Plug-ins** | `ConstraintSolver` trait impls (`libslvs` adapter the primary today). |
| **Contract owner** | `kinematic-constraints-v02.md` / `kinematic-constraints-toplevel.md`. |

### §3.6 — Freshness-only propagation walk seam

| | |
|---|---|
| **Call site** | `propagate_freshness_only` — `crates/reify-eval/src/freshness_walk.rs:50`. Zero non-test callers in production today (audit freshness-4-variant M-013 evidence). |
| **Registration** | None — this is a walk, not a registry. The "wiring" is whether edit handlers / kernel-completion paths **call** it. |
| **Plug-ins** | N/A. Candidate call sites: `engine_edit.rs::edit_param` / `edit_source` (currently use `mark_pending` bulk passes); kernel-job-completion paths flipping upstream Intermediate→Final. |
| **Contract owner** | `freshness-4-variant.md` (its own decomposition resolves M-013 — fix lives there, not here). |

Included in the catalog because it's a Type-A producer-orphan that fits the cluster-C-14 shape, even though the "plug-in" is a call edge rather than a registered impl.

### §3.7 — Cross-kernel attribute-propagation seam

| | |
|---|---|
| **Call site** | `propagate_via_kernel_attribute_hook` — `crates/reify-eval/src/kernel_attribute_hook.rs`; per-kernel hook via `ManifoldKernel::attribute_hook` returning `Some(self)`. |
| **Registration** | Per-kernel adapter; the `KernelAttributeHook` trait (`reify-types/src/geometry.rs`) is implemented by kernels that participate in attribute propagation across kernel boundaries. |
| **Plug-ins** | Per-kernel attribute-propagation implementations. Manifold's `propagate_attributes` body is currently a `Discarded`+WARN stub (audit multi-kernel M-018 / persistent-naming-v2 task 9). |
| **Contract owner** | **Contested** between `persistent-naming-v2.md` and `multi-kernel-phase-3.md` per `docs/architecture-audit/phase-3-breadcrumb-map.md` §3 reciprocal-ownership pair #2. Unresolved at norm-authoring time; this PRD lists the seam but does not assign the owner. |

### §3.8 — Check-time DFM measurement walk seam

| | |
|---|---|
| **Call site** | `Engine::measure_dfm_rules` — `crates/reify-eval/src/engine_constraints.rs:811`, invoked from `Engine::check` (`engine_constraints.rs:1346`) after `check_constraints_against_templates`. |
| **Invoked from** | `Engine::check` — a check-time walk over the module's `DFMRule` structure-instances. Realizes each rule's `subject : Solid` to a kernel handle from the engine's realized state, runs the matching metrology selector (overhang / draft), compares the result against the process capability, and routes the result + the rule's `DFMSeverity` through `dfm::diagnose`, emitting a DFMSeverity-tagged `{W,E}_DFM_OVERHANG` / `_DRAFT` / `E_DFM_UNDERCUT` diagnostic. Structurally a sibling of the `RepresentationWithin` interception in `dispatch_constraints`. |
| **Registration** | None — this is a walk, not a registry (sibling of §3.6); the plug-in is a call edge (whether `Engine::check` calls `measure_dfm_rules`). |
| **Plug-ins / selectors** | The overhang/draft measurement selectors (`unsupported_overhang_faces` / `min_draft_angle`) ride the **existing §3.1 op-execute / `GeometryKernel` query path** (`FaceNormal` / `tessellate` against the realized kernel handle, exactly as `fits_build_volume` rides `BoundingBox`) — no norm change for the selectors; only the pass (the walk) is the new seam. |
| **Contract owner** | `process-dfm-overhang-draft.md` (this PRD introduces the seam). Sibling: the GD&T conformance walk (`measure_gdt_conformance`, `gdt-geometric-zones-and-containment.md`) is the same seam shape — whichever lands second cross-references the first. |
| **Consumer policy** | No default kernel → no realized subject handle → the pass degrades to Indeterminate / no-op, **never** a false `Violated` (C1 invariant; guard at `engine_constraints.rs:812`, mirroring the `RepresentationWithin` empty-`achieved_repr_tol` → Indeterminate path). |

### §3.9 — Legacy: OptimizedImpl seam (deprecated)

| | |
|---|---|
| **Call site** | Engine evaluation path for fns annotated under the pre-ComputeNode shim. |
| **Registration** | `Engine::register_optimized_impl(target, Box<dyn OptimizedImpl>)` — `engine_admin.rs:415`. |
| **Status** | **Deprecated** by CN-contract §2. Existing registrations are grandfathered; new producers MUST use §3.4 ComputeNode dispatch. Migration on touch. |

### §3.10 — Seams excluded from this catalog

These seam-shaped surfaces are real but outside the in-engine norm:

- **GUI → backend event channel** (Tauri IPC). Owned by `gui-event-channel-inventory.md` (GR-016). Has its own catalog discipline; not duplicated here.
- **Debug-MCP RPCs**. Subsumed by the GR-016 PRD (gui-event-channel-inventory §2.3).
- **Compile-pipeline call sites** (e.g. auto-resolve orchestrator in `compile_*`). Compile-time, not engine-time; covered by per-PRD decomposition (auto-type-param-resolution and siblings).
- **CLI subcommand wiring** (`reify-cli`). User-surface, but not an engine seam — a sibling category that `/prd` G2 already covers via user-observable signal.

Catalog churn: new seams added by the PRD that introduces them, in the same commit as that PRD's resolution. See §13 question 2 for the governance question (Leo-owned).

## §4 — Per-seam consumer policy

A mechanism-kind-to-seam matrix. When authoring a PRD that introduces a kernel module, match the kind to the seam.

| Mechanism kind | Plug into | Notes |
|---|---|---|
| Geometry-kernel primitive op (boolean, fillet, chamfer, primitive-create, tessellate) | §3.1 op-execute via `GeometryKernel` impl | Multi-kernel selection is §3.3's job once enabled |
| Per-realization-kind specialized producer (tet mesher, hex/wedge sweep, mesh-morph, shell-extract, future voxel/sdf realizers) | §3.2 realization-kind dispatch | The C-14-canonical seam — §3.2's dispatcher (`dispatch_volume_mesh` for VolumeMesh) plus its call edge from `execute_realization_ops` |
| Cross-kernel conversion path (BRep→Mesh, Voxel→Mesh, Mesh→BRep, Sdf→Mesh) | §3.3 multi-kernel dispatch | Declared as `Convert { from: X }` capability edges; BFS-planned. Inventory in `multi-kernel-phase-3.md` §2 |
| Solver-shaped expensive computation (FEA, eigensolver, optimization, importers ≥~50 ms) | §3.4 ComputeNode dispatch | Per CN-contract §6 — threshold heuristic, cache/warm-state/cancellation/significance machinery applies |
| Kinematic constraint solver | §3.5 ConstraintSolver | Named slot in per-Engine registry |
| Cross-kernel attribute carry-through (selectors across booleans/fillets through different kernels) | §3.7 KernelAttributeHook | Trait implemented per kernel adapter |
| Edit-driven freshness propagation without value change | §3.6 `propagate_freshness_only` | Call edge from edit handlers; no registry |
| Legacy `@optimized` impl pre-dating CN-contract | §3.9 (grandfathered) | Migrate to §3.4 when the surface is touched |

A mechanism that plausibly fits multiple seams gets a PRD-time decision. Mesh-morph is the worked example (§7): it plugs into §3.2 (realization-kind dispatcher for VolumeMesh) **and** the call is wrapped at §3.4 (ComputeNode dispatch) for cache / warm-state / cancellation discipline. The two are orthogonal axes per CN-contract §6 (axis-1 = ComputeNode-wrapped; axis-2 = internal composition).

## §5 — G1 checklist for `/prd`

When `/prd`'s G1 (consumer named) is walked during PRD-authoring for any PRD that introduces a kernel-module mechanism, also walk this checklist. Concretely:

1. **Name the seam.** Pick from §3.1–§3.8. If none fit, escalate to add a new §3 entry (see §13 question 2).
2. **Name the consumer call site.** File:line (or function name) in `reify-eval` where the seam will invoke this PRD's producer. If the call site doesn't exist yet (the seam itself is in-flight), reference the seam-owning PRD as a prereq and recognize this PRD blocks until that PRD lands the call site.
3. **Name the user-observable signal.** Same artifact `/prd` G2 demands — CLI difference, viewport state, LSP behavior, diagnostic, or stdlib `.ri` example that runs in CI.
4. **Confirm the seam's owner.** If the seam is owned by another PRD's contract (CN-contract for §3.4; multi-kernel-phase-3 for §3.1/§3.3; persistent-naming-v2 *or* multi-kernel for §3.7 — contested), reference that contract as a hard prereq.
5. **Grandfather check.** If a producer already exists in `crates/kernel-*` and the G-tool (`scripts/audit-orphan-producers.sh`) lists it as orphan, either: (a) name the seam + plan the consumer task in this PRD's decomposition, or (b) add a `// G-allow: <reason> per engine-integration-norm §3.X; consumer pending task #NNNN` marker, mark the producer grandfathered, and defer integration. Option (b) is honest and acceptable per §9.

The `/prd` skill update under §12 task β embeds this checklist by reference. The hand-back paragraph names this PRD's path so a fresh `/prd` session loads it.

## §6 — Relationship to existing infrastructure

### §6.1 — G-tool (shipped)

`scripts/audit-orphan-producers.sh` + `scripts/cargo-audit-orphans` wrapper + baseline at `docs/architecture-audit/g-tool-baseline-report.md` (422 orphan candidates / 1306 pub-fns scanned, 2026-05-12). The tool detects Type-A producer-orphans: `pub fn`s in workspace `kernel-*` (and related) crates whose only callers are tests.

**Relationship to this norm:** G is the backward-facing detective; this PRD's §3 is the forward-facing prescriptive. G says "X has no caller"; §3 says "X plugs into seam Y; the caller is owned by PRD Z."

**Allow-list integration:** existing `// G-allow:` markers gain a recommended citation form:

```rust
// G-allow: realization-kind dispatch seam (engine-integration-norm §3.2);
//         consumer pending task #NNNN (mesh-morph engine wiring under CN-contract §8 task κ)
pub fn morph(...) -> Result<...> { ... }
```

Reason mandatory (existing G convention). PRD-norm citation makes the deferral auditable.

### §6.2 — F-infra (queued)

The audit-cadence infrastructure (Approach F per `preferences_implementation_chain_portfolio`) is queued separately. When F lands, it will run periodic audits consuming both G's detector output and this PRD's §3 catalog: "for each §3 seam, are its declared plug-ins all wired? for each orphan in G's baseline, does it have a §3 cross-reference (either as wired-and-allow-listed or as pending-with-task-link)?" This PRD declares §3 as the catalog F will consume; F's hooks into §3 are out of scope here.

### §6.3 — compute-node-contract.md

CN-contract owns §3.4 normatively. This PRD's §3.4 is a listing entry that defers all substantive content to CN-contract §2 (cancellation), §3 (pending), §4 (dispatch registry), §5 (OpaqueState), §6 (consumer policy), §7 (boundary tests), §8 (DAG).

The relationship is hierarchical:
- **CN-contract** = single-seam contract (the gold-standard exemplar of B+H for one seam).
- **engine-integration-norm** = cross-seam meta-policy (the umbrella that lists CN as one entry among seven).

This PRD does **not** redefine ComputeNode dispatch rules; it cross-references them. Same shape as multi-kernel-phase-3 §6 (which clarifies its relationship to CN-contract).

### §6.4 — multi-kernel-phase-3.md

multi-kernel-phase-3 owns §3.1 and §3.3. This PRD's §3.1 and §3.3 are listing entries; multi-kernel-phase-3 normatively specifies how the kernel registry, capability descriptors, Convert edges, BFS planner, and engine-level kernel selection compose.

## §7 — Worked example: mesh-morph engine wiring under the norm

Mesh-morphing PRD's task 2947 ("Wire `reify-mesh-morph::morph` into `engine_build.rs::dispatch_volume_mesh`") is the canonical C-14 case. After CN-contract §8 superseded the open mesh-morph wiring work into task κ, the wiring runs through the norm as follows.

**Step 1 — Name the seam.** Mesh-morph plugs into §3.2 (realization-kind dispatch). The dispatcher `dispatch_volume_mesh` is the §3.2 instance for VolumeMesh outputs; it already has tet / hex / wedge arms (truth table at `engine_build.rs:2403`) but no morph arm. Mesh-morph wiring adds the morph arm.

**Step 2 — Name the consumer call site.** `engine_build.rs::dispatch_volume_mesh` (currently `pub(crate)` `#[allow(dead_code)]`) gains a morph branch. Separately, `execute_realization_ops` gains the call edge **to** `dispatch_volume_mesh` for `ReprKind::VolumeMesh` outputs — without that call edge, all three existing tet/hex/wedge arms remain orphan as well.

**Step 3 — Name the user-observable signal.** CN-contract §8 task κ already specifies this: a `.ri` parametric design where varying a non-structural parameter triggers `dispatch_volume_mesh` → morph → reused FEA warm-state on subsequent solve; CLI `--verbose` shows `morphed: true`; ≥10× wall-clock reduction at 100K elements per mesh-morph PRD task 2953 acceptance.

**Step 4 — Confirm the seam's owner.** §3.2 has **no upstream owner** — it's a "this PRD owns the seam shape; the dispatcher implementer owns the realization-kind dispatcher instance." `dispatch_volume_mesh` is owned by hex-wedge-meshing PRD's task 2989 (done — defined the truth table) plus mesh-morphing PRD's task 2947 (pending — adds morph branch) plus this norm (specifies that the call edge from `execute_realization_ops` to `dispatch_volume_mesh` is part of the morph PRD's scope, not phantom). Additionally **§3.4** (ComputeNode dispatch) owns the cache/warm-state/cancellation wrapper around the morph call — per CN-contract §6's "axis-1: morph routes through ComputeNode" disposition and §8 task κ.

**Step 5 — Grandfather check.** `reify-mesh-morph::morph` and friends appear in G-tool's baseline as Type-A orphans today. Under §6.1, they get `// G-allow: realization-kind dispatch seam (engine-integration-norm §3.2); consumer pending CN-contract §8 task κ` markers until task κ lands. The allow-list sweep is `§12 task ε` (optional).

**Worked-example summary.** Mesh-morph is the **two-seam** case (§3.2 for the realization-kind branch + §3.4 for the ComputeNode wrap). Mechanism-to-seam matrix entry (§4 row 2) handles this: "per-realization-kind specialized producer" → §3.2; the §3.4 wrap is independent and decided at task κ's design time per CN-contract §6.

### §7.2 — Brief: shell-extract under the norm

The GR-021 PRD (in-flight, session prompt at `docs/architecture-audit/gr021-shell-extract-engine-bridge-session-prompt.md`) bridges `reify-shell-extract` to engine + solver + GUI. Under this norm:

- **Engine bridge**: §3.2 realization-kind dispatch for a mid-surface-mesh output kind (likely new realization-kind dispatcher, parallel to `dispatch_volume_mesh`). May be wrapped in §3.4 ComputeNode dispatch per the geometry-expensive heuristic in CN-contract §6.
- **Solver bridge**: `reify-solver-elastic::ElasticResult` consumer of mid-surface mesh; not a new seam — direct fn call inside the FEA solve path, gated by the realization-kind dispatch producing the mid-surface mesh first.
- **GUI bridge**: §3.10 — owned by `gui-event-channel-inventory.md` (GR-016), not this norm.

GR-021's PRD will declare its plug-in seams and pre-condition on this norm.

## §8 — Boundary test sketch (B+H component)

The norm's two-way boundary: facing PRD authors (does the catalog drive correct decomposition?) and facing the catalog (does §3 match the engine reality?).

### §8.1 — Catalog-side (does §3 match the engine?)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Every §3 entry has a real citation.** | This PRD lands. | `rg <call-site-symbol> crates/reify-eval/src/` returns the cited file:line for every §3 entry. Manual verification at PRD-resolve. (Repeated at F-cadence once F lands.) |
| **Every §3 seam has a contract owner OR is explicitly listed as orphan.** | This PRD lands. | §3.7 marked contested (resolution under future PRD-resolve); §3.2/§3.6 marked "this PRD owns the seam shape, instance-owners per row"; §3.4/§3.1/§3.3/§3.5 reference upstream contract. No row is silent. |
| **Catalog drift signal.** | A new engine seam ships in `reify-eval` without a §3 entry. | The owning PRD's decomposition includes a §3-entry addition task (§13 question 2 disposition). G-tool's allow-list sweep flags producer-orphans without §3 citation. |

### §8.2 — PRD-author-side (does `/prd` G1 walk land?)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Next kernel-module PRD passes G1 via §5 checklist.** | `/prd` skill `gates.md` references this PRD (§12 task β). | At PRD-resolve for a kernel-module PRD, the conversational walk includes the seam-naming step and surfaces a §3 entry. The PRD's prose names the seam, the call site, the signal. |
| **PRD introducing a new seam triggers catalog addition.** | A PRD's mechanism doesn't fit §3.1–§3.7. | `/prd` G1 escalates to "add a §3 entry as part of this PRD's resolution"; the PRD's commit pairs the new seam with the catalog update. |
| **Grandfather case is recordable.** | A PRD touches existing code that ships an orphan producer. | The PRD's task or follow-up adds `// G-allow:` marker citing §3.N and the future consumer task, instead of being unable to express "deferred but accountable." |

These boundary tests are evaluated at PRD-author time by reading the produced PRD; no automated harness. F-infra (when it lands) will run §8.1 row 1 automatically.

## §9 — Migration policy

**Grandfathered until touched.** Per `feedback_orchestrator_narrow_locks_favor_upfront_design`, wholesale retrofit of existing kernel modules under the norm is high-coordination work the orchestrator's narrow file locks make expensive. The norm applies to new PRDs authored after this lands; the eight or more cluster-C-14 stranded modules stay as-is until natural occasion arises:

- `reify-mesh-morph` → wired under CN-contract §8 task κ.
- `reify-shell-extract` → wired under GR-021 PRD (in-flight).
- `dispatcher::dispatch` engine integration → wired under `multi-kernel-phase-3.md` §8 Phase 2 (vertical slice).
- `dispatch_volume_mesh` live caller → wired under whichever of CN-contract task κ / mesh-morph task 2947 lands first (they're the same work).
- `mesh_surface_to_volume_with_diagnostics` eval-side binding → wired alongside `dispatch_volume_mesh` tet fall-back closure binding (mesh-morph task 2947 / CN-contract task κ).
- `propagate_freshness_only` production caller → wired under `freshness-4-variant.md`'s own decomposition resolving M-013 (not addressed here).

The G-tool baseline at `docs/architecture-audit/g-tool-baseline-report.md` is the current orphan inventory. Items retire naturally as their owning PRDs reach engine integration. The optional §12 task ε pre-emptively annotates the engine-seam subset of the baseline with `// G-allow:` markers citing §3, so G's `--strict` mode passes for the annotated subset before integration tasks land.

## §10 — Out of scope for this PRD

- **GUI → backend event channel** — owned by `gui-event-channel-inventory.md` (GR-016). The two PRDs are sibling norms: this one for in-engine seams; GR-016 for engine↔frontend.
- **Type B (consumer-with-stub) and Type C (parallel-not-bridged) gaps** — Phase-3-critique §1.3 sub-shapes that this norm does not address. Type B (e.g. `render_html_stub`) and Type C (e.g. NodeTraits vs NodePolicyOverrides) get individual cluster-level dispositions; G-tool's design also excludes B/C explicitly.
- **Grammar fictions** — `/prd`'s G3 (grammar gate) handles this; orthogonal to engine integration.
- **F-infra audit cadence implementation** — separate queued effort.
- **Code-side enforcement work beyond what G already ships** — G is done; this norm consumes G; further code-side detectors are F-infra territory.
- **Retroactive sweep of existing 40+ PRDs** to add §3 cross-references — per §9, these are touched naturally as their owning PRDs evolve. The mesh-morph and CN-contract cross-references (§12 tasks γ, δ) are the only proactive PRD-prose edits.
- **Compile-pipeline call-site analogues** — auto-resolve orchestrator's `compile_*` call site is a compile-time seam, not an engine seam; PRDs owning compile-pipeline mechanisms handle their own integration discipline.

## §11 — Cross-PRD relationship

| Other PRD / artifact | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/compute-node-contract.md` | this PRD lists, CN owns | §3.4 ComputeNode dispatch | compute-node-contract | wired — §3.4 defers all substantive content |
| `docs/prds/v0_3/multi-kernel-phase-3.md` | this PRD lists, multi-kernel-phase-3 owns | §3.1 op-execute, §3.3 dispatcher | multi-kernel-phase-3 | wired — §3.1/§3.3 defer |
| `docs/prds/v0_3/mesh-morphing.md` | consumes (§7 worked example) | §3.2 + §3.4 application | mesh-morph PRD + CN-contract task κ | §12 task δ adds cross-ref |
| `docs/architecture-audit/gr021-shell-extract-engine-bridge-session-prompt.md` (PRD pending) | consumes (§7.2 mention) | §3.2 (likely) + §3.4 (likely) | future GR-021 PRD | future — referenced for visibility |
| `docs/prds/freshness-4-variant.md` | this PRD lists; freshness-4-variant resolves the orphan call | §3.6 propagate_freshness_only | freshness-4-variant | listed; production caller fix belongs to that PRD |
| `docs/prds/persistent-naming-v2.md` | listed; ownership of §3.7 is contested | §3.7 KernelAttributeHook | contested with multi-kernel-phase-3 | unresolved per breadcrumb-map §3 #2 — flagged for future PRD-resolve |
| `docs/prds/v0_3/gui-event-channel-inventory.md` | sibling norm | (out of scope per §10) | gui-event-channel-inventory | sibling — GR-016 owns frontend↔backend IPC norm |
| `.claude/skills/prd/references/gates.md` | consumes (§5 G1 checklist) | gate G1 reference | `/prd` skill | wired upon §12 task β |
| `scripts/audit-orphan-producers.sh` (G-tool) | parallel infrastructure | normative complement to detection | G-infra (shipped) | wired — `// G-allow:` marker convention extended per §6.1 |
| `docs/architecture-audit/g-tool-baseline-report.md` | input artifact | orphan inventory | G-infra | baseline established 2026-05-12; subset annotated per §12 task ε (optional) |
| F-infra (queued) | future consumer | §3 catalog as audit input | F-infra (future) | future hook |

## §12 — Decomposition plan

Decomposition style: **B (vertical slice) + H (design-first / contract + boundary-tests)** per `preferences_implementation_chain_portfolio`. Each leaf names its user-observable signal. The DAG is small (5 leaves; 4 mandatory + 1 optional) because the deliverable is primarily a doc artifact + a `/prd` skill reference; the producer-side work is in other PRDs.

### Phase 1 — Foundation: the norm doc lands

- **Task α** — This PRD committed at `docs/prds/v0_3/engine-integration-norm.md`; `gap-register.md` GR-017 Notes updated with cross-link.
  - **Observable signal:** `git log -- docs/prds/v0_3/engine-integration-norm.md` returns the commit; `rg engine-integration-norm docs/architecture-audit/gap-register.md` returns the GR-017 Notes cross-link.
  - **Prereqs:** none.
  - **Crates touched:** none.

### Phase 2 — `/prd` G1 integration

- **Task β** — Update `.claude/skills/prd/references/gates.md` § G1 to cross-reference this PRD's §5 checklist when the authored PRD introduces a kernel-module mechanism. Add the citation paragraph (~5 lines).
  - **Observable signal:** `gates.md` diff contains the cross-reference. Next `/prd` author-mode session on a kernel-module PRD walks the seam-naming sub-step (verifiable by inspecting the next session's conversational prompts; or by reading the updated `gates.md` and confirming the trigger condition is named).
  - **Prereqs:** α.
  - **Crates touched:** none (`.claude/skills/prd/references/`).

### Phase 3 — Worked-example cross-references

- **Task γ** — Add a one-paragraph cross-reference in `docs/prds/v0_3/compute-node-contract.md` §6 (Consumer policy) pointing to this PRD's §3.4 entry + §7 worked example. Bidirectional linkage between CN-contract (owns the seam) and this norm (lists the seam).
  - **Observable signal:** CN-contract §6 diff contains the cross-reference; `rg engine-integration-norm crates/ docs/prds/` returns the citation.
  - **Prereqs:** α.
  - **Crates touched:** none.

- **Task δ** — Add a one-paragraph cross-reference in `docs/prds/v0_3/mesh-morphing.md` (near task 2947 description or in §"Relationship to other PRDs"). Cross-references this PRD's §7 worked example and notes that engine wiring runs through §3.2 + §3.4.
  - **Observable signal:** mesh-morphing PRD diff; doc lint clean.
  - **Prereqs:** α.
  - **Crates touched:** none.

### Phase 4 — Optional: allow-list sweep on engine-seam orphans

- **Task ε (optional)** — Sweep G-tool's baseline (`docs/architecture-audit/g-tool-baseline-report.md`) for the engine-seam subset of orphan candidates (mesh-morph public API, shell-extract public API, `dispatch_volume_mesh`, `mesh_surface_to_volume_with_diagnostics`, `propagate_freshness_only`, `dispatcher::dispatch` etc.). For each, add a `// G-allow: <reason> per engine-integration-norm §3.N; consumer pending task #NNNN` marker line preceding the `pub fn` declaration. Pure annotation-add diffs in `kernel-*` crates and `reify-eval`. Regenerate G-tool baseline.
  - **Observable signal:** Re-running `./scripts/audit-orphan-producers.sh --strict` against the annotated subset exits 0 (or excludes the now-annotated entries). Baseline diff shows the subset's orphan count drops to zero (now allow-listed with citation); total baseline count drops by the swept subset size; `rg "G-allow:.*engine-integration-norm" crates/` returns the new markers.
  - **Prereqs:** α.
  - **Crates touched:** `reify-eval`, `reify-mesh-morph`, `reify-shell-extract`, `reify-kernel-gmsh`, possibly `reify-kernel-occt` / `reify-kernel-manifold` for §3.7 hook entries. Annotation-only — no semantic code change.
  - **Optionality rationale:** purely operational hygiene; the §3 norm + §6.1 marker convention give the right policy regardless; this task only retires baseline-tracked entries early.

### Dependency view

```
α ─┬─→ β (/prd G1 integration)
   ├─→ γ (CN-contract cross-ref)
   ├─→ δ (mesh-morph PRD cross-ref)
   └─→ ε (optional G-allow sweep)
```

α is the only blocker. β/γ/δ/ε are independent of each other.

## §13 — Open questions (surfaced but not decided in this session)

1. **Machine-readable §3 catalog.** Today §3 is a human-readable markdown table. A future machine-readable form (e.g. `docs/architecture-audit/engine-seams.yaml`) would let F-infra consume the catalog programmatically and let G-tool validate `// G-allow:` citations resolve to real §3 entries. **Suggested resolution:** defer until F-infra design phase names the consumption shape; YAGNI until then.

2. **Catalog governance: who adds §3 entries?** When a new engine seam ships, who is responsible for adding its §3 entry? Options: (a) the implementing PRD's own scope (entry is part of the PRD's commit); (b) a designated `/prd` G1 sub-step that forces the PRD to either match an existing §3 entry or propose a new one; (c) an after-the-fact F-infra audit catches missing entries. **Suggested resolution:** (a) is simplest; (b) is the natural `/prd` G1 hook; the two can coexist. Decide when the first post-norm PRD encounters a missing seam.

3. **§3.7 (KernelAttributeHook) ownership.** Genuinely contested between persistent-naming-v2 and multi-kernel-phase-3 (breadcrumb-map §3 #2). Until owner is assigned, the seam stays catalog-listed-but-unowned. **Suggested resolution:** flag for a future PRD-resolve session that touches either parent PRD; do not block this norm.

4. **GR-021 PRD interaction.** The GR-021 (shell-extract bridge) PRD is in-flight (session prompt drafted). Whether GR-021 needs a new §3 entry (e.g. §3.2-shell-extract realization-kind dispatch) or reuses §3.2 with shell-extract as a second instance is GR-021's call. **Suggested resolution:** GR-021 author decides; if a new entry is needed, the catalog update lands in GR-021's commit per §13 question 2 option (a).

5. **Per-seam test-discipline templates.** §8.2 is hand-evaluated. A future enhancement: per-seam "what must the integration test prove" templates (analogous to CN-contract §7 producer-side/consumer-side tables). Defer until a second example PRD shows whether the template-shape is reusable.

6. **Relationship to compile-pipeline call sites.** This PRD scoped explicitly to engine seams. Some PRDs ship orchestrators that should be called from `compile_*` (e.g. auto-type-param-resolution M-009). A sibling norm for compile-pipeline call sites would parallel this one. **Suggested resolution:** YAGNI until the compile-pipeline producer-orphan pattern recurs across ≥3 PRDs (not the case today — auto-resolve is the main instance).

7. **Catalog churn cadence.** Should the catalog be reviewed on each release / quarter / never (read-and-respond only)? **Suggested resolution:** when F-infra lands, fold catalog-review into F's cadence. Until then, ad-hoc.
