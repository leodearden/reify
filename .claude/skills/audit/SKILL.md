---
name: audit
description: "Periodic architecture-audit sweep for the Reify codebase. ALWAYS use this skill for: /audit commands, running the architecture-audit detector CLI against live task state, filing follow-up tasks for phantom-done or orphan-symbol findings, and producing per-run JSON artifacts under data/audit-runs/. Triggers on: '/audit', '/audit --task <id>', '/audit --since <date>', '/audit --pattern P1|P2|P5|PTODO|PDEAD|PUNTESTED|PLAYER', '/audit --format markdown', any request to run the F-infra audit sweep, or any mention of TODO-tracking invariant detection or PTODO. This is NOT for: editing audit findings or gap-register.md (that is manual curation), running tasks (/orchestrate), reviewing landed code (/review), unblocking tasks (/unblock)."
---

# Architecture Audit Sweep (`/audit`)

This skill operationalizes the **F-infra** portfolio entry from the 2026-05-12 architecture audit: a periodic-sweep cadence that detects phantom-done tasks, producer-orphan symbols (P1), consumer-stub symbols (P2), pre-done-hook phantom-done (P5), and TODO-tracking invariant violations (PTODO). The detection logic lives in the `reify-audit` Rust CLI (T-4 / task 3672). This skill is the human-facing glue: it shells out to the CLI, routes findings by severity, and persists a per-run JSON time-series.

Design foundation: `docs/architecture-audit/f-infra-design.md` §3 (modes), §6 (severity routing), §7 (storage/output).

## Modes

Pick from the user's invocation and context:

| Invocation | Behaviour | Detail |
|---|---|---|
| `/audit` (default) | Window sweep over the last 14 days — all four default detectors (P1, P2, P5, PTODO) | `references/modes.md` §1 |
| `/audit --task <id>` | Spot-check a single task — all four default detectors | `references/modes.md` §2 |
| `/audit --since <iso-date>` | Window sweep from a given date to now | `references/modes.md` §3 |
| `/audit --pattern P1\|P2\|P5` | Restrict to one of the standard detectors | `references/modes.md` §4 |
| `/audit --pattern PTODO` | Run only the TODO-tracking invariant detector (deterministic, no jcodemunch) | `references/modes.md` §4 |
| `/audit --pattern PDEAD\|PUNTESTED\|PLAYER` | Run one advisory jcodemunch detector (opt-in, Severity Low, serve-dependent) | `references/modes.md` §4 |
| `/audit --format markdown` | Any mode + emit a fenced markdown report in addition to the JSON artifact | `references/modes.md` §5 |

`--task`, `--since`, and `--pattern` compose. `--pre-done` is reserved for the dark-factory D-1 pre-done hook and is **not callable from this skill**. See `references/modes.md` §6 (Mode composition).

**jcodemunch resilience:** The default sweep, `--pattern P1`, and the advisory patterns (`--pattern PDEAD|PUNTESTED|PLAYER`) are resilient to a down jcodemunch substrate. When jcodemunch-serve is unreachable, P1 and all three advisory P-* patterns degrade to **zero findings** (a `reify-audit: jcodemunch unreachable …` breadcrumb appears on stderr) while P2/P5 still run normally — the sweep does **not** exit 125. **PTODO is unaffected by jcodemunch outages** — it is deterministic (grep + read-only sqlite) and never contacts jcodemunch; only its liveness lane degrades when `tasks.db` is absent (stderr breadcrumb, structural lane still runs). Use `--no-jcodemunch` to force the inert stub and silence the breadcrumb. See `references/cli-invocation.md` §4.1 for failure-mode detail and recovery hints.

## Advisory jcodemunch patterns (PDEAD / PUNTESTED / PLAYER)

Three opt-in detectors backed by jcodemunch — invoked only when named explicitly via `--pattern`:

| Pattern | What it detects |
|---|---|
| `PDEAD` | Dead code — exported or public symbols with no callers/references (as observed by jcodemunch) |
| `PUNTESTED` | Untested symbols — public symbols with no corresponding test coverage paths |
| `PLAYER` | Layer/import-boundary violations — modules importing from layers they should not depend on |

**Severity and routing:** All three patterns emit Severity **Low** only — log-only, advisory, **never auto-filed** as a follow-up task, and never promoted to Medium. See `references/severity-routing.md` for the Low row routing details.

**Serve prerequisite:** These patterns require `jcodemunch-serve` to be running and reachable at the configured URL to produce real findings. When unreachable they degrade gracefully to zero findings (same fail-soft path as P1; P2/P5 are unaffected). For activation instructions (port 8901, unit name, enable/status commands) see `docs/architecture-audit/jcodemunch-serve-activation.md` — that document is the single source of truth for serve operational identifiers.

**Key flags:** `--jcodemunch-url <url>` (default: `$JCODEMUNCH_URL` or `http://127.0.0.1:8901/mcp`), `--jcodemunch-repo <id>` (default: `leodearden/reify`), `--no-jcodemunch` (force inert stub, offline/test). See `references/cli-invocation.md` §2 and §4.1 for full flag documentation and the trailing-slash gotcha.

**Not part of the default sweep:** PDEAD/PUNTESTED/PLAYER fire **only** when named explicitly via `--pattern`. Running `/audit` without a `--pattern` flag runs P1/P2/P5/PTODO (the four default-sweep detectors), not the advisory P-* patterns.

## PTODO — TODO-tracking invariant

PTODO detects violations of the project's TODO citation invariant (per `docs/prds/reify-audit-ptodo-detector.md` §8). It is deterministic (grep + read-only sqlite; **no LLM/jcodemunch/MCP**) and runs in the **no-`--pattern` default sweep**. It is distinct from the opt-in advisory P-* detectors above.

**Violation taxonomy (§8.3) — severity as of task η, #4559:**

| Kind | Severity | Meaning |
|------|----------|---------|
| `untracked` | **High** → hard gate (non-zero exit) | Marker present in a tracked source file but cites no task ID |
| `bare-ignore` | **High** → hard gate (non-zero exit) | `#[ignore]` attribute with no reason string |
| `orphaned` | **High** → hard gate (non-zero exit) | Cited task is terminal (done/cancelled). Liveness lane: High only where tasks.db exists |
| `malformed-cite` | Medium | Marker has a cite but not in canonical `#NNNN` form (Greek letter, PRD-relative, legacy) |
| `phantom-tracking` | Medium | Source cite `#N` is in the tasks DB but the cited task does not list this file |
| `unknown-id` | Medium | Cited `#NNNN` not found in the tasks DB (a DB-sync race must not hard-fail verify) |
| `task-cites-deleted-path` | Medium (advisory) | A non-terminal task's `metadata.files` path has git history but is no longer tracked |

**Hard-gate model (task η, #4559):** `untracked`/`orphaned`/`bare-ignore` emit `Severity::High` → `reify-audit` exits non-zero (exit code = High count) and hard-fails the `tests/infra` verify step. The Medium kinds and `task-cites-deleted-path` (advisory) remain exit-neutral.

**Degradation:** when `tasks.db` is absent/unreadable, the liveness and inverse lanes are skipped (one stderr breadcrumb); the structural lane (`untracked`/`malformed-cite`/`bare-ignore`) still runs. The structural High kinds (untracked/bare-ignore) fire everywhere; the liveness High kind (orphaned) fires only where tasks.db exists.

**Default-sweep membership:** PTODO High kinds (`untracked`/`orphaned`/`bare-ignore`) drive a non-zero exit when violations are present. On a clean tree (no violations) PTODO adds zero High findings and the exit code is unchanged.

## Severity ladder

Route each finding by its `severity` field immediately after parsing the CLI's JSON output:

| Severity | Action | Tool |
|---|---|---|
| **High** | Escalate (advisory, non-blocking) | `mcp__escalation__escalate_info` |
| **Medium** | File a deferred follow-up task | `mcp__fused-memory__submit_task(planning_mode=True)` |
| **Low** | Log into per-run JSON only | _(no side effects)_ |

The skill **never** calls `set_task_status`. State-mutation of the offending task is a human decision made during triage. See `references/severity-routing.md` for dedupe contract, per-pattern task-title templates, and the do-not-flip-status invariant.

**PTODO severity routing (post-η):** `untracked`/`orphaned`/`bare-ignore` → High → escalate via `escalate_info`; `malformed-cite`/`phantom-tracking`/`unknown-id` → Medium → file deferred follow-up task; `task-cites-deleted-path` → Medium (advisory) → file deferred follow-up task. See `references/severity-routing.md` for the PTODO title template and per-kind routing notes.

## Outputs

Every run writes two artifacts under `data/audit-runs/` (gitignored):

- `data/audit-runs/<YYYY-MM-DDTHH-MM-SSZ>.json` — per-run JSON record.
- `data/audit-runs/index.json` — dedupe store (append-only within a run, rewritten on each run).

If `--format markdown` is given, emit a fenced markdown report to the user in addition to the JSON.

See `references/output-format.md` for the full JSON schema, `index.json` format, and markdown rendering rules.

## How the skill works

1. Resolve the `reify-audit` binary: source `scripts/reify-audit-freshness.sh` and call `reify_audit_guard` in **rebuild** mode to ensure `target/release/reify-audit` is fresh (rebuilds via `cargo build --release -q -p reify-audit` if stale, since `reify-audit` is absent from `scripts/release-sensitive-crates.txt`). Then prefer the freshness-checked release binary; fallback to `cargo run --release --quiet -p reify-audit --` when no binary can be built. See `references/cli-invocation.md` §1.
2. Build the argv from the user's invocation mode (see `references/modes.md`).
3. Shell out, capturing stderr to a tempfile (via `mktemp /tmp/reify-audit-XXXXXX.json` with an EXIT trap; see `references/cli-invocation.md` §2).
4. Interpret the exit code (0 / 1–254 / 125) per `references/cli-invocation.md`.
5. Parse the JSON array of `Finding` objects from the tempfile.
6. For each finding, apply the severity ladder (see `references/severity-routing.md`), performing dedupe lookup before filing any medium-severity task.
7. Write the per-run JSON artifact and update `data/audit-runs/index.json`.
8. If `--format markdown`, render and emit the markdown report.
9. Remove the tempfile.

## Anti-triggers

- **Editing audit findings or `gap-register.md`** → not this skill. Manual curation of the gap register is a human-driven prose edit, not a skill invocation.
- **Running tasks** → `/orchestrate`.
- **Reviewing landed code** → `/review`.
- **Unblocking tasks** → `/unblock`.
- **Authoring PRDs** → `/prd`.
- **Authoring `.ri` design files** → `/reify-design`.

## Related memories / docs

- `docs/architecture-audit/f-infra-design.md` — the canonical design for this skill and its CLI dependency.
- `docs/architecture-audit/audit-brief.md` — failure-mode catalog (F1–F7); P1/P2/P5 map to F1/F3/F4.
- `docs/architecture-audit/gap-register.md` — the gap register this skill's findings may reference.
- fused-memory entity `feedback_task_chain_user_observable` — user-observable signal discipline (why G2 matters and how it connects to phantom-done detection).
- fused-memory entity `project_phantom_done_metadata_files_strip_may09` — the strip-metadata event that motivated P5 phantom-done detection.
- fused-memory entity `preferences_implementation_chain_portfolio` — the 8-approach portfolio; F-infra is approach F.
