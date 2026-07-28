#!/usr/bin/env bash
# tests/infra/test_agent_cache_redirect.sh
# Hermetic tests for scripts/setup-agent-cache-redirect.sh (task 5729).
#
# The script under test provisions the /tmp toolchain-cache redirect that task
# 5332's landlock sandboxing points CARGO_HOME / npm_config_cache at:
#   * hardlink-pre-seeds the crates.io + npm caches into /tmp  (PROBLEM A)
#   * excludes them from systemd-tmpfiles age-cleaning           (PROBLEM B)
#   * re-seeds after the boot-time `D /tmp` wipe via a --user oneshot
#
# Everything here is hermetic: a fake $HOME holding synthetic cargo/npm cache
# trees, REIFY_AGENT_CACHE_ROOT / REIFY_TMPFILES_DIR / XDG_CONFIG_HOME pointed
# at mktemp dirs, and PATH-stubbed `sudo` / `systemctl` recording argv to a
# CALLS_FILE.  Nothing touches the real /tmp redirect, /etc/tmpfiles.d, the
# real ~/.cargo, ~/.npm or ~/.config.  Harness shape lifted from
# tests/infra/test_warm_lane_boot_persistence.sh.
#
# Blocks:
#   A — CLI contract (exists, executable, --help, unknown-flag exit 2)
#   B — hardlink pre-seed (Fix A): seeded set, byte-identity, INODE identity
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; declared
# `pool` in tests/infra/run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/setup-agent-cache-redirect.sh"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== agent toolchain-cache redirect hermetic tests (task 5729) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

# Everything is minted under ONE tmp root so the fake $HOME and the fake cache
# root are provably on the SAME filesystem — the script's cross-device guard
# would otherwise legitimately skip the seed and Block B could never observe a
# hardlink.  This mirrors the real host, where /tmp and $HOME share a device.
_TMPBASE="${TMPDIR:-/tmp}"

STUB_DIR="$(mktemp -d "$_TMPBASE/test-agent-cache-stub-XXXXXX")"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp "$_TMPBASE/test-agent-cache-calls-XXXXXX")"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp "$_TMPBASE/test-agent-cache-err-XXXXXX")"
_TMPDIRS+=("$ERR_FILE")

reset_calls() { : > "$CALLS_FILE"; }

# ── fake $HOME with synthetic cargo + npm cache trees ─────────────────────────
# Sets the FAKE_HOME global (rather than echoing it) because the _TMPDIRS
# registration must land in the MAIN shell: a `_TMPDIRS+=(...)` performed
# inside a `$(...)` subshell is discarded when that subshell exits, leaking the
# tree for the whole run.  Same reason test_helpers.sh's make_isolated_lane
# keeps its registration out of the subshell.
#
# `registry/src` and `_npx` are populated ON PURPOSE: they are the two trees
# the seeder must NOT copy (registry/src is derived from the .crate tarballs in
# registry/cache; _npx is churn, not a build input), so their presence at the
# source is what gives the "absent at the destination" assertions their teeth.
FAKE_HOME=""
make_fake_home() {
    FAKE_HOME="$(mktemp -d "$_TMPBASE/test-agent-cache-home-XXXXXX")"
    _TMPDIRS+=("$FAKE_HOME")
    mkdir -p "$FAKE_HOME/.cargo/registry/cache" \
             "$FAKE_HOME/.cargo/registry/index" \
             "$FAKE_HOME/.cargo/registry/src/github.com-1ecc/alpha-1.0.0" \
             "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab" \
             "$FAKE_HOME/.npm/_npx"
    printf 'crate-tarball-alpha\n' > "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate"
    printf 'crate-tarball-beta\n'  > "$FAKE_HOME/.cargo/registry/cache/beta-2.0.0.crate"
    printf 'index-entry-alpha\n'   > "$FAKE_HOME/.cargo/registry/index/alpha.json"
    printf 'extracted-lib-rs\n'    > "$FAKE_HOME/.cargo/registry/src/github.com-1ecc/alpha-1.0.0/lib.rs"
    printf 'cacache-blob\n'        > "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob"
    printf 'npx-scratch\n'         > "$FAKE_HOME/.npm/_npx/scratch"
}

# ── fake cache root (stand-in for /tmp) + fake /etc/tmpfiles.d ────────────────
CACHE_ROOT=""
TMPFILES_DIR=""
make_cache_root() {
    CACHE_ROOT="$(mktemp -d "$_TMPBASE/test-agent-cache-root-XXXXXX")"
    _TMPDIRS+=("$CACHE_ROOT")
    TMPFILES_DIR="$(mktemp -d "$_TMPBASE/test-agent-cache-tmpfiles-XXXXXX")"
    _TMPDIRS+=("$TMPFILES_DIR")
}

# ── run_redirect [args...] — invoke the script under test ─────────────────────
# CARGO_HOME / npm_config_cache / XDG_CONFIG_HOME are UNSET rather than merely
# left alone: the ambient environment of a real agent run has CARGO_HOME set,
# which would silently point the seeder at the host's real ~/.cargo and make
# every assertion below meaningless.  A block that needs one of them sets it
# explicitly via the _REDIRECT_ENV array (entries are plain KEY=VALUE strings,
# passed straight through to `env`).
_REDIRECT_ENV=()
RC=0
OUT=""
ERR_OUT=""
run_redirect() {
    local rc=0
    : > "$ERR_FILE"
    OUT="$(
        env -u CARGO_HOME -u npm_config_cache -u XDG_CONFIG_HOME \
            HOME="$FAKE_HOME" \
            REIFY_AGENT_CACHE_ROOT="$CACHE_ROOT" \
            REIFY_TMPFILES_DIR="$TMPFILES_DIR" \
            REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
            PATH="$STUB_DIR:$PATH" \
            "${_REDIRECT_ENV[@]+${_REDIRECT_ENV[@]}}" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# same_inode <a> <b> — the load-bearing hardlink probe.  A `cp -a` regression
# passes every content assertion in Block B and fails only this one.
same_inode() {
    local a b
    a="$(stat -c %i "$1" 2>/dev/null)" || return 1
    b="$(stat -c %i "$2" 2>/dev/null)" || return 1
    [ -n "$a" ] && [ "$a" = "$b" ]
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI contract
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI contract ---"

assert "A1: scripts/setup-agent-cache-redirect.sh exists" \
    test -f "$SCRIPT"

assert "A2: scripts/setup-agent-cache-redirect.sh is executable" \
    test -x "$SCRIPT"

make_fake_home
make_cache_root

reset_calls
run_redirect --help
assert "A3: --help exits 0" test "$RC" -eq 0
assert "A4: --help prints a Usage line" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qi "usage"' _ "$OUT" "$ERR_OUT"

reset_calls
run_redirect --no-such-flag
assert "A5: unknown flag exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Block B — hardlink pre-seed (Fix A)
#
# One --seed-only run against the fake HOME must materialise exactly the three
# seeded trees, byte-identical, sharing INODES with their sources — and must
# NOT materialise registry/src or _npx.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: hardlink pre-seed ---"

make_fake_home
make_cache_root
B_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
B_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

reset_calls
run_redirect --seed-only

assert "B1: --seed-only exits 0" test "$RC" -eq 0

assert "B2: registry/cache seeded" \
    test -d "$B_CARGO_DEST/registry/cache"
assert "B3: registry/index seeded" \
    test -d "$B_CARGO_DEST/registry/index"
assert "B4: npm _cacache seeded" \
    test -d "$B_NPM_DEST/_cacache"

assert "B5: alpha-1.0.0.crate present at destination" \
    test -f "$B_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "B6: beta-2.0.0.crate present at destination" \
    test -f "$B_CARGO_DEST/registry/cache/beta-2.0.0.crate"
assert "B7: index/alpha.json present at destination" \
    test -f "$B_CARGO_DEST/registry/index/alpha.json"
assert "B8: nested _cacache blob present at destination" \
    test -f "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"

assert "B9: seeded .crate is byte-identical to its source" \
    cmp -s "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate" \
           "$B_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "B10: seeded _cacache blob is byte-identical to its source" \
    cmp -s "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob" \
           "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"

# The deliberate narrowing (DD2): registry/src is DERIVED from the .crate
# tarballs already seeded into registry/cache, so seeding it would buy nothing
# and would hardlink-share a mutable extracted tree — the exact write-through
# hazard PROBLEM B is about.  _npx is churn, not a build input.
assert "B11: registry/src is NOT seeded (derived from registry/cache)" \
    test ! -e "$B_CARGO_DEST/registry/src"
assert "B12: _npx is NOT seeded (churn, not a build input)" \
    test ! -e "$B_NPM_DEST/_npx"

# ── the load-bearing assertions ───────────────────────────────────────────────
# Inode identity is what distinguishes the near-free `cp -al` hardlink copy from
# a real 6+ GB byte copy.  Without B13-B15 a plain `cp -a` implementation is
# indistinguishable from the intended one on every other assertion in this
# block, while silently doubling the host's cache footprint on every boot.
assert "B13: seeded .crate SHARES AN INODE with its source (hardlink, not copy)" \
    same_inode "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate" \
               "$B_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "B14: seeded index entry SHARES AN INODE with its source" \
    same_inode "$FAKE_HOME/.cargo/registry/index/alpha.json" \
               "$B_CARGO_DEST/registry/index/alpha.json"
assert "B15: seeded _cacache blob SHARES AN INODE with its source" \
    same_inode "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob" \
               "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"

test_summary
