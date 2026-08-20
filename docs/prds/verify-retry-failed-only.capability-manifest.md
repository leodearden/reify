# Capability manifest — `verify-retry-failed-only` (decompose 2026-07-19)

Per-leaf capability→evidence bindings (mechanizes G3 + G6). Each binding ties a leaf's asserted capability to **evidence**; any **FAIL** (`declared-only | test-only | producer-downstream | producer-absent | producer-extent-short | fixture-ERROR | bound≤floor | rejection-absent`) blocks queueing until resolved. All bindings **PASS**. Reify-side substrate anchors re-verified against `main@e6595a38ff` at decompose time (author-time anchors were `fc47d707b3`); DF-side anchors verified by the author against post-recovery main and consumed as producer-side (each DF leaf's own unit test is its mechanical check).

**Domain notes.** This PRD is **verify-pipeline / merge-gate plumbing** — no `.ri` DSL syntax, no result-field production. The reify grammar gate and the `Value::Undef` field-population sentinel are therefore **N/A by construction** (recorded, not silently skipped); the `scripts/prd-decompose-verify.mjs` grammar/semantic-substrate workflow does not apply (no DSL premise to probe) — same disposition as the sibling infra PRDs `merge-gate-compile-cost` and `merge-gate-riders`. The G6 surface here is (a) **rejection-mechanism** bindings — the tree-OID/empty-subset/ceiling refusals (§4.3) must be *observed to fall back loud*, not merely defined; and (b) the **savings numbers** (~2.5 h/14d direct, compile+link ≈ 55–65% of gate CPU) are ratified survey figures (`docs/notes/merge-verify-cpu-survey-2026-07.md`), not accuracy bounds on a numerical method — no method error-floor applies.

**G3 substrate re-verification (reify anchors, main@e6595a38ff):**

| Capability | Status | Evidence (re-verified) |
|---|---|---|
| `emit_nextest_pass` / nextest `-E` selector construction | **exists** | `scripts/verify.sh:1138` (`emit_nextest_pass`), command build `:1161`; `-E` support is the nextest-plan feature (`:1078` refuses cargo-test fallback precisely because it lacks `-E`) |
| run_all Phase-2.5 serial per-member retry + FLAKY ledger | **exists** | `tests/infra/run_all.sh` `REIFY_RUN_ALL_FLAKY_LEDGER` + `=== FLAKY (passed on serial retry) ===` Phase-2.5 emission (~:1030-1049 neighborhood) |
| `gui-test.sh -- <specs>` forwards to `vitest run` | **exists** | `scripts/gui-test.sh:8,42,90` (forward args after `--`); verify.sh gui block runs `npm test` `:1346` |
| clock-stop marker emitter family (honest-marker precedent) | **exists** | `scripts/verify.sh:178-196` `@@REIFY_CLOCK_STOP@@`/`@@SEMAPHORE_ACQUIRE@@` via `scripts/lib_clock_stop.sh` — the `@@REIFY_RETRY_SCOPE@@` marker joins this family |
| sidecar survives reseed under `git clean -xfd -e target` | **exists** | CLAUDE.md warm-lane invariant (lanes seeded tracked-files-only + `git clean -xfd -e target`) |
| nextest exact-name filterset `test(=<exact>)` runs exactly named tests | **exists** | author-time live verify (2-of-6 subset ran); §3 |
| nextest record/`-R` round-trip | **falsified → NOT used** | §3.1 — record inoperable in 0.9.136; design pivots to filter-file (version-stable, ARG_MAX-safe). Recorded as future bookmark (§11), **not** a dependency. |

**Consumers wired (anti-orphan):** α/β/γ are consumed by the reify boundary test δ (B1–B6, same batch) and by DF `verify_env` (D2 sets the retry envs); δ is consumed by ε + milestone 5254; ε + D1–D5 are consumed by the merge-verify CPU-reduction program (milestone 5254). No orphan producer.

---

## α — reify:5287 (verify.sh nextest retry-subset consumption)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate seam exists (G3) | **PASS** | `emit_nextest_pass`/`-E` construction on main (table above); the `REIFY_VERIFY_RETRY_NEXTEST_FILTER_FILE` env is the leaf's own new consumption at that seam, not an assumed-existing capability. |
| Anti-orphan / wired | **PASS** | Consumed by δ (B1/B2/B3) + DF `verify_env` (D2). |
| Rejection-mechanism (G6 branch 4 / INV-4) | **PASS (observed by δ)** | Tree-OID mismatch / absent-or-empty filter / over-ceiling subset each force a **loud** full-verify with a distinct log line — RED-first inside δ (B2/B3), the rejection ships with the capability, same batch. |
| No lock-step subset re-derivation (INV-5) | **PASS** | verify.sh builds the `-E` expression from the file DF writes; the sound-subset **definition** is authored once in DF (D2), never re-derived here. |
| Field-population / Grammar | **N/A** | No result field, no `.ri` syntax. |

## β — reify:5288 (run_all member-subset knob)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate seam exists (G3) | **PASS** | run_all Phase-2.5 per-member invoke path on main (table above); `REIFY_RUN_ALL_MEMBER_SUBSET` is the leaf's new selector reusing that path. |
| Anti-orphan / wired | **PASS** | Consumed by δ (B4) + DF `verify_env` (D2). |
| Soundness (G6) | **PASS** | run_all runs to completion (no cross-member fail-fast) ⇒ subset = {failed members} is complete by construction; members hermetic (H2/4924) ⇒ subset run side-effect-free. |
| Rejection/observable | **PASS (observed by δ B4)** | `REIFY_RUN_ALL_MEMBER_SUBSET="test_x.sh"` ⇒ only that member runs, others reported skipped. |

## γ — reify:5289 (gui failed-spec forwarding)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate seam exists (G3) | **PASS** | `gui-test.sh -- <specs>` forwards to `vitest run`; verify.sh gui block runs `npm test` (table above). `REIFY_GUI_RETRY_SPECS` is the leaf's new forwarding. |
| Anti-orphan / wired | **PASS** | Consumed by δ (B5) + DF `verify_env` (D2). |
| Soundness (G6) | **PASS** | gui vitest runs to completion ⇒ subset = {failed spec files} complete by construction. |
| Spec-path-form agreement (open tactical Q) | **PASS (settled in-leaf)** | §11 open Q — confirm the failed-spec path form DF parses matches `gui-test.sh`'s expected spec-path form; a tactical confirmation inside the leaf, not a substrate gap. |

## δ — reify:5290 (INTEGRATION-GATE: boundary test + honest marker + same-diff drift-guard registrations)

| Check | Verdict | Evidence |
|---|---|---|
| DAG-direction (anti-inversion) | **PASS** | Depends on α, β, γ (all three upstream, wired); δ is the reify terminal. |
| Producer extent (anti-short) | **PASS** | Delivers the boundary test (B1–B6, §9.1) **and** the `@@REIFY_RETRY_SCOPE@@` marker in verify.sh **and** the drift-guard registrations — all in one diff; extent = the integration gate, not name-matched. |
| Honest events (INV-2) | **PASS** | `@@REIFY_RETRY_SCOPE=failed_only@@` + per-suite subset counts joins the clock-stop emitter family; captured by DF into runs.db (D5). Structured emission, not log-scraped. |
| Drift-guard registration (overlay rule; esc-4914-162) | **PASS (same-diff)** | New `tests/infra/test_verify_retry_failed_only.sh`'s `run-all-classification.manifest` bucket row + `test_no_new_wallclock_upper_bounds.sh` registration land in the **same diff** (PRD §6.6) — not a downstream sibling; the A3-before-A6 hazard is closed by construction. |
| Anti-orphan | **PASS** | Consumed by ε + milestone 5254. |

## ε — reify:5291 (operator-observable e2e demo; operational)

| Check | Verdict | Evidence |
|---|---|---|
| Execution path declared | **PASS** | `task_kind="deterministic"`, `metadata.execution_class="operational"`, `always_escalates=true` — run-and-observe against the live orchestrator, then escalate_blocker to Leo, no code/worktree. Honest routing declaration (no `submit_task` routing-intent mismatch). |
| Premise soundness (G6 branch 3 — e2e capability produced by the dep set) | **PASS** | ε's signal (subset-run retry lands + `retry_scope: failed_only` in runs.db) is produced **entirely** by its dependency set: δ (marker) + D1–D5 (field/construction/M1/shadow/event) + dark_factory:2821 (classifier gate) + reify:5261 (durable FLAKY ledger). No capability demanded that a dependency-of-ε doesn't produce (the false-premise trap this branch exists to catch). |
| FLAKY-ledger durability dependency (context-updated) | **PASS** | The ledger-durability edge targets **reify:5261** (run_all INTERRUPTED marker + Phase-2.5 serial-member ledger extension), not a nonexistent `merge-gate-health` W5a task. Ledger **durability substrate** (`REIFY_RUN_ALL_FLAKY_LEDGER`) already **deployed** (dark-factory-orchestrator.yaml, landed `bc235a616a`, orchestrator restarted 22:07). |
| Anti-orphan | **PASS** | Consumed by milestone 5254 (`add_dependency 5254 → ε`). |

## D1 — dark_factory:2833 (`retry_failed_only` on `merge_request` + threading)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `verified_green: bool = False` caller-vouched-bool precedent at `escalation/src/escalation/server.py:1099` (author-verified). |
| Anti-orphan / wired | **PASS** | Consumed by ε (operator resubmit) + D2 (retry-set construction). |
| Cross-repo seam ownership (G4) | **PASS** | reify ships the primitive (α/β/γ); DF wires the invocation (D1). Seam table §8; no contested pair. |
| Mechanical check | **manual** | Delivered in the dark-factory repo — reify-side git grep cannot observe it; the DF unit test (arg reaches worker retry path; default False no-op) is the mechanical check. |

## D2 — dark_factory:2834 (retry-set construction + verify_env wiring + tree-OID gate)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `parse_per_test_results` `merge_shadow.py:171`; M4 `_reverify_rebased_tree` `merge_queue.py:79,1685` (author-verified). |
| Soundness (G6 / INV-3) | **PASS** | nextest subset = {failed ∪ not-started} (a failed-only filter is UNSOUND under fail-fast); tree-OID gate re-corroborates the pinned tree before narrowing (belt-and-suspenders with the reify sidecar); rebase ⇒ full verify via M4. |
| DAG-direction | **PASS** | Depends on D1 (upstream, wired). |
| Mechanical check | **manual** | DF-side; the DF unit test (fail-fast map ⇒ {failed ∪ not-started}; envs+files populated; rebased tree ⇒ full verify) is the mechanical check. |

## D3 — dark_factory:2835 (budget policy + M1 autonomous, category-gated on 2821)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | M1 bounded classified-infra-transient retry seam `merge_queue.py:1787-1902`, `CategoryPolicy.is_infra_transient` (author-verified). |
| Category-gate soundness (INV-4) | **PASS** | Gated on dark_factory:2821 (done — positive-anchored `semaphore_timeout`); a deterministic red never enters the retry path, it surfaces. |
| DAG-direction | **PASS** | Depends on D2 (upstream, wired); gates on 2821 (done). |
| Mechanical check | **manual** | DF-side; the DF unit test (retryable-class ⇒ narrowed M1 retry; deterministic-red ⇒ none) is the mechanical check. |

## D4 — dark_factory:2836 (shadow-baseline map merge)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `on_result` callback `merge_queue.py:1556,1583-1587` → `merge_shadow.py` warm baseline store (author-verified). |
| Soundness (G6 / INV-2) | **PASS** | A subset retry's partial map is merged (attempt-0 ∪ retry) before storage ⇒ no phantom cold-shadow divergence. |
| DAG-direction | **PASS** | Depends on D2 (upstream, wired). |
| Mechanical check | **manual** | DF-side; the DF unit test (partial map merged before storage; no phantom divergence) is the mechanical check. |

## D5 — dark_factory:2837 (`merge_verify` event marker → runs.db)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `merge_verify` event + runs.db row emission path (author-verified); reify's `@@REIFY_RETRY_SCOPE@@` marker (δ) is the source. |
| Honest events (INV-2) | **PASS** | `retry_scope: failed_only` + subset sizes on every narrowed retry ⇒ a narrowed retry is never mis-mined as a full green gate. |
| DAG-direction | **PASS** | Depends on D2 (upstream, wired). |
| Mechanical check | **manual** | DF-side; the DF check (runs.db row carries `retry_scope: failed_only` + subset sizes) is the mechanical check. |

---

## G7 — design-invariants walk (five slugs; `docs/legibility/design-invariants.md` absent on main)

The normative doc is not yet on main (produced by the undelivered dark_factory `design-invariants-gate-prd`); per the `merge-gate-riders` / `merge-gate-compile-cost` precedent the walk is done manually against the five canonical slugs (source: `dark-factory/plans/design-invariants-gate-prd.md §The-five-invariants`). Walked against **every** task in the batch (α–ε, D1–D5). **No hits; no waivers required.**

| Slug | Disposition |
|---|---|
| **INV-1 `contracts-machine-checked`** | PASS. The retry contract (which envs, subset semantics, refusal conditions) is machine-checked at the seam — δ's B1–B6 boundary test (reify side) + D1–D5 unit tests (DF side) — never prose-only or dispatcher-internal. The `@@REIFY_RETRY_SCOPE@@` marker is a machine-observable structured emission. |
| **INV-2 `structured-facts-at-failure`** | PASS. The honest marker + per-suite subset counts are emitted at the decision point (clock-stop emitter family) and captured **structurally** by DF into the runs.db row (D5) — no log-scraping of emitter-known facts. Each fallback logs a **distinct** reason at the point of decision (§4.3). |
| **INV-3 `corroborate-before-acting`** | PASS. Before narrowing, verify.sh re-corroborates the pinned tree OID (sidecar vs `REIFY_VERIFY_RETRY_TREE_OID`) against ground truth; DF enforces independently; any rebase ⇒ full verify via M4. The corroboration step is present and named, not assumed. |
| **INV-4 `storm-escape-required`** | PASS. Every fail-soft narrowing path is **loud and bounded**: mandatory loud full-fallback on tree-drift / empty-subset / over-ceiling (§4.3); the category-gate (D3, on 2821) keeps deterministic reds out of the retry path entirely; the retry budget (D3) bounds retry count. No silent suppression. |
| **INV-5 `no-lockstep-duplication`** | PASS. The sound-subset **definition** lives once in DF (D2); verify.sh only honors the file it's handed and builds the `-E` expression from it (reusing `emit_nextest_pass`); run_all reuses the Phase-2.5 per-member invoke; the marker reuses the clock-stop emitter family. The tree-OID double-check (reify sidecar + DF env) is **belt-and-suspenders corroboration of one shared ground-truth fact** (the git tree OID — a single source, not a copied constant), a trivial equality on each side — defense-in-depth, not lock-step-duplicated logic. |

---

## Milestone wiring

Program milestone **5254** (merge-verify CPU-reduction release gate) `depends_on` **δ (5290)** and **ε (5291)** — wired 2026-07-19 (`add_dependency id=5254 depends_on=<leaf>`). Intra-reify: δ(5290) ← α(5287)/β(5288)/γ(5289); ε(5291) ← δ(5290), reify:5261. ε external deps: dark_factory:2833/2834/2835/2836/2837 + dark_factory:2821. Intra-DF: D2(2834) ← D1(2833); D3(2835)/D4(2836)/D5(2837) ← D2(2834).
