# PRD: Retire the #4593 perf-anchor pattern

**Status:** active — F-infra (audit cadence + tracking infrastructure), version-agnostic.
**Date:** 2026-06-17.
**Approach:** B (single subtractive change; no new mechanism, no integration seam — see §7).
**Amends:** `docs/prds/reify-audit-ptodo-detector.md` §6.1 + §10 (this PRD owns the amendment — see §6).

## 1. Goal

Retire the perf-backlog **anchor** pattern that the PTODO detector's §6.1 policy
sanctioned. Anchor task **#4593** ("Perf backlog anchor v4") is `status: deferred`
with `metadata.do_not_complete: true` / `do_not_dispatch: true` — a *permanently
non-terminal sentinel* that ~13 `TODO(#4593)` / `FIXME(#4593)` perf markers cite. The
detector's `is_terminal_status()` (`crates/reify-audit/src/ptodo.rs:532`) treats
`deferred` as **live**, so these cites pass the hard gate even though the markers will
**never** be done.

This defeats the very invariant the detector enforces. The detector PRD
(`reify-audit-ptodo-detector.md` §1) requires every marker to cite "a **specific**
non-terminal task **whose brief names resolving that TODO as a completion condition**."
#4593 is neither specific (it is a shared catch-all) nor ever-completable (its brief
says "DO NOT COMPLETE"). The v1→v4 anchor churn (#4551 → #4590 → #4592 → #4593) is
direct evidence the pattern fights the system: each prior anchor closed and re-orphaned
its surviving cites, forcing a fresh anchor.

A marker-by-marker audit (read each in context; see §5) found **all of them are
speculative perf notes about currently-correct code**, each conditional on a scale
trigger that has **not fired**, with **no named consumer today**. None is a well-formed
task under G1/G2. The honest disposition is therefore to **stop marking them as tracked
debt** (reword them to plain explanatory comments), not to file perf tasks — then
decommission #4593.

## 2. Consumers (G1)

This PRD introduces **no new mechanism**; it is subtractive. The consumers of the
change (who relies on the markers either citing real actionable tasks or not being debt
markers) all exist today:

1. **The PTODO hard gate** (`crates/reify-audit/src/ptodo.rs`, §8.4 High-severity
   `untracked`/`orphaned`/`bare-ignore`) — keeping the gate honest is the whole point:
   after the change, every remaining marker either cites a real live actionable task or
   is not a debt marker at all.
2. **The `CLAUDE.md` "TODO citation convention"** — the contract every dispatched agent
   reads. Its canonical-forms example block currently teaches `TODO(#4593)`; after
   retirement that example must not point at a retired id (§5, item C).
3. **Every dispatched implementer** — the feedback loop that catches a fresh untracked
   marker at verify time. Retiring the anchor removes the sanctioned "cite the
   catch-all" escape hatch, so new perf notes must either become genuine cited tasks or
   plain comments.

Not an in-engine seam — the engine-integration-norm §3 catalogue does not apply (dev
tooling, same class as the detector itself).

## 3. Substrate (G3 — verified 2026-06-17 by direct host inspection)

This PRD assumes **no novel substrate**. Every capability it relies on exists and was
verified live:

| Capability | Evidence |
|---|---|
| PTODO detector + allowlist | `crates/reify-audit/src/ptodo.rs`; `ALLOWLIST_PREFIXES` includes `crates/reify-audit/` (`:296`); `is_allowlisted` (`:313`); swept-file gate `if !is_swept_ext(path) \|\| is_allowlisted(path) { continue }` (`:905`) |
| `is_terminal_status` = done/cancelled only | `ptodo.rs:532` — confirms `deferred` is treated as **live** (the finding's premise) |
| Committed baseline (shrink-only) | `crates/reify-audit/ptodo-baseline.txt` — **empty (0 lines)**; PTODO is green at zero on main today |
| Liveness lane resolves cites vs task DB | `ptodo.rs:609/738`; DB path `REIFY_PTODO_TASKS_DB`, default `<root>/.taskmaster/tasks/tasks.db`, rows `tag='master'` (§6.7 of the detector PRD) |
| `set_task_status` can cancel #4593 | fused-memory MCP; `do_not_complete`/`do_not_dispatch` metadata blocks *dispatch/auto-complete*, **not** a manual cancel |
| The 13 markers + 8 files | `git grep -n 4593 -- '*.rs'` (see §5) matches #4593's own brief ("~13 perf-PTODO markers in 8 files: 6 swept + 2 allowlisted") |

No `.ri` syntax, no numeric premise, no new field/symbol/dispatch arm — the overlay's
D3 `.ri`-probe verification workflow (`scripts/prd-decompose-verify.mjs`,
grammar/check/ir probes) is **N/A** to a comment-reword + control-plane change and would
force-fit to a spurious block. G3 substrate was verified by direct inspection instead.

## 4. Cancel-safety — the load-bearing proof (G6 / lifecycle)

The detector PRD §6.1 states a lifecycle invariant: *"the citable anchor must remain
non-terminal for as long as any perf marker cites it."* Cancelling #4593 while a **live
cite** remains in a **detector-visible** file would flip every such cite to `orphaned`
(High → the PTODO hard gate goes **RED** on main). This PRD discharges that invariant by
construction, in this order:

1. **Reword first.** Every `TODO/FIXME/HACK(#4593)` marker in a non-allowlisted swept
   file (the six `reify-eval`/`reify-expr`/`reify-kernel-fidget` files) is reworded to a
   plain comment with **no marker token and no cite**. After landing, `git grep` shows
   **zero** `#4593` debt markers outside `crates/reify-audit/`.
2. **Allowlist absorbs the residual.** The only `#4593` strings that remain in tracked
   swept source live under `crates/reify-audit/` (the p2 test fixtures + the
   `p2_consumer_stub.rs:96` cite-form example), which the detector **skips at the
   `is_allowlisted` gate (`ptodo.rs:905`) before any scan**. The two genuine perf
   *markers* inside `reify-audit` (`p1_producer_orphan.rs:53`, `p2_consumer_stub.rs:418`)
   are also reworded for honesty, but even unreworded they are invisible to the detector.
3. **Then cancel.** With zero detector-visible #4593 cites on main, `set_task_status(4593,
   cancelled)` produces **zero `orphaned` findings** — the gate stays green.

The ordering guard (reword-merges-to-main **then** cancel) is therefore an **intra-task
sequencing requirement**, encoded in the leaf task's definition (§8), not a DAG edge.

## 5. Sketch of approach — the marker inventory

A single reviewable change. Each site is a speculative perf note about correct code,
conditional on an unfired trigger, with no consumer today.

**A. Reword (drop the `TODO`/`FIXME`/`HACK` token and the `(#4593)` cite; keep the prose
as a plain explanatory comment so §8.1's `\b(TODO|FIXME|HACK)\b\s*[(:]` regex no longer
fires).** Detector-visible (non-allowlisted) sites:

- `crates/reify-eval/src/dispatcher.rs:720` — O(K·S) per popped state ("if a future
  kernel grows a large supports table").
- `crates/reify-eval/src/engine_eval.rs:1372` — streaming hash for big `.vdb` ("if
  `ContentHash` later exposes an incremental constructor" — blocked on a non-existent
  upstream API).
- `crates/reify-eval/src/engine_purposes.rs:637` — prefix-trie tolerance scan ("for 1-3
  purposes this is fine, but if … hot path").
- `crates/reify-expr/src/analysis.rs:141` and `:177` — `Arc<Value>` one-time
  field-construction clone.
- `crates/reify-expr/src/calculus.rs:148`, `:221`, `:288`, `:353` — same `Arc<Value>`
  cluster ("a broader architectural change" with no measurement).

Allowlisted (`crates/reify-audit/`) genuine perf markers — reword too, for honesty
(detector already ignores them):

- `crates/reify-audit/src/p1_producer_orphan.rs:53` — O(tasks²) rescan ("if
  `task_metadata` ever grows").
- `crates/reify-audit/src/p2_consumer_stub.rs:418` — coalesce git-diff calls.

**B. SPECIAL CASE — `crates/reify-kernel-fidget/src/kernel.rs:219` (+ the `#4593`
re-cite at `:225`): per-handle `JitShape` cache. → REWORD (G1 applied honestly).** This
is the only marker with a *plausible* near-term consumer (GUI per-pixel SDF raster
preview repeatedly calling `evaluate_sdf_at` on one handle). The G1 consumer probe is
decisive: `git grep evaluate_sdf_at` shows **zero production callers** — every caller is
either `kernel.rs`'s own doc-comments/tests or `tests/dispatcher_integration.rs`. No
real, named, or imminent hot-loop consumer exists. Filing a perf task now would itself
be a **G1 orphan-producer** (a perf mechanism with no consumer) — exactly the pattern
this PRD retires. The gate decides: **reword with the rest** (the brief's stated default
when no consumer materializes).

**C. `CLAUDE.md` canonical-forms example block (lines ~280–285).** It teaches the cite
grammar using `#4593` as the example id (`// TODO(#4593): brief description`, …).
Leaving a *retired* id as the contract's example is a footgun (an agent copying it cites
a cancelled task). Replace `#4593` with the metavariable `#NNNN` (already used in the
same doc's prose; not a parseable cite, and `.md` is unswept). In scope because G1 names
`CLAUDE.md` as a consumer.

**Leave (intentional, allowlisted reify-audit test-fixture / grammar-example data — NOT
debt markers):**
- `crates/reify-audit/src/p2_consumer_stub.rs:96` — `// … (e.g. #4593)` cite-form
  example in P2's own documentation.
- `crates/reify-audit/tests/p2.rs:1723` (doc comment) and `:1743` (fixture string
  `"// TODO(#4593): perf, see anchor"`) — test data exercising P2's canonical-cite
  recognition. P2 has no liveness lane, so a cancelled #4593 does not break the test;
  the file is allowlisted so the detector never scans it.

**D. Decommission #4593.** After the reword lands on main (zero detector-visible cites
verified), `set_task_status(4593, cancelled)` with a disposition note recording this
PRD. Cancel (not done): the markers were never "completed"; the disposition is *retired*.

## 6. Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/reify-audit-ptodo-detector.md` §6.1 (anchor policy) | **amends** | delete the anchor policy; record that speculative perf notes are documented as **non-debt comments**, not citations; strike the "permanently non-terminal standing owner" lifecycle text | **this PRD** | queued (leaf task α) |
| `docs/prds/reify-audit-ptodo-detector.md` §10 (G4 table, row referencing 4593, ~line 285) | **amends** | replace the "4593 standing anchor" row with a pointer to this PRD; record 4593 retired/cancelled | **this PRD** | queued (leaf task α) |
| Sibling session: PTODO detector hardening ("anchor-laundering guard") | **parallel — no dep** | sibling hardens the detector (code + a new guard policy); this PRD rewords markers + retires §6.1's anchor policy. Logically disjoint edits | **sibling owns the guard; this PRD owns the reword + §6.1 retirement** | informational — **no code dependency, no shared mechanism** |

**No reciprocal-ownership ambiguity.** The sibling's anchor-laundering guard *adds*
detector behavior; this PRD *removes* the anchor policy and the cites. The only possible
overlap is both touching the detector PRD `.md` (§6.x prose). Per the detector PRD §7
precedent, comment/prose merge conflicts are trivial and the orchestrator's file locks
serialize PRD-file edits. Do **not** depend on the sibling and do **not** touch detector
code (`ptodo.rs`) — scope is marker reword + #4593 retirement + §6.1/§10 amendment only.

## 7. Approach (G5) — bare B, no H

Not high-stakes / architecturally-complex: no load-bearing seam is modified (the
detector's *behavior* is unchanged — only the cites it sees change), no new contract,
no integration step to starve. Blast radius is comment-only across several crates plus
one `.md` policy edit plus one control-plane status flip. **Bare B** (single vertical
change, single observable signal). No contract section / boundary-test sketch required.

## 8. Decomposition plan

One leaf task. All signals CLI/task-state-observable.

- **α — Retire the #4593 perf-anchor: reword markers, amend the detector PRD, retire the
  task.** Reword the 13 markers of §5.A/§5.B to plain comments; apply the §5.C
  `CLAUDE.md` example fix; amend `reify-audit-ptodo-detector.md` §6.1 + §10 (§6 above);
  leave the §5 "Leave" fixtures. Land via the **orchestrator merge queue**
  (`/merge-queue`, never raw `git merge`). **As the final post-merge action** (ordering
  guard, §4): verify `git grep` shows zero detector-visible #4593 cites on main, then
  `set_task_status(4593, cancelled)` with a disposition note citing this PRD.

  **Leaf. User-observable signal:** on main after the change, `reify-audit --pattern
  PTODO` (run with `REIFY_PTODO_TASKS_DB=/home/leo/src/reify/.taskmaster/tasks/tasks.db`)
  exits 0 with **zero violations above the empty committed baseline**; `git grep -nE
  '\b(TODO|FIXME|HACK)\b\s*[(:]' -- '*.rs' | grep 4593` returns **nothing** (no debt
  marker cites #4593 anywhere in tracked source); the only residual `#4593` strings in
  swept source are the three allowlisted `crates/reify-audit/` test-fixture/example lines
  (§5 "Leave"); and `get_task(4593)` returns status `cancelled`.

  **Consumer:** the PTODO hard gate + the `CLAUDE.md` convention + every dispatched
  implementer (§2). **Substrate-confirmed:** yes (no novel substrate; §3).

No intermediate tasks. The #4593 decommission is folded into α as its terminal
control-plane step (a *separate* dependent task would be a zero-file-change orchestrator
task — the precise auto-complete failure mode #4593's `deferred` status was created to
prevent).

## 9. Out of scope

- **Detector code changes** (`ptodo.rs`, allowlist, `is_terminal_status`) — the
  sibling's territory; the bug is the *pattern*, not the detector.
- **The 3 allowlisted reify-audit test fixtures/examples** (§5 "Leave") — legitimate
  grammar-demonstration data, not debt.
- **`docs/prds/gui-diagnostics-…` `4593`** — a source *line number* (`engine.rs:4593`),
  not a task cite. `Cargo.lock` / `measurements-cold.json` / `probe-d.json` `4593` —
  incidental data, not cites.
- **Filing any perf task** — every marker is speculative/no-consumer (§1, §5); the audit
  conclusion is reword, not track. (Revisit per-site only when a real hot-loop consumer
  materializes — then it becomes a genuine G1/G2 task with a live cite.)
- **Migrating the 4 production `STUB_MSG` sites** (detector PRD §14) — unrelated.

## 10. Open questions (tactical)

1. **Exact reword prose per site** — implementer's call, constrained only by: no
   `TODO`/`FIXME`/`HACK` token, no `#NNNN` cite, prose preserved. (A `Perf note:` /
   `Scaling note:` prefix reads cleanly.)
2. **`done` vs `cancelled` for #4593** — resolved: **cancelled** (the markers were never
   completed; the disposition is *retired*). Recorded here to forestall re-litigation.
