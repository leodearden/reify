# CLI Invocation Contract

How the `/audit` skill shells out to `reify-audit`. This is the first skill in the Reify repo that invokes a Rust CLI as a subprocess — future skills can crib from this pattern.

---

## §1 Binary resolution

Prefer the pre-built release binary if present; fall back to `cargo run`:

```bash
# Resolve repo root — anchor all paths against it.
# Makes /audit safe to invoke from any subdirectory of the worktree.
REPO_ROOT=$(git rev-parse --show-toplevel)

# Preferred — release binary already built
if [ -x "$REPO_ROOT/target/release/reify-audit" ]; then
    REIFY_AUDIT_BIN="$REPO_ROOT/target/release/reify-audit"
else
    # Fallback — build and run (quiet suppresses cargo progress chatter on stdout)
    REIFY_AUDIT_BIN="cargo run --release --quiet -p reify-audit --"
fi
```

**Pre-flight:** Always resolve `REPO_ROOT` first — every subsequent path (`--tasks-file`, `--runs-db`, `--project-root`) is anchored against it. This makes `/audit` safe to invoke from any subdirectory of the worktree. Before invoking `reify-audit`, materialize a TaskMetadata snapshot from fused-memory (see §2); `--tasks-file` is a required flag with no default.

**Why release?** The binary will be invoked the same way by the dark-factory D-1 pre-done hook (`REIFY_AUDIT_PREDONE_CMD`). Using the release binary in the skill keeps the invocation in parity with how D-1 sees it.

**Debug builds** (`target/debug/reify-audit`) work for ad-hoc iteration but are not the documented canonical path. Do not document or encourage debug-binary invocation in user-facing output.

---

## §2 Bash invocation template

The `reify-audit` binary emits **JSON on stderr** and a human-readable summary on **stdout**. Capture stderr to a tempfile so both streams stay cleanly separated:

```bash
# Step 1: Materialize a TaskMetadata JSON snapshot from fused-memory.
# The filter lives in scripts/reify-audit-snapshot-filter.jq (single
# point of truth, shared with the systemd pre-done hook wrapper).
# It derives done_at from updatedAt for done tasks (required for P1).
SNAPSHOT=$(mktemp /tmp/reify-audit-snapshot-XXXXXX.json)
trap 'rm -f "$SNAPSHOT" "$TMPFILE"' EXIT

mcp__fused-memory__get_tasks project_root="$REPO_ROOT" \
  | jq -f "$REPO_ROOT/scripts/reify-audit-snapshot-filter.jq" > "$SNAPSHOT"

# Step 2: Invoke reify-audit with the snapshot as --tasks-file.
TMPFILE=$(mktemp /tmp/reify-audit-XXXXXX.json)

$REIFY_AUDIT_BIN \
  [--task <id>] \
  [--since <iso-date>] \
  [--pattern P1|P2|P5] \
  --tasks-file "$SNAPSHOT" \
  --runs-db    "$REPO_ROOT/data/orchestrator/runs.db" \
  --project-root "$REPO_ROOT" \
  2>"$TMPFILE"

EXIT_CODE=$?

# Parse the JSON findings from the tempfile.
FINDINGS=$(cat "$TMPFILE")

# Clean up (EXIT trap above also covers abnormal exits).
rm -f "$SNAPSHOT" "$TMPFILE"
```

**Why a tempfile?** The CLI writes the human-readable summary to stdout and the JSON array to stderr in a single run. A tempfile sink for stderr survives multi-line pretty-printed JSON without the LLM needing to parse a mixed stream inline. `mktemp` generates a collision-free name (`XXXXXX` entropy), safer than a `$$`-based name (which reuses the parent shell PID in long-lived shells and risks stale-file cross-contamination across runs). The `trap 'rm -f "$TMPFILE"' EXIT` guard ensures cleanup even on early exit (parse failure, exception, `Ctrl-C`).

**Source:** The JSON-on-stderr convention is documented in `crates/reify-audit/src/bin/reify-audit.rs` lines 29–41 and in the `--help` output:

> `stderr: JSON array of Finding objects`  
> `stdout: human-readable summary`

If you need JSON on stdout (for piping to `jq` in a one-off shell session), redirect: `reify-audit … 2>&1 >/dev/null | jq '.[].severity'`.

---

## §3 Exit-code interpretation

| Exit code | Meaning | Skill action |
|-----------|---------|--------------|
| `0` | No High-severity findings | Parse findings (may still contain Medium/Low); proceed with severity routing |
| `1`–`124`, `126`–`254` | Count of High-severity findings (capped at 254) | Parse findings; route each by severity (§6 in severity-routing.md) |
| `125`* | Ambiguous: infra/setup error **or** exactly 125 High findings | Disambiguate via tempfile parse — see §3.1 |

\* Exit code `125` collides with a literal count of 125 High findings because `high_severity_exit_code` returns `count.min(254)`. The disambiguator in §3.1 resolves the ambiguity without requiring any change to the CLI.

### §3.1 Disambiguating exit code 125

When `exit_code == 125`, the skill **MUST** attempt to parse the tempfile as a JSON array before treating it as an infra error:

- **Parse succeeds** (tempfile contains a valid JSON array): additionally verify that `len(findings) == 125` and every element has `severity == "High"`. If both checks pass, treat as **125 High findings** — route each via `references/severity-routing.md`. This is NOT an infra error. If either check fails (unexpected array length or non-High severity present), the tempfile may contain a JSON array from an unexpected code path — fall through to the infra-error branch.
- **Parse fails or tempfile is empty**: treat as an **infra error** — surface the tempfile contents verbatim to the user and stop.

**Why this works:** The CLI's successful runs always emit a JSON array on stderr (via `serde_json::to_writer_pretty`). Error paths emit human-readable text via `eprintln!` and never produce parseable JSON. A non-empty JSON array on stderr always wins over the `exit_code == 125` infra-error reading.

**Why this is arm (b):** The CLI in task 3672 does NOT remap a literal-125-count to 124 or 126 (that would be arm (a), requiring changes to `crates/reify-audit/src/bin/reify-audit.rs::high_severity_exit_code` — T-4 territory, outside this task's scope). Arm (b) keeps all disambiguation inside the skill: zero coupling to the CLI's exit-code remapping. Future re-evaluation could move to arm (a) if the boundary case appears in practice.

---

## §4 Failure modes

Each failure mode yields exit code 125. The skill should surface the human-readable message from stdout/stderr to the user and stop the run (do not write a per-run JSON artifact for infra errors):

| Failure | Error message pattern | Recovery hint to user |
|---------|-----------------------|-----------------------|
| Missing tasks-file | `error reading tasks-file '/tmp/reify-audit-snapshot-XXXXXX.json': …` | Confirm fused-memory MCP is responsive; the snapshot tempfile should have been written by the snapshot step above. Check `systemctl --user status fused-memory`. |
| Malformed tasks-file | `error parsing tasks-file '…': …` | Tasks JSON is not a valid array of `TaskMetadata`; check fused-memory sync |
| Unreadable runs.db | `error opening runs-db 'data/orchestrator/runs.db': …` | DB may not exist yet; confirm orchestrator has run at least once |
| Broken stderr serialization | `error serializing findings to JSON (broken stderr?)` | Rare; may indicate a resource limit; retry or report as infra issue |
| Unknown flag or missing value | `error: unknown flag '…'` or `error: --<flag> requires a value` | Bug in skill argv construction — check `references/modes.md` |
| Literal 125 High findings (boundary) | tempfile contains a JSON array of 125 Finding objects | NOT an infra error — route as findings per §3.1 disambiguator |

---

## §5 Worked examples

### Clean run (0 findings)

```
$ reify-audit --since 2026-05-02 2>/tmp/out.json; echo "exit=$?"
reify-audit: 0 findings.
exit=0

$ cat /tmp/out.json
[]
```

Skill behaviour: parse empty array, write per-run JSON with `findings: []`, report "0 findings" to user.

### High-severity run (2 High findings)

```
$ reify-audit --task 3242 2>/tmp/out.json; echo "exit=$?"
reify-audit: 2 finding(s):
  [High] P5PhantomDone task=3242: task marked done but metadata files missing
  [High] P5PhantomDone task=3242: done_provenance field absent
exit=2

$ cat /tmp/out.json
[
  {
    "pattern": "P5PhantomDone",
    "severity": "High",
    "task_id": "3242",
    "summary": "task marked done but metadata files missing",
    "evidence": [{"RunsDb": {"table": "task_runs", "key": "task_id=3242"}}]
  },
  {
    "pattern": "P5PhantomDone",
    "severity": "High",
    "task_id": "3242",
    "summary": "done_provenance field absent",
    "evidence": [{"RunsDb": {"table": "task_runs", "key": "task_id=3242"}}]
  }
]
```

Skill behaviour: exit code 2 (2 High findings); parse 2 findings; escalate both via `mcp__escalation__escalate_info`; write per-run JSON with `action_taken: "escalated"` for each.

### 125 legitimate High findings (boundary collision)

```
$ reify-audit --since 2026-05-02 2>/tmp/out.json; echo "exit=$?"
reify-audit: 125 finding(s):
  [High] P1OrphanExport task=3100: exported symbol has no downstream consumer
  … (124 more High findings)
exit=125

$ jq 'length' /tmp/out.json
125
```

Skill behaviour: `exit_code == 125` triggers the disambiguator (§3.1): skill parses `/tmp/out.json`, sees a JSON array of 125 Finding objects — parse succeeds. Classifies as **125 High findings** (not infra error), routes each via `references/severity-routing.md`.

### Infrastructure error (exit 125)

```
$ reify-audit --since 2026-05-02 2>/tmp/out.json; echo "exit=$?"
reify-audit: error: --tasks-file is required (path to JSON array of TaskMetadata)
exit=125
```

Skill behaviour: `exit_code == 125` triggers the disambiguator (§3.1): skill attempts to parse `/tmp/out.json` as a JSON array. The tempfile does not parse as a JSON array (it is human-readable text), so the disambiguator confirms infra error. Surface the error message to the user; stop — do NOT write a per-run JSON artifact.
