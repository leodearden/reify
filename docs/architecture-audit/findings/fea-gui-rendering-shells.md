# Audit: GUI Rendering of Shell-Element FEA Results

**PRD path:** `docs/prds/v0_4/fea-gui-rendering-shells.md`
**Auditor:** audit-fea-gui-rendering-shells
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 13 (1 WIRED, 0 PARTIAL, 0 TODO, 12 FICTION, 1 DRIFT, 0 ORPHAN — gap count = total − WIRED − ORPHAN = 13)

## Top concerns

- **PRD is a stub with no decomposition section and zero tasks filed.** Every "mechanism the PRD assumes will exist at runtime" is a placeholder for v0.4-deferred work — yet several of those placeholders have **non-trivial design surface** (mid-surface↔extruded coordination, mixed shell/tet body rendering with unified colormap range, flipped-normal diagnostic) that will need their own architectural decisions before tasks can land. The "small (4-6 tasks)" estimate in §"Why deferred" looks optimistic against the actual mechanism count enumerated below.
- **Both upstream prereqs are themselves incomplete.** v0.3 `fea-gui-rendering.md` is largely FICTION end-to-end (see sibling finding — task 2924 pending with 14 deps, including GR-001 transitively via 3426). v0.4 `structural-analysis-shells.md` decomposition is queued but most tasks are deferred; the mid-surface extractor exists (`crates/reify-shell-extract/`) but engine-integration tasks T18-T20 have not landed. This PRD therefore stacks on two foundations that haven't shipped — gap propagation is total.
- **Stdlib `ShellStress.top/mid/bottom` is declared but unwired at the Rust persistent-cache layer.** `crates/reify-compiler/stdlib/solver_elastic.ri:352-356` declares the three-channel structure_def; `crates/reify-solver-elastic/src/shell_result.rs:78-86` defines `ShellElementStress { top, mid, bottom }`; **but** `crates/reify-eval/src/persistent_cache.rs:450-457` `ElasticResult` is flat `stress: Vec<f64>` with no `top/bottom/frame` fields. The GUI-side three-position stress toggle this PRD specifies has no upstream data path until that DRIFT closes.
- **There is no extrude-from-mid-surface + thickness reconstruction primitive.** `crates/reify-shell-extract/src/mid_surface.rs` emits per-vertex thickness as a free byproduct of medial extraction, but no code thickens a `MidSurfaceMesh` back into a closed shell-bodies-style surface mesh. The PRD's "extruded mode" default depends on this primitive existing somewhere (kernel? GUI? unclear from the PRD).

## Mechanisms

### M-001: Three-mode geometry-display toggle (mid-surface / extruded / both)

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 1; no display-mode store, no toggle UI, no view branching)
- **Evidence:** `grep -rn "geometry_display_mode\|extruded\|mid_surface\|midSurface" /home/leo/src/reify/gui` returns zero meaningful hits (only three.js node_modules and an unrelated `assistant message shell` test). `gui/src/stores/feaModeStore.ts` exposes no display-mode state. No `displayMode: 'mid' | 'extruded' | 'both'` field on any IPC type (`types.ts`/`types.rs`).
- **Blocks:** every visual-regression baseline in PRD §"Sketch of approach" final list (extruded mode is the default the PRD prescribes)
- **Note:** Persists per-document — needs a backing store. The "both" mode (extruded body + translucent mid-surface overlay) leans on overlay-group machinery that already exists for the deformed-shape overlay (`meshManager.ts:161`) but neither is wired to a display-mode discriminator.

### M-002: Extrude-from-mid-surface + thickness reconstruction primitive

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 1 "extruded" mode; no thickening primitive anywhere)
- **Evidence:** `grep -rn "extrude_thin\|thicken_mid\|reconstruct_thin\|fn extrude.*thickness" /home/leo/src/reify/crates` returns no matches. `crates/reify-shell-extract/src/mid_surface.rs:5` documents the output as "per-vertex through-thickness annotations" but offers no consumer; `mesher.rs:455` "shell-element-ready mid-surface mesh" is the *element-meshing* product, not a visual-display reconstruction. Per-vertex thickness is tested in `mid_surface.rs:1075-1145` but is just metadata.
- **Blocks:** M-001 default mode (extruded); the cantilever-flexure + pinched-cylinder visual baselines
- **Note:** Open whether reconstruction lives kernel-side (offset of the medial mid-surface by thickness/2 along the normal) or GUI-side (Three.js geometry routine over the IPC'd mid-surface + thickness scalar channel). PRD doesn't say. Either way no implementation exists.

### M-003: Three-position top/mid/bottom stress-channel toggle

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 2; no through-thickness selector, no consumer of multi-channel stress)
- **Evidence:** `feaModeStore.ts:12-13` `colorize.channel` is a single string ("vonMises", "displacement_magnitude") — no `'top' | 'mid' | 'bottom'` discriminator. `gui/src/types.ts:37` `scalar_channels?: Record<string, Float32Array>` is producer-agnostic; an ElasticResult populated with `vonMises_top`/`vonMises_mid`/`vonMises_bottom` channel keys could backfill, but no Rust emitter exists. PRD specifies a "max(|top|, |bottom|)" default — that's a derived channel (synthesised from two backing channels), no Rust/TS helper today.
- **Blocks:** the cantilever-flexure visual baseline (top-stress contour), correct bending-stress visualization on every flexure scene
- **Note:** Composition with the existing colormap-range modes (min/max/auto) is called out — but the existing colormap-range logic in `gui/src/viewport/colormap.ts` takes a single channel, no multi-channel coupling.

### M-004: ElasticResult `top` / `bottom` / `frame` Rust fields populated end-to-end

- **State:** DRIFT
- **Failure mode:** F5 (stdlib structure_def declares 3-channel shape; Rust runtime container still flat)
- **Evidence:** `crates/reify-compiler/stdlib/solver_elastic.ri:295-316` declares `ElasticResult { displacement, stress, frame, max_von_mises, converged, iterations }` and `:352-356` declares `ShellStress { top, mid, bottom }`. `crates/reify-eval/src/persistent_cache.rs:450-457` `pub struct ElasticResult { displacement, stress: Vec<f64>, max_von_mises, converged, iterations, solve_time_ms }` — **no `top/bottom/frame` fields**. `crates/reify-solver-elastic/src/shell_result.rs` defines `ShellElementStress { top, mid, bottom }` but it's per-element local-frame tensors, not yet bridged to the cache-layer `ElasticResult`. Header `ElasticResultHeader` (persistent_cache.rs:381) and serialization paths (`:697-758`) have no codepath for the shell channels. The shells PRD §"Stress through thickness" calls this contract explicitly.
- **Blocks:** M-003 (three-position toggle has no upstream data), M-009 (shell-normal overlay needs per-element frame), all shell visual baselines
- **Note:** Backward-compat alias `result.stress → result.stress.mid` is documented as a T18-T20 engine-integration responsibility in the shells PRD; not landed.

### M-005: Mixed shell/tet body rendering — single mesh, unified colormap

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 3; no per-element-kind tagging on MeshData, no unified range across kinds)
- **Evidence:** `gui/src-tauri/src/types.rs:177-260` `MeshData { vertices, normals, indices, scalar_channels, displaced_positions }` has no element-kind field, no per-region tag, no MPC-interface marking. `gui/src/viewport/colormap.ts` bakes a single range across the whole vertex array — which would actually produce a unified range *by accident* across a hypothetical mixed mesh, **but** the PRD prescription that the renderer "render shell sub-mesh as extruded thin region and tet sub-mesh as standard tet surface" requires distinguishing them, which the IPC schema can't express today.
- **Blocks:** the "mixed shell/tet body" visual baseline; correct extrude-thin-only-for-shell-regions display
- **Note:** Open architectural question: split into two MeshData objects per body (one per kind) or extend MeshData with an `element_kind` per-face/per-element channel.

### M-006: MPC-tied shell/tet interface band highlighting

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 3 "MPC-tied interface visualized as a thin highlighted band on toggle"; no interface metadata IPC'd, no highlight renderer)
- **Evidence:** `grep -rn "MPC\|interface_band\|mpc_tied" /home/leo/src/reify/gui` returns zero. Kernel side: `crates/reify-solver-elastic/src/mpc.rs` exists (MPC constraint plumbing) but no API to emit interface-vertex sets to a result/visualization-side consumer. The shells-PRD MPC tying tasks (T11-T12, part of `structural-analysis-shells.md` decomp) are deferred.
- **Blocks:** mixed-shell/tet body baseline "interface visible" assertion
- **Note:** Toggle implies a stable user-controllable state — needs another feaModeStore field or sibling overlay store.

### M-007: Shell-normal arrow-field debug overlay (toggleable)

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 4; no arrow-field overlay primitive in the viewport)
- **Evidence:** `grep -rn "ArrowHelper\|shell_normal\|normal_overlay" /home/leo/src/reify/gui/src` returns zero hits. The generic Three.js `ArrowHelper` is available in node_modules but not wrapped by any viewport component. The shell-normal data itself would need to flow on a new IPC channel (per-element normal vector array — not expressible by the current `scalar_channels: Record<string, Vec<f32>>` shape).
- **Blocks:** "flipped-normal failure case" visual baseline (PRD calls this "dominant diagnostic for 'why does my flexure stress look wrong'")
- **Note:** Cross-PRD: shares "per-element vector field overlay" shape with the rigid-body-mode arrows in v0.3 PRD M-014 (which is also FICTION). A shared overlay-layer abstraction would amortize across both — but neither side has built it.

### M-008: Thickness heat-map mode on mid-surface

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 5; no thickness scalar channel emitted by anyone)
- **Evidence:** `crates/reify-shell-extract/src/mid_surface.rs:1075-1145` emits per-vertex thickness (Rust-side, in `MidSurfaceMesh`). No code path serializes a mid-surface mesh through the engine→GUI IPC. No `"thickness"` channel name appears in `gui/src/stores/feaModeStore.ts` defaults (just `vonMises`, displacement_magnitude). The PRD itself notes "mostly a no-op visualization (everything is one color) [for v0.4 constant-thickness]; only meaningful with varying-thickness shells PRD".
- **Blocks:** —
- **Note:** Listed by PRD as "rendering primitive ready for v0.5 extension." Lowest-priority gap; included for completeness.

### M-009: Per-document persistence of geometry-display mode

- **State:** FICTION
- **Failure mode:** F1 (PRD §Sketch item 1 "Mode persists per-document")
- **Evidence:** `gui/src/stores/` document-state stores exist (e.g. `engineStore.ts`, `feaModeStore.ts`) but none has per-document persistence — the feaModeStore is process-wide singleton state. No serialization to / from the `.ri` document or a sidecar settings file.
- **Blocks:** UX expectation: closing+reopening a document reverts to default (extruded) rather than restoring the user's last choice.
- **Note:** Coupled to the broader "per-document GUI preferences" architecture, which isn't in evidence in either v0.3 GUI PRD or this one.

### M-010: Probe popup — three-stacked-card (top/mid/bottom)

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Open design questions"; depends on a probe system that itself is FICTION)
- **Evidence:** Probe system entirely is FICTION in v0.3 (`fea-gui-rendering` audit M-012, task 2964 pending — `grep -rn "ProbeSystem\|probe_at"` returns no hits). No `ProbePopup` component, no stacked-card rendering, no promotion logic per toggle state.
- **Blocks:** all shell-on-flexure visual baselines that include a probe.
- **Note:** Cross-PRD: depends on M-012 from v0.3 PRD findings.

### M-011: Unified colormap range across element kinds (mixed body)

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Open design questions"; range-computation logic exists but only over single channel of single MeshData)
- **Evidence:** `gui/src/viewport/colormap.ts` computes range over a single Float32Array (the per-vertex scalar channel of a single mesh). If a body is rendered as two MeshData objects (one per kind, per M-005), no cross-mesh range-coupling logic exists. If a body becomes one MeshData with element-kind tagging, the existing single-array range *would* be unified — but then the kind-specific extrude-display can't render. Architecture decision not made; either path requires new code.
- **Blocks:** mixed-shell/tet visual baseline (PRD specifies "continuous color scale spans the whole body")

### M-012: Visual regression scenes under `gui/test/fixtures/fea-shells/`

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Sketch of approach" trailing list; directory does not exist)
- **Evidence:** `find /home/leo/src/reify/gui/test -type d` returns only `gui/test/{visual,screenshots,fixtures}`. `ls gui/test/fixtures/` shows one file (`bracket.ri`) and no `fea-shells/` subdirectory. None of the four PRD-named scenes (cantilever flexure, pinched cylinder, mixed shell/tet body, flipped-normal failure) have fixture files. `gui/test/visual/run.ts:51-58` SCENARIOS list is single-entry per v0.3 findings.
- **Blocks:** all visual-regression coverage
- **Note:** Inherits from v0.3 `fea-gui-rendering` M-005 PARTIAL state (harness is wired but not CI-integrated and has no FEA fixtures). Even if this PRD's fixtures were added today, no CI runs them.

### M-013: Existing v0.3 GUI infrastructure reuse (mesh pipeline, colormap, probe, harness)

- **State:** WIRED (for the parts genuinely WIRED in v0.3) / FICTION transitively for the parts that are themselves FICTION in v0.3
- **Failure mode:** N/A for the WIRED subset; otherwise F1 transitively
- **Evidence:** PRD §"Why deferred" item 3 claims this is purely additive on v0.3 infrastructure. Cross-reference v0.3 `fea-gui-rendering.md` findings: scalar-channel IPC schema (M-006 PARTIAL), displaced_positions (M-007 PARTIAL), colormap utility (M-008 WIRED), FEA-mode store (M-009 WIRED), stress-contour rendering (M-010 FICTION), probe (M-012 FICTION), visual-regression harness (M-005 PARTIAL). The "additive" framing of this PRD only holds for the WIRED leaves.
- **Blocks:** —
- **Note:** This PRD's risk profile is "additive on a load-bearing foundation that is mostly aspirational." The single WIRED line for completeness.

### M-014: Auto-classification + body-segment tagging surviving into MeshData

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Sketch of approach" item 3 + open question about mixed-body colormap range; segmentation result exists but isn't propagated to GUI)
- **Evidence:** `crates/reify-shell-extract/src/segmentation.rs:196` exposes `segment_regions` and a `SegmentationResult` type carrying per-region kind + connectivity. No emitter wires this through `engine.rs` to a GUI-side region-tag channel. `gui/src-tauri/src/engine.rs:949` constructs MeshData with empty `scalar_channels` always (per v0.3 finding M-006). So even though the Rust side computes the segmentation, the GUI can't see it.
- **Blocks:** M-005 (mixed-body rendering needs the segmentation), M-006 (interface band needs the interface vertex set), M-011 (colormap-range decision needs per-region knowledge if per-kind ranges are chosen)
- **Note:** This is an interesting cross-cutting case: the *computation* lands kernel-side via the v0.4 shells decomposition; the *transport* to the renderer is unowned by either PRD.

## Cross-PRD breadcrumbs

- **`fea-gui-rendering.md` (v0.3 sibling)** owns every reusable GUI primitive listed in PRD §"Why deferred" item 3. Its own findings file shows 14 of 22 mechanisms are not WIRED — the additive framing assumes load-bearing primitives that are mostly aspirational. Direct dependency on M-005/M-006/M-007/M-010/M-012 from that PRD.
- **`structural-analysis-shells.md` (v0.4 sibling)** owns the kernel-side shell solver, the `ShellStress.top/mid/bottom` schema, the `ElasticResult.frame` field, the MPC plumbing, the mid-surface extractor (already partially landed in `crates/reify-shell-extract/`), and the segmentation API. Engine-integration tasks T18-T20 are unshipped; this PRD's IPC layer is the gating consumer.
- **`structural-analysis-fea.md` (v0.3)** owns ElasticResult, the engine integration (#2924, pending with 14 prereqs), the `@optimized` + ComputeNode dispatch. Transitive GR-001 (struct-ctor eval) blocker via 3426.
- **`compute-node-infrastructure.md`** owns tasks 3377-3385 that 2924 chains through.
- **`varying-thickness-shells.md`** PRD-relationship reference; the thickness heat-map (M-008) becomes meaningful when thickness varies. v0.5 work.
- **`composite-laminated-shells.md`** future per-ply stress display extends the top/mid/bottom toggle into a per-ply selector. Mentioned in PRD §"Out of scope".
- **`mesh-morphing.md`** noted in PRD §"Relationship to other PRDs" as a future composing target for live rendering; mid-surface morphs alongside original body. Not blocking.
- **`persistent-naming-v2.md`** the mid-surface entities (face/edge naming for BC attachment) are tracked there; the shell-normal debug overlay would benefit from stable element IDs but doesn't strictly require them at the visualization layer.

## Notable observations

- **The PRD's "small (4-6 tasks)" sizing claim does not match the enumerated mechanism count.** 13 distinct mechanisms in FICTION state, with at least 4 requiring architectural decisions (extrude reconstruction location, mixed-body IPC shape, per-element vector overlay primitive, per-document persistence) before tasks can be filed. The "4-6 tasks" estimate appears to count only the visible UI elements (toggle, overlay, heat-map) and undercount the IPC/data-path work behind each.
- **The "additive on existing v0.3 GUI" framing is **mostly aspirational** today.** v0.3 `fea-gui-rendering` findings show its core wiring (FEA→IPC bridge, probe system, diagnostic overlay, stress contour end-to-end) is FICTION. Building on it is sound *if* v0.3 lands; treating the v0.3 surface as a stable foundation in this PRD's prose is optimistic.
- **One DRIFT case worth flagging for Phase 3:** the `ShellStress` structure_def is declared in stdlib (`solver_elastic.ri:352-356`) but the Rust persistent-cache `ElasticResult` (`persistent_cache.rs:450-457`) is still flat with `stress: Vec<f64>` and no `frame` field. This is a "stdlib declared, Rust hasn't caught up" gap rather than a pure F1 absence — the contract exists on one side and not the other. Likely owned by shells-PRD task T16 (or T18-T20 engine integration), but worth noting because consumers of this PRD reading the stdlib in good faith will assume the runtime contract holds.
- **A surprising thing:** the v0.4 shells PRD's mid-surface extractor (`crates/reify-shell-extract/`) is **further along than I expected**, with extraction + segmentation + naming + mesher modules each around 1k-2k LOC. The kernel-side groundwork for several mechanisms here (per-vertex thickness, region segmentation, mid-surface mesh) is present but never bridged to the IPC. The wall between kernel-computed-but-untransported and GUI-consumed is the recurring failure pattern in this audit.
