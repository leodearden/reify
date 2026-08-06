#!/usr/bin/env bash
# scripts/thin-warm-lane.sh — Free-first target-reclaim primitive for the
# warm-lane CoW pool.
#
# PRD docs/prds/warm-lane-pool-sizing-lifecycle.md §9.3 (Pillar C — lifecycle).
#
# FREE-FIRST: removes <lane_dir>/target outright, freeing the divergent
# extents directly — no reseed staged by default (acquire_lane always
# re-seeds from base at acquire time — D10, cow-seeding). Progresses at true
# ENOSPC (a straight rm never needs the transient 2x space a reflink clone
# briefly does). Optional --reseed re-seeds a thin base clone via
# seed-warm-lane.sh --fresh-checkout AFTER the free (T2: free-before-stage
# ordering — never inherits the ENOSPC-deadlock class task 5168 fixed for
# warm-lane-gc.sh).
#
# Usage:
#   scripts/thin-warm-lane.sh <lane_dir> [--reseed] [--base <base_target_dir>] \
#       [--seed-script <path>]
#
# Options:
#   --reseed              After freeing target/, re-seed a thin base clone:
#                         <seed-script> <base_target_dir> <lane_dir> --fresh-checkout
#                         --assume-lane-lock-held. thin already holds
#                         ${LANE_DIR}.lock on FD 9 (T3) across the seed call, so it
#                         passes --assume-lane-lock-held so seed-warm-lane.sh skips
#                         its own (now default-on) lane-lock acquire rather than
#                         self-refusing against thin's held lock (esc-5214/task 5354).
#                         Best-effort: a failed re-seed is logged but does not
#                         change the exit code (see Exit codes below). A NON-ZERO
#                         seed exit additionally DISCARDS <lane_dir>/target: the
#                         seed's fail-closed post-conditions all fire after
#                         target/ has been replaced with the CoW clone, so an
#                         abort leaves an UNCERTIFIED clone the caller must
#                         remove (PRD docs/prds/warm-lane-pool-cow-seeding.md
#                         §9.5 inv.13, "Caller obligation on the fail-closed
#                         path" — the single normative home for that ruling).
#                         Requires --base. Default OFF (no clone staged).
#   --base DIR             base_target_dir to seed from. Required with --reseed.
#                         Per seed-warm-lane.sh's D8 resolve convention, this MUST
#                         be the CONCRETE .gen.N path, NOT the <base>/target
#                         symlink -- thin-warm-lane.sh forwards it verbatim,
#                         unresolved, to seed-warm-lane.sh.
#   --seed-script PATH     Seed primitive invoked by --reseed (default: sibling
#                         scripts/seed-warm-lane.sh). Hermetic test seam.
#   -h, --help              Print this message and exit (0).
#
# Preconditions (caller-asserted FREE is independently re-verified via the
# lane flock — T3; the other two are checked here before any work):
#   lane_dir must exist and be a directory.
#   lane_dir must be under $REIFY_WARM_LANE_MOUNT, when that env var is set.
#   lane_dir must not resolve to (or be literally named) the base dir, and
#   must not be the mount root itself.
#
# Stdout: resolved <lane_dir> on success. Stderr: all diagnostics.
#
# Exit codes:
#   0   — target/ freed. With --reseed, re-seeding is attempted best-effort:
#         a failed re-seed is logged but does NOT change the exit code, since
#         the free — the operation this script guarantees — already succeeded
#         (D10: acquire always re-seeds from base anyway, so a lane a caller
#         re-acquires is never left cold). With --reseed, a failed re-seed
#         additionally DISCARDS <lane_dir>/target, so the lane is returned
#         cold-but-safe rather than carrying an uncertified clone — which is
#         once again exactly "target/ freed", the post-condition 0 asserts.
#   1   — Precondition guard refusal (nonexistent lane, ==base dir, not under
#         $REIFY_WARM_LANE_MOUNT); or, under --reseed, the post-abort discard of
#         <lane_dir>/target failed — the lane is left carrying an uncertified
#         clone and is deliberately NOT emitted on stdout.
#   2   — Usage error (unknown flag, missing positional, --reseed without --base);
#         or a WIRING error — the sibling scripts/lib_live_refs.sh is missing, so
#         the T4 liveness check could not be loaded (an incomplete deployment,
#         deliberately loud rather than silently fail-open).
#   75  — EX_TEMPFAIL: the lane is BUSY. Either (a) its own flock is held by a
#         live consumer (ASSIGNED lane; inv.2 one-consumer-per-lane-at-a-time),
#         or (b) a live process references lane_dir or anything under it
#         (cwd / open fd / mmap — T4). Both are benign "skip, retry next cycle"
#         verdicts, never faults, and nothing was removed in either case.
#
#         ONE code for TWO reasons, deliberately: dark-factory's
#         GitOps._run_thin_warm_lane (orchestrator/src/orchestrator/git_ops.py)
#         treats rc=75 as a benign skip logged at DEBUG — "never logged at
#         WARNING (§9.5 inv.11: release-thin is not an escalation/fault)" — and
#         routes every OTHER non-zero rc to WARNING. A separate code for (b)
#         would therefore turn every lingering-reference release-thin into a
#         false fault and would require a cross-repo change to silence; reusing
#         75 needs ZERO dark-factory change to ROUTE correctly, which is the
#         strongest form of the "reify ships the primitive, dark-factory wires
#         the invocation" seam.
#
#         The two reasons stay ATTRIBUTABLE through a machine-greppable stderr
#         TOKEN prefixed on each refusal's first line, NOT through free-form
#         English: `LANE_LOCK_CONTENDED:` for (a) and `LANE_LIVE_REF:` for (b).
#         That follows the discriminant convention task 5568 ratified for
#         seed-warm-lane.sh's identical problem — see the normative "Refusal
#         signal" clause under docs/prds/warm-lane-pool-cow-seeding.md §9.5
#         inv.11 — and reuses ITS literal token for (a), so one
#         `grep LANE_LOCK_CONTENDED` separates lock contention across both
#         scripts. Both tokens are UNCONDITIONAL (never flag-gated): a token
#         that stays dark until a fleet is patched is dark for exactly the
#         window most likely to need triage. Pinned in both directions by
#         tests/infra/test_thin_warm_lane.sh (F3/F4, B3, and B3b's ordering arm).
#
#         KNOWN DF-side text drift (routing is correct, WORDING is not):
#         dark-factory's rc=75 arm logs the fixed string "lane %s already
#         re-acquired (rc=75, flock held) — benign skip", which MISNAMES the
#         cause for (b). Harmless to behaviour, wrong for an operator reading
#         the log. The token above is what makes the true cause recoverable
#         from thin's own stderr until that string is widened DF-side; filed
#         as a cross-repo follow-up.
#
#   The exit code tracks whether the lane is left SAFE, not whether the re-seed
#   succeeded: a re-seed that failed but whose clone was discarded leaves exactly
#   the post-condition this script guarantees (target/ freed), so it stays 0.
#
# Invariants (PRD §9.3):
#   T1 — only target/ is ever removed; source tree / .git / uncommitted WIP
#        are never touched, so unlanded WIP always survives on the branch.
#   T2 — free-first: bytes are freed BEFORE any reseed clone is staged.
#   T3 — the lane's own flock (<lane_dir>.lock) is acquired non-blocking;
#        refuses (75) rather than blocking or stealing an ASSIGNED lane.
#        Assumes <lane_dir>.lock already exists (pool acquire/release
#        convention, inv.2): if it does not, opening it below creates it on
#        the fly, which can itself fail under a truly exhausted filesystem
#        (ENOSPC) before any bytes are freed. Only bites a lane thinned
#        outside the pool's acquire/release lifecycle.
#   T4 — a live PROCESS REFERENCE at or under lane_dir (cwd / open fd / mmap;
#        `live_ref_present`, scripts/lib_live_refs.sh) refuses the thin (75),
#        preserving target/. Checked AFTER the T3 flock acquire and IMMEDIATELY
#        BEFORE the rm — never as an up-front snapshot, which would reintroduce
#        the very TOCTOU this gate closes. The mechanism (the /proc scan itself)
#        is recorded once in docs/prds/warm-lane-pool-space-safety.md §8.4 and is
#        deliberately not restated here.
#
#        T3 alone is INSUFFICIENT, and that is the whole reason T4 exists: the
#        inv.2 flock is a reseed MUTEX, not a liveness oracle. It is held only
#        across the acquire reseed (task 5354) and across dark-factory's
#        run_scoped_verification (DF 3027) — NEVER across the implement phase,
#        where an agent runs cargo build/test for tens of minutes. Root cause
#        esc-5334-6 (2026-07-26): _lane-27, _lane-28 (both mid-`cargo` /
#        `cargo-clippy`) and _lane-50 all had live consumers and ZERO held
#        <lane>.lock. Without T4, thin would `rm -rf <lane>/target` under a live
#        build — the failure that reset _lane-5 six times on 2026-07-25 and
#        produced a 218-target "No such file or directory" storm on the sibling
#        gc path (closed there by task 5572).
#
#        SKIP, never DEFER: on a live reference thin refuses and returns; it
#        does not sleep or retry. A retry loop would hold <lane_dir>.lock on
#        FD 9 across an unbounded wait, blocking the very acquire_lane reseed
#        that lock exists to serialise (§9.5 inv.11) — turning a benign skip
#        into a pool-wide stall.
#
#        Over-preserving is SAFE and TEMPORARY: acquire_lane ALWAYS re-seeds
#        target/ from base (D10, cow-seeding), so a preserved divergent target/
#        is never reused as warm cache; it costs one extra cycle of resident
#        disk and is reclaimed by the next release-thin or gc pass once the
#        reference clears. Residual risk, named rather than worked around here:
#        a truly ORPHANED reference (a leaked test binary still holding an fd)
#        shields its lane indefinitely — that is owned by the orphaned-test-
#        binary reaper, docs/notes/orphaned-test-binary-reaper.md.
#
#        UNDISCHARGED CROSS-REPO SYNC, recorded rather than silently carried:
#        dark-factory resolves the warm-lane scripts through a two-candidate
#        preference order (GitOps._resolve_warm_lane_script) — FIRST
#        <project_root>/scripts/<name>, THEN its own copy under
#        orchestrator/scripts/warm-lane/. For reify the project copy (this file)
#        always exists and always wins, so T4 is live in production here. But
#        DF's copy of THIS script carries no T4 gate, so any project resolving
#        through the FALLBACK candidate gets a thin that fails OPEN into exactly
#        the `rm -rf <lane>/target`-under-a-live-build this gate closes. Task
#        5572 discharged the equivalent sync for warm-lane-gc.sh (DF's copy has
#        the gate); the thin half is NOT discharged. Porting it is a
#        dark-factory change and is out of scope for a reify task — filed as a
#        cross-repo follow-up rather than attempted from here.
#
#        MEASURED COST (this host, 2026-08-06: 1306 live processes, 13328 fd+cwd
#        symlinks): one live_ref_present call returning the NEGATIVE verdict
#        ~1.25s (3 samples, 1.24-1.32s). The negative is the RELEASE-path common
#        case — the agent has already exited — and it cannot short-circuit, since
#        "not referenced" is only established by exhausting every reference kind.
#        Unlike gc's per-entry pass (entries x ~1.9s), thin pays exactly ONE call
#        per invocation. It lands on dark-factory's release_lane, which awaits
#        thin inline alongside an `rm -rf` of a multi-GB target/ that already
#        costs seconds. Accepted on that basis. See scripts/lib_live_refs.sh's
#        own cost table for the per-pass split.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Shared live-reference /proc scanner (live_ref_present). Fail LOUDLY if it is
# missing: a silently-absent liveness guard is precisely the failure mode task
# 5823 exists to remove. Exit 2 (usage/WIRING), not 1 (runtime) — an incomplete
# deployment, in the same class as an unknown flag; see the exit-code table.
# Sited BEFORE _usage and argv parsing so even --help trips it (pinned by A12),
# and using `echo >&2` rather than err() because err() is defined below.
if [ ! -f "$SCRIPT_DIR/lib_live_refs.sh" ]; then
    echo "thin-warm-lane.sh: ERROR — scripts/lib_live_refs.sh not found next to thin-warm-lane.sh" >&2
    exit 2
fi
# shellcheck source=scripts/lib_live_refs.sh
source "$SCRIPT_DIR/lib_live_refs.sh"

# ── log helpers (all write to stderr) ────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ─────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") <lane_dir> [--reseed] [--base <base_target_dir>] [--seed-script <path>]

  Free-first target-reclaim primitive for the warm-lane CoW pool (PRD
  docs/prds/warm-lane-pool-sizing-lifecycle.md §9.3). Removes <lane_dir>/target
  outright, freeing bytes BEFORE any reseed clone is staged (T2). No reseed is
  staged by default (acquire_lane always re-seeds from base — D10).

  <lane_dir>              Warm-lane pool lane to thin. Must exist, be under
                          \$REIFY_WARM_LANE_MOUNT (when set), and not be the
                          base dir.

  --reseed                After freeing target/, re-seed a thin base clone
                          (--seed-script <base_target_dir> <lane_dir> --fresh-checkout
                          --assume-lane-lock-held; thin holds \${LANE_DIR}.lock on FD 9
                          across the call, so seed skips its own default acquire).
                          Best-effort: a failed re-seed is logged but does not
                          change the exit code; a NON-ZERO seed exit additionally
                          DISCARDS <lane_dir>/target, which an aborting seed
                          leaves behind UNCERTIFIED (PRD
                          docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.13).
                          Requires --base. Default OFF.
  --base DIR              base_target_dir to seed from. Required with --reseed.
                          Per seed-warm-lane.sh's D8 resolve convention, this MUST
                          be the CONCRETE .gen.N path, NOT the <base>/target
                          symlink -- forwarded verbatim, unresolved.
  --seed-script PATH      Seed primitive to invoke for --reseed (default:
                          sibling scripts/seed-warm-lane.sh). Hermetic test seam.
  -h, --help              Print this message and exit (0).

  Stdout: resolved <lane_dir> on success.
  Stderr: all diagnostics.

  Exit codes:
    0   — target/ freed. With --reseed, re-seeding is best-effort: a failed
          re-seed is logged (stderr) but does not change the exit code, and
          additionally DISCARDS <lane_dir>/target so the lane is returned
          cold-but-safe rather than carrying an uncertified clone.
    1   — Precondition guard refusal (nonexistent lane, ==base dir, not under
          \$REIFY_WARM_LANE_MOUNT); or, under --reseed, the post-abort discard
          of <lane_dir>/target failed — the lane is left carrying an uncertified
          clone and is deliberately NOT emitted on stdout. (The exit code tracks
          whether the lane is left SAFE, not whether the re-seed succeeded.)
    2   — Usage error (unknown flag, missing positional, --reseed without
          --base); or a WIRING error — the sibling scripts/lib_live_refs.sh is
          missing, so the liveness check could not be loaded.
    75  — EX_TEMPFAIL: the lane is BUSY. Either (a) its flock is held by a live
          consumer (ASSIGNED lane; inv.2 one-consumer-per-lane), or (b) a live
          process references the lane dir or anything under it (cwd / open fd /
          mmap). Both are benign "skip, retry next cycle" verdicts, never
          faults; nothing is removed in either case. One code for two reasons
          because dark-factory logs rc=75 at DEBUG as a benign skip and every
          other non-zero rc at WARNING — a distinct code would report each
          lingering-reference release-thin as a false fault. The two reasons
          stay attributable via an unconditional machine-greppable stderr
          token on the first line of each refusal: \`LANE_LOCK_CONTENDED:\`
          for (a) — the same literal seed-warm-lane.sh emits — and
          \`LANE_LIVE_REF:\` for (b).
EOF
}

# ── arg parsing ───────────────────────────────────────────────────────────────
RESEED=""
BASE_TARGET_DIR=""
SEED_SCRIPT=""
_POSITIONALS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage
            exit 0
            ;;
        --reseed)
            RESEED=1
            shift
            ;;
        --base)
            [ $# -ge 2 ] || { err "--base requires a value"; exit 2; }
            BASE_TARGET_DIR="$2"
            shift 2
            ;;
        --seed-script)
            [ $# -ge 2 ] || { err "--seed-script requires a value"; exit 2; }
            SEED_SCRIPT="$2"
            shift 2
            ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2
            ;;
        *)
            _POSITIONALS+=("$1")
            shift
            ;;
    esac
done

if [ "${#_POSITIONALS[@]}" -lt 1 ]; then
    err "Missing required positional argument: <lane_dir>"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ "${#_POSITIONALS[@]}" -gt 1 ]; then
    err "Unexpected extra positional argument(s): ${_POSITIONALS[*]:1}"
    exit 2
fi
# Strip a trailing slash so sibling-path constructions (e.g. "$LANE_DIR.lock")
# never land inside the lane instead of beside it.
LANE_DIR="${_POSITIONALS[0]%/}"

# --reseed requires --base (usage-shape check; fail fast, before any work).
if [ -n "$RESEED" ] && [ -z "$BASE_TARGET_DIR" ]; then
    err "--reseed requires --base <base_target_dir>"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# Default --seed-script: sibling scripts/seed-warm-lane.sh, resolved via the
# SCRIPT_DIR hoisted at the top (itself BASH_SOURCE-derived, so this script works
# when invoked via a relative/symlinked path). One definition, not two.
if [ -z "$SEED_SCRIPT" ]; then
    SEED_SCRIPT="$SCRIPT_DIR/seed-warm-lane.sh"
fi

# ── lane_dir existence guard ───────────────────────────────────────────────────
if [ ! -d "$LANE_DIR" ]; then
    err "lane_dir does not exist or is not a directory: $LANE_DIR"
    exit 1
fi

_rp_lane_dir="$(realpath -m "$LANE_DIR")"

# ── under-mount guard: gated on REIFY_WARM_LANE_MOUNT being set, trailing-
# slash prefix compare (mirrors seed-warm-lane.sh L428-440) so a sibling path
# like /mnt/warm-lanes-evil never falsely matches /mnt/warm-lanes ──────────────
if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ]; then
    _rp_mount="$(realpath -m "${REIFY_WARM_LANE_MOUNT}")"
    case "$_rp_lane_dir/" in
        "$_rp_mount"/*) ;;
        *)
            err "Precondition guard: lane_dir is not under REIFY_WARM_LANE_MOUNT"
            err "  lane_dir: $_rp_lane_dir"
            err "  REIFY_WARM_LANE_MOUNT (canonicalized): $_rp_mount"
            exit 1
            ;;
    esac
fi

# ── self-clobber (≠base, ≠mount-root) guard: refuse if lane_dir resolves to
# <mount>/base (when REIFY_WARM_LANE_MOUNT is set), is literally named "base"
# regardless of the mount, or IS the mount root itself (mirrors seed-warm-lane.sh's
# self-clobber guard). The mount-root case is NOT caught by the under-mount
# guard above: its trailing-slash case-match treats "$mount/" as matching
# "$mount"/* (glob * also matches the empty tail), so the mount root alone
# would otherwise slip through as "under" the mount ───────────────────────────
_self_clobber=0
if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ]; then
    _rp_mount_base="$(realpath -m "${REIFY_WARM_LANE_MOUNT}/base")"
    [ "$_rp_lane_dir" = "$_rp_mount_base" ] && _self_clobber=1
    [ "$_rp_lane_dir" = "$_rp_mount" ] && _self_clobber=1
fi
[ "$(basename "$_rp_lane_dir")" = "base" ] && _self_clobber=1
if [ "$_self_clobber" = "1" ]; then
    err "Precondition guard: lane_dir resolves to (or is named) the base dir, or is the mount root — refusing to thin"
    err "  lane_dir: $_rp_lane_dir"
    exit 1
fi

# ── T3: acquire the lane's own flock (non-blocking); refuse on contention ─────
# Mirrors warm-lane-gc.sh's live-consumer probe (L488-500): the same lane-lock
# convention (<lane_dir>.lock) other pool tooling (acquire/release, GC) uses.
LANE_LOCK="${LANE_DIR}.lock"
# The pool's acquire/release convention (inv.2) guarantees <lane_dir>.lock
# already exists for any lane that went through acquire_lane. If it is
# missing here, the exec below creates it on the fly -- on a genuinely
# exhausted filesystem that create can itself fail with ENOSPC before any
# bytes are freed. Log a breadcrumb so that failure mode reads as a lock-file
# creation problem rather than a bare, unexplained set -e abort.
[ -e "$LANE_LOCK" ] || info "Lane lock does not exist yet, creating: $LANE_LOCK (lane may never have been acquired through the pool)"
exec 9>"$LANE_LOCK"
if ! flock -n 9; then
    exec 9>&-
    err "LANE_LOCK_CONTENDED: Lane lock held by a live consumer (flock -n failed): $LANE_LOCK"
    err "Refusing to thin an ASSIGNED lane (inv.2: one consumer per lane at a time)."
    exit 75
fi
# FD 9 stays open (lock held) for the rest of the run; bash releases it on exit.

# ── T4: live-process-reference gate (task 5823) ───────────────────────────────
# The flock above proves no consumer is mid-ACQUIRE; it does NOT prove no
# consumer is mid-BUILD. The inv.2 lane lock is a reseed MUTEX, not a liveness
# oracle: it is held only across the acquire reseed (task 5354) and across
# dark-factory's run_scoped_verification (DF 3027), NEVER across the implement
# phase, where an agent runs cargo build/test for tens of minutes (esc-5334-6;
# earlier instances esc-5375-1, esc-5236/5069/5275/5062). So `flock -n 9` above
# is blind for most of an ASSIGNED lane's life. Before freeing, ask /proc
# directly: does any live process reference this lane dir or anything under it
# (cwd / open fd / mmap)?
#
# Placement is load-bearing on three axes (mirrors warm-lane-gc.sh L570-593):
#   - AFTER the flock acquire, so a flock-held lane keeps its own distinct
#     diagnostic — the two exit-75 reasons must stay distinguishable in
#     dark-factory's logs, which is what the LANE_LOCK_CONTENDED: /
#     LANE_LIVE_REF: stderr tokens deliver (see the exit-code table; convention
#     ratified by task 5568 for seed-warm-lane.sh) — and is spared the ~1.9s
#     O(processes × fds) walk. This ordering is pinned by B3b, whose lane is
#     BOTH flock-held AND live-referenced. Do
#     NOT read that as the cost being small: on the RELEASE path the flock is
#     normally FREE, so thin pays the full walk (and a negative verdict, the
#     common one there, cannot short-circuit).
#   - IMMEDIATELY BEFORE the destructive rm, never as an up-front snapshot: a
#     verdict computed earlier and consumed later IS the TOCTOU this gate exists
#     to close.
#   - Sitting before the rm therefore ALSO covers the --reseed branch below for
#     free: a refusal exits before the free, before the seed call, and before
#     the post-abort discard of an uncertified clone (§9.5 inv.13), so no
#     destructive path can bypass it.
#
# Bias toward OVER-preserving: a genuinely FREE lane has no referencing process,
# and preserving is TEMPORARY — acquire_lane always re-seeds target/ from base
# (D10), so a preserved divergent target/ is never reused as warm cache; it is
# reclaimed by the next release-thin or gc pass once the reference clears.
# Under-preserving corrupts a live build. A truly ORPHANED reference (a leaked
# test binary holding an fd) would shield a lane indefinitely; that is owned by
# the orphaned-test-binary reaper (docs/notes/orphaned-test-binary-reaper.md),
# not worked around here.
#
# SKIP, not defer: thin is a one-shot primitive holding ${LANE_DIR}.lock on FD 9,
# so a retry/sleep loop here would hold that lock across an unbounded wait —
# blocking the very acquire_lane reseed the lock exists to serialise (§9.5
# inv.11) and turning a benign skip into a pool-wide stall.
#
# The gate's /proc mechanism is NOT restated here — it is recorded once in
# docs/prds/warm-lane-pool-space-safety.md §8.4.
if live_ref_present "$LANE_DIR"; then
    exec 9>&-
    err "LANE_LIVE_REF: Live process reference at or under the lane (cwd / open fd / mmap): $_rp_lane_dir"
    err "Refusing to thin a lane with a live consumer build; preserving target/ (retry next cycle)."
    exit 75
fi

# ── FREE-FIRST reclaim (T1/T2): only target/ is ever removed ──────────────────
# Mirrors warm-lane-gc.sh's disk-pressure fast-path (L521-539): rm-stderr
# capture avoided set -e (the if-condition context) so a failure is reported
# with the actual error rather than a bare non-zero abort.
info "Thinning lane: $_rp_lane_dir"
_rm_err=""
if _rm_err="$(rm -rf "$LANE_DIR/target" 2>&1)"; then
    ok "Freed $LANE_DIR/target"
else
    err "rm -rf $LANE_DIR/target failed: ${_rm_err:-<rm produced no output>}"
    exit 1
fi

# ── optional --reseed (AFTER the free; T2 free-before-stage ordering) ─────────
# Redirect the seed-script's own stdout to stderr: thin-warm-lane's stdout
# contract is exactly one line (the resolved lane_dir, echoed below) and
# seed-warm-lane.sh has its own "resolved target path" stdout contract that
# would otherwise corrupt it.
if [ -n "$RESEED" ]; then
    info "Re-seeding thin base clone via $SEED_SCRIPT ..."
    # --assume-lane-lock-held: thin ALREADY holds ${LANE_DIR}.lock on FD 9 (T3,
    # line 236), held across this seed call. seed-warm-lane.sh acquires the lane
    # lock BY DEFAULT under --fresh-checkout (esc-5214/task 5354 fail-safe), so
    # without this opt-out it would re-open+flock the same file on its own FD 9
    # and self-refuse against thin's held lock (flock is not re-entrant across a
    # process tree). --assume-lane-lock-held tells seed to skip its own acquire;
    # thin's FD-9 lock already provides the inv.2 one-consumer exclusivity.
    if "$SEED_SCRIPT" "$BASE_TARGET_DIR" "$LANE_DIR" --fresh-checkout --assume-lane-lock-held >&2; then
        ok "Re-seeded: $LANE_DIR/target"
    else
        # A non-zero seed exit means the seed did NOT certify this lane. Keyed on
        # the EXIT STATUS rather than on the seed's stdout, and that is the same
        # predicate: seed-warm-lane.sh runs under `set -euo pipefail` and writes
        # stdout exactly once, at its terminal echo, so a guard's `err …; return 1`
        # always yields non-zero exit AND empty stdout together. (Observing the
        # stdout is not open to us anyway — it is redirected to stderr above, to
        # protect this script's own single-line stdout contract.)
        #
        # The clone must go. The seed's fail-closed post-conditions all fire AFTER
        # target/ has been replaced with the CoW clone, so an abort lands ONTO the
        # hazardous state it just refused to certify; the seed deliberately leaves
        # that clone in place, which makes discarding it the caller's obligation.
        # docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.13, "Caller obligation
        # on the fail-closed path", is the single normative home for the ruling.
        warn "Re-seed did not certify the lane ($SEED_SCRIPT exited non-zero); discarding any clone it left behind"
        # Probe BEFORE the rm: `rm -rf` exits 0 on a missing path, so its own
        # status cannot tell "removed the uncertified clone" from "there was
        # nothing there". Both happen here — a seed that refuses BEFORE staging
        # (base absent, RUSTFLAGS mismatch, a usage error) leaves the lane exactly
        # as the free-first rm above left it — and claiming a discard that never
        # happened would be a plain falsehood in the operator's log.
        _had_clone=0
        [ -e "$LANE_DIR/target" ] && _had_clone=1
        _rm_err=""
        if _rm_err="$(rm -rf "$LANE_DIR/target" 2>&1)"; then
            if [ "$_had_clone" = "1" ]; then
                warn "Discarded uncertified clone: $LANE_DIR/target (§9.5 inv.13 caller obligation; lane returned cold-but-safe)"
            else
                warn "Nothing to discard at $LANE_DIR/target — the seed refused before staging a clone (the free-first rm already freed it); lane returned cold-but-safe"
            fi
        else
            # The discard we just took on as an obligation did not happen, so the
            # lane is NOT safe. Exit 1 without reaching the trailing echo, leaving
            # stdout empty — this script mirroring the seed's own empty-stdout
            # refusal one level up. Exit code 1 is reused deliberately: it is
            # already this script's "the rm we guarantee did not happen" code (the
            # free-first failure above), and the condition is the same one.
            err "rm -rf $LANE_DIR/target failed: ${_rm_err:-<rm produced no output>}"
            err "Lane RETAINS an uncertified clone at $LANE_DIR/target — it must not be built in; NOT returned on stdout."
            exit 1
        fi
    fi
fi

ok "Thinned lane: $_rp_lane_dir"
echo "$_rp_lane_dir"
exit 0
