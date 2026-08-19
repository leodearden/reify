# Capability manifest — jcodemunch substrate restoration

PRD: `docs/prds/jcodemunch-substrate-restoration.md` (committed `22472828a1`).
Sidecar twin: `docs/prds/jcodemunch-substrate-restoration.capability-manifest.yaml`.
Built at decompose time 2026-08-07 against main `22472828a1`.

Mechanizes G3 + G6 per leaf. Every binding below was resolved by **direct
observation** (source read, host probe, live sqlite read), not by citation of the
PRD's own prose.

---

## 0. Scope notes for this manifest

**G3 substrate class.** This PRD introduces **no `.ri` language surface** — no
grammar production, builtin, diagnostic, geometry semantic, or CLI/GUI behaviour a
`.ri` author observes. The grammar gate (`references/grammar-gate.md`) and the
overlay's docs-truth gate (doc-chunk / exemplar-corpus / cheatsheet / discoverability
leaves) are therefore **N/A**. The D3 verification workflow
(`scripts/prd-decompose-verify.mjs` → `scripts/prd-capability-check.py`) is likewise
structurally inapplicable: its only probe kinds are `grammar` / `check` / `ir`, each
of which runs a `.ri` fixture. Substrate was verified by the manual G3 path instead —
the probes are recorded per binding below.

**The one thing this manifest must not do.** The PRD deliberately asserts **no P1
finding count** anywhere. P1 has produced zero findings in its one recorded live run
(2026-06-09). Any `≥N findings` signal would be an unvalidated premise — the exact
mistake that made the prior PRD's `L-SMOKE` binding vacuous (PRD §2.4). ε's binding
`no-finding-count-assertion` enforces this **mechanically**, with an `expect: absent`
check that also removes the count-asserting test name
`live_audit_produces_p1_and_pdead_findings`.

---

## 1. Substrate probes run at decompose time (shared evidence base)

| # | Probe | Result |
|---|---|---|
| S1 | `sed` of `crates/reify-audit/src/jcodemunch_client.rs` ctor + `post()` | **Confirmed** — `session_id: random_hex_32()` in `JcodemunchClient::new`; `.set("mcp-session-id", &self.session_id)` on **every** POST including `initialize`. PRD §2.3 verified at source. |
| S2 | `ls scripts/smoke-jcodemunch-audit.sh` | **Absent** — confirms PRD §2.4: the prior manifest's `L-SMOKE` bound a script that was never written. |
| S3 | `crates/reify-audit/tests/jcodemunch_live.rs` read | **Confirmed** — `#[ignore = "live integration: …"]` + `jcodemunch_serve_reachable()` bare-TCP preflight + "Preflight: skip gracefully". PASS-shaped whether or not the chain works. |
| S4 | `systemctl --user list-unit-files \| grep jcodemunch` | **Confirmed** — `jcodemunch-serve.service  bad  enabled`; both `~/.config/systemd/user/` entries are dangling symlinks into reaped worktree `4102`. `jcodemunch-watcher.service` is `enabled enabled` and **must not be touched** (it serves 5 other repos). |
| S5 | `python3 sha1('/home/leo/src/reify')[:8]` + cross-check against a live index | **D1 verified.** Predicted `local/reify-4ae45bbd`. Cross-check: predicted `local/3484-e93a7bf3` for `/home/leo/src/dark-factory/.worktrees/3484` == observed `meta.repo`. Source confirms: `storage/git_root.py:55-57` `_local_repo_name` = `f"{folder_path.name}-{sha1(str(folder_path))[:8]}"`, **identical in 1.108.27 and 1.108.74**. `~/.code-index/local-reify-4ae45bbd.db` does not exist (PRD §2.2 verified). |
| S6 | read-only `sqlite3` on a live index DB | **`meta.git_head` readable** (`git_head\|bafed25c…`); **`select count(*) from symbols` = 54233**. Both §4.3 conjuncts are obtainable with **no MCP session** — γ has an α-free route (OQ1). |
| S7 | `get_repo_outline.py:155-162` (1.108.74) | **D2 verified** — takes the `current_sha != index.git_head` branch and emits "Index SHA (…) does not match current HEAD". `staleness_days` is not consulted on that path. |
| S8 | `server.py` argparse scan | **`--once`, `--no-ai-summaries`, `--paths-from` all real flags.** §4.4's ban on `--paths-from` targets a flag that genuinely exists and genuinely deletes (`server.py:6426`, `:7436-7441`). |
| S9 | `watcher.py:305` (1.108.27 **and** 1.108.74) vs. `watcher.py:881-936` (1.108.54, `sync_folders()`) | **G6 CATCH, corrected by task 6270** — `watcher.py:305`'s `changed={changed} new={new} deleted={deleted} ({duration}s)` line lives in the **continuous** watch loop's re-index callback, unreachable from `watch --once`. The one-shot path is `sync_folders()`, which prints to **stderr only**: `<folder>: No changes detected (<duration>s)` on a no-op, or `<folder>: <N> symbols (<duration>s)` when changed (`index_folder.py:1303`/`:1372`/`:1755` set `message`; `:1456-1466`/`:1859-1870` omit it). Neither `changed: 0` nor `changed=0` is emitted on `--once`. See β/`upstream-summary-token`. |
| S10 | PyPI `jcodemunch-mcp` release index | **`1.108.54` present**, `1.108.27` **absent** (only the git tag survives). PRD §8's pin move is required and available. `uvx` present at `/home/leo/.local/bin/uvx`. |
| S11 | `scripts/install-warm-lane-units.sh` + `deploy/systemd/reify-warm-lane-gc.timer` | **Present** — ζ's reify-side installer precedent is real; no dark-factory counterpart needed (G4). |
| S12 | `scripts/assert-crate-dag.sh` + `crates/reify-build-utils/tests/crate_dag_assertion.rs` + `.jcodemunch.jsonc` | **All present** — λ's overlap premise has a real comparand. |
| S13 | `crates/reify-audit/src/bin/reify-audit.rs` grep | **`leodearden/reify` live at `:111`, `:190`, `:215`, `:774`** (the last inside a `#[cfg(test)]` assertion — γ must update it too). |
| S14 | `docs/prds/reify-audit-p1-jcodemunch-substrate.md` + `docs/architecture-audit/jcodemunch-serve-activation.md` + `.claude/skills/audit/SKILL.md` | **All three drift targets confirmed present verbatim** — "degrades to exit 125 when serve is down", "**Status (2026-05-30):** Active", `leodearden-reify.db`, and the SKILL's serve-prerequisite prose. |

---

## 2. Per-leaf bindings

### α — Fix `JcodemunchClient` MCP session handling

| Capability | Evidence | Verdict |
|---|---|---|
| `client-mints-own-session-id` (the defect exists) | S1 — `grep:crates/reify-audit/src/jcodemunch_client.rs` `session_id: random_hex_32()` wired into the production ctor, reached from `RealJCodemunchOps::new` → `bin/reify-audit.rs:574`. Not test-only. | **PASS** |
| `server-assigned-session-id-is-the-contract` | `scripts/smoke-jcodemunch-serve.sh:68-113` is an in-repo **working** reference implementation of the correct flow. | **PASS** |
| `self-minted-id-is-rejected` (G6 branch 4 — rejection-mechanism) | **Rejection observed to fire**: PRD §2.3 reproduced HTTP **404** across three independent serve/SDK combinations (1.108.54+mcp 1.29.0; 1.108.27+1.29.0; 1.108.27+1.27.2), each paired with a 200 control on the without-id arm. This is an observed positive, not an inferred one. | **PASS** |
| `transient-serve-is-spawnable-without-δ` (G6 branch 3 — DAG direction) | S10 — `uvx --from "jcodemunch-mcp==1.108.54"` is the raw capability; α's test spawns its own transient serve exactly as the §2.3 reproduction did. **δ is the reusable wrapper, not the capability** — α does **not** depend on δ, and δ→α is the correct direction. | **PASS** |
| `tools/list-is-served` | jcodemunch is an MCP streamable-HTTP server; `tools/list` is a protocol method, exercised by `smoke-jcodemunch-serve.sh`. α adds the client-side call. | **PASS** |

### β — `scripts/jcodemunch-index-reify.sh`

| Capability | Evidence | Verdict |
|---|---|---|
| `watch-once-flags-exist` | S8 — `--once`, `--no-ai-summaries` real argparse flags in `server.py`. | **PASS** |
| `paths-from-is-destructive-and-banned` | S8 + §4.4 (`index_folder.py:1505-1511` short-circuit; `sqlite_store.py:1698`, `:1480-1483` DELETE). The flag exists, so the ban is meaningful rather than vacuous. | **PASS** |
| `symbol-count-is-readable` | S6 — `select count(*) from symbols` on the index DB. β's `N sym`, N>0 guard is producible. | **PASS** |
| `upstream-summary-token` (**G6 branch-1/4 resolution**) | S9 (corrected by task 6270 against the **pinned 1.108.54** wheel) — `watch --once` runs `sync_folders()`, which prints stderr-only `No changes detected (<duration>s)` on a no-op or `<N> symbols (<duration>s)` when changed; upstream emits **neither** `changed: 0` **nor** `changed=0` on this code path (that token belongs to the continuous watch loop only, `watcher.py:305`). β's signal token is **β's own stdout** and β owns its format; no test may bind to either the colon or equals form as if it came from the tool on `--once`. **Resolution recorded**: β prints its own summary line and does not assume the tool's continuous-loop token appears on `--once`. | **PASS (resolved)** |
| `max_folder_files-truncation-guard` (**G7 INV-SF-3 resolution**) | `max_folder_files: 10000` truncates **with a warning, not an error** (PRD §10 OQ4). Under `declared-intent-consumed-or-diagnosed`, silently dropping files from the index is declared intent going unconsumed. **Resolution recorded**: OQ4's *whether* is settled to **yes, guard**; the *how* stays tactical. | **PASS (resolved)** |

### γ — Identity resolution + freshness gate

| Capability | Evidence | Verdict |
|---|---|---|
| `per-path-identity-derivable` | S5 — sha1 rule verified by prediction-then-measurement **and** at source, stable across 1.108.27/1.108.74. | **PASS** |
| `legacy-default-exists-to-remove` | S13 — `leodearden/reify` live at four sites in `bin/reify-audit.rs`, including a `#[cfg(test)]` assertion. | **PASS** |
| `index_head-readable` | S6 (sqlite route, **no serve required**) **and** S7 (`get_repo_outline` MCP route). Both OQ1 answers are substantiated. | **PASS** |
| `symbol_count-readable` | S6 — `symbols` table, `count(*)` = 54233 on a live index. The `symbol_count > 0` conjunct is not aspirational. | **PASS** |
| `refusal-mechanism` (G6 branch 4) | γ **is** the producer of the rejection; nothing upstream is required. Bound by construction. | **PASS** |
| `mcp-route-needs-α` (**G6 branch-3 resolution**) | If OQ1 resolves to the MCP route, `meta.git_head` is read over a live MCP session ⇒ requires α. The PRD lists γ's prereqs as "none". **Resolution recorded**: **γ → α wired as a real `add_dependency` edge**, so both OQ1 answers are safe. The α-free sqlite route (S6) remains available and is noted in γ's task text — prose ordering was explicitly rejected (overlay drift-guard rule). | **PASS (resolved)** |
| `refusal-carries-a-code` (**G7 INV-SF-6 resolution**) | B4/B5 assertions would otherwise bind to prose. **Resolution recorded**: γ's two refusals carry the stable greppable markers **`E_JC_INDEX_STALE`** and **`E_JC_INDEX_EMPTY`**, so boundary tests bind to code identity, not substrings (INV-SF-6 house pattern: tasks 2255/3416). | **PASS (resolved)** |

### δ — `scripts/with-jcodemunch-serve.sh`

| Capability | Evidence | Verdict |
|---|---|---|
| `serve-is-spawnable-and-pinnable` | S10 — 1.108.54 on PyPI, `uvx` present. | **PASS** |
| `readiness-probe-shape` | `scripts/smoke-jcodemunch-serve.sh` already polls `/mcp` correctly (`:68-113`); δ reuses that shape. Note the trailing-slash gotcha already pinned at `bin/reify-audit.rs:185-188` (`/mcp/` → 307 drops `mcp-session-id`). | **PASS** |
| `wrapped-command-emits-findings` (G6 branch 3) | Requires a working client (**α**, upstream ✓) **and an index that exists** (**β**). PRD lists δ's prereqs as α only; an index-less run emits `[]`, which is literally "a findings array" and is exactly the vacuity shape §2.4 warns about. **Resolution recorded**: **δ → β wired as a real edge** (β has no prereqs, so this costs no parallelism). Not wired to γ — δ owns serve *lifecycle*; a γ refusal is a legitimate δ outcome. | **PASS (resolved)** |

### ε — Non-vacuous live capstone (INTEGRATION GATE)

| Capability | Evidence | Verdict |
|---|---|---|
| `real-mcp-session` | producer **α**, upstream ✓ | **PASS** |
| `fresh-non-empty-index` | producer **β** (builds it) + **γ** (enforces + surfaces `symbol_count > 0`), both upstream ✓ | **PASS** |
| `serve-lifecycle` | producer **δ**, upstream ✓ | **PASS** |
| `no-finding-count-assertion` (**the central G6 discipline**) | P1's one recorded live run (2026-06-09) produced **zero** findings — a `≥N` assertion has no achievability basis and would reproduce §2.4's vacuity in mirror image. Enforced mechanically: `expect: absent` on `findings\.len\(\) *> *0` / `!findings\.is_empty\(\)`, **and** on the count-asserting test name `live_audit_produces_p1_and_pdead_findings` (S3 — present today). "Well-formed findings array, count may legitimately be 0" is the assertion. | **PASS** |
| `hard-fail-replaces-graceful-skip` (G6 branch 4) | ε **is** the producer of that rejection; S3 confirms the graceful skip exists to remove. | **PASS** |
| `capstone-must-not-become-gate-resident` (**G7 INV-SF-2-corollary resolution**) | B8's hard-fail is scoped to "capstone **invoked**". jcodemunch is legitimately absent in task worktrees (PRD §9), so a gate-resident hard-failing capstone turns main RED for **every** merge — an Error-severity outcome on a healthy path. **Resolution recorded**: ε stays excluded from the default merge/task gate (`#[ignore]` or infra classification); the hard-fail applies only on deliberate invocation. | **PASS (resolved)** |
| `oq3-drift-guard-same-diff` (**overlay gate-test drift-guard**) | OQ3 may relocate ε to `tests/infra/test_*.sh`. `tests/infra/run-all-classification.manifest` exists (S-check) and `tests/infra/test_run_all_classification.sh` catches declared-vs-discovered drift. **Resolution recorded**: if OQ3 takes that route, ε's **own diff** carries the manifest bucket row — no separate registration task, no prose ordering (esc-4914-162 precedent). | **PASS (resolved)** |
| `live-neighbour-lock` | `#5830` (in-progress, merge phase) declares `crates/reify-audit/tests/jcodemunch_live.rs` — the file ε rewrites. **Resolution recorded**: **ε → 5830** and **α → 5830** wired as real edges (PRD §7: "α must land after or coordinate with it"). | **PASS (resolved)** |

### ζ — Index timer + installer

| Capability | Evidence | Verdict |
|---|---|---|
| `reify-side-timer-precedent` | S11 — `scripts/install-warm-lane-units.sh` installs `deploy/systemd/reify-warm-lane-gc.timer` entirely reify-side. G4: no dark-factory counterpart. | **PASS** |
| `index-script-exists` | producer **β**, upstream ✓ | **PASS** |
| `meta.git_head-comparable-to-HEAD` | S6 + S7 | **PASS** |

### η — Retire `jcodemunch-serve.service`

| Capability | Evidence | Verdict |
|---|---|---|
| `unit-file-exists-to-delete` | S4 — `deploy/systemd/jcodemunch-serve.service` tracked; both user symlinks dangling. | **PASS** |
| `replacement-exists-first` | producer **δ**, upstream ✓ (do not retire the persistent unit before the spawn/teardown wrapper exists). | **PASS** |
| `watcher-unit-is-out-of-scope` (**guardrail**) | S4 — `jcodemunch-watcher.service` is `enabled enabled` and serves 5 other repos (`--repos` list read from the unit; reify is deliberately excluded per D4). η must **not** touch it. `grep jcodemunch-serve` does not match it, so the signal is precise. | **PASS** |
| `runbook-edit-belongs-to-μ` (**intra-PRD seam resolution**) | η's PRD text says "update the runbook" and μ's says "correct the record" — both name `docs/architecture-audit/jcodemunch-serve-activation.md`. **Resolution recorded**: η deletes unit + symlinks only; **all** doc correction is μ's (μ → η already ordered). | **PASS (resolved)** |

### θ — SPIKE: can `entry_point_patterns` express Rust roots?

| Capability | Evidence | Verdict |
|---|---|---|
| `pdead-baseline-is-measured` | PRD §2.6 — live `get_dead_code_v2` on a 48-file reify-audit index: 100 findings, **all** confidence 1.0, all signals `['unreachable_file','no_callers','not_barrel_exported']`, including `bin/reify-audit.rs::main` and every `#[test]`. The server's own `framework_warning` corroborates. A measured positive baseline, not an inference. | **PASS** |
| `puntested-baseline-is-NOT-measured` (**G6 branch-3 resolution**) | §2.6 measured **PDEAD only**. κ's premise ("same shape; rides the same reachability substrate") is an **inference**. **Resolution recorded**: θ's scope extended to record baselines for **both** `get_dead_code_v2` **and** `get_untested_symbols` in the same session (one extra tool call), so κ's negative assertion has an upstream-measured comparand. | **PASS (resolved)** |
| `both-arms-are-a-pass` | θ is a spike. NOT-CONFIGURABLE is a **delivered verdict**, not a block; ι/κ are then cancelled by a human reading the record. θ never changes sibling task state itself. | **PASS** |
| `query-substrate` | producers **α**, **β**, **γ**, all upstream ✓ | **PASS** |

### ι — Activate PDEAD

| Capability | Evidence | Verdict |
|---|---|---|
| `reachability-configured` | producer **θ** (pass arm), upstream ✓ | **PASS** |
| `false-positive-baseline` | θ's committed before/after record (PDEAD half measured — §2.6). | **PASS** |
| `sample-floor-relaxed` (**G6 branch-1 resolution**) | The PRD's "a hand-reviewed sample of 30 findings" is a **count assertion**. After θ's configuration lands, the PDEAD finding count may legitimately collapse toward zero — a *good* outcome that would make a hard floor of 30 unachievable, reintroducing exactly the unvalidated-count shape ε forbids. **Resolution recorded**: read **"up to 30 findings (all of them if fewer than 30 are produced)"**. The load-bearing half — the PRD's own "checkable negative assertion, not a guessed false-positive percentage" — is untouched. | **PASS (resolved)** |
| `negative-assertion-fires-today` (G6 branch 4) | §2.6 observed `main` and every `#[test]` **being flagged at confidence 1.0** — the positive currently fires, so asserting its absence after configuration is meaningful. | **PASS** |
| `advisory-severity-unchanged` | D6 + prior PRD §5 + task 4115. `Severity::Low`, opt-in `--pattern`, log-only, never auto-filed. | **PASS** |

### κ — Activate PUNTESTED

| Capability | Evidence | Verdict |
|---|---|---|
| `reachability-configured` | producer **θ** (pass arm), upstream ✓ | **PASS** |
| `puntested-baseline` | **producer θ**, upstream ✓ — via the θ scope extension above. Without it this binding would have been `producer-absent`. | **PASS (resolved)** |
| `sample-floor-relaxed` | same resolution as ι. | **PASS (resolved)** |
| `advisory-severity-unchanged` | D6. | **PASS** |

### λ — PLAYER: activate or retire

| Capability | Evidence | Verdict |
|---|---|---|
| `comparand-exists` | S12 — `scripts/assert-crate-dag.sh`, `crates/reify-build-utils/tests/crate_dag_assertion.rs`, per-crate `dag_invariant.rs`. B1–B6 already hard-gated **without** jcodemunch. | **PASS** |
| `layer-rules-exist` | S12 — `.jcodemunch.jsonc` present; its own header states a healthy run "returns an empty finding set". | **PASS** |
| `both-arms-are-a-pass` | Activate **or** retire; a determination naming the overlap is the deliverable either way. | **PASS** |
| `query-substrate` | producers **α**, **β**, **γ**, all upstream ✓ | **PASS** |

### μ — Correct the record

| Capability | Evidence | Verdict |
|---|---|---|
| `drift-targets-exist-verbatim` | S14 — all four confirmed present today: `jcodemunch-serve-activation.md:3` "**Status (2026-05-30):** Active"; `:23/:25` `leodearden-reify` / `leodearden-reify.db`; prior PRD's "degrades to exit 125 when serve is down" row; `.claude/skills/audit/SKILL.md` serve-prerequisite prose + `leodearden/reify` default. | **PASS** |
| `landed-behaviour-to-document` | producers **γ** (identity + refusal), **ζ** (replacement freshness story), **η** (unit retirement) — **all three wired upstream**. The PRD lists η only; γ and ζ added because μ documents *their* landed behaviour. | **PASS (resolved)** |
| `player-doc-edit-owned-by-λ` | If λ retires PLAYER, the SKILL edit is part of λ's retirement, not μ's. No unordered overlap. | **PASS** |

---

## 3. Decompose-time resolutions (summary)

Ten bindings needed resolution before the batch could queue. None was a rewrite of a
PRD design decision; all are DAG edges, scope pins, or bound relaxations.

| # | Leaf | Class | Resolution |
|---|---|---|---|
| R1 | γ | G6 branch 3 (unordered sibling) | **γ → α** wired as a real edge, so both OQ1 routes are safe. |
| R2 | δ | G6 branch 3 (vacuous signal) | **δ → β** wired, so "emits a findings array" is not satisfied by an empty array over a nonexistent index. |
| R3 | μ | G6 branch 3 | **μ → γ, ζ** added alongside μ → η. |
| R4 | α, ε | live-neighbour lock | **α → 5830**, **ε → 5830** wired (PRD §7). |
| R5 | ι, κ | G6 branch 1 (count floor) | "sample of 30" → **"up to 30 (all if fewer)"**. |
| R6 | θ, κ | G6 branch 3 (unmeasured baseline) | θ's scope extended to baseline **`get_untested_symbols`** as well as `get_dead_code_v2`. |
| R7 | γ | G7 `diagnostics-carry-codes` | Refusals pinned to **`E_JC_INDEX_STALE`** / **`E_JC_INDEX_EMPTY`**. |
| R8 | β | G7 `declared-intent-consumed-or-diagnosed` | OQ4's *whether* settled: `max_folder_files` truncation **must** be guarded. |
| R9 | ε | G7 INV-SF-2 corollary + overlay drift-guard | Capstone stays off the default gate; if OQ3 → `tests/infra/`, the classification-manifest row lands **same diff**. |
| R10 | η, μ | intra-PRD seam | Runbook correction is **μ's alone**; η deletes unit + symlinks only. |

Plus one out-of-batch correction, recorded here because it is PRD §4.1 item 5:
**#5832 → α** wired, and #5832's description annotated — its premise ("the handshake
succeeds and we accept too much") is **falsified by §2.3**: the handshake returns 404
and `RealJCodemunchOps::new` never returns `Ok` against a real serve today. Its hole
(the three `Ok(Value::Null)` acceptance paths) is real but only reachable **after** α.
`#5834` already depends on `#5832`; no new edge.

## 4. Gate verdicts

| Gate | Verdict |
|---|---|
| G1 consumer named | **PASS** — every mechanism names a consumer; α/β/γ/δ are intermediates consumed by ε (C-as-integration-gate); ζ/η/θ consumed by μ/μ/ι+κ; ι/κ/λ/μ are leaves with operator-observable signals. |
| G2 user-observable leaf | **PASS** — ε is the integration-gate leaf whose signal is PRD §1's headline command. No leaf's signal is "a unit test passes against synthetic input"; α's signal is explicitly *not a mock* ("mocks are what let §2.3 survive ten weeks"). |
| G3 substrate verified | **PASS** — S1–S14. No `.ri` substrate; grammar gate and docs-truth gate N/A. |
| G4 seam ownership | **PASS** — §7 table; dark-factory has **no seam** (stdio, not 8901 — D5), and ζ needs no counterpart (S11). No reciprocal ownership. No fourth contested pair. |
| G5 B+H | **PASS** — §4 contract, §5 boundary sketch (B1–B8), ε is the integration gate and names B6/B8. |
| G6 premise validity | **PASS with R1–R6** — the P1 count assertion is absent by construction and now mechanically enforced. |
| G7 design invariants | **PASS with R7–R9, zero waivers** — INV-SF-1/-7 N/A; -2/-3/-4/-5 the batch is the *fix*; -6 resolved by R7. |
| Capability manifest | **PASS** — no binding resolves to `declared-only`/`test-only`/`producer-absent`/`producer-downstream`/`producer-extent-short`/`fixture-ERROR`/`bound≤floor`/`rejection-absent`. |
