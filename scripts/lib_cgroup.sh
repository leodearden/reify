#!/usr/bin/env bash
# scripts/lib_cgroup.sh — cgroup-v2 governance helpers for cpu.weight placement
# (task 4632). Designed to be sourced by cpu-governed-exec.sh and tests.
#
# Functions:
#   cgroup_role_weight <role>       echo cpu.weight for task|merge role
#   cgroup_role_slice <role>        echo systemd slice name for task|merge role
#   cgroup_governance_supported     return 0 if cgroup governance is available
#   cgroup_set_slice_weight <slice> <weight>  best-effort set slice cpu.weight
#
# Knobs:
#   REIFY_CPU_GOVERN_DISABLE           set to 1 for total bypass (break-glass)
#   REIFY_CPU_GOVERN_W_TASK            task role cpu.weight (default 100)
#   REIFY_CPU_GOVERN_W_MERGE           merge role cpu.weight (default 300)
#   REIFY_CPU_GOVERN_SLICE_TASK        task role slice (default reify-governed-agents.slice)
#   REIFY_CPU_GOVERN_SLICE_MERGE       merge role slice (default reify-governed-merge.slice)
#   REIFY_CPU_GOVERN_CONTROLLERS_PATH  override for delegation detection (test seam;
#                                      mirrors REIFY_COMPILE_GATE_PROC_PATH pattern)

# Source guard — prevent double-sourcing (mirrors lib_portable.sh / lib_test_semaphore.sh).
if [ "${_REIFY_LIB_CGROUP_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_CGROUP_SH_SOURCED=1

# ---------------------------------------------------------------------------
# _cgroup_validate_weight <value>
#   Internal helper. Return 0 if value is a positive integer, 64 otherwise.
# ---------------------------------------------------------------------------
_cgroup_validate_weight() {
    local v="$1"
    # Must be all digits (non-empty, no sign, no whitespace).
    case "$v" in
        ''|*[!0-9]*)
            echo "lib_cgroup.sh: weight must be a positive integer (got '${v}')" >&2
            return 64
            ;;
    esac
    if [ "$v" -lt 1 ]; then
        echo "lib_cgroup.sh: weight must be >= 1 (got '${v}')" >&2
        return 64
    fi
    return 0
}

# ---------------------------------------------------------------------------
# cgroup_role_weight <role>
#   Echo the cpu.weight for the given role (task|merge).
#   Honors REIFY_CPU_GOVERN_W_TASK / REIFY_CPU_GOVERN_W_MERGE overrides.
#   Returns 64 (EX_USAGE) if the resolved weight is not a positive integer.
# ---------------------------------------------------------------------------
cgroup_role_weight() {
    local role="$1"
    local weight
    case "$role" in
        task)
            weight="${REIFY_CPU_GOVERN_W_TASK:-100}"
            ;;
        merge)
            weight="${REIFY_CPU_GOVERN_W_MERGE:-300}"
            ;;
        *)
            echo "lib_cgroup.sh: unknown role '${role}' (expected task|merge)" >&2
            return 64
            ;;
    esac
    _cgroup_validate_weight "$weight" || return $?
    echo "$weight"
}

# ---------------------------------------------------------------------------
# cgroup_role_slice <role>
#   Echo the systemd slice name for the given role (task|merge).
#   Honors REIFY_CPU_GOVERN_SLICE_TASK / REIFY_CPU_GOVERN_SLICE_MERGE overrides.
#   Slice hierarchy (systemd dash-nesting):
#     reify-governed.slice
#       ├── reify-governed-agents.slice  (task, W_task)
#       └── reify-governed-merge.slice   (merge, W_merge)
#   Siblings of one parent → cpu.weight values are comparable (C-G2).
# ---------------------------------------------------------------------------
cgroup_role_slice() {
    local role="$1"
    case "$role" in
        task)
            echo "${REIFY_CPU_GOVERN_SLICE_TASK:-reify-governed-agents.slice}"
            ;;
        merge)
            echo "${REIFY_CPU_GOVERN_SLICE_MERGE:-reify-governed-merge.slice}"
            ;;
        *)
            echo "lib_cgroup.sh: unknown role '${role}' (expected task|merge)" >&2
            return 64
            ;;
    esac
}

# ---------------------------------------------------------------------------
# cgroup_governance_supported
#   Return 0 if cgroup-v2 cpu.weight governance is available on this host.
#   Return non-zero (1) if any prerequisite is missing or DISABLE is set.
#
#   Prerequisites (all must be true):
#     1. REIFY_CPU_GOVERN_DISABLE != 1
#     2. systemd-run is on PATH
#     3. cgroup-v2 unified hierarchy is mounted (/sys/fs/cgroup/cgroup.controllers)
#     4. 'cpu' controller is present in the user manager's delegated controllers
#        file (REIFY_CPU_GOVERN_CONTROLLERS_PATH, defaulting to the user manager's
#        delegated cgroup.controllers — mirrors REIFY_COMPILE_GATE_PROC_PATH pattern)
# ---------------------------------------------------------------------------
cgroup_governance_supported() {
    # (1) Break-glass bypass.
    if [ "${REIFY_CPU_GOVERN_DISABLE:-}" = "1" ]; then
        return 1
    fi

    # (2) systemd-run must be available.
    if ! command -v systemd-run >/dev/null 2>&1; then
        return 1
    fi

    # (3) cgroup-v2 unified hierarchy: /sys/fs/cgroup/cgroup.controllers must exist.
    if [ ! -f "/sys/fs/cgroup/cgroup.controllers" ]; then
        return 1
    fi

    # (4) 'cpu' controller delegated to the user manager.
    #   Resolve the controllers file: use the override if set (test seam), otherwise
    #   locate the user manager's own delegated cgroup.controllers.
    local _controllers_path
    if [ -n "${REIFY_CPU_GOVERN_CONTROLLERS_PATH:-}" ]; then
        _controllers_path="$REIFY_CPU_GOVERN_CONTROLLERS_PATH"
    else
        # Find the user manager's cgroup via /proc/self/cgroup and walk up to find
        # the delegated controllers file. The user manager (user@UID.service) has
        # its own cgroup.controllers listing what is delegated to it.
        # Path: /sys/fs/cgroup/<user-manager-cgroup>/cgroup.controllers
        local _user_cgroup _walk
        _user_cgroup="$(grep '^0::' /proc/self/cgroup 2>/dev/null | sed 's|^0::||')"
        if [ -z "$_user_cgroup" ]; then
            return 1
        fi
        # Walk from process cgroup up toward root to find a cgroup.controllers
        # that lists 'cpu'.  Stop before the root /sys/fs/cgroup/cgroup.controllers:
        # that file reflects globally registered controllers, NOT what is actually
        # delegated to the user manager — using it would produce a false positive on
        # hosts where the cpu controller exists globally but is NOT delegated to the
        # user session (cgroup_governance_supported returns 0 while systemd-run --user
        # scope creation subsequently fails because CPUWeight is not a delegated knob).
        _walk="$_user_cgroup"
        _controllers_path=""
        while [ -n "$_walk" ] && [ "$_walk" != "/" ]; do
            local _candidate="/sys/fs/cgroup${_walk}/cgroup.controllers"
            if [ -f "$_candidate" ]; then
                _controllers_path="$_candidate"
                break
            fi
            _walk="${_walk%/*}"
        done
        if [ -z "$_controllers_path" ]; then
            # No cgroup.controllers found in process hierarchy (below root).
            return 1
        fi
    fi

    # Check that the controllers file exists and contains 'cpu'.
    if [ ! -f "$_controllers_path" ]; then
        return 1
    fi
    if ! grep -qw "cpu" "$_controllers_path" 2>/dev/null; then
        return 1
    fi

    return 0
}

# ---------------------------------------------------------------------------
# cgroup_set_slice_weight <slice> <weight>
#   Best-effort: start the slice (vivifying it if needed), then set its
#   cpu.weight via systemctl --user set-property.  Both operations are
#   best-effort (failures are silently swallowed); callers use || true.
# ---------------------------------------------------------------------------
cgroup_set_slice_weight() {
    local slice="$1"
    local weight="$2"
    _cgroup_validate_weight "$weight" || return $?
    # Ensure the slice is live before set-property: on a cold start the slice
    # has not been vivified yet, and `systemctl --user set-property` on a
    # non-existent transient unit silently no-ops.  Starting the slice first
    # guarantees the CPUWeight assignment takes effect at the first scope
    # placement rather than waiting for a second run to self-heal (without this,
    # a fresh reify-governed-merge.slice is auto-vivified at systemd's default
    # weight 100 instead of the intended 300, giving no cross-role differential
    # on cold start).
    systemctl --user start "$slice" 2>/dev/null || true
    systemctl --user set-property "$slice" CPUWeight="$weight" 2>/dev/null || true
}
