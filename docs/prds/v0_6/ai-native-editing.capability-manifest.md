# Capability manifest — `ai-native-editing`

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/ai-native-editing.md`. Built at decompose (2026-07-06).

**Substrate-verifier posture.** This PRD introduces **no novel `.ri` syntax or semantics** — it is pure Rust/TS wiring plus a span-based rewrite of *existing* param-default literals. Per `procedural_prd_d3_verify_workflow_is_ri_only`, the D3 `.ri`-fixture workflow (`scripts/prd-decompose-verify.mjs`) is **N/A** and not run; substrate is bound below by **wired-on-main grep evidence** and **upstream-producer** references. G6 numeric-floor and field-population sub-checks are **N/A** (no numeric bounds; no result-field sampling).

**Upstream (out-of-batch) producers referenced** — the `gui-state-sync` batch (5029–5037):
- `task-5034` — L5 `#[derive(GuiSync)]` compile-time-forced full-field `GuiState` coverage (INV-GUI-1). *pending.*
- `task-5035` — L6 route debug_server mutations through the delta choke-point (INV-GUI-2). *pending.*
- `task-5037` — L8 delete the dead `reify_`-prefix `claude_bridge` interception. *in-progress.*
- `task-5036` — L7 sidecar prompt names the real `mcp__reify-debug__*` surface. **done.**

Leaves: **ζ, η, θ, ι** (α, β, γ, δ are intermediates — bound transitively as producers).

---

## ζ — integration gate (PRIMARY LEAF)

Signal: Claude issues `mcp__reify-debug__reify_set_parameter(cellId, V)` in the ChatPanel on `prj/printer_v01` → viewport + property panel update live **without a file reload** AND `prj/printer_v01/printer.ri` **on disk** reflects `V`.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `reify_set_parameter` reachable as `mcp__reify-debug__*` | anti-orphan + DAG-dir | `producer:δ` upstream (exposes it on the reify-debug server); allowlist glob grants it — `grep gui/sidecar/src/session.ts:81` `ALLOWED_TOOLS='… mcp__reify-debug__*'` wired on main | PASS |
| Viewport syncs (meshes) via delta | wired-on-main | `grep gui/src-tauri/src/diff.rs:19` `changed_meshes`; consumed `gui/src/stores/engineStore.ts:149` `applyMeshUpdate` | PASS |
| Property panel syncs (values) via delta | wired-on-main | `grep gui/src-tauri/src/diff.rs:20` `changed_values`; consumed `engineStore.ts:153` | PASS |
| No field silently desyncs after AI edit (INV-GUI-1) | producer + DAG-dir | `producer:task-5034` upstream (compile-time-forced coverage) — **the hard-req-1 edge** | PASS |
| AI mutation flows through the delta choke-point (INV-GUI-2) | producer | `producer:task-5035` upstream (via δ) | PASS |
| Write-back to `.ri` (source-canonical, INV-GUI-3) | producer + sink wired | `producer:γ` upstream; sink `grep gui/src-tauri/src/engine.rs:2158` `update_source` → `:274` `commit_state` (rebuilds `source_map`) wired on main | PASS |
| Sidecar prompt names the real tools | producer | `producer:task-5036` (**done**) | PASS |
| No shadow unsynced path (dead interception removed) | producer | `producer:task-5037` upstream (via δ) | PASS |
| Rejection: `reify_set_parameter` on a no-default param → structured error (B7) | rejection-mechanism (batch-delivered) | `producer:α` (`resolve_param_default_span` → `None`) + `producer:γ` (`Err` on `None`); observed to fire at the ζ boundary test B7 | PASS |

## η — user-slider durable-edit fix (LEAF)

Signal: user edits a param via GUI slider/edit-box → `.ri` on disk updates durably; viewport syncs (debug-MCP drag + on-disk assertion).

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Slider `set_parameter` writes back to source | producer | `producer:γ` upstream | PASS |
| Command-path delta choke-point | wired-on-main | `grep gui/src-tauri/src/main.rs:312` `compute_delta` → `:34` `emit_delta` | PASS |
| No field desyncs on the command path (INV-GUI-1) | producer | `producer:task-5034` upstream | PASS |
| Write-back sink | wired-on-main | `grep gui/src-tauri/src/engine.rs:2158` `update_source` | PASS |

## θ — INV-GUI-2 architecture test, AI/MCP entry point (LEAF)

Signal: the arch test **fails** if any reify-debug write tool bypasses the delta choke-point.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| The routed property exists to assert | producer | `producer:δ` (routes write tools through the choke-point) + `producer:task-5035` upstream | PASS |
| An arch-test harness precedent exists | wired-on-main | `check_event_inventory.sh` name-drift lint + `debug_boundary_tests.rs` (contract-guard precedent, `docs/debug-mcp-contract.md`) | PASS |

## ι — INV-GUI-3 source-canonical enforcement (LEAF)

Signal: the test **fails** on any value-mutation path that skips source write-back.

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| Every value-mutation path writes back to source | producer | `producer:γ` (AI `reify_set_parameter`) + `producer:η` (slider) upstream | PASS |

---

## Intermediate-task substrate notes (not leaves — bound for completeness)

- **α** `resolve_param_default_span`: substrate present — `ParamDecl.default: Option<Expr>` with `Expr.span` (`crates/reify-ast/src/decl.rs:237`, `crates/reify-ast/src/ast.rs:9`) in `EngineSession.parsed_cache` (`gui/src-tauri/src/engine.rs:464`). No accessor exists (build it). PASS (substrate wired; accessor is the deliverable).
- **β** `value_to_ri_literal`: G6 branch-2 (exactness) note — `Value` `Display` emits `0.08 m`, **not** round-trippable `80mm` (`crates/reify-ir/src/value.rs:3447`). The round-trip property `parse(serialize(v)) ≡ v` is achievable **by configuration** (emit shortest round-tripping f64 repr; prefer the existing default literal's unit). This is β's own within-task property test — **not** a leaf premise — so it does not gate the batch as a leaf; flagged here so the implementer earns the exactness rather than assuming `Display` suffices.
- **γ** `apply_param_to_source`: sink `update_source`→`commit_state` wired on main (`engine.rs:2158`/`:274`); atomicity (all-or-nothing) is the deliverable's invariant.
- **δ** MCP exposure: transport + allowlist already wired (`session.ts:330` reify-debug MCP config; `session.ts:81` `mcp__reify-debug__*` glob); `tool_defs()` extension + choke-point routing is the deliverable.

**All leaf bindings PASS. No FAIL values → batch not blocked.** External producers 5034/5035/5037 are upstream (DAG-direction correct); 5036 done.
