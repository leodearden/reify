---
name: audit
description: "Periodic architecture-audit sweep for the Reify codebase. ALWAYS use this skill for: /audit commands, running the architecture-audit detector CLI against live task state, filing follow-up tasks for phantom-done or orphan-symbol findings, and producing per-run JSON artifacts under data/audit-runs/. Triggers on: '/audit', '/audit --task <id>', '/audit --since <date>', '/audit --pattern P1|P2|P5', '/audit --format markdown', or any request to run the F-infra audit sweep. This is NOT for: editing audit findings or gap-register.md (that is manual curation), running tasks (/orchestrate), reviewing landed code (/review), unblocking tasks (/unblock)."
---

# Architecture Audit Sweep (`/audit`)

This skill operationalizes the **F-infra** portfolio entry from the 2026-05-12 architecture audit: a periodic-sweep cadence that detects phantom-done tasks, producer-orphan symbols (P1), consumer-stub symbols (P2), and pre-done-hook phantom-done (P5). The detection logic lives in the `reify-audit` Rust CLI (T-4 / task 3672). This skill is the human-facing glue: it shells out to the CLI, routes findings by severity, and persists a per-run JSON time-series.

Design foundation: `docs/architecture-audit/f-infra-design.md` §3 (modes), §6 (severity routing), §7 (storage/output).

## Modes

Pick from the user's invocation and context:

| Invocation | Behaviour | Detail |
|---|---|---|
| `/audit` (default) | Window sweep over the last 14 days — all three detectors (P1, P2, P5) | `references/modes.md` §1 |
| `/audit --task <id>` | Spot-check a single task — all three detectors | `references/modes.md` §2 |
| `/audit --since <iso-date>` | Window sweep from a given date to now | `references/modes.md` §3 |
| `/audit --pattern P1\|P2\|P5` | Restrict to one detector | `references/modes.md` §4 |
| `/audit --format markdown` | Any mode + emit a fenced markdown report in addition to the JSON artifact | `references/modes.md` §5 |

`--task`, `--since`, and `--pattern` compose. `--pre-done` is reserved for the dark-factory D-1 pre-done hook and is **not callable from this skill**. See `references/modes.md` §6 (Mode composition).

**jcodemunch resilience:** The default sweep and `--pattern P1` are resilient to a down jcodemunch substrate. When jcodemunch-serve is unreachable, P1 degrades to zero findings (a `reify-audit: jcodemunch unreachable …` breadcrumb appears on stderr) while P2/P5 still run normally — the sweep does **not** exit 125. Use `--no-jcodemunch` to force the inert stub and silence the breadcrumb. See `references/cli-invocation.md` §4.1 for failure-mode detail and recovery hints.

## Severity ladder

Route each finding by its `severity` field immediately after parsing the CLI's JSON output:

| Severity | Action | Tool |
|---|---|---|
| **High** | Escalate (advisory, non-blocking) | `mcp__escalation__escalate_info` |
| **Medium** | File a deferred follow-up task | `mcp__fused-memory__submit_task(planning_mode=True)` |
| **Low** | Log into per-run JSON only | _(no side effects)_ |

The skill **never** calls `set_task_status`. State-mutation of the offending task is a human decision made during triage. See `references/severity-routing.md` for dedupe contract, per-pattern task-title templates, and the do-not-flip-status invariant.

## Outputs

Every run writes two artifacts under `data/audit-runs/` (gitignored):

- `data/audit-runs/<YYYY-MM-DDTHH-MM-SSZ>.json` — per-run JSON record.
- `data/audit-runs/index.json` — dedupe store (append-only within a run, rewritten on each run).

If `--format markdown` is given, emit a fenced markdown report to the user in addition to the JSON.

See `references/output-format.md` for the full JSON schema, `index.json` format, and markdown rendering rules.

## How the skill works

1. Resolve the `reify-audit` binary (prefer `target/release/reify-audit`; fallback to `cargo run --release --quiet -p reify-audit --`).
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
