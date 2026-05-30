# reify-audit P1 jcodemunch substrate (F-infra slice 2)

**Date:** 2026-05-30
**Status:** design — decompose-ready
**Scope marker:** slice 2 of the shipped F-infra (`docs/architecture-audit/f-infra-design.md`). Slice 1 landed the `reify-audit` library + CLI + `/audit` skill + pre-done P5 hook (tasks T-1…T-8, 3670/3675/3731). This slice makes the **P1 producer-orphan detector actually produce findings on live sweeps** by replacing the `NoopJCodemunchOps` stub with a real jcodemunch-MCP-backed implementation, and adds three new jcodemunch-backed detectors.

---

## 0. Why this PRD exists

F-infra slice 1 shipped P1's *detector logic* (`crates/reify-audit/src/p1_producer_orphan.rs`) against a `JCodemunchOps` seam, but wired the production binary to `NoopJCodemunchOps` — a stub whose `get_changed_symbols` returns `[]` (see `bin/reify-audit.rs:69`, design §11 D-1 deferral). Consequently **P1 is inert on every live `/audit` sweep**: it reports zero findings not because the codebase has no producer-orphans, but because the substrate that would surface them was never connected. The 2026-05-30 `/audit` review (memory `reference_audit_first_live_sweep_detector_effectiveness`) confirmed P1's "zero findings" was an artifact of the noop, not a clean bill of health.

The jcodemunch substrate is now **live** (G3 passes): `jcodemunch-watcher.service` indexes the reify repo continuously (index-schema v16), pinned to `git+https://github.com/jgravelle/jcodemunch-mcp.git@v1.108.27`. This PRD connects `reify-audit` to it.

## 1. Consumer + user-observable surface (G1)

**This is not an in-engine seam** (it is audit tooling, not the Reify kernel), so the `engine-integration-norm.md §3` sub-check does not apply.

| Mechanism introduced | Named consumer | User-observable surface |
|---|---|---|
| `RealJCodemunchOps` (HTTP MCP client) | The `reify-audit` binary's sweep path (`bin/reify-audit.rs`), which is in turn invoked by the **`/audit` skill** and by **`/review` Phase 2**. | `reify-audit --pattern P1` over a real commit range emits a **non-empty** P1 finding stream (or correctly suppresses a known-consumed symbol) instead of trivially exiting 0. Observable in `/audit` reports + `data/audit-runs/<ts>.json`. |
| `P-DEAD` / `P-UNTESTED` / `P-LAYER` detectors | Same: `/audit` sweep + `/review` Phase 2. | `reify-audit --pattern PDEAD\|PUNTESTED\|PLAYER` emits its pattern's findings for the reify repo; documented in the `/audit` skill. |
| `jcodemunch-serve` systemd unit | The `RealJCodemunchOps` client connects to it. | A persistent local streamable-HTTP MCP endpoint; `scripts/smoke-jcodemunch-serve.sh` exits 0 against it. |

The consumer chain is **closed within this PRD** — there is no downstream PRD waiting on an orphan producer. The capstone leaf (§8 L-SMOKE) proves the whole chain end-to-end against the live server, which is the G2 signal that the slice-1 noop could never provide.

**Out-of-consumer note:** the pre-done hook path (slice-1 D-1 / task 3675) runs **P5 only** and must remain jcodemunch-free (it is a latency-sensitive validator in fused-memory's request path). The jcodemunch substrate is for the *sweep* path exclusively; the binary connects the client lazily, so a pre-done P5 invocation never pays jcodemunch cost or fails when the serve unit is down.

## 2. Sketch of approach

Four moving parts, mirroring the slice-1 architecture and the existing in-crate `fused_memory_client.rs` HTTP pattern:

1. **A persistent query server.** The running `jcodemunch-watcher.service` uses the `watch-claude` subcommand — it only keeps the on-disk index fresh; it does **not** answer queries. So we stand up a *second* unit, `jcodemunch-serve.service`, running `jcodemunch-mcp serve --transport streamable-http --host 127.0.0.1 --port <P>` against the **same shared on-disk index**. (Dark-factory parity: persistent watcher + persistent query serve, both off one index.)
2. **An HTTP MCP client** (`crates/reify-audit/src/jcodemunch_client.rs`) — a near-clone of `fused_memory_client.rs`: sync `ureq`, streamable-HTTP protocol `2024-11-05`, `initialize` → `notifications/initialized` → `tools/call`. It calls the jcodemunch tools and adapts their wire shapes to the crate's `ChangedSymbol`/`SymbolReference`/new result structs. `RealJCodemunchOps` implements the `JCodemunchOps` trait over this client.
3. **A redesigned `JCodemunchOps` trait** — commit-range, plus the new-detector methods (§4-b). P1's `check` maps a done task → commit range via `done_provenance.commit`.
4. **Three new detectors** — `P-DEAD` (`get_dead_code_v2`), `P-UNTESTED` (`get_untested_symbols`), `P-LAYER` (`get_layer_violations`), each a new module behind a new `Pattern` enum variant and a `--pattern` CLI arm.

## 3. Pre-conditions / verified substrate (G3)

All verified against the on-disk v1.108.27 source (`~/.cache/uv/git-v0/checkouts/222e86ded376d2c0/29faf00/src/jcodemunch_mcp/`) this session — **not** the cached v2.1.0 source the original briefing read from:

| Capability | Verified signature / fact | Used by |
|---|---|---|
| `get_changed_symbols` | `(repo, since_sha=None, until_sha="HEAD", …) → {from_sha, to_sha, changed_files, changed_symbols, added_symbols, removed_symbols}`. **Commit-range**, re-parses both versions via `git diff` (no historical index needed; repo must be locally indexed). | P1 |
| `find_references` | `(repo, identifier, max_results=50, identifiers?, include_call_chain?) → references`. **Name-based; no file-scoping parameter exists.** | P1 |
| `check_references` | `(repo, identifier|identifiers, …) → {is_referenced, …}` (singular) / `{results:[…]}` (batch). | P1 (optional fast pre-filter) |
| `get_dead_code_v2` | `(repo, min_confidence=0.5, include_tests=False, max_results=100, file_pattern?) → {dead_symbols:[{id,name,kind,file,line,confidence,signals}], total_analysed, …}`. Multi-signal (import-graph + call-graph + barrel-export); `min_confidence=0.5` ≈ "≥2 of 3 signals". | P-DEAD |
| `get_untested_symbols` | `(repo, file_pattern?, min_confidence=0.5, …) → {untested_count, reached_pct, symbols:[{symbol_id,name,…}]}`. Static test-reachability, **not** runtime coverage. | P-UNTESTED |
| `get_layer_violations` | `(repo, rules?:list[dict], …) → violations`. Rules pass directly **or** from `.jcodemunch.jsonc`. | P-LAYER |
| `serve` transport | `serve --transport {stdio,sse,streamable-http} --host --port` (default port 8901; also `JCODEMUNCH_TRANSPORT/HOST/PORT`). `[http]` extra = `uvicorn`/`starlette`/`anyio`. Same streamable-HTTP wire `fused_memory_client.rs` already speaks. | client transport |

**Two corrections to the slice-1 assumptions** (both are G6 premise fixes — see §4):
- The trait's `get_changed_symbols(branch, since_epoch)` is **wrong**; the real tool is commit-range `(since_sha, until_sha)`.
- The trait doc claims `find_references` is "scoped to the symbol's declaring file (pass the file path to jcodemunch-MCP)" — **no such parameter exists**. File-scoping must be done client-side by filtering the returned reference list on `symbol.file`.

**Open substrate detail (resolved by L-SERVE spike, not blocking):** the exact `repo` identifier string + `storage_path` that map to the watcher's on-disk reify index. Confirmed live by the L-SERVE connectivity smoke before any Rust depends on it.

## 4. Resolved design decisions

**(a) P1 mechanism — Hybrid.** P1 keeps its existing scoped shape: `get_changed_symbols(commit-range)` → `find_references` → filter non-test callers. This is low-false-positive (only flags symbols the task actually introduced), per-done-task attributable, and a minimal change to the slice-1 detector. `get_dead_code_v2` is **not** P1's mechanism — its repo-wide multi-signal output becomes a **separate** detector (`P-DEAD`), so P1's correctness is not bet on jcodemunch's (unproven-for-Rust) dead-code accuracy.

**(b) `JCodemunchOps` trait redesign (commit-range + new methods).** The full trait surface is defined once (in L-TRAIT), so the new-detector leaves add modules + CLI arms but never re-touch the trait:

```rust
pub trait JCodemunchOps {
    // P1 — commit-range (was: get_changed_symbols(branch, since_epoch))
    fn get_changed_symbols(&self, since_sha: &str, until_sha: &str) -> Vec<ChangedSymbol>;
    fn find_references(&self, symbol: &ChangedSymbol) -> Vec<SymbolReference>;
    // P-DEAD
    fn get_dead_code(&self, min_confidence: f64) -> Vec<DeadSymbol>;
    // P-UNTESTED
    fn get_untested_symbols(&self, min_confidence: f64) -> Vec<UntestedSymbol>;
    // P-LAYER
    fn get_layer_violations(&self) -> Vec<LayerViolation>;
}
```

- **`find_references` vs `check_references`:** P1 needs the *reference list* to filter non-test paths client-side; `check_references`' bare `is_referenced` bool cannot do that. So P1 uses `find_references`. (`check_references` may be added later as a cheap pre-filter; not in scope.)
- **Suppression metadata.** `ChangedSymbol` carries `has_allow_dead_code` / `has_cfg_test` / `g_allow_marker`. jcodemunch symbol records do **not** carry these Rust source attributes, so `RealJCodemunchOps` populates them by reading the declaring source at `file:line` — keeping the detector pure-logic (symmetric with how `GitOps::diff_added_lines` pre-extracts strings; design §3).
- **done → commit-range mapping.** P1 resolves a done task's `done_provenance.commit` to the range `<commit>^1 .. <commit>` (the landing commit's diff), the **same mechanism the re-scoped task 4074 uses for P2** — reuse, don't reinvent. `done_at` is still used for the 14-day grace-window age calc; only the `get_changed_symbols` arguments change.

**(c) Query mechanism — persistent `serve` over streamable-HTTP** (already decided: dark-factory parity). Confirmed `serve --transport streamable-http` exists. A dedicated `jcodemunch-serve.service` unit reads the shared on-disk index; `RealJCodemunchOps` is an HTTP client mirroring `fused_memory_client.rs`. **Not** stdio-spawn (cold-start per run) and **not** folding query-serve into the `watch-claude` watcher (different subcommand; the watcher also serves dark-factory + autopilot-video and must stay untouched).

**(d) New detectors (all severity Low / log-only initially).** `P-DEAD`, `P-UNTESTED`, `P-LAYER`. They are **advisory** — Low severity, logged to `data/audit-runs/<ts>.json`, **no auto-filed follow-up tasks** — because jcodemunch's Rust accuracy is unproven and these heuristics are noisier than P1's scoped check. (This is the honest-bound discipline from the L2-chokepoint survey: ship the real signal at a severity its confidence supports; promote to Medium/auto-file only after the live corpus shows low FP.) `P-LAYER` requires reify layer rules (`.jcodemunch.jsonc`) authored first.

**(e) Decomposition** — §8. Single-crate-file / single-skill-file leaves per the narrow-lock norm; the full trait + `Pattern` enum live in one leaf (L-TRAIT) so detector leaves never re-touch `lib.rs`'s trait; `bin/reify-audit.rs` CLI arms are serialized via dependencies.

## 5. Out of scope

- **Auto-promoting P-DEAD/P-UNTESTED/P-LAYER to Medium / auto-file.** Revisit after a live corpus FP review.
- **Pre-done hook running P1** (slice-1 D-1 deliberately runs P5 only; jcodemunch in the hot path is out of scope).
- **`get_dependency_graph` / `get_blast_radius` / `find_importers` as detectors** — they are lookups (building blocks), not finding-producers. May enrich existing findings (e.g. annotate a P1 orphan with its blast radius) in a future slice.
- **`check_references` as P1's mechanism** (needs the list, not a bool — see §4-b).
- **Cross-repo audit** (`cross_repo=true`) — reify-local only.
- Changing slice-1's P2/P5 detectors, the `/prd`/`/review`/`/orchestrate`/`/unblock` skills beyond the `/audit` doc update, or `gap-register.md` auto-promotion.

## 6. Cross-PRD relationship + seam ownership (G4)

| Seam | Owner | Notes |
|---|---|---|
| `jcodemunch-serve.service` systemd unit + activation | **This PRD** (L-SERVE), modeled on slice-1's T-8 activation pattern. | Operator action may be required to `systemctl --user enable --now`; the leaf commits the unit + smoke + activation doc (an orchestrator worktree cannot reliably manage user systemd). |
| `jcodemunch-watcher.service` (index freshness) | **Operational infra, already running** — not modified by this PRD. | Shared on-disk index (schema v16). Risk: concurrent watcher-write + serve-read — verified non-fatal in the L-SERVE spike (dark-factory already runs this pattern; memory `reference … jcodemunch-watcher`). |
| `JCodemunchOps` trait + P1 + new detectors | **This PRD.** | No contested ownership; entirely within `crates/reify-audit`. |
| Task **4074** (P2 recall, pending; deps 4076) | dark-factory/reify-audit, **separate**. | Owns the *done→commit-range mapping* mechanism for **P2**; this PRD's P1 remap (§4-b) **reuses that mechanism**. Wire a soft dependency so P1 lands the shared mapping consistently. |
| Task **4076** (P2 FP-suppression, in-progress) | separate. | Sibling slice-2 work on P2; no code overlap with P1/jcodemunch. |
| Task **3670** (P1 library, done) | slice-1. | The detector logic this PRD activates. |

D-1 (slice-1 pre-done hook, task 3675) is **done** and is the *hook*, not the jcodemunch data source — the data source is this PRD's subject.

## 7. Boundary-test sketch (G5 — B+H, light)

This slice is single-crate + one skill + one unit (not FEA/ComputeNode/grammar/persistent-naming/multi-kernel), so it does **not** warrant a full contract document. But the exact failure that motivated the slice — a trait whose signature didn't match the live tool — is a wire-boundary mismatch, so one **two-way boundary test** is warranted (the H component):

- **Decode direction (L-CLIENT):** a unit test decodes a **response captured from the live server** (by the L-SERVE spike) into `ChangedSymbol`/`DeadSymbol`/etc. — proving the adapter matches the real wire shape, not a guessed one. (A captured-from-live fixture, *not* synthetic — this is the anti-pattern G2 rejects.)
- **End-to-end direction (L-SMOKE):** the real binary against the live serve produces a real finding from the reify corpus — proving the whole client→trait→detector→CLI chain, the signal slice-1's noop could never give.

## 8. Decomposition plan (one bullet per leaf; observable signal in **bold**)

DAG: `L-SERVE → {L-CLIENT, L-SMOKE}`; `L-TRAIT → L-CLIENT → L-WIRE → L-PDEAD → L-PUNTESTED → L-PLAYER → L-SKILL`; `L-WIRE,L-PDEAD → L-SMOKE`. (`bin/reify-audit.rs` and `lib.rs` writers are sequenced by these deps to avoid narrow-lock contention.)

1. **L-SERVE** — *jcodemunch query-serve activation + connectivity spike.* Add `jcodemunch-serve.service` (`serve --transport streamable-http --port <P>` off the shared index) + `scripts/smoke-jcodemunch-serve.sh` + activation doc; capture real-wire response fixtures for `get_changed_symbols`/`get_dead_code_v2`/`get_untested_symbols`/`get_layer_violations` into `crates/reify-audit/tests/fixtures/jcodemunch/`. Resolve the `repo` identifier + `storage_path`. **`scripts/smoke-jcodemunch-serve.sh` exits 0 and prints non-empty symbol data for the reify repo from the live serve.** *(Lock: systemd unit + scripts/ + docs/ + tests/fixtures/. No Cargo source.)*
2. **L-TRAIT** — *commit-range trait redesign + P1 remap.* Rewrite `JCodemunchOps` to the full §4-b surface; add `DeadSymbol`/`UntestedSymbol`/`LayerViolation` structs + `Pattern::{PDeadCode,PUntested,PLayerViolation}` variants; update `MockJCodemunchOps`; rewrite P1's `check` to resolve `done_provenance.commit` → `^1..commit` range; update `tests/p1.rs` to the new signature (assert orphan-in-range fires, consumed-symbol suppresses). **`cargo test -p reify-audit p1` green under the commit-range signature; no test references the old `(branch, since_epoch)` API.** *(Lock: `lib.rs` + `p1_producer_orphan.rs` + `tests/p1.rs`.)*
3. **L-CLIENT** — *jcodemunch HTTP client + `RealJCodemunchOps`.* New `crates/reify-audit/src/jcodemunch_client.rs` (clone `fused_memory_client.rs`): streamable-HTTP MCP client + wire adapters for all five trait methods; `RealJCodemunchOps` implements the trait, populating `ChangedSymbol` suppression flags by reading source at `file:line`; filters `find_references` results to `symbol.file` client-side. **`cargo test -p reify-audit jcodemunch_client` decodes the L-SERVE-captured live fixtures into the Rust structs (the H boundary test).** *(Lock: `jcodemunch_client.rs` + one `pub mod` line in `lib.rs`. Deps: L-SERVE, L-TRAIT.)*
4. **L-WIRE** — *replace `NoopJCodemunchOps` in the binary.* In `bin/reify-audit.rs`, construct `RealJCodemunchOps` from a new `--jcodemunch-url` arg (default the serve URL), connecting **lazily** so the P5/pre-done path never touches it; replace `let jcodemunch = NoopJCodemunchOps;` (line 423). **`reify-audit --pattern P1` with the serve up queries real data (no longer trivially exits 0); `--pattern P5 --pre-done` still runs jcodemunch-free.** *(Lock: `bin/reify-audit.rs`. Dep: L-CLIENT.)*
5. **L-PDEAD** — *P-DEAD detector.* New `pdead_dead_code.rs` consuming `JCodemunchOps::get_dead_code`; Low/log-only findings; `--pattern PDEAD` CLI arm; tests via `MockJCodemunchOps`. **`reify-audit --pattern PDEAD` against the live serve emits confidence-scored `dead_symbols` findings for the reify repo.** *(Lock: new module + `bin` arm + `pub mod` line. Deps: L-WIRE, L-TRAIT.)*
6. **L-PUNTESTED** — *P-UNTESTED detector.* New `puntested.rs` consuming `get_untested_symbols`; Low/log-only; `--pattern PUNTESTED` arm; tests. **`reify-audit --pattern PUNTESTED` emits untested-symbol findings (with `reached_pct`) for the reify repo.** *(Lock: new module + `bin` arm. Dep: L-PDEAD for `bin` ordering.)*
7. **L-PLAYER** — *P-LAYER detector + reify layer rules.* Author `.jcodemunch.jsonc` layer rules for the reify crate stack (e.g. `reify-types` ← `reify-eval` ← `reify-cli`/`reify-gui`); new `player.rs` consuming `get_layer_violations`; Low/log-only; `--pattern PLAYER` arm; tests. **`reify-audit --pattern PLAYER` reports any import crossing a declared forbidden boundary (and is empty/clean when none).** *(Lock: `.jcodemunch.jsonc` + new module + `bin` arm. Dep: L-PUNTESTED for `bin` ordering.)*
8. **L-SKILL** — *`/audit` skill documentation.* Update `.claude/skills/audit/SKILL.md` + references: the new patterns, the `jcodemunch-serve` requirement, the `--jcodemunch-url` flag, and that P1 now needs the serve unit up. **`/audit --pattern PDEAD` (etc.) is documented; the serve prerequisite is stated.** *(Lock: skill dir, outside Cargo. Dep: L-PLAYER.)*
9. **L-SMOKE** — *capstone live integration (the G2 end-to-end proof).* `scripts/smoke-jcodemunch-audit.sh` (or `tests/jcodemunch_live.rs`, gated on serve availability) runs the **real** `reify-audit` binary against the **live** serve over a real reify commit range and asserts ≥1 real P1 finding shape **and** a P-DEAD finding from the reify corpus. **The script exits 0 and prints real findings — the proof P1 is no longer inert.** *(Lock: scripts/ or tests/. Deps: L-WIRE, L-PDEAD.)*

## 9. Open (tactical) questions

- Exact serve port (default 8901 collides with nothing known; confirm in L-SERVE) and whether the unit should `--watcher=false` explicitly (it should — watch-claude owns indexing).
- Whether `RealJCodemunchOps` should retry/backoff on a cold serve, or fail fast to exit 125 like `fused_memory_client.rs` (lean fail-fast; the sweep is human-driven and can retry).
- Final reify `.jcodemunch.jsonc` layer-rule set — the precise forbidden edges (decide against the actual crate DAG in L-PLAYER).
- Whether L-SMOKE lives as a committed script (always runnable) or a `#[ignore]`-by-default live integration test (run on demand) — pick whichever the slice-1 `/audit` smoke convention already uses.
