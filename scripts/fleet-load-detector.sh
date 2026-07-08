#!/usr/bin/env bash
# scripts/fleet-load-detector.sh — Fleet-wide host-oversubscription DETECTOR.
#
# Observes HOST-AGGREGATE CPU oversubscription across ALL concurrently-
# dispatched orchestrator lanes by reading host-global /proc/loadavg and
# /proc/pressure/cpu — host-wide sources ARE the aggregate contributed by
# every dispatched lane, so no fragile per-lane enumeration is needed.
# Reify-side signal for the DF-owned L3b dispatch-admission companion
# (docs/prds/run-all-pool-contention-tiering-fix.md §9; cross-referenced
# from docs/prds/cpu-load-admission-control.md §7 L3b). Reify ships this
# primitive; dark-factory wires the periodic invocation + dispatch-admission
# cap/escalation action (out of scope here — cross-repo seam, mirrors the
# verify-pipeline-guard / cpu_governance DF_AGENT_CPU_GOVERN pattern).
#
# Usage:
#   scripts/fleet-load-detector.sh check [--ratio-threshold N] [--avg10-threshold N]
#                                         [--loadavg-path PATH] [--psi-path PATH] [--nproc N]
#
# Subcommands:
#   check   Measure host loadavg1/nproc ratio AND PSI avg10; flag oversubscription.
#
# Options (env defaults shown):
#   --ratio-threshold N    Oversubscription ratio ceiling (load1/nproc)
#                          (env: REIFY_FLEET_LOAD_RATIO_THRESHOLD; default: 4.0)
#   --avg10-threshold N    PSI avg10 %% ceiling (env: REIFY_FLEET_LOAD_AVG10_THRESHOLD; default: 80)
#   --loadavg-path PATH    /proc/loadavg-format source — test seam
#                          (env: REIFY_FLEET_LOAD_LOADAVG_PATH; default: /proc/loadavg)
#   --psi-path PATH        /proc/pressure/cpu-format source — test seam
#                          (env: REIFY_FLEET_LOAD_PSI_PATH; default: /proc/pressure/cpu)
#   --nproc N              CPU count override — test seam
#                          (env: REIFY_FLEET_LOAD_NPROC; default: `nproc`)
#   -h, --help             Print this message and exit.
#
# Output — three redundant channels for the DF consumer:
#   stdout — always one machine-parseable verdict line:
#     FLEET_LOAD status=<ok|oversubscribed> load1=<f> nproc=<n> ratio=<f> \
#       ratio_threshold=<f> avg10=<f|unavailable> avg10_threshold=<f> reason=<ratio|avg10|both|none>
#   stderr — when flagged, a marker line (grep-able, same convention as @@REIFY_CLOCK_*@@):
#     @@REIFY_FLEET_OVERSUBSCRIBED@@ ratio=<f> load1=<f> nproc=<n> avg10=<f>
#   exit code:
#     0 — Healthy: ratio < ratio_threshold AND avg10 < avg10_threshold.
#     3 — Oversubscribed: ratio >= ratio_threshold OR avg10 >= avg10_threshold.
#     2 — Usage error.
#   (3 is deliberately distinct from 75 — the EX_TEMPFAIL "requeue THIS
#   command" sentinel used by per-command admission guards like cpu-admit.sh
#   and warm-lane-disk-guard.sh; this is a host monitor, not a per-command
#   admission gate, so requeue semantics do not apply.)
#
# Decision: FLAGGED iff ratio >= ratio_threshold OR avg10 >= avg10_threshold (dual-axis;
# docs/prds/cpu-load-admission-control.md §G6: PSI is the pathology-correct primitive,
# loadavg is the reference incident's human-observable headline metric — both covered).
#
# Fail-open (C-A4 philosophy, mirrors cpu-admit.sh / load_tolerance_lib.sh): an
# unreadable/unparseable axis is treated as ABSENT (not 0-or-huge) and excluded
# from the decision; the other axis alone decides. Both axes unreadable →
# status=ok + a WARNING on stderr, exit 0. A monitor that cannot measure must
# never spuriously flag/throttle fleet dispatch.
#
# Env knobs:
#   REIFY_FLEET_LOAD_RATIO_THRESHOLD   oversubscription ratio ceiling (default 4.0)
#   REIFY_FLEET_LOAD_AVG10_THRESHOLD   PSI avg10 %% ceiling (default 80)
#   REIFY_FLEET_LOAD_LOADAVG_PATH      /proc/loadavg source override (test seam)
#   REIFY_FLEET_LOAD_PSI_PATH          /proc/pressure/cpu source override (test seam)
#   REIFY_FLEET_LOAD_NPROC             nproc override (test seam)
#
# Config: orchestrator.yaml cpu_governance.fleet_load_detector documents these
# defaults (values MUST byte-match the :- fallbacks below); DF L3b consumes it.
# orchestrator.yaml loads once at startup — DF runtime consumption of a config
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
Usage: $(basename "$0") check [--ratio-threshold N] [--avg10-threshold N]
                               [--loadavg-path PATH] [--psi-path PATH] [--nproc N]

  Fleet-wide host-oversubscription DETECTOR. Reads host-global /proc/loadavg
  and /proc/pressure/cpu (the aggregate across every dispatched orchestrator
  lane) and FLAGS when either axis crosses its threshold.

  Subcommands:
    check   Measure load1/nproc ratio AND PSI avg10; print a verdict line.

  Options:
    --ratio-threshold N    Oversubscription ratio ceiling (load1/nproc)
                           (default: \$REIFY_FLEET_LOAD_RATIO_THRESHOLD or 4.0)
    --avg10-threshold N    PSI avg10 % ceiling
                           (default: \$REIFY_FLEET_LOAD_AVG10_THRESHOLD or 80)
    --loadavg-path PATH    /proc/loadavg-format source (test seam)
                           (default: \$REIFY_FLEET_LOAD_LOADAVG_PATH or /proc/loadavg)
    --psi-path PATH        /proc/pressure/cpu-format source (test seam)
                           (default: \$REIFY_FLEET_LOAD_PSI_PATH or /proc/pressure/cpu)
    --nproc N              CPU count override (test seam)
                           (default: \$REIFY_FLEET_LOAD_NPROC or \`nproc\`)
    -h, --help             Print this message and exit.

  Exit codes:
    0   — Healthy: ratio < ratio_threshold AND avg10 < avg10_threshold.
    3   — Oversubscribed: ratio >= ratio_threshold OR avg10 >= avg10_threshold.
    2   — Usage error.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
RATIO_THRESHOLD="${REIFY_FLEET_LOAD_RATIO_THRESHOLD:-4.0}"
AVG10_THRESHOLD="${REIFY_FLEET_LOAD_AVG10_THRESHOLD:-80}"
LOADAVG_PATH="${REIFY_FLEET_LOAD_LOADAVG_PATH:-/proc/loadavg}"
PSI_PATH="${REIFY_FLEET_LOAD_PSI_PATH:-/proc/pressure/cpu}"
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
        --avg10-threshold)
            [ $# -ge 2 ] || { err "--avg10-threshold requires a value"; exit 2; }
            AVG10_THRESHOLD="$2"; shift 2 ;;
        --loadavg-path)
            [ $# -ge 2 ] || { err "--loadavg-path requires a value"; exit 2; }
            LOADAVG_PATH="$2"; shift 2 ;;
        --psi-path)
            [ $# -ge 2 ] || { err "--psi-path requires a value"; exit 2; }
            PSI_PATH="$2"; shift 2 ;;
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
# ratio_threshold=/avg10_threshold= fields, and a non-numeric string on one
# side of an awk comparison forces a STRING (not numeric) comparison —
# unpredictable rather than safely defaulting.
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
AVG10_THRESHOLD="$(_validate_threshold "$AVG10_THRESHOLD" "80")"

# ── avg10 parser: guarded-source cpu-admit.sh (PRD decision #3: single PSI
# parser, no drift) when it AND lib_clock_stop.sh both exist next to this
# script; inline parity fallback otherwise. cpu-admit.sh sources
# lib_clock_stop.sh unconditionally at source-time and `exit 1`s if it is
# missing — sourcing cpu-admit.sh unconditionally would let that exit kill
# this detector, violating the fail-open contract, so the guard below only
# sources it when both files are confirmed present. ──────────────────────────
_FLEET_LOAD_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -f "$_FLEET_LOAD_SCRIPT_DIR/cpu-admit.sh" ] && [ -f "$_FLEET_LOAD_SCRIPT_DIR/lib_clock_stop.sh" ]; then
    # shellcheck source=scripts/cpu-admit.sh
    source "$_FLEET_LOAD_SCRIPT_DIR/cpu-admit.sh"
fi

# _read_avg10 PATH — echoes the `some` avg10 value from a /proc/pressure/cpu-
# format file, or "" on any error (missing file, unparseable content).
_read_avg10() {
    if declare -F cpu_admit_read_avg10 >/dev/null 2>&1; then
        cpu_admit_read_avg10 "$1"
    else
        awk '$1 == "some" {
            for (i=2; i<=NF; i++) {
                if ($i ~ /^avg10=/) { v=$i; sub(/^avg10=/, "", v); print v; exit }
            }
        }' "$1" 2>/dev/null || echo ""
    fi
}

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

AVG10="$(_read_avg10 "$PSI_PATH")"

# ── decision: FLAGGED iff ratio>=ratio_threshold OR avg10>=avg10_threshold ──────
# An unavailable axis (empty RATIO/AVG10) never participates — treated as
# absent, not 0-or-huge (fail-open; C-A4 philosophy).
RATIO_HIT=0
if [ -n "$RATIO" ] && awk -v r="$RATIO" -v t="$RATIO_THRESHOLD" 'BEGIN{exit !(r>=t)}'; then
    RATIO_HIT=1
fi

AVG10_HIT=0
if [ -n "$AVG10" ] && awk -v a="$AVG10" -v t="$AVG10_THRESHOLD" 'BEGIN{exit !(a>=t)}'; then
    AVG10_HIT=1
fi

if [ "$RATIO_HIT" -eq 1 ] && [ "$AVG10_HIT" -eq 1 ]; then
    STATUS="oversubscribed"; REASON="both"
elif [ "$RATIO_HIT" -eq 1 ]; then
    STATUS="oversubscribed"; REASON="ratio"
elif [ "$AVG10_HIT" -eq 1 ]; then
    STATUS="oversubscribed"; REASON="avg10"
else
    STATUS="ok"; REASON="none"
fi

# AVG10_DISPLAY — the stdout line renders the literal "unavailable" (not a
# blank field) when the PSI axis could not be read/parsed, so a consumer
# parsing the line sees an unambiguous token. The raw (possibly empty) AVG10
# above still drives the decision and below drives the stderr marker.
if [ -n "$AVG10" ]; then
    AVG10_DISPLAY="$AVG10"
else
    AVG10_DISPLAY="unavailable"
fi

# Both axes unavailable → the detector cannot assess fleet load at all; never
# spuriously flag/throttle dispatch when the detector itself is blind
# (fail-open; mirrors cpu-admit.sh C-A4 / load_tolerance_lib.sh's fail-safe
# philosophy). STATUS/REASON above already resolve to ok/none in this case
# (both *_HIT stay 0), so this only adds the stderr warning.
if [ -z "$RATIO" ] && [ -z "$AVG10" ]; then
    warn "loadavg ($LOADAVG_PATH) and PSI ($PSI_PATH) are both unreadable/unparseable — cannot assess fleet load; reporting healthy (fail-open)"
fi

printf 'FLEET_LOAD status=%s load1=%s nproc=%s ratio=%s ratio_threshold=%s avg10=%s avg10_threshold=%s reason=%s\n' \
    "$STATUS" "$LOAD1" "$NPROC" "$RATIO" "$RATIO_THRESHOLD" "$AVG10_DISPLAY" "$AVG10_THRESHOLD" "$REASON"

# ── flagged path ───────────────────────────────────────────────────────────────
# stdout verdict line above is emitted in BOTH cases (it already carries
# status=/reason=); the marker below is stderr-only and additive, so a
# consumer reading either channel works standalone.
if [ "$STATUS" = "oversubscribed" ]; then
    printf '@@REIFY_FLEET_OVERSUBSCRIBED@@ ratio=%s load1=%s nproc=%s avg10=%s\n' \
        "$RATIO" "$LOAD1" "$NPROC" "$AVG10" >&2
    exit 3
fi

exit 0
