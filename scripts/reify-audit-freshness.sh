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
#   reify_audit_guard "$BIN" warn-open "$REPO_ROOT"       # fail-open + alarm (predone wrapper)
#   reify_audit_guard "$BIN" refuse "$REPO_ROOT"          # fail-closed (NO live consumer)
#   reify_audit_guard "$BIN" rebuild "$REPO_ROOT"         # self-heal (audit skill)
#   reify_audit_guard "$BIN" rebuild-budget-safe "$REPO"  # budget-safe skip (verify.sh)
#
# MACHINE TOKENS AND KNOBS
# -------------------------
# Every warn-open message is prefixed with a stable, greppable token, and
# consumers must branch on the TOKEN rather than on message prose (the
# convention from crates/reify-audit/src/jcodemunch_index.rs:522-543):
#   E_AUDIT_BIN_STALE    the binary exists and runs, it is merely old.
#                        rc 0 by default (advisory); rc 125 under strict.
#   E_AUDIT_BIN_MISSING  there is no runnable binary at all. Always rc 125.
#   E_AUDIT_GUARD_BAD_MODE  the CALLER passed a mode string this library does
#                        not know (a typo at a call site). Reported on EVERY
#                        call, fresh or stale; the call is then treated as
#                        warn-open. See the mode-validation arm below.
# The STALE/MISSING distinction is load-bearing: both refusals exit 125, so
# only the token tells a reader whether the operator chose to fail closed or
# whether nothing is on disk.
#
# REIFY_AUDIT_FRESHNESS_STRICT=1 makes warn-open refuse (125) on a stale-but-
# runnable binary instead of falling open. Only the literal "1" arms it. It is
# an env knob, not a mode, because the wrapper's call site is fixed in the
# script while the systemd unit is where an operator can actually set
# something — the delivery path REIFY_AUDIT_PREDONE_WARN_ONLY already uses.
# The two gate DIFFERENT things: WARN_ONLY gates a FINDING's severity,
# FRESHNESS_STRICT gates BINARY freshness policy.
#
# CONSUMER POLICY
# ----------------
# - Predone wrapper: WARN-OPEN mode (#7139).
#     present + executable + stale -> loud E_AUDIT_BIN_STALE advisory, rc 0.
#                                     The stale detector runs and still gates
#                                     on its own findings.
#     unrunnable, or STRICT armed  -> rc 125.
#   Measured reason: dark-factory's
#   fused_memory/middleware/pre_done_hook.py returns None (allowing the flip)
#   only on rc 0 (:222-223) and blocks it on ANY non-zero rc, and it runs
#   inside fused-memory's per-project write lock with a 30s timeout (:25-31,
#   :151). See WHY FAIL-OPEN below.
#   DELIVERY NOTE: that same hook captures stderr with stderr=PIPE and surfaces
#   it ONLY on a non-zero rc, so the rc-0 ADVISORY this library writes to stderr
#   is discarded on the live path. The wrapper therefore captures this stderr
#   and tees it to the systemd journal and a sentinel file before re-emitting
#   it (see its "Durable advisory channel" block). Any FUTURE warn-open
#   consumer on a path that drops rc-0 stderr must do the same, or the alarm is
#   silent exactly where it matters.
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
#   on binaries that run perfectly well (#5962 review).  warn-open does this
#   split INTERNALLY, which is the main thing distinguishing it from refuse.
# - REFUSE mode is still supported and still exercised (Checks 8/9), but it has
#   NO live consumer.  It must NEVER be wired to a SYNCHRONOUS done-flip path
#   again: on such a path its 125 is not a safeguard, it is a project-wide
#   outage (see WHY FAIL-OPEN).  It remains appropriate for an ATTENDED, manual
#   invocation where a human reads the refusal and acts on it.
#
# WHY FAIL-OPEN ON THE PREDONE PATH (task #7139)
# -----------------------------------------------
# The predone wrapper is a synchronous pre-done hook.  Under REFUSE, one stale
# binary blocked EVERY done-flip in the project until a human reinstalled.
# crates/reify-audit lands ~4 commits/day, so the freshness epoch advances
# several times a day and the outage recurred by construction — it ran ~15.7h
# over 2026-08-30/31 and produced three escalations (esc-7042-2, esc-6315-2,
# esc-6120-5), none of which identified the real cause; all three blamed
# metadata.files or the done_provenance ancestor check.  Hence the messages
# below explicitly disclaim both.
#
# Running a STALE detector is strictly the pre-existing risk profile of the
# binary already installed; being unable to mark ANY work done is not.  This
# also aligns the shell guard with reify-audit's OWN house rule:
# crates/reify-audit/src/p5_phantom_done.rs:953-954 degrades a would-be-
# blocking High to an "[advisory - <reason>] " exit 0 whenever its evidence is
# incomplete.  Binary AGE is strictly weaker evidence than a degraded git leg,
# so fail-closed here was an inversion of the crate's own policy.
#
# RESIDUAL RISK, accepted: a stale detector may miss findings a fresh one would
# catch, or emit a stale false positive that blocks the flip on its own merits.
# REIFY_AUDIT_PREDONE_WARN_ONLY=1 remains the break-glass for a misfiring
# finding, and REIFY_AUDIT_FRESHNESS_STRICT=1 the opt-out from fail-open.
#
# TWO ALTERNATIVES REJECTED, both on measurement rather than taste:
# - Auto-reinstall inline (i.e. REBUILD mode here).  It cannot fit: the hook's
#   budget is a 30s timeout held under fused-memory's per-project write lock,
#   while `cargo install --path crates/reify-audit` pulls the whole reify
#   compiler stack via reify-test-support.  It would convert a refusal into a
#   TIMEOUT refusal AND serialize every task mutation on the project behind a
#   30s stall per flip.
# - Adding reify-audit to scripts/release-sensitive-crates.txt.  Wrong
#   mechanism: that file declares which crates have tests whose behaviour
#   differs between debug and release, so the merge-gate RELEASE pass runs
#   them.  It never invokes `cargo install` and never touches ~/.cargo/bin.
#   Worse, tests/infra/test_release_scoped_scope.sh asserts SET EQUALITY
#   between that list and a grep-derived set, so an unjustified entry reds the
#   gate.  A real post-landing install path is orchestrator/merge-worker
#   territory, not this library's.

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
REIFY_AUDIT_E_GUARD_BAD_MODE="E_AUDIT_GUARD_BAD_MODE"

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
# mode=warn-open: If fresh, return 0 silently.
#               If stale AND [ -x $bin ] AND not REIFY_AUDIT_FRESHNESS_STRICT=1,
#               print a self-describing E_AUDIT_BIN_STALE advisory and return 0
#               (FAIL OPEN — the stale detector runs anyway).
#               If stale AND unrunnable, print E_AUDIT_BIN_MISSING, return 125.
#               If stale AND strict armed, print E_AUDIT_BIN_STALE, return 125.
#               This is the mode for any SYNCHRONOUS done-flip path.
# mode=refuse:  If stale, print a reinstall hint to stderr and exit 125.
#               If fresh, return 0 silently.
#               NO live consumer — never wire this to a done-flip path (#7139).
# mode=rebuild: If stale, run `cargo build --release -q -p reify-audit`
#               (cwd=repo_root), then re-check freshness.
#               If still stale after rebuild, print hint and return 125.
#               If fresh (before or after rebuild), return 0.
# mode=<other>: UNKNOWN mode — print E_AUDIT_GUARD_BAD_MODE and proceed as
#               warn-open. Never falls through to a refusal (#7139 review).
reify_audit_guard() {
    local bin="$1"
    local mode="$2"
    local repo_root="${3:-$PWD}"

    # ── Mode validation (#7139 review) ───────────────────────────────────────
    # Before this arm there was NO mode validation: an unrecognised mode string
    # fell through every `if [ "$mode" = ... ]` test below to the terminal
    # `return 125` refuse path whenever the binary was judged stale.  On the
    # synchronous done-flip path that is precisely the outage this task exists
    # to remove, so a single-character slip at the wrapper's call site
    # (`warm-open` — literally the RED-step spelling in this task's own plan —
    # or `refuse-open`, `warnopen`, ...) would silently reinstate it, with the
    # OLD non-self-describing message and none of the tokens above.  The tests
    # cannot catch that on their own: they only ever pass VALID mode strings.
    #
    # The fallback is warn-open, not refuse, for the same reason the wrapper
    # uses warn-open: on the one path where the mode string is hardcoded and
    # therefore mistypeable, blocking every done-flip is by far the worse
    # failure, and a caller typo is not evidence about the binary.
    #
    # Validated BEFORE the fresh fast-path below, deliberately: a typo must be
    # reported on every call, not only on the day the binary happens to go
    # stale — otherwise it stays invisible until it is already an incident.
    case "$mode" in
        warn-open|refuse|rebuild|rebuild-budget-safe) ;;
        *)
            echo "$REIFY_AUDIT_E_GUARD_BAD_MODE: unknown reify_audit_guard mode '$mode' (expected one of: warn-open, refuse, rebuild, rebuild-budget-safe). Treating this call as warn-open so a caller typo cannot block done-flips. Fix the call site in the calling script." >&2
            mode="warn-open"
            ;;
    esac

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
