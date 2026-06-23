# Capability Manifest — engine-unified-build-dag

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/engine-unified-build-dag.md`. Evidence verified at `HEAD b0077500f5` (re-locate at implementation time). **No novel `.ri` syntax** (`fillet(b,e,r)`, `edges_at_height`, `fits_build_volume` all parse) → grammar-fixture N/A. **No absolute-accuracy numeric bounds** → numeric-floor N/A (the η assertions are *non-equality* of volumes + *definite-verdict* of a constraint, not tolerances).

## δ — `engine_fixpoint.rs` worklist driver + cycle contract (leaf+intermediate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| Populated `ReverseDependencyIndex` edge graph | capability→producer + DAG-direction | `producer:Part1-α/β` (`engine-build-dag-substrate.md`) — **upstream** | **PASS** |
| `NodeId` enum (Value/Constraint/Realization/Resolution/Compute) | wired-on-main | `grep:crates/reify-eval/src/cache.rs:18` (design ledger C3 — no new kinds) | **PASS** |
| `DiagnosticCode` additive (`EvalCycle`, `EvalUnresolved`) | wired-on-main | `#[non_exhaustive]` `DiagnosticCode` in `crates/reify-core/src/diagnostics.rs` (design ledger C14) | **PASS** (additive) |
| `DeterminacyState::Determined` readiness gate | wired-on-main | design ledger C13 (`reify-ir/.../value.rs`) | **PASS** |
| `BTreeSet<DebugOrd>` determinism | wired-on-main | `DebugOrd` deterministic tie-break, `dirty.rs:253` (C2) | **PASS** |

## ε — geometry-path executors + rewrite arm + cross-sub resolution + C7 retirement (intermediate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `rewrite_geometry_queries` (new FunctionCall-args arm) | wired-on-main (extend) | `grep:crates/reify-eval/src/geometry_ops.rs:1908` + fall-through `:1942` (C5) — ε adds the arm | **PASS** (producer of the arm) |
| `resolve_geometry_handle_arg` cross-sub/`IndexAccess` resolution | wired-on-main (extend) | `grep:crates/reify-eval/src/geometry_ops.rs:4208` (ValueRef-only today; ε adds member-access) | **PASS** (producer of the addition) |
| `execute_realization_ops` + rollback (`handle_start`→`truncate`) | wired-on-main | design ledger C9 (`engine_build.rs:3754/:4592`) — wrapped verbatim | **PASS** |
| kernel-less `SimpleConstraintChecker` (no trait break) | wired-on-main | design ledger C6 (`reify-constraints/src/lib.rs:55`) | **PASS** |
| 3205 curated-fillet **machinery** (3-arg IR, `resolve_subhandle_list`, per-edge FFI, `E_EMPTY_SELECTION`) | capability→producer + DAG-direction | main `Fillet` is **2-arg** `grep:crates/reify-ir/src/geometry.rs:585` → NOT on main → `producer:task-3205` (re-scoped) **upstream** | **PASS** (gated; the absence on main is exactly why it must be an upstream prereq) |

## η — unified-only acceptance: 3205 + 4275 e2e (leaf — the anti-inversion linchpin)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `fillet_curated_edges_3205_e2e`: 3-arg fillet machinery | capability→producer + DAG-direction | `producer:task-3205` (re-scoped machinery) — **upstream** | **PASS** |
| `fillet_curated_edges_3205_e2e`: correct in-loop scheduling (selector before op) | capability→producer + DAG-direction | `producer:ε` (the executors + edges) — **upstream** | **PASS** (this is the capability that was **inverted** in the design doc; the in-loop greenness is downstream of the driver, so it lives on this leaf, cfg-gated to `unified-dag`, **not** asserted on legacy) |
| `dfm_fits_build_volume_4275_e2e`: post-geometry constraint re-check + cross-sub leaf fold | capability→producer + DAG-direction | `producer:ε` (C7 retirement + cross-sub `resolve_geometry_handle_arg`) — **upstream** | **PASS** |
| definite `Satisfied`/`Violated` (not a tolerance) | premise (branch-3 end-to-end) | every required capability traced upstream above; no numeric bound asserted | **PASS** |

> **Anti-inversion note (the binding this manifest exists to catch):** had η's signal been left as "task 3205 green in-loop on legacy" (the design doc's original framing), this binding would resolve `producer-downstream` / unsatisfiable — the in-loop dispatch cannot succeed on the legacy pipeline (design §1). Re-homing the in-loop e2e onto η (downstream of ε's scheduling) is the resolution; the *machinery* alone stays the upstream 3205 prereq.

## ζ — differential corpus (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| Both schedulers runnable | capability→producer | `producer:δ` (the `UnifiedDag` flag + driver) **upstream**; `LegacyMultiPass` is today's default | **PASS** |
| Full corpus + golden | wired-on-main | `crates/reify-eval/tests/` + `tests/golden` | **PASS** |

## θ — warm/incremental unification (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `build_snapshot` positional terminal-handle export (fix `step_handles.last()`) | wired-on-main (fix) | `grep:engine_build.rs:2100` (the bug) + positional pattern already at `:2369` (build()'s `terminal_handles[t_idx][r_idx]`) — θ copies it | **PASS** |
| `eval_cached`/`concurrent` are expr-only (no kernel/named_steps stack) | premise | design D7 (confirmed zero references) — θ threads the stack in | **PASS** (scoped as its own stage) |
| `compute_dirty_cone_with_realizations` is dead code (full-flush retained) | premise | `grep:crates/reify-eval/src/dirty.rs:95` (only test callers — C15) | **PASS** (D8: value-cell-scoped incremental; full realization flush retained) |
| Cross-kernel `KernelHandle` re-key (if multi-kernel in warm corpus) | capability→producer (conditional) | `producer:task-4349/4351` — **conditional** θ pre-condition (only if a multi-kernel module enters the warm test set) | **PASS** (conditional; wired as a dep only if the warm corpus includes a cross-kernel case) |

## ι — cutover + legacy removal (leaf; human-gated)

✅ **Stage 4 landed (#4362, 2026-06-23).** Default is now `UnifiedDag`; `REIFY_BUILD_SCHEDULER=legacy` is the one-release kill-switch. Stage 5 (legacy deletion) is deferred to #4727.

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| N green CI runs + go/no-go | operational (not a code capability) | human-gated per Open Question 2; not a substrate binding | **PASS** (Stage-4 go ratified by Leo 2026-06-22) |
| Legacy loop + `BuildScheduler` enum exist to delete | wired-on-main | introduced by δ/ε; Stage-5 deletion deferred to #4727 | **PASS** (Stage-5 pending) |

**No FAIL bindings.** The single binding that *would* have failed (`producer-downstream` on the 3205 in-loop e2e) is resolved by the D6 split: machinery upstream (task 3205), in-loop e2e on η downstream of ε. Queue-blocking conditions: Part 1 merged + re-scoped task 3205 machinery merged (both enforced as real `add_dependency` edges at decompose time).
