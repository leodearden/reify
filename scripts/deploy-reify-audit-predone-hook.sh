#!/usr/bin/env bash
# scripts/deploy-reify-audit-predone-hook.sh
#
# Deploy the P5 pre-done hook: rebuild reify-audit from main and repoint
# fused-memory.service's FUSED_MEMORY_PREDONE_HOOK_REIFY at the staleness-guard
# wrapper (scripts/reify-audit-predone-wrapper.sh) instead of the raw binary.
#
# WHAT THIS FIXES
# ---------------
# systemd's FUSED_MEMORY_PREDONE_HOOK_REIFY invokes ~/.cargo/bin/reify-audit
# DIRECTLY, bypassing the wrapper's REFUSE-mode staleness guard
# (scripts/reify-audit-freshness.sh :: reify_audit_guard ... refuse ...).  The
# installed binary is therefore free to drift arbitrarily far behind
# crates/reify-audit/ with nothing to say so — measured 2026-08-27 at ~11 weeks
# behind the crate tip.  This script rebuilds the binary and connects the guard
# that would have refused a stale one.
#
# HOW IT IS INVOKED
# -----------------
# This script is the `before_done.script` of reify deterministic task #6939
# (task_kind=deterministic, always_escalates=false, target_unit=
# fused-memory.service, kind=deploy).  The orchestrator's DeterministicRunner
# runs it to completion when #6939 becomes dispatchable — i.e. at the moment its
# dependency #6345 (the P5 vacuity fix) lands on main.
#
#   * exit 0    -> the runner re-inspects fused-memory.service, requires a
#                  FRESH MainPID + strictly-later ActiveEnterTimestampMonotonic,
#                  and drives #6939 to done with
#                  done_provenance.kind='deterministic-deploy'.
#   * exit != 0 -> the runner files a born-at-L2 `infra_issue` escalation and
#                  blocks the task.  It NEVER re-runs the script (I1) and never
#                  phantom-dones it.
#
# So: ANY non-zero exit from this script is the escalation trigger.  Every check
# below is deliberately fatal — this script must fail loudly rather than deploy
# half-way or deploy the wrong thing.  It is also idempotent: a second run over
# an already-deployed system re-installs the binary, finds the systemd line
# already pointing at the wrapper, and skips the edit.
#
# THE DISRUPTIVE STEP
# -------------------
# Step 5 runs `systemctl --user restart fused-memory.service`.  fused-memory is
# the MCP server every task, watcher and agent in the fleet talks to — the
# restart briefly severs all of them, INCLUDING the orchestrator's own
# connection.  That is expected and is why the runner's writeback is patiently
# retried.  Do not run this script by hand at a busy moment; use --dry-run to
# inspect what it would do.
#
# ORDER OF OPERATIONS (each step aborts non-zero on any problem)
# --------------------------------------------------------------
#   1. Precondition: verify #6345's fix is actually present in the working tree
#      (the vacuous `done_provenance.as_ref()?` bail is GONE from
#      crates/reify-audit/src/p5_phantom_done.rs).  Deploying before that lands
#      would refresh the binary to still-vacuous P5 logic and burn the action.
#      The dependency gate is not trusted — it is verified.
#   2. Rebuild:  cargo install --path crates/reify-audit --root ~/.cargo --force
#   3. Verify the rebuild took: the installed binary's mtime must now satisfy
#      the same freshness predicate reify_audit_guard uses.
#   4. Repoint the systemd Environment line, idempotently, after backing the
#      unit file up to a timestamped copy.
#   5. daemon-reload + restart, then wait for `is-active` and confirm the unit's
#      effective Environment now names the wrapper.
#   6. Sanity-check the guard is live end-to-end by invoking the wrapper for a
#      real task id: it must NOT exit 125 with the stale/reinstall hint, and —
#      since task #7139 made the wrapper FAIL OPEN on a stale-but-runnable
#      binary — a rc-0 run whose stderr carries the E_AUDIT_BIN_STALE advisory
#      is EQUALLY fatal here.  Step 3 has already hard-asserted the installed
#      binary is fresh, so either signal means the deploy did not achieve its
#      purpose.  Fail-open is the right policy for the unattended done-flip hot
#      path; it is the wrong answer for an attended deploy that just claimed to
#      have installed a fresh binary.
#
# USAGE
# -----
#   deploy-reify-audit-predone-hook.sh [<task-id>] [--dry-run] [--help]
#
#   <task-id>   Optional. A live task id used for step 6's end-to-end probe.
#               When omitted, step 6 is SKIPPED gracefully (never invented).
#   --dry-run   Run every non-mutating check, print the exact commands and the
#               exact replacement unit line that a real run would apply, and
#               mutate nothing: no cargo install, no unit-file edit, no
#               daemon-reload, no restart.  Steps whose meaning depends on a
#               mutation having happened (3, and 5's freshness verify) degrade
#               to informational.  Step 1 and the step-4 unit-file validation
#               stay FATAL — they are exactly the checks worth rehearsing.
#
# EXIT CODES
# ----------
#   0   deploy completed (or --dry-run validated cleanly)
#   1   any failure — this is the infra_issue escalation trigger
#
# ENVIRONMENT OVERRIDES (all default to the production values; they exist so the
# logic can be exercised against throwaway copies without touching the fleet)
#   REIFY_DEPLOY_MAIN_CHECKOUT      Expected repo root (default /home/leo/src/reify)
#   REIFY_PREDONE_UNIT_FILE         systemd unit path
#                                   (default $HOME/.config/systemd/user/fused-memory.service)
#   REIFY_PREDONE_SERVICE           unit name (default fused-memory.service)
#   REIFY_AUDIT_BIN                 installed binary (default $HOME/.cargo/bin/reify-audit)
#   REIFY_AUDIT_INSTALL_TARGET_DIR  cargo --target-dir for the install
#                                   (default $REPO_ROOT/target — see step 2)
#   REIFY_DEPLOY_ACTIVE_WAIT_SECS   seconds to wait for is-active (default 90)
#   REIFY_DEPLOY_PROBE_RETRIES      step-6 retries while the MCP is still
#                                   coming back up (default 12, 5s apart)
#
# RELATED
# -------
#   scripts/reify-audit-predone-wrapper.sh   the wrapper being wired in
#   scripts/reify-audit-freshness.sh         the REFUSE-mode guard it sources
#   task #6939 (this deploy), #6345 (the P5 vacuity fix it waits for)
#   provenance: reify esc-6345-6, ruled by Leo 2026-08-28

set -euo pipefail

# ── PATH / session hardening ─────────────────────────────────────────────────
# The orchestrator spawns this as a plain subprocess and its environment is not
# guaranteed to carry ~/.cargo/bin or a user-bus address.  Both are required:
# cargo for step 2, XDG_RUNTIME_DIR for every `systemctl --user` call.
export PATH="$HOME/.cargo/bin:$PATH"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"

# ── Self-locate ──────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# portable_mtime() — the house helper; see scripts/lib_portable.sh.
# shellcheck source=scripts/lib_portable.sh
source "$REPO_ROOT/scripts/lib_portable.sh"

MAIN_CHECKOUT="${REIFY_DEPLOY_MAIN_CHECKOUT:-/home/leo/src/reify}"
UNIT_FILE="${REIFY_PREDONE_UNIT_FILE:-$HOME/.config/systemd/user/fused-memory.service}"
SERVICE="${REIFY_PREDONE_SERVICE:-fused-memory.service}"
AUDIT_BIN="${REIFY_AUDIT_BIN:-$HOME/.cargo/bin/reify-audit}"
ACTIVE_WAIT_SECS="${REIFY_DEPLOY_ACTIVE_WAIT_SECS:-90}"
PROBE_RETRIES="${REIFY_DEPLOY_PROBE_RETRIES:-12}"

ENV_VAR="FUSED_MEMORY_PREDONE_HOOK_REIFY"

DRY_RUN=0
PROBE_TASK_ID=""

# ── Output helpers ───────────────────────────────────────────────────────────
die()  { echo "deploy-reify-audit-predone-hook.sh: FATAL: $*" >&2; exit 1; }
step() { echo "==> $*"; }
note() { echo "    $*"; }
plan() { echo "    [dry-run] would run: $*"; }

usage() {
    cat <<EOF
Usage: deploy-reify-audit-predone-hook.sh [<task-id>] [--dry-run] [--help]

Rebuilds reify-audit and repoints $SERVICE's $ENV_VAR
Environment line at scripts/reify-audit-predone-wrapper.sh, then restarts the
service.  Invoked as reify task #6939's before_done action; any non-zero exit
becomes an infra_issue escalation.

  <task-id>   live task id for the end-to-end wrapper probe (step 6).
              Omitted => step 6 is skipped, not faked.
  --dry-run   validate and print the plan; mutate nothing.
  --help,-h   this text.

WARNING: a real run restarts $SERVICE — the MCP server the whole
fleet depends on.
EOF
}

# ── Argument parsing ─────────────────────────────────────────────────────────
while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)   DRY_RUN=1 ;;
        --help|-h)   usage; exit 0 ;;
        --)          shift; break ;;
        -*)          die "unknown flag '$1' (try --help)" ;;
        *)
            if [ -n "$PROBE_TASK_ID" ]; then
                die "unexpected extra argument '$1' (only one task id is accepted)"
            fi
            PROBE_TASK_ID="$1"
            ;;
    esac
    shift
done

if [ "$DRY_RUN" = "1" ]; then
    step "DRY RUN — nothing will be installed, edited, reloaded or restarted."
fi

# ── Step 0: refuse to run from anywhere but the main checkout ────────────────
# The path this script writes into the systemd unit must be a PERMANENT one.
# Reify has ~250 linked worktrees; deriving the wrapper path from a warm lane
# would wire the fleet's pre-done hook to a directory that gets reclaimed out
# from under it.  Fail loudly rather than guess.
step "Step 0: checkout identity"
if [ "$REPO_ROOT" != "$MAIN_CHECKOUT" ]; then
    die "this script must run from the main checkout ($MAIN_CHECKOUT), not '$REPO_ROOT'.
     The systemd unit is rewritten to reference an absolute path in this repo; a
     worktree path would be reclaimed and would break the fleet's pre-done hook.
     (Override with REIFY_DEPLOY_MAIN_CHECKOUT only for testing.)"
fi
note "repo root: $REPO_ROOT"

WRAPPER="$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"
[ -f "$WRAPPER" ] || die "wrapper not found: $WRAPPER"
[ -x "$WRAPPER" ] || die "wrapper is not executable: $WRAPPER"
note "wrapper:   $WRAPPER"

# ── Step 1: precondition — did #6345 actually land? ──────────────────────────
# The dependency gate opening is NOT proof the fix landed; verify the artifact.
# The vacuous early bail this greps for is what made P5 phantom-done detection a
# no-op: `let prov = meta.done_provenance.as_ref()?;` returns None (= "no
# finding") for exactly the tasks that lack provenance, i.e. the ones the
# detector exists to catch.
step "Step 1: precondition — #6345's P5 vacuity fix is present"
P5_SRC="$REPO_ROOT/crates/reify-audit/src/p5_phantom_done.rs"
[ -f "$P5_SRC" ] || die "expected source file missing: $P5_SRC
     Either the crate was restructured or this is not a reify checkout.
     Refusing to deploy against an unrecognised tree."

if grep -qF 'done_provenance.as_ref()?' "$P5_SRC"; then
    die "the vacuous P5 bail is STILL PRESENT in $P5_SRC:
$(grep -nF 'done_provenance.as_ref()?' "$P5_SRC" | sed 's/^/       /')
     The dependency gate opened but #6345's fix did NOT land on main.
     Deploying now would refresh the binary to still-vacuous P5 logic and burn
     the action. Aborting — re-run this deploy once #6345 is genuinely on main."
fi
note "vacuous bail absent — #6345's fix is in the tree"

CRATE_EPOCH="$(git -C "$REPO_ROOT" log -1 --format=%ct -- crates/reify-audit/ 2>/dev/null || true)"
case "$CRATE_EPOCH" in
    ''|*[!0-9]*) die "could not resolve the crates/reify-audit tip commit epoch
     (git -C $REPO_ROOT log -1 --format=%ct -- crates/reify-audit/ returned '$CRATE_EPOCH').
     Without it the post-install freshness assertion in step 3 cannot be made." ;;
esac
note "crates/reify-audit tip epoch: $CRATE_EPOCH ($(date -d "@$CRATE_EPOCH" '+%Y-%m-%d %H:%M:%S' 2>/dev/null || echo '?'))"

if [ -e "$AUDIT_BIN" ]; then
    _before_mtime="$(portable_mtime "$AUDIT_BIN" 2>/dev/null || echo '?')"
    note "installed binary mtime (before): $_before_mtime ($(date -d "@$_before_mtime" '+%Y-%m-%d %H:%M:%S' 2>/dev/null || echo '?'))"
else
    note "installed binary absent (before): $AUDIT_BIN"
fi

# ── Step 2: rebuild ──────────────────────────────────────────────────────────
# --target-dir defaults to the repo's own target/ rather than cargo install's
# private temp dir. reify-audit depends on reify-test-support, which pulls the
# whole reify compiler stack; a private temp dir means a full COLD release build
# every time. The repo target/ already carries release artifacts for this exact
# graph (scripts/reify-audit-freshness.sh's rebuild mode builds there), so the
# install is normally near-incremental. Cargo's own lock serialises against any
# concurrent build in that directory.
step "Step 2: rebuild and install reify-audit"
INSTALL_TARGET_DIR="${REIFY_AUDIT_INSTALL_TARGET_DIR:-$REPO_ROOT/target}"
CARGO_BIN="$(command -v cargo || true)"
[ -n "$CARGO_BIN" ] || die "cargo not found on PATH ($PATH) — cannot rebuild reify-audit."
note "cargo: $CARGO_BIN"
note "target-dir: $INSTALL_TARGET_DIR"

install_cmd=(
    "$CARGO_BIN" install
    --path "$REPO_ROOT/crates/reify-audit"
    --root "$HOME/.cargo"
    --target-dir "$INSTALL_TARGET_DIR"
    --force
)

if [ "$DRY_RUN" = "1" ]; then
    plan "(cd $REPO_ROOT && ${install_cmd[*]})"
else
    if ! (cd "$REPO_ROOT" && "${install_cmd[@]}"); then
        die "cargo install failed — reify-audit was NOT rebuilt. Nothing has been
     changed: the systemd unit is untouched and the service was not restarted."
    fi
    note "install completed"
fi

# ── Step 3: verify the rebuild actually took ─────────────────────────────────
# The predicate is deliberately the SAME one reify_audit_guard uses for
# "fresh" (binary mtime >= crate tip epoch), so a pass here is exactly the
# condition step 6's wrapper probe will re-evaluate.
step "Step 3: verify the installed binary is now fresh"
if [ "$DRY_RUN" = "1" ]; then
    note "[dry-run] skipped — nothing was installed, so this assertion is vacuous here."
else
    [ -f "$AUDIT_BIN" ] || die "cargo install reported success but '$AUDIT_BIN' does not exist.
     Check --root/\$HOME resolution: HOME=$HOME"
    [ -x "$AUDIT_BIN" ] || die "'$AUDIT_BIN' exists but is not executable."
    BIN_MTIME="$(portable_mtime "$AUDIT_BIN" 2>/dev/null || true)"
    case "$BIN_MTIME" in
        ''|*[!0-9]*) die "could not read the mtime of '$AUDIT_BIN'." ;;
    esac
    if [ "$BIN_MTIME" -lt "$CRATE_EPOCH" ]; then
        die "the rebuild did NOT take: '$AUDIT_BIN' mtime $BIN_MTIME still predates the
     crates/reify-audit tip commit epoch $CRATE_EPOCH. cargo install exited 0 but
     the binary on disk is unchanged (wrong --root? a shadowing binary earlier on
     PATH? an install into a different HOME?).
     Refusing to repoint systemd at a wrapper that would immediately refuse."
    fi
    note "binary mtime $BIN_MTIME >= crate epoch $CRATE_EPOCH — fresh"
fi

# ── Step 4: repoint the systemd Environment line, idempotently ───────────────
step "Step 4: repoint $ENV_VAR in $UNIT_FILE"
[ -f "$UNIT_FILE" ] || die "systemd unit file not found: $UNIT_FILE"
[ -w "$UNIT_FILE" ] || die "systemd unit file is not writable: $UNIT_FILE"

# Match ONLY assignment lines, never the surrounding prose comments.
HOOK_LINE_RE="^Environment=.*${ENV_VAR}"
hook_line_count="$(grep -c -- "$HOOK_LINE_RE" "$UNIT_FILE" || true)"
if [ "$hook_line_count" != "1" ]; then
    die "expected exactly ONE 'Environment=' line naming $ENV_VAR in
     $UNIT_FILE, found $hook_line_count. Refusing to guess which one to rewrite."
fi
current_line="$(grep -- "$HOOK_LINE_RE" "$UNIT_FILE")"
note "current: $current_line"

# The unit quotes the whole assignment because the value contains spaces.
desired_line="Environment=\"${ENV_VAR}=${WRAPPER} --task {id} --pre-done\""
expected_stale_line="Environment=\"${ENV_VAR}=${AUDIT_BIN} --task {id} --pre-done\""

if [ "$current_line" = "$desired_line" ]; then
    note "already points at the wrapper — nothing to change (idempotent no-op)."
elif [ "$current_line" != "$expected_stale_line" ]; then
    die "the $ENV_VAR line is in an UNRECOGNISED form; refusing to rewrite it.
       found:    $current_line
       expected: $expected_stale_line
       (or the already-deployed form: $desired_line)
     Someone changed this line by hand. Reconcile it manually, then re-run."
else
    note "desired: $desired_line"
    backup="${UNIT_FILE}.bak.$(date -u +%Y%m%dT%H%M%SZ)"
    # The backup sits beside the unit deliberately (most discoverable place for
    # an operator). Its name does not end in a systemd unit suffix, so the unit
    # loader ignores it.
    if [ "$DRY_RUN" = "1" ]; then
        plan "cp -p '$UNIT_FILE' '$backup'   # then rewrite the line above"
    else
        cp -p "$UNIT_FILE" "$backup" || die "failed to back up $UNIT_FILE to $backup — not editing."
        note "backup: $backup"

        tmp="$(mktemp "${UNIT_FILE}.new.XXXXXX")"
        # shellcheck disable=SC2064
        trap "rm -f '$tmp'" EXIT
        awk -v want="$desired_line" -v var="$ENV_VAR" '
            $0 ~ ("^Environment=.*" var) { print want; next }
            { print }
        ' "$UNIT_FILE" > "$tmp" || die "failed to render the rewritten unit file."

        # cp INTO the existing file so mode/ownership/inode are preserved.
        cp "$tmp" "$UNIT_FILE" || die "failed to write $UNIT_FILE (backup at $backup)."
        rm -f "$tmp"
        trap - EXIT

        readback="$(grep -- "$HOOK_LINE_RE" "$UNIT_FILE" || true)"
        [ "$readback" = "$desired_line" ] || die "readback mismatch after rewriting $UNIT_FILE.
       wanted: $desired_line
       got:    $readback
     Restore from the backup: cp '$backup' '$UNIT_FILE'"
        note "rewritten and read back OK"
    fi
fi

# ── Step 5: daemon-reload + restart (THE DISRUPTIVE STEP) ────────────────────
step "Step 5: daemon-reload and restart $SERVICE  [DISRUPTIVE]"
if [ "$DRY_RUN" = "1" ]; then
    plan "systemctl --user daemon-reload"
    plan "systemctl --user restart $SERVICE"
    note "current state: $(systemctl --user is-active "$SERVICE" 2>/dev/null || echo unknown)"
else
    systemctl --user daemon-reload || die "systemctl --user daemon-reload failed."
    systemctl --user restart "$SERVICE" || die "systemctl --user restart $SERVICE failed.
     Inspect: systemctl --user status $SERVICE ; journalctl --user -u $SERVICE -n 100"

    waited=0
    state=""
    while [ "$waited" -lt "$ACTIVE_WAIT_SECS" ]; do
        state="$(systemctl --user is-active "$SERVICE" 2>/dev/null || true)"
        [ "$state" = "active" ] && break
        sleep 2
        waited=$((waited + 2))
    done
    [ "$state" = "active" ] || die "$SERVICE did not reach 'active' within ${ACTIVE_WAIT_SECS}s (last state: '${state:-unknown}').
     The fleet's MCP server is DOWN — this needs a human now.
     Inspect: systemctl --user status $SERVICE ; journalctl --user -u $SERVICE -n 200"
    note "$SERVICE is active (after ${waited}s)"

    # Prove daemon-reload actually picked the new value up, rather than trusting
    # that editing the file was enough.
    effective="$(systemctl --user show "$SERVICE" -p Environment 2>/dev/null || true)"
    case "$effective" in
        *"$WRAPPER"*) note "effective Environment now names the wrapper" ;;
        *) die "the running unit's Environment does NOT name the wrapper after reload+restart.
       effective: $effective
     The file was rewritten but systemd is serving something else (a drop-in
     override under ${UNIT_FILE}.d/ ?). The hook is still bypassing the guard." ;;
    esac
fi

# ── Step 6: end-to-end sanity check of the now-live guard ────────────────────
step "Step 6: probe the wrapper end-to-end"
if [ -z "$PROBE_TASK_ID" ]; then
    note "no task id given — SKIPPED (a task id is never invented; pass one as \$1 to enable)."
else
    probe_ok=0
    attempt=0
    while [ "$attempt" -lt "$PROBE_RETRIES" ]; do
        attempt=$((attempt + 1))
        probe_err="$(mktemp /tmp/reify-predone-probe-XXXXXX)"
        probe_rc=0
        "$WRAPPER" --task "$PROBE_TASK_ID" --pre-done >/dev/null 2>"$probe_err" || probe_rc=$?

        # Task #7139: the wrapper now FAILS OPEN on a stale-but-runnable binary
        # (rc 0 + an E_AUDIT_BIN_STALE advisory on stderr), so an rc-only test
        # would fall through to the success branch below and report a green
        # deploy over a stale fleet.  Check the token regardless of rc, and
        # branch on the TOKEN rather than message prose (the convention in
        # crates/reify-audit/src/jcodemunch_index.rs:522).
        if grep -qF 'E_AUDIT_BIN_STALE' "$probe_err"; then
            msg="$(head -8 "$probe_err")"
            rm -f "$probe_err"
            die "the wrapper judges '$AUDIT_BIN' STALE after the deploy (exit $probe_rc):
       $msg
     Step 3 already asserted the freshly-installed binary is fresh, so this
     contradicts it — the deploy did not achieve its purpose.  Note the wrapper
     FELL OPEN (or refused under REIFY_AUDIT_FRESHNESS_STRICT=1): done-flips are
     not blocked, but every one of them is now running a stale detector."
        fi

        if [ "$probe_rc" = "125" ] && grep -q 'reinstall with: cargo install' "$probe_err"; then
            msg="$(head -5 "$probe_err")"
            rm -f "$probe_err"
            die "the wrapper still REFUSES as stale after the deploy (exit 125):
       $msg
     The freshness guard is wired in but judges '$AUDIT_BIN' stale — the deploy
     did not achieve its purpose."
        fi

        if [ "$probe_rc" = "125" ]; then
            # Infrastructure error that is NOT the staleness refusal — almost
            # always the MCP still finishing its restart. Retry, then fail.
            msg="$(head -3 "$probe_err")"
            rm -f "$probe_err"
            note "probe attempt $attempt/$PROBE_RETRIES: exit 125 (not staleness): $msg"
            sleep 5
            continue
        fi

        rm -f "$probe_err"
        # 0 = no High findings; 1..254 = High-finding COUNT. Both mean the
        # wrapper got past the freshness guard and actually ran the detector,
        # which is what this step asserts.
        note "probe exit $probe_rc — freshness guard passed, detector ran"
        probe_ok=1
        break
    done
    [ "$probe_ok" = "1" ] || die "the wrapper never got past exit 125 in $PROBE_RETRIES attempts.
     The pre-done hook is wired in but is failing for an infrastructure reason
     (fused-memory MCP unreachable? jq/curl missing?). Every done-flip on reify
     will now be blocked until this is fixed — this needs a human now.
     Reproduce: $WRAPPER --task $PROBE_TASK_ID --pre-done"
fi

if [ "$DRY_RUN" = "1" ]; then
    step "DRY RUN complete — validated, nothing changed."
else
    step "Deploy complete: reify-audit rebuilt, $SERVICE now routes $ENV_VAR through the staleness guard."
fi
