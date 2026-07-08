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

# step-2: CLI skeleton only — measurement/decision logic lands in later steps
# (task 5135 plan.json step-4 onward).
exit 0
