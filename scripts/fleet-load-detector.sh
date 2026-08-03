#!/usr/bin/env bash
# scripts/fleet-load-detector.sh — Fleet-wide host-oversubscription DETECTOR.
#
# Observes HOST-AGGREGATE CPU oversubscription across ALL concurrently-
# dispatched orchestrator lanes by reading host-global /proc/loadavg — a
# host-wide source IS the aggregate contributed by every dispatched lane, so
# no fragile per-lane enumeration is needed.
# Reify-side signal for the DF-owned L3b dispatch-admission companion
# (docs/prds/run-all-pool-contention-tiering-fix.md §9; cross-referenced
# from docs/prds/cpu-load-admission-control.md §7 L3b). Reify ships this
# primitive; dark-factory wires the periodic invocation + dispatch-admission
# cap/escalation action (out of scope here — cross-repo seam, mirrors the
# verify-pipeline-guard / cpu_governance DF_AGENT_CPU_GOVERN pattern).
#
# Usage:
#   scripts/fleet-load-detector.sh check [--ratio-threshold N]
#                                         [--loadavg-path PATH] [--nproc N]
#
# Subcommands:
#   check   Measure host loadavg1/nproc ratio; flag oversubscription.
#
# Options (env defaults shown):
#   --ratio-threshold N    Oversubscription ratio ceiling (load1/nproc)
#                          (env: REIFY_FLEET_LOAD_RATIO_THRESHOLD; default: 4.0)
#   --loadavg-path PATH    /proc/loadavg-format source — test seam
#                          (env: REIFY_FLEET_LOAD_LOADAVG_PATH; default: /proc/loadavg)
#   --nproc N              CPU count override — test seam
#                          (env: REIFY_FLEET_LOAD_NPROC; default: `nproc`)
#   -h, --help             Print this message and exit.
#
# Output — three redundant channels for the DF consumer:
#   stdout — always one machine-parseable verdict line:
#     FLEET_LOAD status=<ok|oversubscribed> load1=<f> nproc=<n> ratio=<f> \
#       ratio_threshold=<f> reason=<ratio|none>
#   stderr — when flagged, a marker line (grep-able, same convention as @@REIFY_CLOCK_*@@):
#     @@REIFY_FLEET_OVERSUBSCRIBED@@ ratio=<f> load1=<f> nproc=<n>
#   exit code:
#     0 — Healthy: ratio < ratio_threshold.
#     3 — Oversubscribed: ratio >= ratio_threshold.
#     2 — Usage error.
#   (3 is deliberately distinct from 75 — the EX_TEMPFAIL "requeue THIS
#   command" sentinel used by per-command admission guards like cpu-admit.sh
#   and warm-lane-disk-guard.sh; this is a host monitor, not a per-command
#   admission gate, so requeue semantics do not apply.)
#
# Decision: FLAGGED iff ratio >= ratio_threshold (SINGLE-AXIS since task 5985).
#
# A second axis — PSI `some avg10 >= 80`, read from /proc/pressure/cpu — used to
# sit beside the ratio arm here. Task 5985 DROPPED it (deliberately: dropped,
# not retuned), adopting dark_factory:3590's recommended disposition.
#
# THIS PARAGRAPH IS THE CANONICAL RECORD of that decision — this file is the
# artifact that implements it. dark-factory-orchestrator.yaml and
# tests/infra/test_fleet_load_detector.sh carry one-line pointers here rather
# than their own copies of the numbers, so a future retune updates the evidence
# in exactly one place instead of leaving two stale copies behind.
#
# Evidence:
# two 1 Hz samples three days apart show CPU PSI `some avg10` does not
# discriminate load regimes on this 32-core host. At load ~85 the distribution
# was min 5.5 / med 46.3 / p90 60.8 / max 68.9; at load ~190 it was min 62.2 /
# med 66.8 / p90 71.2 / max 74.1 — the WHOLE distribution slides upward with
# load (~1.4x separation) rather than spreading out, and the two ranges OVERLAP
# (the quiet sample's max, 68.9, exceeds the busy sample's min, 62.2), so no
# threshold cleanly separates the regimes. The observed maximum across both
# samples is 74.1, so the 80 ceiling has never fired and cannot; and retuning
# would mean picking from the 70-75 band, which lies entirely in the top decile
# of an ALREADY-saturated host — at 70 the gate engages on ~13.5% of busy ticks
# and 0% of quiet ones, and by 75 it is inert on both. A 5-point window whose
# useful end discriminates only the worst tenth of a bad day is not a signal
# worth carrying. The load1/nproc arm separates the same two regimes ~3x (2.4x
# cores quiet, 7.1x cores busy) and was already doing all the real work.
#
# (Those engagement fractions are measured, from the same two samples; the full
# per-threshold table is in dark-factory-orchestrator.yaml's PsiAdmissionConfig
# comment. That gate reads the SAME signal and retuned to 70 rather than
# dropping it — not a contradiction: a per-dispatch admission gate is USEFUL
# when it engages on the worst tenth of a busy day, whereas a fleet monitor
# that only speaks then, and never on a merely-loaded host, adds nothing the
# ratio arm does not already say sooner.)
#
# This SUPERSEDES docs/prds/cpu-load-admission-control.md §G6's dual-axis
# rationale FOR THIS DETECTOR ONLY. §G6's PSI-over-loadavg argument still
# governs scripts/cpu-admit.sh, which is a per-command admission gate on a
# short timescale rather than a host monitor, and is untouched here.
#
# The ratio arm's threshold (4.0) and semantics are UNCHANGED by 5985, so
# dark_factory:3590's `PsiAdmissionConfig.runqueue_ratio` default of 4.0 —
# added specifically to mirror this arm — needs NO follow-up change.
#
# The VERDICT LINE did change, though, and a DF-side parser must expect the
# narrowed shape: 5985 removed the `avg10=` and `avg10_threshold=` fields from
# both output channels (stdout line + @@REIFY_FLEET_OVERSUBSCRIBED@@ marker),
# and shrank the `reason=` domain from {ratio,avg10,both,none} to {ratio,none}.
# No in-repo consumer parses these (a repo-wide grep finds only doc references
# to the exit-3/marker convention, which is unchanged), so nothing broke here —
# this is forward-notice for the DF-side L3b consumer, whose invocation is the
# cross-repo half of this seam and has not landed yet.
#
# Fail-open (C-A4 philosophy, mirrors cpu-admit.sh / load_tolerance_lib.sh): an
# unreadable/unparseable loadavg is treated as ABSENT (not 0-or-huge) and
# excluded from the decision → status=ok + a WARNING on stderr, exit 0. A
# monitor that cannot measure must never spuriously flag/throttle fleet
# dispatch.
#
# Env knobs:
#   REIFY_FLEET_LOAD_RATIO_THRESHOLD   oversubscription ratio ceiling (default 4.0)
#   REIFY_FLEET_LOAD_LOADAVG_PATH      /proc/loadavg source override (test seam)
#   REIFY_FLEET_LOAD_NPROC             nproc override (test seam)
#
# Config: dark-factory-orchestrator.yaml cpu_governance.fleet_load_detector documents these
# defaults (values MUST byte-match the :- fallbacks below); DF L3b consumes it.
# dark-factory-orchestrator.yaml loads once at startup — DF runtime consumption of a config
# edit needs commit-then-restart (deploy-time DF concern, not verified here).

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  WARNING — %s\n' "$*" >&2; }
hint()  { err "Hint:  $*"; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") check [--ratio-threshold N]
                               [--loadavg-path PATH] [--nproc N]

  Fleet-wide host-oversubscription DETECTOR. Reads host-global /proc/loadavg
  (the aggregate across every dispatched orchestrator lane) and FLAGS when the
  load1/nproc ratio crosses its threshold.

  Subcommands:
    check   Measure load1/nproc ratio; print a verdict line.

  Options:
    --ratio-threshold N    Oversubscription ratio ceiling (load1/nproc)
                           (default: \$REIFY_FLEET_LOAD_RATIO_THRESHOLD or 4.0)
    --loadavg-path PATH    /proc/loadavg-format source (test seam)
                           (default: \$REIFY_FLEET_LOAD_LOADAVG_PATH or /proc/loadavg)
    --nproc N              CPU count override (test seam)
                           (default: \$REIFY_FLEET_LOAD_NPROC or \`nproc\`)
    -h, --help             Print this message and exit.

  Exit codes:
    0   — Healthy: ratio < ratio_threshold.
    3   — Oversubscribed: ratio >= ratio_threshold.
    2   — Usage error.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
RATIO_THRESHOLD="${REIFY_FLEET_LOAD_RATIO_THRESHOLD:-4.0}"
LOADAVG_PATH="${REIFY_FLEET_LOAD_LOADAVG_PATH:-/proc/loadavg}"
NPROC_OVERRIDE="${REIFY_FLEET_LOAD_NPROC:-}"

# ── arg parsing ────────────────────────────────────────────────────────────────
SUBCOMMAND=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --ratio-threshold)
            [ $# -ge 2 ] || { err "--ratio-threshold requires a value"; exit 2; }
            RATIO_THRESHOLD="$2"; shift 2 ;;
        --loadavg-path)
            [ $# -ge 2 ] || { err "--loadavg-path requires a value"; exit 2; }
            LOADAVG_PATH="$2"; shift 2 ;;
        --nproc)
            [ $# -ge 2 ] || { err "--nproc requires a value"; exit 2; }
            NPROC_OVERRIDE="$2"; shift 2 ;;
        check)
            SUBCOMMAND="check"; shift ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unknown subcommand: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# Validate subcommand
if [ -z "$SUBCOMMAND" ]; then
    err "Missing subcommand. Expected: check"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# _validate_threshold VALUE DEFAULT — echoes VALUE if it is a valid number
# (awk numeric-validity guard, mirroring load_tolerance_lib.sh's $1+0==$1
# idiom); else echoes DEFAULT. A garbage threshold must never reach the awk
# comparison below unvalidated: it would leak verbatim into the stdout
# ratio_threshold= field, and a non-numeric string on one side of an awk
# comparison forces a STRING (not numeric) comparison — unpredictable rather
# than safely defaulting.
_validate_threshold() {
    local _val="$1" _default="$2" _valid
    _valid="$(printf '%s' "$_val" | awk '{if ($1+0 == $1 && $1 != "") print "ok"}' 2>/dev/null || true)"
    if [ "$_valid" = "ok" ]; then
        echo "$_val"
    else
        echo "$_default"
    fi
}
RATIO_THRESHOLD="$(_validate_threshold "$RATIO_THRESHOLD" "4.0")"

# NOTE: task 5985 removed the guarded `source scripts/cpu-admit.sh` block that
# used to live here. It existed solely to share cpu_admit_read_avg10 with the
# now-dropped PSI axis (PRD decision #3: single PSI parser, no drift), and it
# carried a fragile source-time dependency on lib_clock_stop.sh — cpu-admit.sh
# `exit 1`s if that is missing, which would have killed this detector mid-run
# and violated the fail-open contract. cpu_admit_read_avg10 is NOT orphaned by
# its removal: scripts/cpu-admit.sh and tests/infra/run_all.sh still call it.

# _read_load1 PATH — echoes field 1 of a /proc/loadavg-format file, or "" on
# any error (unreadable file, non-numeric field). Mirrors load_tolerance_lib
# .sh's load_tolerance_factor read+awk-validate idiom ($1+0==$1 numeric guard).
_read_load1() {
    local _path="$1" _raw _valid
    if [ ! -r "$_path" ]; then
        echo ""
        return 0
    fi
    _raw="$(awk '{print $1}' "$_path" 2>/dev/null || true)"
    _valid="$(printf '%s' "$_raw" | awk '{if ($1+0 == $1 && $1 != "") print "ok"}' 2>/dev/null || true)"
    if [ "$_valid" = "ok" ]; then
        echo "$_raw"
    else
        echo ""
    fi
}

# _resolve_nproc — echoes NPROC_OVERRIDE (validated) or `nproc`'s output, or
# "" if neither yields a valid positive integer. Mirrors load_tolerance_lib
# .sh's nproc-override + fail-safe idiom (the "_NPROC pattern" reuse item).
_resolve_nproc() {
    local _n
    if [ -n "$NPROC_OVERRIDE" ]; then
        _n="$NPROC_OVERRIDE"
    else
        _n="$(nproc 2>/dev/null || true)"
    fi
    case "$_n" in
        ''|*[!0-9]*) echo ""; return 0 ;;
    esac
    if [ "$_n" -le 0 ] 2>/dev/null; then
        echo ""
        return 0
    fi
    echo "$_n"
}

# ── measurement ────────────────────────────────────────────────────────────────
LOAD1="$(_read_load1 "$LOADAVG_PATH")"
NPROC="$(_resolve_nproc)"

RATIO=""
if [ -n "$LOAD1" ] && [ -n "$NPROC" ]; then
    RATIO="$(awk -v l="$LOAD1" -v n="$NPROC" 'BEGIN{printf "%.2f", l/n}')"
fi

# ── decision: FLAGGED iff ratio>=ratio_threshold ───────────────────────────────
# An unavailable ratio (empty RATIO) never participates — treated as absent,
# not 0-or-huge (fail-open; C-A4 philosophy).
RATIO_HIT=0
if [ -n "$RATIO" ] && awk -v r="$RATIO" -v t="$RATIO_THRESHOLD" 'BEGIN{exit !(r>=t)}'; then
    RATIO_HIT=1
fi

if [ "$RATIO_HIT" -eq 1 ]; then
    STATUS="oversubscribed"; REASON="ratio"
else
    STATUS="ok"; REASON="none"
fi

# Ratio unavailable → the detector cannot assess fleet load at all; never
# spuriously flag/throttle dispatch when the detector itself is blind
# (fail-open; mirrors cpu-admit.sh C-A4 / load_tolerance_lib.sh's fail-safe
# philosophy). STATUS/REASON above already resolve to ok/none in this case
# (RATIO_HIT stays 0), so this only adds the stderr warning.
#
# The ratio needs BOTH inputs, so blame the one that actually failed: nproc can
# fail to resolve while loadavg parses fine (invalid --nproc/REIFY_FLEET_LOAD
# _NPROC, or no `nproc` binary). Unconditionally blaming loadavg there would
# print a diagnostic the very next line contradicts (`load1=1.00 nproc=`).
if [ -z "$RATIO" ]; then
    if [ -z "$LOAD1" ]; then
        warn "loadavg ($LOADAVG_PATH) is unreadable/unparseable — cannot assess fleet load; reporting healthy (fail-open)"
    else
        warn "nproc could not be resolved (invalid --nproc/REIFY_FLEET_LOAD_NPROC, or \`nproc\` unavailable) — cannot compute ratio; reporting healthy (fail-open)"
    fi
fi

printf 'FLEET_LOAD status=%s load1=%s nproc=%s ratio=%s ratio_threshold=%s reason=%s\n' \
    "$STATUS" "$LOAD1" "$NPROC" "$RATIO" "$RATIO_THRESHOLD" "$REASON"

# ── flagged path ───────────────────────────────────────────────────────────────
# stdout verdict line above is emitted in BOTH cases (it already carries
# status=/reason=); the marker below is stderr-only and additive, so a
# consumer reading either channel works standalone.
if [ "$STATUS" = "oversubscribed" ]; then
    printf '@@REIFY_FLEET_OVERSUBSCRIBED@@ ratio=%s load1=%s nproc=%s\n' \
        "$RATIO" "$LOAD1" "$NPROC" >&2
    exit 3
fi

exit 0
