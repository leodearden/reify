# Audit Modes

Five invocation modes. All modes write a per-run JSON artifact; `--format markdown` adds a fenced markdown report on top.

> All argv paths use `$REPO_ROOT/` — see `references/cli-invocation.md` §1 for the pre-flight that resolves it.
>
> Every invocation below assumes `$SNAPSHOT` has already been materialized
> per `references/cli-invocation.md` §2 (an MCP-tool call from the LLM
> followed by a `jq` filter step — **not** a single shell pipeline). The
> per-mode `reify-audit` argvs below are the bash-side invocation only; do
> not re-inline the snapshot setup here. Single point of truth: §2.

---

## §1 Default mode (14-day window sweep)

**When to use:** Routine periodic sweep — no specific task or date in mind.

**Argv produced** (after `$SNAPSHOT` is materialized per `cli-invocation.md` §2):

```bash
reify-audit \
  --since <14d-ago-iso> \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT"
```

**Pre-flight:** Compute `<14d-ago-iso>` as the ISO-8601 date exactly 14 days before `now` (UTC). Example: if today is `2026-05-16`, use `--since 2026-05-02`. The CLI accepts `YYYY-MM-DD` or full ISO-8601 for `--since`.

**Scope object in per-run JSON:**

```json
{ "window": "14d" }
```

**Detectors run:** P1, P2, P5, PTODO (all four default-sweep detectors, no `--pattern` restriction).

---

## §2 Spot-check mode (`--task <id>`)

**When to use:** User says `/audit --task 3242` or wants to audit a specific task.

**Argv produced** (after `$SNAPSHOT` is materialized per `cli-invocation.md` §2):

```bash
reify-audit \
  --task <id> \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT"
```

**Pre-flight:** `--task <id>` always shells out, regardless of the target task's status — including `done`. P5 (phantom-done) is the detector that only fires on done tasks, so spot-checking a freshly-completed task to confirm it is not phantom-done is precisely the intended use of this mode. A clean run (0 findings) on a done task is positive evidence that the task is not phantom-done.

(If a "you ran P1/P2 on a done task and got nothing — that's expected" hint is useful context for the user, it belongs in the summary the skill prints *after* findings come back — e.g. a one-line aside appended to the per-run report — NOT in this pre-flight block. Future authors: do not re-introduce a status-conditional CLI-skip here.)

**Scope object in per-run JSON:**

```json
{ "task": "<id>" }
```

**Detectors run:** P1, P2, P5, PTODO (all four default-sweep detectors, no `--pattern` restriction).

---

## §3 Window sweep mode (`--since <iso-date>`)

**When to use:** User wants to sweep a custom date range, e.g. `/audit --since 2026-04-01`.

**Argv produced** (after `$SNAPSHOT` is materialized per `cli-invocation.md` §2):

```bash
reify-audit \
  --since <iso-date> \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT"
```

**Pre-flight:** Validate that `<iso-date>` parses as a date (YYYY-MM-DD or full ISO-8601) and is in the past. If not, surface an error to the user and stop.

**Scope object in per-run JSON:**

```json
{ "window": "<iso-date>..now" }
```

**Detectors run:** P1, P2, P5, PTODO (all four default-sweep detectors, unless `--pattern` also supplied — see §6).

---

## §4 Pattern-restricted mode (`--pattern P1|P2|P5|PTODO|PDEAD|PUNTESTED|PLAYER`)

**When to use:** User wants to run only one detector, e.g. `/audit --pattern P5`, `/audit --pattern PTODO`, or `/audit --pattern PDEAD`.

**Argv produced** (after `$SNAPSHOT` is materialized per `cli-invocation.md` §2):

```bash
reify-audit \
  --since <14d-ago-iso> \
  --pattern <P1|P2|P5|PTODO|PDEAD|PUNTESTED|PLAYER> \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT"
```

(If `--since` or `--task` is also given, use that instead of the default 14d window.)

**Scope object in per-run JSON:**

```json
{ "patterns": ["P1"] }        // or ["P2"] or ["P5"]
{ "patterns": ["PTODO"] }     // TODO-tracking invariant (default-sweep, deterministic)
{ "patterns": ["PDEAD"] }     // advisory: dead code
{ "patterns": ["PUNTESTED"] } // advisory: untested symbols
{ "patterns": ["PLAYER"] }    // advisory: layer/import-boundary violations
```

**Detectors run:** The named detector only.

### PTODO — notes

PTODO (`--pattern PTODO`) is **part of the no-`--pattern` default all-detector sweep** (P1/P2/P5/PTODO) — this section documents its explicit invocation. It is distinct from the opt-in advisory P-* patterns below.

- **Severity:** All PTODO violation kinds emit **Severity Medium** → file a deferred follow-up task per `references/severity-routing.md` PTODO row.
- **Implementation:** Deterministic grep + read-only sqlite; **no jcodemunch/LLM/MCP**. Unaffected by jcodemunch outages. Only its liveness lane degrades gracefully when `tasks.db` is absent (one stderr breadcrumb; structural lane still runs). See `references/cli-invocation.md` §4.1 PTODO note.
- **Exit-neutrality:** PTODO emits Medium only; exit code = High-severity count, so a PTODO-only run always exits 0 on a clean tree.

### Advisory P-* patterns (PDEAD / PUNTESTED / PLAYER) — notes

These three patterns are **opt-in only** — they are NOT part of the default all-detector sweep (which runs P1/P2/P5/PTODO). They fire only when named explicitly via `--pattern`.

- **Severity:** All three emit Severity Low — log-only, advisory, **never auto-filed** as a follow-up task. See `references/severity-routing.md` for routing details.
- **Serve dependency:** PDEAD, PUNTESTED, and PLAYER all require `jcodemunch-serve` to be running. When the serve is unreachable, they degrade to **zero findings** (same fail-soft path as P1; P2/P5/PTODO are unaffected — NOT exit 125). See `references/cli-invocation.md` §4.1 for the fail-soft behaviour and `--jcodemunch-url` flag.
- **Activation:** For serve startup instructions see `docs/architecture-audit/jcodemunch-serve-activation.md`.

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
| `--since <date> --pattern PTODO` | Window sweep from `<date>`, PTODO only (Medium; deterministic, no jcodemunch) |
| `--since <date> --pattern PDEAD` | Window sweep from `<date>`, PDEAD advisory only (Low/log) |
| `--task <id> --since <date>` | Both flags accepted; `AuditContext` receives both `target_task_id` and `window` (CLI source: `reify-audit.rs` lines 333–342). Whether detectors treat this as a strict scope intersection depends on the detector implementation — verify against the detector source or CLI `--help` if exact semantics matter. |
| `--format markdown` | Adds markdown output to **any** of the above |

**`--pre-done` is NOT composable from the skill.** It is reserved exclusively for the dark-factory D-1 pre-done hook (`REIFY_AUDIT_PREDONE_CMD`). The skill never passes `--pre-done` to the CLI. If a user asks to simulate a pre-done check, use `--task <id> --pattern P5` instead, which runs P5 in periodic-sweep (not blocking) mode.
