# F-infra slice 1 — filing log

Session: 2026-05-15 decompose-mode filing of `docs/architecture-audit/f-infra-design.md` (commit `58101627f7`) into the task tracker.

Source design: `docs/architecture-audit/f-infra-design.md` — audit cadence + tracking infra; ships P1/P2/P5 detectors, CLI binary, `/audit` skill, integration smoke, and `/review` wiring.

## Pre-existing tasks (not filed in this session)

| Letter | Task ID | Status | Notes |
|---|---|---|---|
| T-1 | 3531 | done | reify-audit P5 phantom-done detector library + tests. Landed at `9db2747ffd` via found_on_main; the `crates/reify-audit/` workspace crate exists with the `p5_phantom_done` module, `GitOps` seam, and `MockGitOps` test-support. |
| — | 3667 | in-progress | G-allow markers stopgap for reify-audit's library-only public surface until the T-4 CLI consumer lands. Wired as a real dependency of T-4 in this session. |
| D-1 | — | done (out-of-tree) | Pre-done validator hook lives in the task-routing MCP layer (env-var-driven, fail-closed). Activation on Reify's side is T-8's concern; no Reify-side bookmark needed for D-1 itself. |

## Task IDs assigned (this session)

| Letter | Task ID | Title | Prereqs (intra-batch task IDs) |
|---|---|---|---|
| T-2 | 3669 | reify-audit P2 consumer-stub detector library + tests | T-1 (3531) |
| T-3 | 3670 | reify-audit P1 producer-orphan detector library + tests | T-1 (3531) |
| T-4 | 3672 | reify-audit CLI binary (callable from /audit skill + pre-done hook) | T-1, T-2, T-3, 3667 |
| T-5 | 3671 | /audit skill (.claude/skills/audit/) + references | T-4 (3672) |
| T-6 | 3673 | reify-audit integration smoke (three seeded incidents) | T-1, T-2, T-3, T-4, T-5 |
| T-7 | 3674 | /review Phase-2 wires in /audit invocation | T-6 (3673) |
| T-8 | 3675 | activate pre-done hook by setting FUSED_MEMORY_PREDONE_HOOK_REIFY | T-4 (3672) |

All filed via `mcp__fused-memory__submit_task(planning_mode=true)`. All started in `deferred` status; flipped to `pending` via `commit_planning` at the end of this session.

Note on filing: T-4's first submission was rejected by the `DarkFactoryPathScopeViolation` guard because the description referenced upstream paths under `fused-memory/`. Re-filed with the upstream paths abstracted to "task-routing MCP layer" prose.

## Dependency edges added (15 edges total)

Intra-batch + cross-batch (3531 done; 3667 in-progress):

| From | To (depends on) | Rationale |
|---|---|---|
| 3669 (T-2) | 3531 (T-1) | P2 module reuses the public surface T-1 pinned (`Pattern`, `Finding`, `AuditContext`, `Severity`) |
| 3670 (T-3) | 3531 (T-1) | P1 module reuses the public surface T-1 pinned |
| 3672 (T-4) | 3531 (T-1) | CLI consumes T-1's `p5_phantom_done::check` |
| 3672 (T-4) | 3669 (T-2) | CLI consumes T-2's `p2_consumer_stub::check` |
| 3672 (T-4) | 3670 (T-3) | CLI consumes T-3's `p1_producer_orphan::check` |
| 3672 (T-4) | 3667 | T-4 removes the G-allow markers 3667 (and T-2/T-3) installed; must land after 3667's markers are in tree |
| 3671 (T-5) | 3672 (T-4) | `/audit` skill shells out to `reify-audit` CLI binary |
| 3673 (T-6) | 3531 (T-1) | Integration smoke exercises P5 through the public lib surface |
| 3673 (T-6) | 3669 (T-2) | Integration smoke exercises P2 |
| 3673 (T-6) | 3670 (T-3) | Integration smoke exercises P1 |
| 3673 (T-6) | 3672 (T-4) | Integration smoke shells through the CLI for end-to-end coverage |
| 3673 (T-6) | 3671 (T-5) | Smoke verifies skill-side rendering on a seeded fixture |
| 3674 (T-7) | 3673 (T-6) | `/review` Phase 2 composes a known-working `/audit` |
| 3675 (T-8) | 3672 (T-4) | Hook activation needs the CLI binary on PATH (or its absolute path in the template) before flipping the env var |
| 3667 | 3531 (T-1) | Retrospective edge: 3667 modifies T-1's already-shipped code; recorded for the dep DAG |

## Gates walked

| Gate | Outcome |
|---|---|
| G1 (consumer named) | ✅ Every leaf names a consumer (T-2/T-3 → T-4+T-5; T-4 → T-5+T-7+T-8; T-5/T-7 → end users; T-6 integration gate; T-8 → pre-done loop). |
| G2 (user-observable leaf signal) | ✅ Every task carries a concrete `user_observable_signal` field — see design §10 table and each task's metadata. |
| G3 (novel grammar verified) | ✅ N/A — all Rust + Markdown skill files; no `.ri` grammar work. All tasks tagged `grammar_confirmed: true`. |
| G4 (cross-PRD seams) | ✅ `/prd` (F reads metadata; one-way), `/review` (T-7 owns the wiring), escalation MCP (F writes), pre-done hook (already-shipped contract; T-4 + T-8 own the Reify side). No reciprocal-ownership ambiguity. |
| G5 (design-first + contracts) | ✅ Standard portfolio B (vertical slice with T-6 integration gate). No H needed — detector shape is simple and fixture-tested. |
| META (coherent on decompose-and-queue) | ✅ Chain terminates at user-observable surfaces (T-6 integration smoke + T-7 /review wiring + T-8 actual pre-done activation). No producer-orphan shape in slice 1 itself. |

## D-1 discovery

Design §10/§11 describes D-1 as "dark-factory: pre-write validator hook on `set_task_status(done)` in fused-memory MCP. … implement session queues this dark-factory task at decomposition time. Non-blocking for slice 1 of F-infra."

While walking gates this session, found D-1 already shipped upstream — pre-done hook lives in the task-routing MCP layer at `pre_done_hook.py`, env-var-driven (`FUSED_MEMORY_PREDONE_HOOK_<PROJECT_ID_UPPER>`), fail-closed on config errors, returns structured `pre_done_hook_rejected` / `pre_done_hook_timeout` / `pre_done_hook_misconfigured` errors. So no deferred bookmark task is needed for D-1.

What remains on the Reify side to activate pre-done gating: (a) the CLI binary with `--pre-done` flag (T-4 scope) and (b) wiring the env var on the task-routing service's runtime env (T-8 scope). T-8 was added as a slice-1 leaf to close this gap.

## Hand-back note

The orchestrator does **not** currently read the `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata fields the `/prd` decompose flow writes. They are substrate for the F-infra follow-up — specifically, T-3 (P1 producer-orphan) reads `consumer_ref`, and T-7 (/review wiring) can fold them into its report. Until those consumers land the fields are inert but auditable via `get_task`.

## 2026-05-17: T-7 split + closed

Task 3674 (T-7) was filed with "Files touched: `.claude/skills/review/SKILL.md` … `.claude/skills/review-briefing/references/briefing-schema.md`". Architect blocked the task as unactionable because none of those files exist in Reify — they live in dark-factory (`/home/leo/src/dark-factory/skills/review*/`), reached via the `/home/leo/.claude/commands/review.md` symlink. The design doc itself flagged this ambiguity at §11 ("the dark-factory copy — confirm in implement session"). G4 review in this filing log called the wiring "T-7 owns" without flagging that the wiring file is cross-repo.

Interactive resolution 2026-05-17:

- **Dark-factory side (load-bearing):** commit `ca3eccffa7` on `dark-factory/main` — `review/Phase 2: invoke project /audit and fold findings into report`. Added Phase 2 Step 1 invoking `/audit --pattern P1,P2,P5 --since <window>`, folded results under `f_infra_findings` in the Phase 2 report (high/medium/low), and added Phase 3 dedupe against `filed_task_ids` so `/review` never re-files what `/audit` already filed. Also documented `audit.window_days` in the `review-briefing` schema doc.
- **Reify side:** added `audit.window_days: 14` knob to `review/briefing.yaml` (sibling commit on Reify main, paired with the dark-factory commit).
- **Task 3674:** cancelled. Escalations `esc-3674-5` (orchestrator task_failure) and `esc-3674-6` (orphan-reaper L0) resolved. Branch `task/3674` and worktree `.worktrees/3674/` reaped.

F-infra slice 1 is now closed end-to-end: T-1..T-6 + T-8 done, T-7 split-and-landed across both repos.

### G4 lesson for future cross-repo seams

Cross-repo seam-ownership cannot be expressed inside the Reify task graph alone. When a Reify PRD names a "Files touched" file that lives outside `git ls-files`, decomposition should either (a) abstract the file to a behavioral contract and trust an interactive session to land the cross-repo side, or (b) explicitly route the cross-repo half as a "manual action item for Leo" rather than a queued Reify task. The orchestrator's `DarkFactoryPathScopeViolation` guard correctly refuses dark-factory paths in task descriptions; the same guard would have caught this at filing time if the original "Files touched" list had named the dark-factory paths explicitly.
