#!/usr/bin/env bash
# scripts/lib_systemd_user_install.sh — SOURCED skeleton for installers of
# reify-owned systemd --user timer units: tagged log helpers, the --user bus
# probe, the linger advisory, and the copy/daemon-reload/enable primitive.
# Consumer: scripts/install-docs-truth-sweep-units.sh.

if [ "${_LIB_SYSTEMD_USER_INSTALL_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_LIB_SYSTEMD_USER_INSTALL_SOURCED=1

_SYSTEMD_USER_INSTALL_TAG="$(basename "$0" .sh)"

systemd_user_log_info() { echo "[$_SYSTEMD_USER_INSTALL_TAG] INFO:  $*" >&2; }
systemd_user_log_ok()   { echo "[$_SYSTEMD_USER_INSTALL_TAG] OK:    $*" >&2; }
systemd_user_log_warn() { echo "[$_SYSTEMD_USER_INSTALL_TAG] WARN:  $*" >&2; }

# systemd_user_bus_available — exit 0 iff a systemd --user manager answers.
systemd_user_bus_available() {
    systemctl --user show-environment &>/dev/null
}

# systemd_user_warn_unless_lingering <timer> — a --user timer fires only while
# the user manager runs, which without lingering means only while logged in.
# Advisory only: warns, never fails.
systemd_user_warn_unless_lingering() {
    local timer="$1" user linger
    command -v loginctl &>/dev/null || return 0
    user="$(id -un)"
    linger="$(loginctl show-user "$user" -p Linger --value 2>/dev/null || true)"
    if [ "$linger" != "yes" ]; then
        systemd_user_log_warn "user lingering is NOT enabled — $timer will"
        systemd_user_log_warn "  run only while $user is logged in, not unattended.  Enable once with:"
        systemd_user_log_warn "    sudo loginctl enable-linger $user"
    fi
    return 0
}

# systemd_user_install_and_enable_timer <timer> <unit-source>... — plain-cp the
# sources into the user unit dir, daemon-reload, then enable --now the TIMER
# (never the service: the timer owns activation).
systemd_user_install_and_enable_timer() {
    local timer="$1" unit_dir source
    shift
    unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    mkdir -p "$unit_dir"
    for source in "$@"; do
        systemd_user_log_info "copying $source → $unit_dir/"
        cp "$source" "$unit_dir/"
    done
    systemd_user_log_info "systemctl --user daemon-reload"
    systemctl --user daemon-reload
    systemd_user_log_info "systemctl --user enable --now $timer"
    systemctl --user enable --now "$timer"
}
