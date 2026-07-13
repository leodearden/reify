# Invariant Registry

Canonical registry of principled, uniformly-enforced invariants. Seeded 2026-07-06 from the
bug-hotspot survey (`docs/notes/bug-hotspot-survey-2026-07-05.md`) per Leo's directive:
*look for principled invariants that can be made explicit and enforced uniformly.*

**Usage contract (meta-invariant INV-META-1):**
- Every task queued from a hotspot-program PRD cites the INV-id(s) it establishes or
  strengthens in its description, and the enforcement mechanism is part of the task's
  done-criteria (an invariant enforced only by intent is not established).
- Every registry entry names its enforcement mechanism: **type** (unrepresentable by
  construction), **test** (CI-gated), **lint/gate** (grep/audit-enforced), or **doc+test**
  (documented posture pinned by a locking test).
- Status values: `proposed` → `enforced(<mechanism>)`. Flip status in the same change that
  lands the enforcement.
- Enforcement posture (Leo, 2026-07-06): **fail-closed is the end-state everywhere.**
  Rollout per invariant: contract spec → one-shot warn-mode corpus sweep to batch-enumerate
  violations → fix bulk producers → flip to enforce. Each runtime enforcement point gets a
  break-glass env knob mirroring the main-gate ENFORCE/BYPASS pattern.

| ID | Invariant | Enforcement | Status | Owner (expected PRD) |
|---|---|---|---|---|
| INV-EVAL-1 | Every cell-eval result commits atomically across values/snapshot/cache/journal; skipped legs are explicit values, never omissions | type (`commit_cell_result` primitive, `CacheLeg::Skip`) | proposed | eval-cell-commit-substrate |
| INV-EVAL-2 | Engine cell-eval contexts always carry determinacy, runtime-diagnostics sink, and containment | type (`CellEvalCtx` required-args constructor) | proposed | eval-cell-commit-substrate |
| INV-EVAL-3 | Diagnostics survive cache hits: a node's diagnostics are replayed on every fast-path serve; post-pass detectors run identically on all eval paths | type+data (diagnostics stored in cache entry; shared detector registry) | proposed | eval-cell-commit-substrate |
| INV-EVAL-4 | snapshot.values and cache content-hashes agree after every eval | test (warn-first divergence audit → assert) | proposed | eval-cell-commit-substrate |
| INV-EVAL-5 | A cell is never scheduled before a producer it reads (graph completeness) | test interim (no-stale-Undef checker, task 4952); owned structurally by the unified DAG executor | proposed | eval-uniform-dependency PRD (existing) |
| INV-EVAL-6 | Async completions always propagate freshness and terminate in Final or Failed — never eternally Pending | type+test (completion transaction + panic boundary) | proposed | async-recalc Phase A (bookmark) |
| INV-BUILD-1 | Per-build engine state resets exactly once per build/tessellate entry point; per-build vs must-survive classification is explicit | type (`reset_per_build_state` exhaustive destructure; later `BuildSession` ownership) | proposed | engine-build-hardening |
| INV-BUILD-2 | Version/snapshot IDs are allocated and read through exactly one API each | type + lint (allocator helper; grep-gate on raw `next_version_id` arithmetic) | proposed | engine-build-hardening |
| INV-BUILD-3 | A cache-hit short-circuit produces the same observable side-effect set as the path it short-circuits | test (extracted `probe_realization_cache` + parity tests) | proposed | engine-build-hardening |
| INV-GEO-1 | Mesh handoffs satisfy the written interchange contract: producer obligations (closed, consistently wound, finite, index-valid) vs consumer-declared capabilities (weldedness) | type+test (MeshContract validator at the seam; fail-closed with `REIFY_MESH_CONTRACT` break-glass) | proposed | kernel-seam-contracts |
| INV-GEO-2 | Handle identity is (kernel, id); `extract_*` results are stable per parent within a session | type+test (KernelHandle-keyed tables, #4351; conformance property tests) | proposed | kernel-seam-contracts + engine-build-hardening |
| INV-GEO-3 | Every kernel's warm-state classifies every per-handle side table as persist/clear/rebuild; round-trips are drift-guarded | type+test (exhaustive `Self` destructure; per-kernel state-inventory test; gmsh leaf compiles/runs only under `cfg(has_gmsh)` — the stub `GmshKernel` has no side tables, so leaving it uninventoried is safe) | enforced(type+test) | kernel-seam-contracts |
| INV-GEO-4 | Stub and real kernels satisfy one shared contract-test suite | test (suite instantiated under both cfgs) | proposed | kernel-seam-contracts |
| INV-COMP-1 | No type information is silently discarded: every resolution/dispatch fallthrough yields a correct type or a diagnostic | type+test (exhaustive matches with named tail arms; Type-variant completeness test) | enforced(type+test) | compiler-type-hygiene |
| INV-COMP-2 | Builtin names are registered in exactly one place (disjointness by construction, not by test suite) | test interim (cross-registry drift test) → type (unified registry, Wave 3) | proposed | compiler-type-hygiene |
| INV-COMP-3 | Static operator/result types match the runtime truth table | test (table-driven `infer_binop_type` + static-vs-runtime parity test) | enforced(test) | compiler-type-hygiene |
| INV-GUI-1 | Every GuiState field has a declared sync mechanism (diffed or explicitly full-reload-only) | lint interim (field-coverage test) → type (derive, compile error on unclassified field) | proposed | gui-state-sync |
| INV-GUI-2 | Every engine-mutation entry point (GUI command, debug server, MCP/AI tools) flows through the same delta choke-point | test (architecture test) + routing | proposed | gui-state-sync (core + GUI/debug/watcher); ai-native-editing (AI/MCP entry point) |
| INV-GUI-3 | The `.ri` source is the canonical truth of the design for all mutations: every value-changing mutation (GUI slider/edit-box, AI/MCP `reify_set_parameter`/`reify_update_source`) writes back to `.ri` source; no mutation leaves a divergent engine-only live value | type+test (single source-write-back path used by all value mutations; test asserts `.ri` reflects the mutation and there is no ephemeral-only durable path) | proposed | ai-native-editing (+ user-slider durable-edit fix) |
| INV-FEA-1 | Every production engine declares its registration posture; full registration has one constructor | type+test (`Engine::new_production`; grep architecture test; locking tests for deliberate opt-outs incl. LSP) | proposed | compute-fea-hardening |
| INV-FEA-2 | The compute-dispatch boundary converts panics to structured Failed outcomes | type+test (catch_unwind wrapper; per-arg Undef regression tests) | proposed | compute-fea-hardening |
| INV-FEA-3 | NaN never enters an ordering | lint+test (`f64::total_cmp`/finite-guards; grep gate on `partial_cmp(...).unwrap_or` in numeric crates) | proposed | compute-fea-hardening |
| INV-META-1 | Tasks cite the INV they serve; every INV names its enforcement mechanism | audit (extend reify-audit / review checklists) | proposed | (registry itself) |

Related existing enforcement precedents in this repo (patterns to copy, cited in the survey):
exhaustive-struct-literal serialization drift guard (`elastic_result.rs`), strum completeness
tests for IR enums, `reset_dispatch_tallies()` bundling, `check_event_inventory.sh`
name-drift lint, the main-gate ENFORCE/BYPASS break-glass pattern, PTODO citation gate.
