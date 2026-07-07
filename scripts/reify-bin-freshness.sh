#!/usr/bin/env bash
# scripts/reify-bin-freshness.sh
#
# Shared freshness-guard library for reify CLI binary provenance detection.
# Designed to be SOURCED, not executed directly.
#
# WHY THIS GUARD EXISTS
# ----------------------
# tests/infra/test_prd_gate_corpus.sh and test_prd_gate_objective_inheritance.sh
# auto-discover whatever target/{release,debug}/reify happens to exist, and
# REIFY_AUDIT_NO_COLD_BUILD forbids rebuilding inside run_all.sh. In the shared
# _merge-verify warm lane, that binary can be a LEFTOVER from a DIFFERENT merge
# candidate that happened to build reify-cli earlier in the same lane. A gate
# verdict computed against the wrong candidate's binary is worse than no
# verdict at all (task #5133: a sibling's selective-demand fix silently
# flipped a corpus row from FAIL to PASS, blocking an innocent branch).
#
# WHY SHA-IDENTITY, NOT MTIME (contrast with scripts/reify-audit-freshness.sh)
# ------------------------------------------------------------------------
# reify-audit-freshness.sh compares binary mtime against the last commit epoch
# of crates/reify-audit/ — a fine heuristic when the only failure mode is "the
# binary predates a fix". It is the WRONG signal here: the contaminating
# sibling binary in the confirmed #5133 incident was built AFTER the current
# candidate's newest source commit. An mtime-vs-commit-epoch check would judge
# that leftover FRESH — a false negative that does not catch cross-candidate
# contamination. Only tree identity (the exact commit a binary was built from)
# distinguishes two candidates that share a build timeframe. The reify binary
# does not self-report its build SHA (crates/reify-cli/build.rs only emits an
# RPATH; `reify --version` prints CARGO_PKG_VERSION), so the build-time HEAD is
# recorded externally in a sidecar file, target/.reify-bin-sha, and compared
# against HEAD at gate time.
#
# FAIL POLICY
# ------------
# A missing binary, a missing sidecar, or a sidecar/HEAD mismatch all mean the
# binary's provenance relative to the CURRENT tree is unproven — treated as
# STALE. Callers must map staleness to a clean SKIP (never a verdict), matching
# the existing toolchain-not-built guard pattern. A non-git repo_root fails
# OPEN (treated as fresh) so edge/CI/unit-test-fixture invocations that aren't
# inside a git checkout are not incorrectly blocked — mirrors
# reify-audit-freshness.sh's fail-open precedent.
#
# USAGE
# -----
#   source "$REPO_ROOT/scripts/reify-bin-freshness.sh"
#   if reify_bin_is_stale "$BIN" "$REPO_ROOT"; then ...treat as unproven... fi
#   reify_bin_stamp "$REPO_ROOT"     # record build-time HEAD (called by verify.sh)
#   resolve_trusted_reify_bin "$REPO_ROOT" || { echo "SKIP: $REIFY_BIN_SKIP_REASON"; exit 0; }
#   # on success: $REIFY_BIN_RESOLVED is the vetted binary path

# Source guard — prevent double-sourcing.
if [ "${_REIFY_BIN_FRESHNESS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_BIN_FRESHNESS_SH_SOURCED=1

# reify_bin_head_sha [repo_root]
#
# Prints the current HEAD commit SHA of repo_root. Prints nothing (empty
# string) when repo_root is not a git repo or has no HEAD.
reify_bin_head_sha() {
    local repo_root="${1:-$PWD}"
    git -C "$repo_root" rev-parse HEAD 2>/dev/null || true
}

# reify_bin_recorded_sha [repo_root]
#
# Prints the trimmed contents of <repo_root>/target/.reify-bin-sha (the SHA
# recorded at build time). Prints nothing when the sidecar is absent.
# Command substitution trims the trailing newline, so it tolerates both a
# bare SHA and one written with a trailing newline (e.g. `git rev-parse HEAD
# > target/.reify-bin-sha`).
reify_bin_recorded_sha() {
    local repo_root="${1:-$PWD}"
    cat "$repo_root/target/.reify-bin-sha" 2>/dev/null || true
}

# reify_bin_is_stale <bin> [repo_root]
#
# Returns 0 (stale — provenance unproven) when:
#   - binary is missing
#   - the sidecar (target/.reify-bin-sha) is absent
#   - the sidecar SHA does not match repo_root's current HEAD
# Returns 1 (fresh) when:
#   - the sidecar SHA matches repo_root's current HEAD
#   - repo_root's HEAD is undeterminable (non-git repo_root; fail-open)
reify_bin_is_stale() {
    local bin="$1"
    local repo_root="${2:-$PWD}"

    # Missing binary is always stale.
    if [ ! -f "$bin" ]; then
        return 0
    fi

    local head
    head=$(reify_bin_head_sha "$repo_root")

    # Fail-open: repo_root isn't a git tree (or has no HEAD) — can't prove
    # cross-candidate contamination without a tree to compare against, and
    # this is a legitimate edge/CI/fixture invocation.
    if [ -z "$head" ]; then
        return 1
    fi

    local recorded
    recorded=$(reify_bin_recorded_sha "$repo_root")

    # No sidecar recorded — provenance unproven — stale.
    if [ -z "$recorded" ]; then
        return 0
    fi

    if [ "$recorded" = "$head" ]; then
        return 1
    fi

    return 0
}

# reify_bin_stamp [repo_root]
#
# Records repo_root's current HEAD SHA into <repo_root>/target/.reify-bin-sha,
# creating target/ if it doesn't already exist. Defaults repo_root to $PWD so
# it could be called argless (verify.sh stamps inline instead — see its
# infra pre-step block — but this keeps the entrypoint self-sufficient for any
# other caller).
#
# Returns 0 on success. Returns 1 (and writes no sidecar) when repo_root is
# not a git repo / has no HEAD — nothing meaningful to stamp.
reify_bin_stamp() {
    local repo_root="${1:-$PWD}"

    local head
    head=$(reify_bin_head_sha "$repo_root")
    if [ -z "$head" ]; then
        return 1
    fi

    mkdir -p "$repo_root/target"
    printf '%s\n' "$head" > "$repo_root/target/.reify-bin-sha"
}
