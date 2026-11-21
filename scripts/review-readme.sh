#!/usr/bin/env bash
# Scheduled README + getting-started audit. Run by
# ~/.config/systemd/user/reify-readme-review.timer every 4 days.
#
# Invokes Claude Code in non-interactive mode to compare the docs against
# the current state of the repo and patch any drift; claude only edits files.
# Before claude runs, the wrapper builds ./target/release/reify from HEAD
# (CARGO_BIN, bounded by REVIEW_README_BUILD_TIMEOUT) and stamps it, so the
# binary the docs are checked against is one the trust rule vouches for.
# The edit lands as a hook-gated commit by the review-readme machine identity
# and is never pushed: docs/notes/unattended-writers-of-main.md.
#
# Manual invocation (for testing):
#   scripts/review-readme.sh
#
# Logs to logs/readme-review-<timestamp>.log.
#
# Exit status:
#   0   no drift, or the edit was committed
#   1   refused: off main, a target untracked at HEAD, no claude CLI, no trusted ./target/release/reify could be built, or the commit was refused (edit kept in <log>.refused.patch)
#   69  the claude session exited non-zero (e.g. not logged in, turn cap): the audit did not complete and nothing landed; any target edit it left is kept in <log>.refused.patch and the targets are restored to HEAD
#   75  deferred: a target or a build input (BUILD_INPUTS) already differs from HEAD (human WIP), so claude was not run

set -euo pipefail

# shellcheck source=scripts/lib_main_checkout.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib_main_checkout.sh"
# shellcheck source=scripts/reify-bin-freshness.sh
source "$(dirname "${BASH_SOURCE[0]}")/reify-bin-freshness.sh"

REPO="$(reify_main_checkout)" || { echo "ERROR: cannot resolve the main checkout" >&2; exit 1; }
CLAUDE_BIN="${CLAUDE_BIN:-/home/leo/.local/bin/claude}"
CARGO_BIN="${CARGO_BIN:-cargo}"
BUILD_TIMEOUT="${REVIEW_README_BUILD_TIMEOUT:-20m}"
# Audit only this checkout's ./target/release/reify: nothing inherited may name another binary or move where cargo writes it.
unset REIFY_BIN CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR CARGO_BUILD_TARGET
TARGET_FILES=(README.md docs/getting-started.md)
BUILD_INPUTS=(crates tree-sitter-reify Cargo.toml Cargo.lock .cargo)
readonly WRITER_ID=review-readme
readonly WRITER_EMAIL="${WRITER_ID}@automation.reify.invalid"

cd "$REPO"

targets_differ_from_head() {
    ! git diff --quiet HEAD -- "${TARGET_FILES[@]}" \
        || ! git diff --cached --quiet HEAD -- "${TARGET_FILES[@]}"
}

mkdir -p logs
LOG="logs/readme-review-$(date +%Y%m%d-%H%M%S).log"

# Only a clean, gated state may reach the session: on any branch but main the
# commit would skip hooks/pre-commit, a target that is untracked or already
# dirty would make the machine commit absorb (or the restore destroy) human WIP,
# and a dirty build input would make the binary under audit reflect that WIP
# instead of HEAD.
preflight() {
    local f dirty_inputs
    if [ "$(git symbolic-ref --short -q HEAD)" != main ]; then
        echo "REFUSE: main checkout is not on main (pre-commit gates only main)"
        exit 1
    fi
    for f in "${TARGET_FILES[@]}"; do
        if ! git cat-file -e "HEAD:$f" 2>/dev/null; then
            echo "REFUSE: $f not tracked at HEAD"
            exit 1
        fi
    done
    if targets_differ_from_head; then
        echo "DEFER: uncommitted changes to target files; not overwriting human WIP"
        exit 75
    fi
    dirty_inputs=$(git --no-optional-locks status --porcelain --untracked-files=normal -- "${BUILD_INPUTS[@]}")
    if [ -n "$dirty_inputs" ]; then
        echo "DEFER: uncommitted changes to the build inputs; not building from a tree that differs from HEAD"
        echo "$dirty_inputs"
        exit 75
    fi
}

# The binary under audit is this checkout's ./target/release/reify, built from
# HEAD here, before the session. A binary the trust rule in
# scripts/reify-bin-freshness.sh does not vouch for never reaches claude.
build_trusted_reify_bin() {
    local rc=0 head_before
    head_before=$(reify_bin_head_sha "$REPO")
    timeout "$BUILD_TIMEOUT" "$CARGO_BIN" build --release -p reify-cli || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "REFUSE: cargo build --release -p reify-cli failed (rc=$rc; 124 = timed out after $BUILD_TIMEOUT)"
        exit 1
    fi

    # reify_bin_stamp records the CURRENT HEAD, so it must not run once main has
    # advanced past the build's start: it would vouch for an older tree's binary.
    # A skipped or failed stamp leaves the sidecar unvouching, and the trust rule
    # below refuses that and says why.
    if [ "$(reify_bin_head_sha "$REPO")" = "$head_before" ]; then
        reify_bin_stamp "$REPO" || true
    fi
    if ! resolve_trusted_reify_bin "$REPO" || [ "$REIFY_BIN_RESOLVED" != "$REPO/target/release/reify" ]; then
        echo "REFUSE: no trusted ./target/release/reify: ${REIFY_BIN_SKIP_REASON:-resolved $REIFY_BIN_RESOLVED}"
        exit 1
    fi
}

# A run that lands nothing leaves no trace in the checkout: the edit is saved
# beside the log and the targets are restored to HEAD.
quarantine_edit() {
    local patch="${LOG%.log}.refused.patch"
    git diff --binary HEAD -- "${TARGET_FILES[@]}" > "$patch"
    git restore --source=HEAD --staged --worktree -- "${TARGET_FILES[@]}"
    echo "edit quarantined at $patch; targets restored to HEAD"
}

# Commit the target files through the pre-commit gate as the machine identity;
# a refused commit quarantines the edit and the run exits 1.
land_or_quarantine() {
    local rc=0
    GIT_AUTHOR_NAME="$WRITER_ID" GIT_AUTHOR_EMAIL="$WRITER_EMAIL" \
    GIT_COMMITTER_NAME="$WRITER_ID" GIT_COMMITTER_EMAIL="$WRITER_EMAIL" \
        git commit --only -m "docs: scheduled 4-day README/getting-started review

Auto-generated by scripts/review-readme.sh via
~/.config/systemd/user/reify-readme-review.timer." -- "${TARGET_FILES[@]}" || rc=$?

    if [ "$rc" -eq 0 ]; then
        echo "committed $(git rev-parse --short HEAD) (publication to origin is the merge worker's post-advance push)"
        return 0
    fi

    echo "commit refused (rc=$rc)"
    quarantine_edit
    exit 1
}

# A session that exits non-zero did not finish the audit, so nothing it left may land.
refuse_failed_session() {
    echo "FAIL: claude exited $1; the audit did not complete, so nothing lands"
    if targets_differ_from_head; then
        quarantine_edit
    fi
    exit 69
}

{
    echo "=== reify-readme-review @ $(date -Iseconds) ==="

    if [ ! -x "$CLAUDE_BIN" ]; then
        echo "ERROR: claude CLI not executable at $CLAUDE_BIN"
        exit 1
    fi

    preflight
    build_trusted_reify_bin

    PROMPT='You are auditing the Reify project documentation. The repo is your working directory.

Goal: keep README.md and docs/getting-started.md accurate as the codebase evolves. Make ONLY corrections to existing claims; do not expand scope, add new sections, or restructure.

Steps:
1. Read README.md and docs/getting-started.md.
2. Run "git log --oneline -50" to see recent changes. Skim commits that look like they might affect documented behavior (CLI changes, new features, removed examples, build-system changes).
3. Know which documented commands can run here. The docs are binary-first:
   - A command that runs a bare "reify" or "reify-gui" from PATH, or names a bare "m5_geometry.ri" fetched by curl into the current directory, documents the INSTALLED release package. The README "Install" and "Try it" sections and docs/getting-started.md sections 1-4 (except the "For contributors" subsection of section 4) are written that way.
   - Those commands cannot run from this checkout as written: it has no m5_geometry.ri at its root, and the binary under audit is ./target/release/reify, not one on PATH. The release clean-container install proof (tests/infra/test_bundle_clean_install.sh) installs and runs the package in containers; do not run it. Never rewrite those commands into checkout forms ("./target/release/reify", "examples/...", "cargo build", "scripts/..."): a difference in binary path or file location alone is NOT drift.
   - The commands runnable from this checkout as written are the checkout forms, chiefly in the README "For contributors (build from source)" section and the "For contributors" subsection of docs/getting-started.md.
   - The exemption for installed-package commands covers their form, never their behavior. A subcommand, flag, exit code or output that the checkout binary contradicts IS drift in an installed-package command too: fix the claim and keep the command in its installed-package form. The opening paragraph of docs/getting-started.md states the checkout equivalents to run for that check. For a check that needs an edited .ri (getting-started section 3 is one), copy the file into a fresh "mktemp -d" directory, edit and run "./target/release/reify" on the copy; never edit any tracked file other than README.md and docs/getting-started.md.
4. Run the CLI from this checkout to verify documented behavior:
   - The wrapper already built ./target/release/reify from HEAD before this session. Use it as is; do not run cargo.
   - "./target/release/reify --help" — confirm the README "CLI commands" list matches.
   - "./target/release/reify --version" — confirm it works.
   - "./target/release/reify check examples/m5_geometry.ri" — confirm the contributor smoke test works; its output should also match what docs/getting-started.md section 1 shows for the installed "reify check m5_geometry.ri".
5. Verify by reading (do not run them) that scripts/setup-dev.sh, scripts/run-gui.sh, scripts/run-gui-dev.sh, scripts/build-bundle.sh and scripts/release-alpha.sh still exist and roughly match what the docs claim they do.
6. Update README.md or docs/getting-started.md to fix any drift you find. Edit in place; do not create new files. Do not touch any other files.
7. If everything is current, change nothing and exit. Conservatism wins.

Do NOT commit, push, or use git mutating commands. The wrapper script handles that.'

    echo "--- claude session ---"
    session_rc=0
    "$CLAUDE_BIN" -p "$PROMPT" \
        --dangerously-skip-permissions \
        --max-turns 40 \
        --output-format text \
        || session_rc=$?
    echo "--- end claude session ---"

    if [ "$session_rc" -ne 0 ]; then
        refuse_failed_session "$session_rc"
    fi

    if ! targets_differ_from_head; then
        echo "no changes against HEAD — done"
        exit 0
    fi

    echo "changes detected; committing target files only"
    land_or_quarantine

    echo "=== done ==="
} 2>&1 | tee -a "$LOG"
