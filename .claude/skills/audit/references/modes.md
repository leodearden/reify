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

**Detectors run:** every default-sweep detector (`references/severity-routing.md` §0), no `--pattern` restriction.

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

**Detectors run:** every default-sweep detector (`references/severity-routing.md` §0), no `--pattern` restriction.

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

**Detectors run:** every default-sweep detector (`references/severity-routing.md` §0), unless `--pattern` also supplied — see §6.

---

## §4 Pattern-restricted mode (`--pattern P1|P2|P5|PTODO|PDSSENTINEL|PDEAD|PUNTESTED|PLAYER|PDIAG|PDOCCOVER|PDCHECK|PPRDSTATUS`)

**When to use:** User wants to run only one detector, e.g. `/audit --pattern P5`, `/audit --pattern PTODO`, `/audit --pattern PDEAD`, or `/audit --pattern PDOCCOVER`.

**Argv produced** (after `$SNAPSHOT` is materialized per `cli-invocation.md` §2):

```bash
reify-audit \
  --since <14d-ago-iso> \
  --pattern <P1|P2|P5|PTODO|PDSSENTINEL|PDEAD|PUNTESTED|PLAYER|PDIAG|PDOCCOVER|PDCHECK|PPRDSTATUS> \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT"
```

(If `--since` or `--task` is also given, use that instead of the default 14d window.)

**Scope object in per-run JSON:**

```json
{ "patterns": ["P1"] }        // or ["P2"] or ["P5"]
{ "patterns": ["PTODO"] }     // TODO-tracking invariant (default-sweep, deterministic)
{ "patterns": ["PDSSENTINEL"] } // ds-sentinel reintroduction guard (default-sweep, deterministic)
{ "patterns": ["PDEAD"] }     // advisory: dead code
{ "patterns": ["PUNTESTED"] } // advisory: untested symbols
{ "patterns": ["PLAYER"] }    // advisory: layer/import-boundary violations
{ "patterns": ["PDIAG"] }     // structural opt-in: codes-mandatory diagnostic ratchet
{ "patterns": ["PDOCCOVER"] } // structural opt-in: registry <-> MCP chunk name drift
{ "patterns": ["PDCHECK"] }   // structural opt-in: dead delivered_checks paths (needs tasks.db)
{ "patterns": ["PPRDSTATUS"] } // opt-in: PRD status-prose drift (reads the loaded task corpus)
```

**Detectors run:** The named detector only.

### PTODO — notes

PTODO (`--pattern PTODO`) is **part of the no-`--pattern` default all-detector sweep** (`references/severity-routing.md` §0) — this section documents its explicit invocation. It is distinct from the opt-in advisory P-* patterns below.

- **Severity:** split by kind since η (#4559), so route each finding by its own `severity` field. The High kinds (`untracked`, `bare-ignore`, `orphaned`, `g-allow-orphaned`) escalate per `references/severity-routing.md` §1; every Medium kind files a deferred follow-up task (§2 PTODO notes).
- **Implementation:** Deterministic grep + read-only sqlite; **no jcodemunch/LLM/MCP**, so unaffected by jcodemunch outages. How its `tasks.db`-backed lanes degrade without the DB: `references/cli-invocation.md` §4.1.
- **Exit code:** the High count, so a PTODO-only run exits non-zero whenever a High kind is present. On main that is the steady state, not a regression (SKILL.md "Default-sweep membership"). The merge gate is the fingerprint ratchet in `tests/infra/test_reify_audit_ptodo.sh`, not this exit code.

### PDSSENTINEL — notes

PDSSENTINEL (`--pattern PDSSENTINEL`) is **part of the no-`--pattern` default all-detector sweep** (`references/severity-routing.md` §0). This section documents its explicit invocation.

- **Severity:** Medium only, so it is exit-neutral: a PDSSENTINEL-only run exits 0 whatever it finds.
- **Scope:** `crates/reify-compiler/src/{entity,functions,traits,expr}.rs` and `crates/reify-compiler/src/conformance/*.rs`, hardcoded in `crates/reify-audit/src/pdssentinel.rs`. A hit is a `dimensionless_scalar()` call within a bounded window after an `UnresolvedType` diagnostic push, with no `// ds-sentinel:allow <reason>` marker in that window.
- **Finding shape:** `task_id` is the offending file's repo path, and the summary reads `ds-sentinel: line <n>: <text>`.
- **Implementation:** tracked-file enumeration plus working-tree reads; no jcodemunch, no task DB.
- **Measured:** 0 findings on main `0bbb9075d3`, 2026-09-23.

### Advisory P-* patterns (PDEAD / PUNTESTED / PLAYER) — notes

These three patterns are **opt-in only** — they are NOT part of the default all-detector sweep (`references/severity-routing.md` §0). They fire only when named explicitly via `--pattern`.

- **Severity:** All three emit Severity Low — log-only, advisory, **never auto-filed** as a follow-up task. See `references/severity-routing.md` for routing details.
- **Serve dependency:** PDEAD, PUNTESTED, and PLAYER all require a serve for the duration of the run. When no serve answers, they degrade to **zero findings** (same fail-soft path as P1; P2/P5/PTODO are unaffected — NOT exit 125). One asymmetry to know: because all three are jcodemunch-backed, invoking them alone (`--pattern PDEAD`, `--pattern PDEAD,PUNTESTED`, …) is an all-jcodemunch run set, so a serve that IS reachable but whose index is stale/empty/unreadable hard-exits 125 instead of fail-softing. See `references/cli-invocation.md` §4.1 for both arms, the refusal codes and their remedies.
- **Activation:** Bring a serve up by wrapping the invocation in `scripts/with-jcodemunch-serve.sh`; there is no persistent unit to start. `docs/architecture-audit/jcodemunch-serve-activation.md` remains the identifier and runbook record.

### Structural opt-in patterns (PDIAG / PDOCCOVER / PDCHECK) — notes

These three are **opt-in only**, because each can emit High findings and the exit code is the High count. A default-sweep member that can emit High would turn every bare `/audit` run non-zero. Each fires only when named via `--pattern`. As with PTODO, a finding's kind is its summary prefix (`pdiag-ratchet: …`, `undocumented-name: …`, `delivered-check-unsatisfiable-path: …`).

- **No jcodemunch:** none of the three is jcodemunch-backed, so adding one to a jcodemunch-backed pattern set makes it *mixed*, and a jcodemunch-side problem then fail-softs instead of exiting 125 (`references/cli-invocation.md` §4.1).
- **PDIAG** — the INV-SF-6 codes-mandatory ratchet over code-less `Diagnostic::error`/`Diagnostic::warning` sites, counted per file against `crates/reify-audit/pdiag-baseline.txt`. The High kinds are `pdiag-ratchet` (a file above its baseline count, or new to the baseline), `pdiag-baseline-unreadable` and `pdiag-census-empty`. `pdiag-baseline-stale` is Medium: a count fell below its row, or a row outlived its file's last site. Regenerate with `cargo run -p reify-audit --bin pdiag-baseline-gen`; site remedies are in `docs/notes/diagnostic-severity-policy.md`. The merge gate `tests/infra/test_reify_audit_pdiag.sh` enforces the ratchet independently of this skill. Measured on main `0bbb9075d3`, 2026-09-23: 2 findings, both Medium `pdiag-baseline-stale`, exit 0.
- **PDOCCOVER** — name drift between the builtin `*_NAMES` registries in `crates/reify-compiler/src/units.rs` and the MCP language chunks `crates/reify-mcp/src/tools/chunks/*.md`. Its categories are `undocumented-name`, `fabricated-name`, `stale-baseline-entry`, `stale-allow-entry` and `allow-missing-reason`, all High. Measured on main `0bbb9075d3`, 2026-09-23: 41 High, exit 41 — 38 `undocumented-name` keyed to `units.rs` and 3 `fabricated-name` keyed to `chunks/geometry.md`. That backlog, the `pdoccover-baseline.txt` seed and the gate are owned by #6931.
- **PDCHECK** — `metadata.delivered_checks` grep rows on non-terminal tasks whose pathspec names no tracked path. `delivered-check-unsatisfiable-path` (`expect: present`) is High, because every dependent blocks at `DEP_CAPABILITY_NOT_DELIVERED`. `delivered-check-vacuous-absent-path` (`expect: absent`) is Medium: the check passes while asserting nothing. It reads `<project-root>/.taskmaster/tasks/tasks.db`, or `REIFY_PTODO_TASKS_DB` when set. Without that DB the lane is skipped, so an empty result then means "not checked" (breadcrumb: `references/cli-invocation.md` §4.1). Last full sweep: 0 findings over 755 rows, 2026-09-19 (#7697). #7712 is weighing a standing gate.

### PPRDSTATUS — notes

PPRDSTATUS (`--pattern PPRDSTATUS`) detects PRD status-prose drift: a PRD's own prose asserting a status that the task graph has since contradicted. It is **opt-in only**: its High findings track a standing backlog of PRD prose, and the exit code is the High count. As with PTODO, a finding's kind is its summary prefix, and both kinds are High:

- `stale-status-header:` — every decomposition leaf whose `metadata.prd` names the PRD is terminal (`done` / `cancelled`), yet the PRD's Status header is live or absent. The summary names the stamp to apply: SHIPPED, or WITHDRAWN when every leaf was cancelled.
- `cite-status-contradiction:` — in a PRD whose header is not terminal, a canonical `#NNNN` cite is immediately followed by a status parenthetical (`` #4876 (`deferred`, high) ``) whose class (live, done or cancelled) contradicts the cited task. Dated parentheticals (an ISO date, `as of`, `at freeze`), unknown ids and live-vs-live differences are silent.

- **Scope:** tracked `docs/prds/**.md`, minus `*.capability-manifest.md`. `task_id` is the PRD's repo path, so the lane ignores `--task` and `--since`.
- **Task source:** the loaded task corpus (`--tasks-file` snapshot, or the fused-memory live loader), never `tasks.db`. An unreachable fused-memory therefore exits 125 like every sweep. An empty corpus prints `reify-audit: PPRDSTATUS skipped — the task corpus is empty; this is NOT a clean bill of health`, and its zero findings mean "not checked".
- **Vocabulary authority:** `.claude/skills/prd/project.md` → "PRD terminal status — closed vocabulary + decompose-close stamp". The detector consumes that list and does not define it. Under its case-insensitive first-token rule, six PRDs were already terminal when the detector landed (2026-09-30), and it is silent on all of them: `v0_6/data-carrying-enums.md`, `v0_6/generic-data-carrying-enums.md`, `v0_6/result-and-fallback.md`, `kernel-seam-contracts.md`, `v0_6/process-dfm-geometry-metrology.md`, and the Title-Case `auto-type-param-resolution.md`.
- **Routing:** one batched escalation per run through `scripts/pprdstatus-escalate.py`, never per finding and never follow-up tasks (`references/severity-routing.md` §2).
- **No jcodemunch:** like the structural lanes above, adding it to a jcodemunch-backed pattern set makes that set *mixed*.

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
| `--since <date> --pattern PTODO` | PTODO only, over the whole tree — `<date>` does not narrow it (Medium; deterministic, no jcodemunch) |
| `--since <date> --pattern PDEAD` | Window sweep from `<date>`, PDEAD advisory only (Low/log) |
| `--task <id> --pattern PDCHECK` | PDCHECK only, checking just task `<id>`'s `delivered_checks` rows (needs `tasks.db`); `--since` does not narrow PDCHECK |
| `--task` or `--since` with PTODO, PDSSENTINEL, PDIAG or PDOCCOVER | No narrowing: these structural lanes ignore both flags and always sweep the whole tracked tree |
| `--task` or `--since` with PPRDSTATUS | No narrowing: findings are keyed by PRD path, so every tracked PRD is checked against the whole loaded task corpus |
| `--task <id> --since <date>` | Both flags accepted; `AuditContext` receives both `target_task_id` and `window` (CLI source: `reify-audit.rs` lines 333–342). Whether detectors treat this as a strict scope intersection depends on the detector implementation — verify against the detector source or CLI `--help` if exact semantics matter. |
| `--format markdown` | Adds markdown output to **any** of the above |

**`--pre-done` is NOT composable from the skill.** It is reserved exclusively for the dark-factory D-1 pre-done hook (`REIFY_AUDIT_PREDONE_CMD`). The skill never passes `--pre-done` to the CLI. If a user asks to simulate a pre-done check, use `--task <id> --pattern P5` instead, which runs P5 in periodic-sweep (not blocking) mode.
