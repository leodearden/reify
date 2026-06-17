# Capability manifest — reify-audit-ptodo-detector.md

Binds each task's signal capabilities to evidence (mechanized G3+G6). Built
2026-06-11 at decompose time; all bindings PASS. Evidence verified against main
@ 39f585ebaa (PRD commit) / 9dbac2ff71 (pre-PRD main).

No grammar-fixture bindings (no novel `.ri` syntax) and no numeric-floor bindings
(no quantitative signal premises) anywhere in this batch.

## α — PTODO structural lane + CLI wiring (intermediate; unlocks β/γ/δ)

| Capability | Evidence | Verdict |
|---|---|---|
| `Pattern` enum extension point + per-module `check(ctx) -> Vec<Finding>` dispatch | grep: `crates/reify-audit/src/lib.rs:84-133`; dispatch `src/bin/reify-audit.rs:590-627` | PASS (wired) |
| `--pattern` token parser accepts new comma-separated tokens | grep: `src/bin/reify-audit.rs:249-270` (hand-rolled validator; PTODO token is an additive arm) | PASS (wired) |
| GitOps subprocess seam for `ls_files()` addition | grep: `crates/reify-audit/src/lib.rs:442-513` (`RealGitOps::run`, existing diff/show methods on same seam) | PASS (wired) |
| Findings JSON on stderr + summary on stdout | grep: `src/bin/reify-audit.rs:629-647` | PASS (wired) |
| Sweep population exists (~1898 tracked code files of 2462) | `git ls-files | grep -cE '\.(rs|ri|sh|py|ts|tsx|js)$'` = 1898, run 2026-06-11 | PASS |

## β — liveness lane (intermediate; unlocks δ/ζ)

| Capability | Evidence | Verdict |
|---|---|---|
| rusqlite available in-crate | grep: `crates/reify-audit/Cargo.toml:20` (`rusqlite 0.31, bundled` — already used for runs.db) | PASS (wired) |
| Task DB schema: `tasks(tag, id, title, …, status)` PK `(tag,id)`, status ∈ {done, cancelled, pending, in-progress, deferred} | live `sqlite3 "file:.taskmaster/tasks/tasks.db?mode=ro" ".schema tasks"` + status group-by, 2026-06-11 | PASS |
| Degradation premise: DB absent in worktrees | `git ls-files .taskmaster` = 0 (untracked) — verified 2026-06-11 | PASS |
| Fail-soft precedent to mirror | grep: `src/bin/reify-audit.rs:542-567` (4109 jcodemunch degradation contract — breadcrumb, no exit 125) | PASS (wired) |
| producer:task-α upstream (marker scan the liveness lane annotates) | DAG: β depends_on α | PASS (upstream) |

## γ — #[ignore] lane (intermediate; unlocks δ)

| Capability | Evidence | Verdict |
|---|---|---|
| Reusable extraction fns in reify-test-support | grep: `crates/reify-test-support/src/ignore_hygiene.rs:24,114,199,251` (`find_stale_plan_pointers_in_source`, `check_ignore_reasons`, `walk_test_rs_files`, `collect_workspace_stale_pointers` — all pub) | PASS (wired) |
| Task-1622 test to reconcile with | `crates/reify-test-support/tests/ignore_reason_hygiene.rs:29-67` (cargo-test invocation) | PASS (wired) |
| Rotted-ignore class is real (signal premise) | audit 2026-06-11: 10 of 22 blocker-citing `#[ignore]`s cited terminal blockers (`/tmp/reify-todo-audit/ignores.txt`) | PASS |
| producer:task-α (marker scan) + task-β (liveness) upstream | DAG: γ depends_on α, β | PASS (upstream) |

## δ — migration sweep + baseline (intermediate; unlocks ε)

| Capability | Evidence | Verdict |
|---|---|---|
| Detector lanes exist to measure the violation set | producer: tasks α, β, γ — all upstream in this batch | PASS (upstream) |
| Migration corpus exists and is bounded | audit inventory `/tmp/reify-todo-audit/numbered.txt` (386 raw → 83 real records, hand-triaged 2026-06-11; owners 4535–4552 filed) | PASS |
| Zeroing batch is NOT a dep (signal achievable regardless) | δ re-cites whatever is current; baseline absorbs residue (PRD §7); 4540/4541/4543 in-progress conflicts are comment-line-trivial | PASS |

## ε — integration gate (LEAF, critical)

| Capability | Evidence | Verdict |
|---|---|---|
| Default-sweep membership mechanism | grep: `src/bin/reify-audit.rs:599-605` (`args.pattern.is_none_or(...)` per-detector predicates — additive) | PASS (wired) |
| Medium severity is exit-neutral (warn-first premise) | grep: `src/bin/reify-audit.rs:629-647` (exit code = High count) | PASS (wired) |
| infra harness auto-discovery | `tests/infra/run_all.sh` glob `test_*.sh` (existing pattern, 50+ tests) | PASS (wired) |
| Freshness guard covers new detector automatically | `scripts/reify-audit-freshness.sh` + `tests/infra/test_reify_audit_freshness.sh` (binary-level staleness, detector-agnostic) | PASS (wired) |
| `/audit` skill doc is tracked + editable | `git ls-files .claude/skills/audit/` → SKILL.md + 4 references | PASS (wired) |
| P2 Family-1 vocabulary site for canonical-form extension | grep: `crates/reify-audit/src/p2_consumer_stub.rs:43-92` (substring families incl. `TODO(task_N)`) | PASS (wired) |
| §10 doc to amend | `docs/prds/reify-audit-p1-jcodemunch-substrate.md:140-157` (4115 NO-decision record) | PASS (wired) |
| Whole-chain producers upstream | producer: tasks α, β, γ, δ — all upstream | PASS (upstream) |
| Signal "introducing an untracked TODO flips the infra check red" | mechanism = baseline-ratchet (§6.6); structural lane needs no task DB → works in worktrees (§6.7) — no hidden DB dependency in the gate signal | PASS |

## ζ — inverse lane (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| `metadata.files` is structured + populated on tasks | live task reads (e.g. 4551 metadata.files list) via fused-memory, 2026-06-11; column `metadata` TEXT in tasks schema | PASS |
| Git deletion-history check | `git log -1 -- <path>` via existing GitOps subprocess seam (`lib.rs:442-458`) | PASS (wired) |
| Violation class is real (signal premise) | audit 2026-06-11: 9+ non-terminal tasks cite the deleted `reify-types` crate (critic gap 8, `/tmp/reify-todo-audit/audit-result.json`) | PASS |
| producer:task-β (DB machinery) upstream | DAG: ζ depends_on β | PASS (upstream) |

## η — ratchet to hard gate (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| Severity flip mechanism | `Finding.severity` + exit-code = High count (`bin/reify-audit.rs:629-647`) — flipping kind→High is a local change | PASS (wired) |
| Dispatch condition is concrete + checkable | "PTODO reports zero violations on main" — one CLI invocation; per feedback_deferred_needs_flip_condition this is a pending task with dep edge (ε) + dispatch-time check, not a deferred park | PASS |
| producer:task-ε upstream | DAG: η depends_on ε | PASS (upstream) |

## θ — vocabulary-expansion ASSESS (LEAF)

| Capability | Evidence | Verdict |
|---|---|---|
| Candidate corpus enumerated with counts | audit critic gap 1: "not yet implemented" 51 (incl. STUB_MSGs at reify-kernel-manifold/kernel.rs:46, reify-kernel-openvdb/kernel.rs:23, mesh-morph lib.rs:168, constraints solver.rs:580), "for now" 42, "placeholder" 973, "stub" 1472, "XXX" 232, "workaround" 68 | PASS |
| FP-review methodology precedent | tasks 4075/4076/4141 (P5/P2 live-corpus reviews); 4115 NO-decision record pattern (PRD §10 amendment commit) | PASS |
| producer:task-ε upstream (detector live before vocabulary extends) | DAG: θ depends_on ε | PASS (upstream) |

## ι — parked-on-anchor guard (LEAF, advisory)

| Capability | Evidence | Verdict |
|---|---|---|
| `metadata.do_not_complete` flag exists in live tasks.db | live tasks.db 2026-06-17: `SELECT id, metadata FROM tasks WHERE tag='master'` shows #4593 metadata `{"do_not_complete":true,...}` (exactly 1 non-terminal do_not_complete at decompose time; now retired) | PASS |
| serde_json metadata-parse pattern to mirror | `crates/reify-audit/src/ptodo.rs` `resolve_inverse` `SELECT id, status, metadata FROM tasks WHERE tag='master'` + serde_json::from_str::<Value> parse | PASS (wired) |
| `metadata` column already nullable in test schema | `tests/common/schema.rs:57-66` `TASKS_DB_SCHEMA` `metadata TEXT` (nullable); `insert_task_with_metadata` helper at line 108 | PASS (wired) |
| Medium severity is exit-neutral | `bin/reify-audit.rs:629-647` (exit code = High count; Medium findings are advisory) | PASS (wired) |
| Liveness-lane fail-soft degrades automatically | `resolve_liveness_keyed` is reached only when DB is open; DB-absent path skips entire liveness lane (§6.7) | PASS (wired) |
| producer:tasks β + ε upstream (liveness machinery + integration gate) | DAG: ι depends_on β, ε | PASS (upstream) |
