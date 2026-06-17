#!/usr/bin/env bash
# scripts/cpu-governed-exec.sh — cgroup-v2 cpu.weight placement wrapper (task 4632).
#
# Usage:
#   cpu-governed-exec.sh --role <task|merge> -- CMD [ARGS...]
#
# Places CMD's process tree in a cgroup-v2 scope under the appropriate role
# slice with cpu.weight set by role, so under contention the kernel shares CPU
# time by weight while a lone scope absorbs the whole box (work-conserving).
#
# Fails-open (C-G4): when cgroup governance is unsupported or force-disabled,
# emits a warning and execs CMD directly (with nice de-prioritization if
# available). Never blocks.
#
# Knobs:
#   REIFY_CPU_GOVERN_DISABLE       set to 1 to force the degrade/fail-open path
#   REIFY_CPU_GOVERN_W_TASK        task role cpu.weight override (default 100)
#   REIFY_CPU_GOVERN_W_MERGE       merge role cpu.weight override (default 300)
#   REIFY_CPU_GOVERN_SLICE_TASK    task role slice override
#   REIFY_CPU_GOVERN_SLICE_MERGE   merge role slice override
#   REIFY_CPU_GOVERN_NICE          nice level for fail-open path (default 10)
#   REIFY_CPU_GOVERN_CONTROLLERS_PATH  override for delegation detection (test seam)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ---------------------------------------------------------------------------
# usage — print usage to stderr and exit 64 (EX_USAGE).
# ---------------------------------------------------------------------------
usage() {
    echo "Usage: $(basename "$0") --role <task|merge> -- CMD [ARGS...]" >&2
    echo "  --role <task|merge>   CPU scheduling role (required)" >&2
    echo "  --                    separator; CMD and its args follow" >&2
    exit 64
}

# ---------------------------------------------------------------------------
# Argument parsing.
# ---------------------------------------------------------------------------
role=""
separator_seen=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --role)
            shift
            if [ "$#" -eq 0 ]; then
                usage
            fi
            role="$1"
            shift
            ;;
        --)
            separator_seen=1
            shift
            break
            ;;
        *)
            usage
            ;;
    esac
done

# Validate: role must be present and valid.
if [ -z "$role" ]; then
    usage
fi
case "$role" in
    task|merge) ;;
    *) usage ;;
esac

# Validate: -- separator must have been seen.
if [ "$separator_seen" -eq 0 ]; then
    usage
fi

# Validate: at least one CMD token must follow --.
if [ "$#" -eq 0 ]; then
    usage
fi

# ---------------------------------------------------------------------------
# Source lib_cgroup.sh for detection and weight/slice resolution.
# ---------------------------------------------------------------------------
LIB="$SCRIPT_DIR/lib_cgroup.sh"
# shellcheck source=scripts/lib_cgroup.sh
source "$LIB"

# ---------------------------------------------------------------------------
# Check if governance is supported; degrade (fail-open) if not.
# ---------------------------------------------------------------------------
if ! cgroup_governance_supported; then
    echo "WARNING — cgroup governance unavailable, degrading (fail-open): role=$role cmd=$1" >&2

    # Best-effort: chain cpu-admit.sh admit if present and executable.
    if [ -x "$SCRIPT_DIR/cpu-admit.sh" ]; then
        "$SCRIPT_DIR/cpu-admit.sh" admit || true
    fi

    # Best-effort: prepend nice if on PATH.
    nice_level="${REIFY_CPU_GOVERN_NICE:-10}"
    if command -v nice >/dev/null 2>&1; then
        exec nice -n "$nice_level" "$@"
    else
        exec "$@"
    fi
fi

# ---------------------------------------------------------------------------
# Governed path: resolve weight + slice, set slice weight, exec via systemd-run.
# ---------------------------------------------------------------------------
weight="$(cgroup_role_weight "$role")"
slice="$(cgroup_role_slice "$role")"

# Best-effort: set the slice's cpu.weight so siblings share proportionally.
cgroup_set_slice_weight "$slice" "$weight" || true

# Place the command in a transient scope under the role slice.
# NEVER pass CPUQuota — keeps cpu.max=max (work-conserving, C-G1).
exec systemd-run --user --scope --quiet \
    --slice="$slice" \
    -p CPUWeight="$weight" \
    -- "$@"
