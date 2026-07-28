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
#   C — idempotence + top-up across repeat runs
#   D — degenerate self-copy guard (script running under its own redirect)
#   E — fail-open on absent/unusable sources
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

# ──────────────────────────────────────────────────────────────────────────────
# Block C — idempotence + top-up
#
# setup-dev.sh is re-run freely and the boot unit fires every boot, so a seeder
# that errors on an already-seeded destination is broken in normal operation.
#
# PROBED (coreutils 9.4): a bare `cp -al` re-run over destination entries that
# are still the SAME hardlink is a silent no-op exiting 0 — so C1-C7 alone do
# NOT distinguish `--update=none` from a bare `cp -al`.  The distinguishing
# case is a destination entry with the same NAME but a DIFFERENT inode, where
# bare `cp -al` dies with "cannot create hard link: File exists".  That is not
# a contrived state: it is the normal steady state of the redirect, because a
# sandboxed agent running with CARGO_HOME=$CARGO_DEST downloads crates
# straight into it.  C8-C11 cover it, and are what make this block a detector.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: idempotence + top-up ---"

make_fake_home
make_cache_root
C_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"

reset_calls
run_redirect --seed-only
assert "C1: first --seed-only run exits 0" test "$RC" -eq 0

reset_calls
run_redirect --seed-only
assert "C2: second --seed-only run exits 0 (idempotent)" test "$RC" -eq 0
assert "C3: second run produced no ERROR output" \
    bash -c '! printf "%s\n%s\n" "$1" "$2" | grep -qE "ERROR|No such file|File exists"' \
    _ "$OUT" "$ERR_OUT"
assert "C4: previously-seeded file survived the second run" \
    test -f "$C_CARGO_DEST/registry/cache/alpha-1.0.0.crate"

# Top-up: a crate downloaded into the host cache AFTER the first seed must
# appear at the destination on the next run — otherwise the boot re-seed would
# freeze the redirect at whatever the caches held on setup-dev.sh day.
printf 'crate-tarball-gamma\n' > "$FAKE_HOME/.cargo/registry/cache/gamma-3.0.0.crate"

reset_calls
run_redirect --seed-only
assert "C5: third run (with a new source crate) exits 0" test "$RC" -eq 0
assert "C6: newly-added source crate was topped up into the destination" \
    test -f "$C_CARGO_DEST/registry/cache/gamma-3.0.0.crate"
assert "C7: topped-up crate SHARES AN INODE with its source" \
    same_inode "$FAKE_HOME/.cargo/registry/cache/gamma-3.0.0.crate" \
               "$C_CARGO_DEST/registry/cache/gamma-3.0.0.crate"

# ── C8-C11: divergent destination entry — the case that actually discriminates ─
# Replace one seeded entry with an independent file of the same name (what a
# sandboxed agent's own `cargo fetch` into the redirect leaves behind), then
# re-seed.  Bare `cp -al` fails the whole run here; `--update=none` skips that
# one name and carries on.  Note the required semantics: the DESTINATION wins.
# Re-linking would replace a file the agent is entitled to own with the host's
# copy — and would do it by unlinking through a path the agent may hold open.
printf 'agent-downloaded-alpha\n' > "$C_CARGO_DEST/registry/cache/alpha-1.0.0.crate.new"
mv "$C_CARGO_DEST/registry/cache/alpha-1.0.0.crate.new" \
   "$C_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
printf 'crate-tarball-delta\n' > "$FAKE_HOME/.cargo/registry/cache/delta-4.0.0.crate"

reset_calls
run_redirect --seed-only

assert "C8: run exits 0 with a divergent (same-name, different-inode) dest entry" \
    test "$RC" -eq 0
assert "C9: no 'File exists' hard-link error on the divergent entry" \
    bash -c '! printf "%s\n%s\n" "$1" "$2" | grep -qiE "File exists|cannot create hard link"' \
    _ "$OUT" "$ERR_OUT"
assert "C10: the divergent destination entry is PRESERVED, not clobbered" \
    bash -c 'printf "agent-downloaded-alpha\n" | cmp -s - "$1"' \
    _ "$C_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "C11: other entries still top up despite the divergent one" \
    test -f "$C_CARGO_DEST/registry/cache/delta-4.0.0.crate"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — degenerate self-copy guard
#
# A sandboxed agent runs with CARGO_HOME ALREADY pointed at the redirect, so a
# setup-dev.sh invocation from inside the sandbox asks the seeder to copy a
# tree onto itself.  `cp -al src/. src/` there is at best a no-op and at worst
# recurses into its own output.  Mirrors task 5332's own "reject degenerate
# /tmp target" amendment.
#
# D3 is the assertion that stops the guard being satisfiable by a DESTRUCTIVE
# no-op: the pre-existing destination contents must still be there afterwards.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: degenerate self-copy guard ---"

make_fake_home
make_cache_root
D_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"

# Seed normally first, so the destination has real contents to preserve.
reset_calls
run_redirect --seed-only
assert "D0: baseline seed exits 0" test "$RC" -eq 0

# Now re-run with CARGO_HOME pointed AT the destination — the sandboxed case.
_REDIRECT_ENV=("CARGO_HOME=$D_CARGO_DEST")
reset_calls
run_redirect --seed-only
_REDIRECT_ENV=()

assert "D1: degenerate source==destination run still exits 0" test "$RC" -eq 0
assert "D2: degenerate run warns and names the skip" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(degenerate|same|itself|skip)"' \
    _ "$OUT" "$ERR_OUT"
assert "D3: pre-existing destination contents survive the degenerate run" \
    test -f "$D_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "D4: destination contents are still byte-intact after the degenerate run" \
    bash -c 'printf "crate-tarball-alpha\n" | cmp -s - "$1"' \
    _ "$D_CARGO_DEST/registry/cache/alpha-1.0.0.crate"

# ── D5-D7: destination NESTED INSIDE source — the other half of the guard ─────
# Same-path is not the only degenerate shape.  A cache root configured under a
# source tree makes the destination a descendant of the source, and `cp -al`
# then recurses into its own output — a self-feeding copy that grows until it
# hits ENOSPC or the path limit, with the seed never converging.  An equality-
# only guard passes this case; a `realpath`-prefix guard catches it.
make_fake_home
D_NEST_ROOT="$FAKE_HOME/.cargo/registry/cache"
CACHE_ROOT="$D_NEST_ROOT"
TMPFILES_DIR="$(mktemp -d "$_TMPBASE/test-agent-cache-tmpfiles-XXXXXX")"
_TMPDIRS+=("$TMPFILES_DIR")

reset_calls
run_redirect --seed-only

assert "D5: nested destination run exits 0" test "$RC" -eq 0
assert "D6: nested destination run warns and names the skip" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(nested|inside|degenerate|itself|skip)"' \
    _ "$OUT" "$ERR_OUT"
assert "D7: no runaway self-copy — dest holds no second-level cache tree" \
    test ! -e "$D_NEST_ROOT/reify-agent-cargo-home/registry/cache/reify-agent-cargo-home"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — fail-open on absent/unusable sources
#
# A fresh contributor machine has no ~/.npm at all.  setup-dev.sh must not abort
# there, and — the independence assertion — one missing source must not suppress
# the other's seed.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: fail-open on absent sources ---"

# E(i): NEITHER source exists.
FAKE_HOME="$(mktemp -d "$_TMPBASE/test-agent-cache-bare-home-XXXXXX")"
_TMPDIRS+=("$FAKE_HOME")
make_cache_root
E_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
E_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

reset_calls
run_redirect --seed-only

assert "E1: exits 0 with NO source caches at all (fresh contributor machine)" \
    test "$RC" -eq 0
assert "E2: warns about the missing sources" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(missing|not found|absent|no such|skip)"' \
    _ "$OUT" "$ERR_OUT"
# No partial tree: a `mkdir -p` performed before checking the source would leave
# an empty destination that looks seeded to a later reader (and to `test -d`).
assert "E3: leaves no partial cargo destination tree behind" \
    test ! -e "$E_CARGO_DEST/registry/cache"
assert "E4: leaves no partial npm destination tree behind" \
    test ! -e "$E_NPM_DEST/_cacache"

# E(ii): only the CARGO source exists — the npm seed must not suppress it.
make_fake_home
rm -rf "${FAKE_HOME:?}/.npm"
make_cache_root
E2_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
E2_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

reset_calls
run_redirect --seed-only

assert "E5: exits 0 when only the cargo source exists" test "$RC" -eq 0
assert "E6: cargo cache IS still seeded despite the missing npm cache" \
    test -f "$E2_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "E7: cargo seed is still a hardlink in the one-source case" \
    same_inode "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate" \
               "$E2_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "E8: no partial npm destination tree when the npm source is absent" \
    test ! -e "$E2_NPM_DEST/_cacache"

# E(iii): only the NPM source exists — symmetric, so neither ordering hides a bug.
make_fake_home
rm -rf "${FAKE_HOME:?}/.cargo"
make_cache_root
E3_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
E3_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

reset_calls
run_redirect --seed-only

assert "E9: exits 0 when only the npm source exists" test "$RC" -eq 0
assert "E10: npm cache IS still seeded despite the missing cargo cache" \
    test -f "$E3_NPM_DEST/_cacache/content-v2/sha512/ab/blob"
assert "E11: no partial cargo destination tree when the cargo source is absent" \
    test ! -e "$E3_CARGO_DEST/registry/cache"

test_summary
