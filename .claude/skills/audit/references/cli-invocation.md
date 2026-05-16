# CLI Invocation Contract

How the `/audit` skill shells out to `reify-audit`. This is the first skill in the Reify repo that invokes a Rust CLI as a subprocess — future skills can crib from this pattern.

---

## §1 Binary resolution

Prefer the pre-built release binary if present; fall back to `cargo run`:

```bash
# Preferred — release binary already built
if [ -x target/release/reify-audit ]; then
    REIFY_AUDIT_BIN="target/release/reify-audit"
else
    # Fallback — build and run (quiet suppresses cargo progress chatter on stdout)
    REIFY_AUDIT_BIN="cargo run --release --quiet -p reify-audit --"
fi
```

**Why release?** The binary will be invoked the same way by the dark-factory D-1 pre-done hook (`REIFY_AUDIT_PREDONE_CMD`). Using the release binary in the skill keeps the invocation in parity with how D-1 sees it.

**Debug builds** (`target/debug/reify-audit`) work for ad-hoc iteration but are not the documented canonical path. Do not document or encourage debug-binary invocation in user-facing output.

---

## §2 Bash invocation template

The `reify-audit` binary emits **JSON on stderr** and a human-readable summary on **stdout**. Capture stderr to a tempfile so both streams stay cleanly separated:

```bash
TMPFILE="/tmp/reify-audit-$$.json"

$REIFY_AUDIT_BIN \
  [--task <id>] \
  [--since <iso-date>] \
  [--pattern P1|P2|P5] \
  --tasks-file .taskmaster/tasks/tasks.json \
  --runs-db data/orchestrator/runs.db \
  --project-root . \
  2>"$TMPFILE"

EXIT_CODE=$?

# Parse the JSON findings from the tempfile.
FINDINGS=$(cat "$TMPFILE")

# Clean up.
rm -f "$TMPFILE"
```

**Why a tempfile?** The CLI writes the human-readable summary to stdout and the JSON array to stderr in a single run. A tempfile sink for stderr survives multi-line pretty-printed JSON without the LLM needing to parse a mixed stream inline. Using `$$` (the shell's PID) for the suffix ensures uniqueness across concurrent skill invocations.

**Source:** The JSON-on-stderr convention is documented in `crates/reify-audit/src/bin/reify-audit.rs` lines 29–41 and in the `--help` output:

> `stderr: JSON array of Finding objects`  
> `stdout: human-readable summary`

If you need JSON on stdout (for piping to `jq` in a one-off shell session), redirect: `reify-audit … 2>&1 >/dev/null | jq '.[].severity'`.

---

## §3 Exit-code interpretation

| Exit code | Meaning | Skill action |
|-----------|---------|--------------|
| `0` | No High-severity findings | Parse findings (may still contain Medium/Low); proceed with severity routing |
| `1`–`254` | Count of High-severity findings (capped at 254) | Parse findings; route each by severity (§6 in severity-routing.md) |
| `125` | Infrastructure/setup error | Surface error to user verbatim and stop — do NOT treat as "125 findings" |

**Critical:** Exit code `125` is reserved for infrastructure errors (arg parse failure, IO error, broken stderr serialization). It never collides with a finding count because High findings are capped at 254. Branch on `exit_code == 125` before treating the exit code as a finding count.

---

## §4 Failure modes

Each failure mode yields exit code 125. The skill should surface the human-readable message from stdout/stderr to the user and stop the run (do not write a per-run JSON artifact for infra errors):

| Failure | Error message pattern | Recovery hint to user |
|---------|-----------------------|-----------------------|
| Missing tasks-file | `error reading tasks-file '.taskmaster/tasks/tasks.json': …` | Confirm the tasks file exists; run `ls .taskmaster/tasks/tasks.json` |
| Malformed tasks-file | `error parsing tasks-file '…': …` | Tasks JSON is not a valid array of `TaskMetadata`; check fused-memory sync |
| Unreadable runs.db | `error opening runs-db 'data/orchestrator/runs.db': …` | DB may not exist yet; confirm orchestrator has run at least once |
| Broken stderr serialization | `error serializing findings to JSON (broken stderr?)` | Rare; may indicate a resource limit; retry or report as infra issue |
| Unknown flag or missing value | `error: unknown flag '…'` or `error: --<flag> requires a value` | Bug in skill argv construction — check `references/modes.md` |

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
    "evidence": ["task status=done; files=[]; done_provenance=null"]
  },
  {
    "pattern": "P5PhantomDone",
    "severity": "High",
    "task_id": "3242",
    "summary": "done_provenance field absent",
    "evidence": ["done_provenance=null"]
  }
]
```

Skill behaviour: exit code 2 (2 High findings); parse 2 findings; escalate both via `mcp__escalation__escalate_info`; write per-run JSON with `action_taken: "escalated"` for each.

### Infrastructure error (exit 125)

```
$ reify-audit --since 2026-05-02 2>/tmp/out.json; echo "exit=$?"
reify-audit: error reading tasks-file '.taskmaster/tasks/tasks.json': No such file or directory
exit=125
```

Skill behaviour: detect `exit == 125`; surface the error message to the user; stop — do NOT attempt to parse `/tmp/out.json` as findings.
