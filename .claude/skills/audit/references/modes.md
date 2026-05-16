# Audit Modes

Five invocation modes. All modes write a per-run JSON artifact; `--format markdown` adds a fenced markdown report on top.

---

## §1 Default mode (14-day window sweep)

**When to use:** Routine periodic sweep — no specific task or date in mind.

**Argv produced:**

```
reify-audit \
  --since <14d-ago-iso> \
  --tasks-file .taskmaster/tasks/tasks.json \
  --runs-db data/orchestrator/runs.db \
  --project-root .
```

**Pre-flight:** Compute `<14d-ago-iso>` as the ISO-8601 date exactly 14 days before `now` (UTC). Example: if today is `2026-05-16`, use `--since 2026-05-02`. The CLI accepts `YYYY-MM-DD` or full ISO-8601 for `--since`.

**Scope object in per-run JSON:**

```json
{ "window": "14d" }
```

**Detectors run:** P1, P2, P5 (all three, no `--pattern` restriction).

---

## §2 Spot-check mode (`--task <id>`)

**When to use:** User says `/audit --task 3242` or wants to audit a specific task.

**Argv produced:**

```
reify-audit \
  --task <id> \
  --tasks-file .taskmaster/tasks/tasks.json \
  --runs-db data/orchestrator/runs.db \
  --project-root .
```

**Pre-flight:** Before shelling out, look up the task status via `mcp__fused-memory__get_task`. If `status == "done"`, emit the "already done" message without running the CLI:

> Task `<id>` is already marked done. No audit sweep needed. If you suspect phantom-done, use `/audit --task <id> --pattern P5` to run just the P5 detector.

(This avoids a redundant run and clarifies the pre-done hook distinction.)

**Scope object in per-run JSON:**

```json
{ "task": "<id>" }
```

**Detectors run:** P1, P2, P5 (all three, no `--pattern` restriction).

---

## §3 Window sweep mode (`--since <iso-date>`)

**When to use:** User wants to sweep a custom date range, e.g. `/audit --since 2026-04-01`.

**Argv produced:**

```
reify-audit \
  --since <iso-date> \
  --tasks-file .taskmaster/tasks/tasks.json \
  --runs-db data/orchestrator/runs.db \
  --project-root .
```

**Pre-flight:** Validate that `<iso-date>` parses as a date (YYYY-MM-DD or full ISO-8601) and is in the past. If not, surface an error to the user and stop.

**Scope object in per-run JSON:**

```json
{ "window": "<iso-date>..now" }
```

**Detectors run:** P1, P2, P5 (all three, unless `--pattern` also supplied — see §6).

---

## §4 Pattern-restricted mode (`--pattern P1|P2|P5`)

**When to use:** User wants to run only one detector, e.g. `/audit --pattern P5`.

**Argv produced:**

```
reify-audit \
  --since <14d-ago-iso> \
  --pattern <P1|P2|P5> \
  --tasks-file .taskmaster/tasks/tasks.json \
  --runs-db data/orchestrator/runs.db \
  --project-root .
```

(If `--since` or `--task` is also given, use that instead of the default 14d window.)

**Scope object in per-run JSON:**

```json
{ "patterns": ["P1"] }   // or ["P2"] or ["P5"]
```

**Detectors run:** The named detector only.

---

## §5 Markdown format (`--format markdown`)

**When to use:** User appends `--format markdown` to any other invocation, e.g. `/audit --format markdown`.

**Behaviour:** This flag is consumed by the **skill** (not passed to the CLI). Run the underlying mode normally, then after writing the per-run JSON artifact, render and emit a fenced markdown report to the user.

**Slice-1 rendering rules** (see `references/output-format.md` §4 for full spec):

1. Open with `# /audit run <timestamp>`.
2. Summary line: `N findings (X high, Y medium, Z low)`.
3. One `## High` / `## Medium` / `## Low` section per severity (omit empty sections).
4. Within each section, a markdown table:
   ```
   | task_id | pattern | summary | action_taken |
   |---------|---------|---------|--------------|
   | 3242    | P5      | …       | escalated    |
   ```

Slice-2 deeper rendering (per-finding evidence expansion, links to task URLs) is deferred per design §7 v1 callout.

---

## §6 Mode composition

`--task`, `--since`, and `--pattern` **compose**:

| Combination | Effect |
|---|---|
| `--task <id> --pattern P5` | Spot-check task `<id>`, P5 only |
| `--since <date> --pattern P1` | Window sweep from `<date>`, P1 only |
| `--task <id> --since <date>` | Both flags → CLI uses `--task` for task scope + `--since` for window; detectors see the intersection |
| `--format markdown` | Adds markdown output to **any** of the above |

**`--pre-done` is NOT composable from the skill.** It is reserved exclusively for the dark-factory D-1 pre-done hook (`REIFY_AUDIT_PREDONE_CMD`). The skill never passes `--pre-done` to the CLI. If a user asks to simulate a pre-done check, use `--task <id> --pattern P5` instead, which runs P5 in periodic-sweep (not blocking) mode.
