#!/usr/bin/env bash
# scripts/reify-audit-freshness.sh
#
# Shared freshness-guard library for reify-audit binary staleness detection.
# Designed to be SOURCED, not executed directly.
#
# WHY THIS GUARD EXISTS
# ----------------------
# reify-audit is absent from scripts/release-sensitive-crates.txt, so the
# merge-gate release pass (verify.sh --profile both) never rebuilds
# target/release/reify-audit. Without a caller-side guard, both the predone
# wrapper and the /audit skill silently serve a stale detector that may predate
# precision fixes (tasks 4074/4075/4076).
#
# WHY THE GUARD IS EXTERNAL (not inside the Rust binary)
# -------------------------------------------------------
# The staleness to catch is precisely a binary built BEFORE any guard existed.
# A Rust self-check cannot fire from a binary that predates the check
# (chicken-and-egg). The guard must live in the caller — the shell wrapper and
# the skill's binary-resolution contract.
#
# FRESHNESS REFERENCE
# --------------------
# Binary mtime is compared against the last git commit epoch of
# crates/reify-audit/ (`git log -1 --format=%ct -- crates/reify-audit`).
# A binary with mtime >= crate epoch is considered fresh.
#
# SCOPE LIMITATION
# -----------------
# The freshness reference only tracks changes inside crates/reify-audit/. A
# fix that lands in a workspace dependency crate (one that reify-audit links but
# that lives elsewhere) does NOT advance this epoch — the binary may be judged
# fresh even though it predates the fix. Dependency changes that affect
# reify-audit behaviour must also touch crates/reify-audit/ (e.g. bump the dep
# version in Cargo.toml) or be added to scripts/release-sensitive-crates.txt so
# the merge-gate release pass rebuilds target/release/reify-audit.
#
# FAIL-OPEN POLICY
# -----------------
# If the crate commit epoch is undeterminable (non-git repo_root / no history),
# the guard fails OPEN (treats binary as fresh) to avoid breaking edge/test
# invocations. A definitively-stale or missing binary always refuses/rebuilds.
# Note: if we ARE inside a git tree but the path yields no history, a warning
# is emitted to stderr (likely a renamed/moved crate path) — see is_stale below.
#
# USAGE
# -----
#   source "$REPO_ROOT/scripts/reify-audit-freshness.sh"
#   reify_audit_guard "$BIN" refuse "$REPO_ROOT"          # fail-closed (predone wrapper)
#   reify_audit_guard "$BIN" rebuild "$REPO_ROOT"         # self-heal (audit skill)
#   reify_audit_guard "$BIN" rebuild-budget-safe "$REPO"  # budget-safe skip (verify.sh)
#
# CONSUMER POLICY
# ----------------
# - Predone wrapper: REFUSE mode — exits 125 with a reinstall hint so stale
#   installs are loud and operators are forced to reinstall.
# - /audit skill: REBUILD mode — `cargo build --release -q -p reify-audit`
#   self-heals the release binary instead of refusing.
# - verify.sh run_all.sh line: REBUILD-BUDGET-SAFE mode — when
#   REIFY_AUDIT_NO_COLD_BUILD=1, returns 75 (EX_TEMPFAIL skip sentinel) instead
#   of invoking `cargo build`.  The caller maps 75 → graceful SKIP (exit 0).
#   When REIFY_AUDIT_NO_COLD_BUILD is unset/0, falls through to the rebuild path.
#   Exit 75 is the codebase's established transient/backpressure sentinel (psi_gate,
#   test_semaphore_acquire) so the orchestrator already understands this signal.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_AUDIT_FRESHNESS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_AUDIT_FRESHNESS_SH_SOURCED=1

# Source portable helpers (portable_mtime).
# Self-locate relative to this script so it works from any working directory.
_FRESHNESS_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_portable.sh
source "$_FRESHNESS_SCRIPT_DIR/lib_portable.sh"

# reify_audit_crate_commit_epoch [repo_root]
#
# Prints the Unix epoch of the last git commit that touched crates/reify-audit/.
# Prints nothing (empty string) when repo_root is not a git repo or has no history.
reify_audit_crate_commit_epoch() {
    local repo_root="${1:-$PWD}"
    git -C "$repo_root" log -1 --format="%ct" -- crates/reify-audit 2>/dev/null || true
}

# reify_audit_is_stale <bin> [repo_root]
#
# Returns 0 (stale) when:
#   - binary is missing
#   - binary mtime < crate commit epoch
# Returns 1 (fresh) when:
#   - binary mtime >= crate commit epoch
#   - crate commit epoch is undeterminable (fail-open)
reify_audit_is_stale() {
    local bin="$1"
    local repo_root="${2:-$PWD}"

    local epoch
    epoch=$(reify_audit_crate_commit_epoch "$repo_root")

    # Fail-open: if we can't determine the epoch, treat as fresh.
    if [ -z "$epoch" ]; then
        # Distinguish two cases:
        #   (a) Not a git repo at all (CI checkout, temp dir) — silent fail-open,
        #       this is a legitimate edge invocation.
        #   (b) Valid git tree but crates/reify-audit has NO history — likely the
        #       crate path was renamed or moved, silently disabling the guard.
        #       Emit a warning so the disabled state is not invisible.
        if git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
            echo "reify-audit freshness guard: crates/reify-audit has no git history under '$repo_root'; guard disabled (fail-open). If the crate path changed, update reify-audit-freshness.sh." >&2
        fi
        return 1
    fi

    # Missing binary is always stale.
    if [ ! -f "$bin" ]; then
        return 0
    fi

    local btime
    btime=$(portable_mtime "$bin" 2>/dev/null) || return 0  # mtime unreadable → stale

    # Stale if binary predates the last crate commit.
    if [ "$btime" -lt "$epoch" ]; then
        return 0
    fi

    return 1
}

# reify_audit_guard <bin> <mode> [repo_root]
#
# mode=refuse:  If stale, print a reinstall hint to stderr and exit 125.
#               If fresh, return 0 silently.
# mode=rebuild: If stale, run `cargo build --release -q -p reify-audit`
#               (cwd=repo_root), then re-check freshness.
#               If still stale after rebuild, print hint and return 125.
#               If fresh (before or after rebuild), return 0.
reify_audit_guard() {
    local bin="$1"
    local mode="$2"
    local repo_root="${3:-$PWD}"

    if ! reify_audit_is_stale "$bin" "$repo_root"; then
        return 0
    fi

    local epoch btime
    epoch=$(reify_audit_crate_commit_epoch "$repo_root")
    btime=$(portable_mtime "$bin" 2>/dev/null) || btime="<unreadable>"

    if [ "$mode" = "rebuild-budget-safe" ]; then
        # Budget-safe variant of rebuild: when REIFY_AUDIT_NO_COLD_BUILD=1, skip
        # the cold build entirely and return 75 (EX_TEMPFAIL skip sentinel).
        # The caller (test_reify_audit_ptodo.sh) maps 75 → graceful SKIP (exit 0).
        # When REIFY_AUDIT_NO_COLD_BUILD is unset or 0, fall through to the normal
        # rebuild path by reassigning mode.
        if [ "${REIFY_AUDIT_NO_COLD_BUILD:-0}" = "1" ]; then
            echo "reify-audit: binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 -- skipping cold build (budget-safe)" >&2
            return 75
        fi
        mode="rebuild"
    fi

    if [ "$mode" = "rebuild" ]; then
        # Attempt to self-heal the release binary.
        (cd "$repo_root" && cargo build --release -q -p reify-audit) || true
        # Re-check: if now fresh, return 0.
        if ! reify_audit_is_stale "$bin" "$repo_root"; then
            return 0
        fi
        # Still stale after rebuild — fall through to the refuse message.
    fi

    echo "reify-audit binary '$bin' is stale (mtime $btime predates last crates/reify-audit commit $epoch); reinstall with: cargo install --path crates/reify-audit --root ~/.cargo --force" >&2
    return 125
}
