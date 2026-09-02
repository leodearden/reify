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
#   of invoking `cargo build`.  This guard returns the SAME 75 whether the binary
#   is present-but-stale or absent, so 75 alone does not tell a caller whether
#   skipping is safe — how the caller must split those two cases is documented at
#   the rebuild-budget-safe branch below (#5962, esc-5405-7).
#   When REIFY_AUDIT_NO_COLD_BUILD is unset/0, falls through to the rebuild path.
#   Exit 75 is the codebase's established transient/backpressure sentinel (psi_gate,
#   test_semaphore_acquire) so the orchestrator already understands this signal.
# - rc 125 is presence-ambiguous in the same way, and for the same reason: see
#   the `return 125` site at the bottom of reify_audit_guard.  A caller that
#   treats it as "no usable detector" without checking `-x $bin` will hard-fail
#   on binaries that run perfectly well (#5962 review).

# Source guard — prevent double-sourcing.
if [ "${_REIFY_AUDIT_FRESHNESS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_AUDIT_FRESHNESS_SH_SOURCED=1

# Stable, greppable machine tokens. Consumers (the deploy probe, the predone
# wrapper's test suite, a triaging agent's grep over the hook's captured
# stderr) must branch on these rather than on message prose — the same
# convention as the pub-const refusal codes in
# crates/reify-audit/src/jcodemunch_index.rs:522-543. Their literal text is
# emitted into the message body so a plain `grep -F` matches.
REIFY_AUDIT_E_BIN_STALE="E_AUDIT_BIN_STALE"
REIFY_AUDIT_E_BIN_MISSING="E_AUDIT_BIN_MISSING"

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
        #
        # rc 75 does NOT mean "skipping is safe".  reify_audit_is_stale treats a
        # missing binary as stale (see "Missing binary is always stale" above), so
        # this same 75 covers both a present-but-stale AND an absent binary.  The
        # caller must split those two cases; test_reify_audit_ptodo.sh does
        # (#5962, esc-5405-7):
        #   PRESENT-but-stale → graceful SKIP, exit 0.  Only the scenarios needing
        #     a FRESH detector are skipped; the rest still run against the stale
        #     binary, so the run does assert something.
        #   ABSENT → exit 1.  Every scenario is guarded on `-x $REIFY_AUDIT_BIN`,
        #     so NONE execute, and exiting 0 would let a hard gate report green
        #     having asserted nothing.  Its no-silent-green floor refuses that.
        # Do not collapse this back to an unconditional 75 → exit 0 mapping.
        #
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

    if [ "$mode" = "warn-open" ]; then
        # Fail-open, but only onto something RUNNABLE.
        #
        # reify_audit_is_stale treats a MISSING binary as stale (see "Missing
        # binary is always stale" above), so this branch is reached by two very
        # different worlds and must split them — exactly the presence-ambiguity
        # contract already documented for rc 75 and rc 125 below.  Handling it
        # HERE rather than pushing it onto the caller is the point of the mode.
        #
        # `-x`, not `-f`: a present-but-non-executable binary is equally
        # unrunnable, and is_stale's own presence check is only `-f`, so the
        # guard must be the stricter of the two.
        # REIFY_AUDIT_FRESHNESS_STRICT=1 is the operator's opt-in escape
        # hatch: refuse rather than fall open.  It is an ENV knob, not a
        # fourth mode, because the wrapper's call site is fixed in the script
        # while the systemd unit is where an operator can actually set
        # something — the delivery path REIFY_AUDIT_PREDONE_WARN_ONLY uses.
        # Only the literal "1" arms it; unset/empty/"0"/"true" all leave the
        # default fail-open in force (the `[ "${VAR:-0}" = "1" ]` idiom already
        # used for REIFY_AUDIT_NO_COLD_BUILD above).
        # MESSAGE DESIGN.  Every form below is read by a human (or an agent)
        # triaging a blocked done-flip, through dark-factory's 2000-char
        # stderr clip.  Modelled on the IndexRefusal Display in
        # crates/reify-audit/src/jcodemunch_index.rs:592-620:
        # "{code}: <what> — <observed values> — <remedy>", token first.
        # Each states (1) that this is BINARY FRESHNESS infrastructure and NOT
        # an audit finding — the three escalations this replaced all blamed
        # metadata.files / done_provenance instead; (2) the exact one-line
        # fix; (3) the two observed numbers.  The rc-125 forms additionally
        # name the blast radius; the rc-0 advisory must NOT, because nothing
        # is blocked there.
        local _not_a_finding="This is a reify-audit BINARY FRESHNESS condition from scripts/reify-audit-freshness.sh — it is infrastructure, NOT an audit finding: it is not about metadata.files and not about the done_provenance ancestor check."
        # Keep the substring "reinstall with: cargo install ..." verbatim:
        # scripts/deploy-reify-audit-predone-hook.sh greps for it.
        local _remedy="Fix: reinstall with: cargo install --path crates/reify-audit --root ~/.cargo --force"
        local _blast="BLOCKING: this exit 125 blocks EVERY done-flip in this project until fixed."

        if [ -x "$bin" ] && [ "${REIFY_AUDIT_FRESHNESS_STRICT:-0}" != "1" ]; then
            # Fail OPEN: run the STALE detector rather than no detector.  It
            # still gates on its own findings; only the freshness guard steps
            # aside.  That is strictly the pre-existing risk profile of the
            # binary already installed, and a far weaker risk than a
            # project-wide inability to mark work done.
            echo "$REIFY_AUDIT_E_BIN_STALE: '$bin' predates crates/reify-audit (mtime $btime < crate commit epoch $epoch).
$_not_a_finding
FAILING OPEN: running the stale detector anyway so done-flips are not blocked project-wide.
$_remedy
Set REIFY_AUDIT_FRESHNESS_STRICT=1 to refuse instead of falling open." >&2
            return 0
        fi

        # Two refusals reach here and BOTH exit 125, so only the token
        # separates them.  Keep them distinguishable: a consumer must never
        # misread "the operator chose strict" as "no binary on disk".
        if [ -x "$bin" ]; then
            # Present and runnable — merely old — and strict is armed.
            echo "$REIFY_AUDIT_E_BIN_STALE: '$bin' predates crates/reify-audit (mtime $btime < crate commit epoch $epoch); REIFY_AUDIT_FRESHNESS_STRICT=1 is set, refusing rather than falling open.
$_not_a_finding
$_blast Unset REIFY_AUDIT_FRESHNESS_STRICT to fall open instead.
$_remedy" >&2
            return 125
        fi
        # Nothing runnable on disk.  Falling open here would exec nothing and
        # block the flip anyway, with a worse, less diagnosable rc — so
        # refusing is the only honest answer.
        echo "$REIFY_AUDIT_E_BIN_MISSING: no runnable reify-audit at '$bin' (crate commit epoch $epoch); there is nothing to fall open onto.
$_not_a_finding
$_blast
$_remedy" >&2
        return 125
    fi

    # rc 125 means "still judged stale", NOT "no usable binary" (#5962 review).
    # Two very different worlds reach here: a failed `cargo build -p reify-audit`
    # (nothing on disk to run), and a build that was a legitimate no-op — cargo's
    # fingerprint says up-to-date — while the on-disk mtime still predates the
    # last crates/reify-audit commit, e.g. a warm-lane seeded target/ with
    # stamped mtimes, where $bin is fully executable.  Callers that refuse on 125
    # must split on `[ -x "$bin" ]` first, exactly as they must for rc 75 above;
    # tests/infra/test_reify_audit_ptodo.sh does (absent → exit 1, present →
    # degrade to RATCHET_SKIP=1 and keep running the staleness-tolerant gates).
    echo "reify-audit binary '$bin' is stale (mtime $btime predates last crates/reify-audit commit $epoch); reinstall with: cargo install --path crates/reify-audit --root ~/.cargo --force" >&2
    return 125
}
