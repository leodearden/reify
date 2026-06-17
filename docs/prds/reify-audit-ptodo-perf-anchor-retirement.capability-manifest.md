# Capability manifest — reify-audit-ptodo-perf-anchor-retirement

Mechanizes G3 + G6 for the single leaf of
`docs/prds/reify-audit-ptodo-perf-anchor-retirement.md`. Built 2026-06-17 by direct
host inspection (the `.ri` D3 probe workflow is N/A — no grammar/check/ir surface; see
PRD §3). Every binding **PASS** → batch is not blocked.

## Leaf α — Retire the #4593 perf-anchor: reword markers, amend the detector PRD, retire the task

**Signal (PRD §8):** on main after the change, `reify-audit --pattern PTODO`
(`REIFY_PTODO_TASKS_DB=<root>/.taskmaster/tasks/tasks.db`) exits 0 with zero violations
above the empty baseline; no `TODO/FIXME/HACK(#4593)` debt marker remains in tracked
source; the only residual `#4593` swept-source strings are the three allowlisted
`crates/reify-audit/` test-fixture/example lines; `get_task(4593)` = `cancelled`.

| Check | Asserted capability | Evidence | Verdict |
|---|---|---|---|
| Capability→producer (anti-orphan) | **No new mechanism** — purely subtractive (reword + prose amend + status flip). Consumers (PTODO gate, `CLAUDE.md` convention, implementers) already exist on main | `crates/reify-audit/src/ptodo.rs:532` (`is_terminal_status`), `:905` (allowlist skip gate); `CLAUDE.md` "TODO citation convention" §; PTODO landed (η #4559) | **PASS** (N/A — nothing produced) |
| DAG-direction (anti-inversion) | Single leaf, no intra-batch prereq; #4593 is the task *being retired*, not a dependency | n/a | **PASS** (N/A) |
| Field-population | No result value / sampled field asserted | n/a | **PASS** (N/A) |
| Grammar-fixture | No `.ri` syntax introduced | n/a | **PASS** (N/A) |
| Numeric-floor | No numeric bound / accuracy claim | n/a | **PASS** (N/A) |
| Baseline non-regression | `reify-audit --pattern PTODO` reports **zero violations above baseline** post-change | `crates/reify-audit/ptodo-baseline.txt` is **empty (0 lines)**; PTODO is green at zero on main today; reword only **removes** markers (`\b(TODO\|FIXME\|HACK)\b\s*[(:]` no longer matches) → strictly subtractive, cannot add a violation | **PASS** |
| **Cancel-safety (negative-assertion / G6 branch 4)** | Cancelling #4593 yields **zero `orphaned` findings** — the gate stays green | The `orphaned` rejection mechanism **exists and fires** on terminal cites (`is_terminal_status` `ptodo.rs:532` + liveness scan `ptodo.rs:609`/`:738`; detector PRD scenario 3). Exhaustive enumeration: `git grep -n 4593 -- '*.rs'` → 16 hits; **13 are reworded markers** (removed by α across `reify-eval`/`reify-expr`/`reify-kernel-fidget` + `reify-audit` p1/p2); the **3 residual** strings (`reify-audit/tests/p2.rs:1723/:1743`, `p2_consumer_stub.rs:96`) all sit under `crates/reify-audit/`, **skipped at the `is_allowlisted` gate (`ptodo.rs:905`) before any scan**. So post-reword the detector sees **zero** #4593 cites → the rejection mechanism has zero inputs → zero `orphaned` findings | **PASS** |

## G1 special-case binding — `kernel.rs:219` JitShape cache → reword (no perf task filed)

The brief deferred this to the gate (file a real perf task vs reword). G1 consumer
probe: `git grep evaluate_sdf_at -- '*.rs'` → **zero production callers** (only
`kernel.rs` doc-comments/tests + `tests/dispatcher_integration.rs`). The hypothesized
"GUI per-pixel SDF raster preview" consumer **does not exist and is not imminent**.
Filing a perf task would be a **G1 orphan-producer** (the exact failure this PRD
retires). **Verdict: reword.** No task filed.

## Ordering guard (intra-task, not a DAG edge)

α must **reword-and-merge to main first**, then (final post-merge action) verify zero
detector-visible #4593 cites and only then `set_task_status(4593, cancelled)`. Cancelling
before the reword lands would orphan the live cites → PTODO RED on main. This sequencing
discharges the detector PRD §6.1 lifecycle invariant ("anchor stays non-terminal while
any marker cites it"); see PRD §4.
