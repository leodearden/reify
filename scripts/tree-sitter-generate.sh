#!/usr/bin/env bash
# Regenerate tree-sitter parser from grammar.js.
# Produces: src/parser.c, src/grammar.json, src/node-types.json
#
# This script is idempotent — safe to run repeatedly.
# Called by: build.rs (auto), orchestrator verification, hooks/project-checks.
# Usage: tree-sitter-generate.sh [--force]

set -euo pipefail

FORCE=false
if [ "${1:-}" = "--force" ]; then
    FORCE=true
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TS_DIR="$(cd "$SCRIPT_DIR/../tree-sitter-reify" && pwd)"

# Maximum seconds to wait for lock acquisition (used by flock and stale-age check).
MAX_WAIT_SECS=120
# Maximum wall-time seconds for the mkdir-based poll loop (relies on the 1s
# sleep at the bottom of the loop so iteration count == wall-time seconds).
# Keeping this separate from MAX_WAIT_SECS (120) preserves the ~45s safety
# buffer between giving up and treating a held lock as stale.
MAX_LOCK_WAIT_SECS=75

if ! command -v tree-sitter >/dev/null 2>&1; then
    echo "ERROR: tree-sitter CLI not found on PATH." >&2
    echo "Install via: cargo install tree-sitter-cli" >&2
    exit 1
fi

if [ ! -f "$TS_DIR/grammar.js" ]; then
    echo "ERROR: $TS_DIR/grammar.js not found." >&2
    exit 1
fi

# Shared utilities (compute_sha256, etc.)
source "$SCRIPT_DIR/lib.sh"

cd "$TS_DIR"

# Compute grammar hash once before generation (avoids TOCTOU race between
# staleness check and stamp write — same pattern as build.rs).
GRAMMAR_HASH=$(compute_sha256 grammar.js | awk '{print $1}')
STAMP_FILE="src/.grammar_hash.stamp"
# Content manifest of the generated outputs (`#6992`).  A SIBLING of STAMP_FILE,
# deliberately not a widening of it: three live consumers assert STAMP_FILE is
# exactly 64 hex chars equal to sha256(grammar.js).
OUTPUTS_STAMP_FILE="src/.generated_outputs.stamp"
# The generated outputs, in LC_ALL=C sort order — the order the manifest is
# written in, and the order build_support.rs's EXPECTED_OUTPUTS is compared as.
OUTPUTS="grammar.json node-types.json parser.c"

# _hash_one <path>
#
# Print the bare sha256 of one file, or return 1 having printed NOTHING.
#
# Mirrors ts_hash_file in scripts/tree-sitter-freshness.sh, and for the same
# reason: `compute_sha256 f | awk '{print $1}'` in a command substitution
# reports only awk's status, and awk succeeds happily on empty input.  That
# yields an EMPTY hash field, which reads downstream as a mismatch — so a
# present-but-failing hasher would be indistinguishable from a genuinely stale
# file.  Fail closed and LOUD instead: this script's job is to keep "changed"
# and "could not tell" apart.
#
# The RETRY matters as much as the fail-closed direction, and is the same
# 3-attempt / 100ms-linear-backoff ladder ts_hash_file and build_support.rs's
# `sha256_of` both run.  The hasher can be on PATH and still fail one call — a
# transient fork/EMFILE spike under the parallel-cargo load this host routinely
# builds at.  Without the retry that spike propagates all the way out: a
# 3.6 s generate that SUCCEEDED loses its attestation, and the next run pays
# another full regeneration for a fault that was over in milliseconds.
_hash_one() {
    local _attempt _raw _hash
    for _attempt in 1 2 3; do
        # stderr suppressed per attempt so a retry that later SUCCEEDS stays
        # quiet; the caller emits the single authoritative diagnostic.
        if _raw=$(compute_sha256 "$1" 2>/dev/null); then
            _hash=$(printf '%s\n' "$_raw" | awk '{print $1}')
            if [ -n "$_hash" ]; then
                printf '%s\n' "$_hash"
                return 0
            fi
        fi
        # Plain `if`, not `[ ... ] && sleep`: as the last statement in the loop
        # body the short-circuit form would make the function's exit status
        # depend on arithmetic rather than on the explicit `return 1` below.
        if [ "$_attempt" -lt 3 ]; then
            sleep "0.$_attempt"
        fi
    done
    return 1
}

# _render_outputs_manifest
#
# Print the '<hash>  <relpath>' manifest for the three generated outputs, sorted
# by relpath.  Byte-identical in format to what build_support.rs renders and to
# ts_fingerprint's manifest.  Hard-fails naming the file if any output will not
# hash — never a partial manifest.
_render_outputs_manifest() {
    local f h
    for f in $OUTPUTS; do
        if ! h=$(_hash_one "src/$f"); then
            echo "ERROR: could not hash src/$f" >&2
            return 1
        fi
        printf '%s  %s\n' "$h" "$f"
    done
}

# _stamp_is_current
#
# Exit 0 when BOTH stamps agree with the tree on disk; non-zero otherwise.
#
# ONE helper, called from both the pre-lock check and the in-lock recheck, so
# the two can never drift — they were textually duplicated before `#6992`, and a
# fix applied to one would silently have missed the other.
#
# The outputs clause is the fix.  Existence used to be the whole test: the stamp
# said "grammar.js hashes to X" and three files were present, and that was
# "up to date".  Nothing looked at their bytes, so a parser.c generated from an
# entirely different grammar rode along on a stamp that was, in its own terms,
# perfectly correct — the state both of #6992's measurements found.  mtime
# cannot substitute: warm-lane seeding stamps every source to 2020-01-01.
#
# Fails CLOSED throughout.  A spurious regeneration costs one `tree-sitter
# generate`, already bounded by the 60 s timeout below; a spurious "up to date"
# links a parser the grammar never produced.
_stamp_is_current() {
    local f recorded actual
    [ -f "$STAMP_FILE" ] || return 1
    [ "$(cat "$STAMP_FILE" 2>/dev/null)" = "$GRAMMAR_HASH" ] || return 1
    for f in $OUTPUTS; do
        [ -f "src/$f" ] || return 1
    done
    [ -f "$OUTPUTS_STAMP_FILE" ] || return 1
    # Every recorded entry must match the file on disk, and the manifest must
    # name exactly the expected set — a short manifest attests nothing about the
    # outputs it omits.
    [ "$(awk '{print $2}' "$OUTPUTS_STAMP_FILE" 2>/dev/null | tr '\n' ' ')" \
        = "$(printf '%s ' $OUTPUTS)" ] || return 1
    while read -r recorded f; do
        [ -n "$f" ] || continue
        actual=$(_hash_one "src/$f") || return 1
        [ "$recorded" = "$actual" ] || return 1
    done < "$OUTPUTS_STAMP_FILE"
    return 0
}

# _write_stamps
#
# Write BOTH stamps atomically (temp file + mv, inside the held lock).
#
# The MANIFEST IS WRITTEN LAST, and the order is load-bearing: a crash between
# the two leaves a grammar stamp with no manifest beside it, which
# _stamp_is_current and build_support.rs both read as STALE.  The reverse order
# would leave a complete manifest beside a grammar stamp describing the PREVIOUS
# grammar — a stamp pair that actively lies.
#
# Temp names carry $$ and the `.tmp-` prefix the root .gitignore covers, so a
# crash between write and mv cannot leave the lane reporting an untracked file.
_write_stamps() {
    local manifest
    manifest=$(_render_outputs_manifest) || return 1
    printf '%s' "$GRAMMAR_HASH" > "src/.tmp-$$-grammar_hash" || return 1
    mv -f "src/.tmp-$$-grammar_hash" "$STAMP_FILE" || return 1
    printf '%s\n' "$manifest" > "src/.tmp-$$-generated_outputs" || return 1
    mv -f "src/.tmp-$$-generated_outputs" "$OUTPUTS_STAMP_FILE" || return 1
}

# Staleness check: skip generation if both stamps agree with the tree on disk.
# --force bypasses this check entirely.
if [ "$FORCE" = false ] && _stamp_is_current; then
    echo "tree-sitter: up to date (grammar.js unchanged)"
    exit 0
fi

# Acquire exclusive advisory lock to prevent concurrent generation from
# corrupting parser.c/grammar.json/node-types.json via interleaved writes.
# Uses flock on Linux; falls back to mkdir-based lock on macOS/other POSIX.
LOCK_FILE="src/.generate.lock"
LOCK_DIR="src/.generate.lock.d"

if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK_FILE"
    if ! flock -x -w $MAX_WAIT_SECS 9; then
        echo "ERROR: could not acquire flock within ${MAX_WAIT_SECS}s" >&2
        exit 1
    fi
else
    # Portable mkdir-based advisory lock (atomic on POSIX).
    # Retry with backoff; stale locks are cleaned up after MAX_WAIT_SECS.
    _lock_elapsed_secs=0
    while ! mkdir "$LOCK_DIR" 2>/dev/null; do
        _lock_elapsed_secs=$((_lock_elapsed_secs + 1))
        if [ "$_lock_elapsed_secs" -ge $MAX_LOCK_WAIT_SECS ]; then
            # Stale lock detection: if lock dir is older than MAX_WAIT_SECS, remove it.
            # Use empty sentinel when stat fails — refuse to remove a lock
            # we cannot verify as stale (avoids unconditional removal on
            # platforms where neither GNU nor BSD stat is available).
            if [ -d "$LOCK_DIR" ]; then
                _lock_mtime=$(stat -c %Y "$LOCK_DIR" 2>/dev/null || stat -f %m "$LOCK_DIR" 2>/dev/null || echo '')
                if [ -n "$_lock_mtime" ]; then
                    _lock_age=$(( $(date +%s) - _lock_mtime ))
                    if [ "$_lock_age" -ge $MAX_WAIT_SECS ]; then
                        echo "WARNING: removing stale lock dir (age=${_lock_age}s)" >&2
                        if rmdir "$LOCK_DIR" 2>/dev/null; then
                            _lock_elapsed_secs=0
                            continue
                        fi
                        # rmdir failed (e.g. NFS/uid-mismatch) — sleep to
                        # avoid tight busy-loop, then retry
                        sleep 1
                        continue
                    fi
                fi
            fi
            echo "ERROR: could not acquire generation lock after ${MAX_LOCK_WAIT_SECS}s" >&2
            exit 1
        fi
        sleep 1
    done
    # Ensure lock dir is removed on exit
    trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT
fi

# Re-check staleness inside lock — another process may have regenerated
# while we waited for the lock (double-check-locking pattern).
GRAMMAR_HASH=$(compute_sha256 grammar.js | awk '{print $1}')
if [ "$FORCE" = false ] && _stamp_is_current; then
    echo "tree-sitter: up to date (regenerated by another process)"
    exit 0
fi

# Helper: remove any partial output files written by a failed/killed
# tree-sitter generate run.  Called on both error branches so there is a
# single place to update if new output files are added in the future.
_cleanup_partial_outputs() {
    # The stamps go with the outputs (`#6992`).  A stamp surviving this deletion
    # vouches for files that no longer exist, so the next run would have to be
    # lucky rather than correct.  The temp files are swept too: a kill between
    # write and mv would otherwise leave the lane reporting an untracked file.
    rm -f src/parser.c src/grammar.json src/node-types.json \
          "$STAMP_FILE" "$OUTPUTS_STAMP_FILE" \
          "src/.tmp-$$-grammar_hash" "src/.tmp-$$-generated_outputs"
}

GEN_EXIT=0
# Use portable_timeout from lib_portable.sh (sourced via lib.sh above).
portable_timeout 60 tree-sitter generate || GEN_EXIT=$?
if [ "$GEN_EXIT" -eq 124 ] && [ "${_PORTABLE_TIMEOUT_TIMED_OUT:-false}" = "true" ]; then
    echo "ERROR: tree-sitter generate timed out after 60s" >&2
    # Remove any partial output files left by the killed process to prevent
    # corrupted parser.c/grammar.json/node-types.json from being used.
    _cleanup_partial_outputs
    exit 1
elif [ "$GEN_EXIT" -ne 0 ]; then
    echo "ERROR: tree-sitter generate failed (exit code $GEN_EXIT)" >&2
    _cleanup_partial_outputs
    exit 1
fi

# Verify expected outputs exist.
for f in src/parser.c src/grammar.json src/node-types.json; do
    if [ ! -f "$f" ]; then
        echo "ERROR: tree-sitter generate did not produce $f" >&2
        exit 1
    fi
done

# Attest what was just generated: the grammar hash, then the output manifest.
# $STAMP_FILE still receives the bare 64-hex hash with no trailing newline,
# byte-identical to what it has always held.
#
# NEVER FATAL, and the outputs are never touched here — the same call
# build_support.rs's `write_shell_stamps` makes for the identical condition.
# The outputs above were generated and existence-verified; only their
# attestation failed, which leaves them UNPROVEN, and both _stamp_is_current
# and build.rs's needs_generate read unproven as STALE.  So the next run
# self-heals at the cost of one regeneration.  Deleting a good parser.c and
# exiting 1 instead would turn a transient hasher fault into a hard failure of
# every caller (build.rs, hooks/project-checks, verify) — a far worse trade
# than the regeneration.  Both stamps go, not just the one that failed: a
# grammar stamp with no manifest beside it is exactly the unproven state
# intended, while a surviving pair could vouch for bytes nothing verified.
if ! _write_stamps; then
    echo "WARNING: generated outputs could not be attested; leaving them" \
         "unstamped (the next run will regenerate)" >&2
    rm -f "$STAMP_FILE" "$OUTPUTS_STAMP_FILE" \
          "src/.tmp-$$-grammar_hash" "src/.tmp-$$-generated_outputs"
fi

# Lock released automatically when script exits (fd 9 is closed).
echo "tree-sitter: generated parser files in $TS_DIR/src/"
