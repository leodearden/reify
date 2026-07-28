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
#   F — tmpfiles age-clean exclusion (Fix B): content, idempotence, fail-open
#   G — desync guard: the conf's paths ARE the seeder's destinations
#   H — boot persistence: the --user oneshot, and --seed-only mode separation
#   I — setup-dev.sh wiring (structural grep on uncommented lines only)
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

# ── sudo stub ─────────────────────────────────────────────────────────────────
# Records argv to CALLS_FILE and then EXECS its remaining arguments, so the
# privileged write actually lands in the temp $REIFY_TMPFILES_DIR and Blocks F
# and G can assert on real file content rather than on an argv string.  With
# `fail=1` it records and exits 1 instead, standing in for a host where sudo is
# absent or refuses.
#
# `sudo -n true`-style probes are also recorded, so the script can preflight.
make_sudo_stub() {
    local fail="${1:-0}"
    cat > "$STUB_DIR/sudo" << STUB_EOF
#!/usr/bin/env bash
echo "sudo \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "$fail" = "1" ]; then
    echo "sudo: refusing (test stub)" >&2
    exit 1
fi
# Strip sudo's own leading options so the remainder is a runnable command.
while [ \$# -gt 0 ]; do
    case "\$1" in
        -n|-A|-k|-E|-H|-S) shift ;;
        -u) shift 2 ;;
        --) shift; break ;;
        *) break ;;
    esac
done
[ \$# -eq 0 ] && exit 0
exec "\$@"
STUB_EOF
    chmod +x "$STUB_DIR/sudo"
}
make_sudo_stub

# ── systemctl stub ────────────────────────────────────────────────────────────
# Installed from the FIRST full-mode run onwards, not merely for Block H: a
# full-mode run reaching the REAL systemctl would daemon-reload and enable a
# unit on the developer's live --user bus.  Lifted in shape from
# test_warm_lane_boot_persistence.sh.
make_systemctl_stub() {
    cat > "$STUB_DIR/systemctl" << 'STUB_EOF'
#!/usr/bin/env bash
echo "systemctl $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
# simulate a missing --user bus when REIFY_TEST_NO_USER_BUS=1
if [ "${REIFY_TEST_NO_USER_BUS:-0}" = "1" ]; then
    for _arg in "$@"; do
        [ "$_arg" = "show-environment" ] && exit 1
    done
fi
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/systemctl"
}
make_systemctl_stub

# ── systemd-tmpfiles stub ─────────────────────────────────────────────────────
# The script best-effort applies the new conf so the exclusion takes effect
# without waiting for a reboot.  Stubbed so a test run can never ask the real
# binary to act on a temp conf naming temp paths.
make_tmpfiles_stub() {
    cat > "$STUB_DIR/systemd-tmpfiles" << 'STUB_EOF'
#!/usr/bin/env bash
echo "systemd-tmpfiles $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
    chmod +x "$STUB_DIR/systemd-tmpfiles"
}
make_tmpfiles_stub

# conf_paths <conf> — the paths the conf actually declares, one per line.
# Used by BOTH Block F and Block G so the two can never disagree about how the
# generated file is parsed.
conf_paths() {
    awk '!/^[[:space:]]*#/ && NF { print $2 }' "$1"
}

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

# ── E12-E15: cross-device source — the guard that keeps the design honest ─────
# `cp -al` cannot hardlink across filesystems and GNU cp does NOT fall back to a
# byte copy, so the whole "near-free seed" premise holds only while /tmp and
# $HOME share a device (they do today: both /dev/nvme2n1p5).  This turns that
# measured premise into an ENFORCED precondition, so a future host that mounts
# /tmp as tmpfs degrades loudly instead of silently filling RAM with a 6+ GB
# real copy.  Without these four the guard has no coverage at all in the
# positive direction — it could be silently inverted and every other assertion
# here would stay green.
#
# SKIPPED, not failed, when no second writable filesystem exists: this suite is
# `pool`-classified and must stay hermetic on any host.  /dev/shm is a tmpfs on
# a distinct device wherever it is present at all.
_XDEV_BASE="/dev/shm"
if [ -d "$_XDEV_BASE" ] && [ -w "$_XDEV_BASE" ] \
   && [ "$(stat -c %d "$_XDEV_BASE" 2>/dev/null)" != "$(stat -c %d "$_TMPBASE" 2>/dev/null)" ]; then
    # Source on the OTHER device; destination stays on $_TMPBASE.
    FAKE_HOME="$(mktemp -d "$_XDEV_BASE/test-agent-cache-xdev-home-XXXXXX")"
    _TMPDIRS+=("$FAKE_HOME")
    mkdir -p "$FAKE_HOME/.cargo/registry/cache" "$FAKE_HOME/.cargo/registry/index" \
             "$FAKE_HOME/.npm/_cacache"
    printf 'xdev-crate\n' > "$FAKE_HOME/.cargo/registry/cache/xdev-1.0.0.crate"
    printf 'xdev-index\n' > "$FAKE_HOME/.cargo/registry/index/xdev.json"
    printf 'xdev-blob\n'  > "$FAKE_HOME/.npm/_cacache/blob"
    make_cache_root
    E4_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
    E4_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

    reset_calls
    run_redirect --seed-only

    assert "E12: exits 0 when source and destination are on different filesystems" \
        test "$RC" -eq 0
    assert "E13: warns that the cross-device seed was skipped" \
        bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(different filesystem|cross-device|cross device)"' \
        _ "$OUT" "$ERR_OUT"
    # The whole point: NO silent fallback to a real byte copy.
    assert "E14: no cargo bytes were copied across the device boundary" \
        test ! -e "$E4_CARGO_DEST/registry/cache/xdev-1.0.0.crate"
    assert "E15: no npm bytes were copied across the device boundary" \
        test ! -e "$E4_NPM_DEST/_cacache/blob"
else
    echo "  SKIP: E12-E15 cross-device guard (no second writable filesystem at $_XDEV_BASE)"
fi

# ──────────────────────────────────────────────────────────────────────────────
# Block F — tmpfiles age-clean exclusion (Fix B)
#
# FULL mode (no --seed-only), with REIFY_TMPFILES_DIR at a temp dir and the
# sudo stub exec'ing its remainder so the write really lands there.
#
# The exposure this closes: systemd-tmpfiles-clean.timer runs `--clean`, which
# age-deletes INDIVIDUAL FILES inside /tmp while the orchestrator stays up.
# That per-file deletion is not atomic w.r.t. cargo's own state — it can remove
# files under $CARGO_HOME/registry/src/<crate>/ while leaving .cargo-ok in
# place, after which cargo skips re-extraction and the build fails with a
# missing-source error that looks nothing like a cache problem.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: tmpfiles age-clean exclusion ---"

make_fake_home
make_cache_root
F_CONF="$TMPFILES_DIR/reify-agent-caches.conf"
F_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
F_NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

reset_calls
run_redirect

assert "F1: full-mode run exits 0" test "$RC" -eq 0
assert "F2: reify-agent-caches.conf is created in \$REIFY_TMPFILES_DIR" \
    test -f "$F_CONF"

# EXACTLY two — a count assertion, not a `grep -q` per row.  A renderer that
# appends instead of rewriting passes both row assertions while doubling the
# file on every setup-dev.sh run.
assert "F3: conf has EXACTLY two non-comment lines" \
    bash -c '[ "$(awk "!/^[[:space:]]*#/ && NF" "$1" | wc -l)" -eq 2 ]' _ "$F_CONF"

assert "F4: conf excludes the cargo destination" \
    bash -c 'awk "!/^[[:space:]]*#/ && NF { print \$2 }" "$1" | grep -Fxq -- "$2"' \
    _ "$F_CONF" "$F_CARGO_DEST"
assert "F5: conf excludes the npm destination" \
    bash -c 'awk "!/^[[:space:]]*#/ && NF { print \$2 }" "$1" | grep -Fxq -- "$2"' \
    _ "$F_CONF" "$F_NPM_DEST"

# The verb matters and is easy to get wrong.  Only lowercase `x` excludes a
# directory AND ITS CONTENTS from age-cleaning; `X` excludes the directory
# itself but leaves its contents cleanable — which is precisely the partial-
# gutting failure this block exists to prevent — and `d`/`D` would create or
# empty the tree rather than protect it.
assert "F6: every non-comment line uses the 'x' verb (not X, not d/D)" \
    bash -c '[ "$(awk "!/^[[:space:]]*#/ && NF { print \$1 }" "$1" | sort -u | tr -d "\n")" = "x" ]' \
    _ "$F_CONF"

# F15 — the conf's COMMENT header is generated too, and F3-F6 deliberately
# ignore comment lines, so nothing above can see the renderer misbehave while
# producing it.  Found live: the renderer's heredoc must stay UNQUOTED for
# $CARGO_DEST/$NPM_DEST to expand, which also makes any backtick in the prose a
# COMMAND SUBSTITUTION.  An earlier revision wrote `x` (not `X`) in that prose
# and shipped a conf with those words blanked — after actually executing the
# host's X server.  What F15 detects is that EXECUTION, observed in the run
# output: the defect itself, not the wording of whatever text survived it.
assert "F15: the run emitted no 'command not found' (heredoc prose is not executed)" \
    bash -c '! printf "%s\n%s\n" "$1" "$2" | grep -qiE "command not found|Xorg"' \
    _ "$OUT" "$ERR_OUT"

# Idempotence, and specifically: compare-before-write.  Re-running setup-dev.sh
# must not re-sudo when the conf is already correct.
F_CONF_SNAPSHOT="$(mktemp "$_TMPBASE/test-agent-cache-conf-snap-XXXXXX")"
_TMPDIRS+=("$F_CONF_SNAPSHOT")
cp "$F_CONF" "$F_CONF_SNAPSHOT" 2>/dev/null || true

reset_calls
run_redirect

assert "F7: second full-mode run exits 0 (idempotent)" test "$RC" -eq 0
assert "F8: conf is byte-unchanged by the second run" \
    cmp -s "$F_CONF_SNAPSHOT" "$F_CONF"
assert "F9: second run invoked NO sudo (compares before writing)" \
    bash -c '! grep -q "^sudo " "$1"' _ "$CALLS_FILE"
assert "F10: conf still has exactly two non-comment lines after re-run" \
    bash -c '[ "$(awk "!/^[[:space:]]*#/ && NF" "$1" | wc -l)" -eq 2 ]' _ "$F_CONF"

# Fail-open: a host where sudo is absent or refuses must not fail setup-dev.sh.
#
# The tmpfiles dir is made NON-WRITABLE for this fixture, and that is the whole
# point rather than incidental setup.  The real /etc/tmpfiles.d is not writable
# by the invoking user, so sudo is the production path; against the writable
# temp dir the other F cases use, the script legitimately writes directly and
# never consults sudo at all — so a failing sudo stub alone would assert
# nothing.  This fixture forces the sudo branch and then fails it.
make_sudo_stub 1
make_fake_home
make_cache_root
chmod 500 "$TMPFILES_DIR"

reset_calls
run_redirect
chmod 700 "$TMPFILES_DIR"
make_sudo_stub 0

assert "F11: exits 0 when sudo refuses (fail-open)" test "$RC" -eq 0
assert "F12: warns when the tmpfiles conf could not be written" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(tmpfiles|exclusion|conf)"' \
    _ "$OUT" "$ERR_OUT"
assert "F13: the sudo branch WAS actually taken (fixture is not vacuous)" \
    bash -c 'grep -q "^sudo " "$1"' _ "$CALLS_FILE"
assert "F14: no conf was left behind by the failed write" \
    test ! -e "$TMPFILES_DIR/reify-agent-caches.conf"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — desync guard
#
# The regression that matters most over time: renaming a cache dir on the
# seeding side while leaving the conf's literal stale would silently
# un-protect the LIVE directory from age-cleaning, with no visible symptom
# until a long-uptime build failed.  Both sides are derived from OBSERVED
# behaviour here — the conf is parsed, and the destinations are the dirs the
# seeder actually created — so no hardcoded literal on either side can hide a
# drift between them.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: conf paths ARE the seeder's destinations ---"

make_fake_home
make_cache_root
G_CONF="$TMPFILES_DIR/reify-agent-caches.conf"

reset_calls
run_redirect

assert "G1: full-mode run exits 0" test "$RC" -eq 0

# Observed seeder destinations: the dirs that exist under the cache root after
# a run, NOT a literal.  If the seeder's naming changes, this side moves with it.
assert "G2: the seeder created exactly two destination dirs under the cache root" \
    bash -c '[ "$(find "$1" -mindepth 1 -maxdepth 1 -type d | wc -l)" -eq 2 ]' _ "$CACHE_ROOT"

assert "G3: conf paths == seeder destinations, exactly (no drift, no extras)" \
    bash -c '
        conf_side="$(awk "!/^[[:space:]]*#/ && NF { print \$2 }" "$1" | sort)"
        seed_side="$(find "$2" -mindepth 1 -maxdepth 1 -type d | sort)"
        [ -n "$conf_side" ] && [ "$conf_side" = "$seed_side" ]
    ' _ "$G_CONF" "$CACHE_ROOT"

# ──────────────────────────────────────────────────────────────────────────────
# Block H — boot persistence + mode separation
#
# WHY THE UNIT EXISTS, given Block F already wrote `x` lines: the two cover
# DIFFERENT tmpfiles mechanisms.  /usr/lib/tmpfiles.d/tmp.conf's
# `D /tmp 1777 root root 30d` empties /tmp at every boot via `--remove`, and
# `x` provably does not exclude a path from that — only from the age-based
# `--clean`.  Without this oneshot the pre-seed dies at the first reboot and
# the first dependency-touching agent run pays a full cold start on a live
# task's critical path, silently.
#
# Ordering is guaranteed rather than incidental: user units start after the
# system manager's basic.target, which is ordered after sysinit.target and
# therefore after systemd-tmpfiles-setup.service — so the re-seed provably runs
# AFTER the wipe rather than racing it.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: boot persistence + mode separation ---"

make_fake_home
make_cache_root
H_XDG="$(mktemp -d "$_TMPBASE/test-agent-cache-xdg-XXXXXX")"
_TMPDIRS+=("$H_XDG")
H_UNIT="$H_XDG/systemd/user/reify-agent-caches.service"

_REDIRECT_ENV=("XDG_CONFIG_HOME=$H_XDG")
reset_calls
run_redirect

assert "H1: full-mode run exits 0" test "$RC" -eq 0
assert "H2: unit installed at \$XDG_CONFIG_HOME/systemd/user/reify-agent-caches.service" \
    test -f "$H_UNIT"
assert "H3: unit declares Type=oneshot" \
    bash -c 'grep -q "^Type=oneshot$" "$1"' _ "$H_UNIT"
assert "H4: unit declares RemainAfterExit=yes" \
    bash -c 'grep -q "^RemainAfterExit=yes$" "$1"' _ "$H_UNIT"
assert "H5: unit orders itself Before=orchestrator-reify.service" \
    bash -c 'grep -q "^Before=orchestrator-reify.service$" "$1"' _ "$H_UNIT"
assert "H6: unit declares WantedBy=default.target" \
    bash -c 'grep -q "^WantedBy=default.target$" "$1"' _ "$H_UNIT"

# Nothing may Requires= this unit: a failed seed must degrade to today's
# cold-start behaviour, not block the orchestrator from starting.  Same
# fail-open posture as the warm-lane units.
assert "H7: unit does NOT hard-require anything (fail-open ordering only)" \
    bash -c '! grep -q "^Requires=" "$1"' _ "$H_UNIT"

# THE assertion of this block: ExecStart is the script's own ABSOLUTE path
# followed by --seed-only.  --seed-only is what makes the boot path
# unprivileged — a boot-time unit that tried to sudo from a non-interactive
# --user context would hang or fail on every boot.
assert "H8: ExecStart is this script's absolute path plus --seed-only" \
    bash -c '[ "$(grep "^ExecStart=" "$1" | head -1)" = "ExecStart=$2 --seed-only" ]' \
    _ "$H_UNIT" "$SCRIPT"

assert "H9: systemctl --user daemon-reload was called" \
    bash -c 'grep -q "systemctl --user daemon-reload" "$1"' _ "$CALLS_FILE"
assert "H10: systemctl --user enable naming the unit was called" \
    bash -c 'grep -qE "systemctl --user enable.*reify-agent-caches.service" "$1"' _ "$CALLS_FILE"
assert "H11: daemon-reload precedes enable in call order" \
    bash -c '
        reload_ln=$(grep -n "daemon-reload" "$1" | head -1 | cut -d: -f1)
        enable_ln=$(grep -n "enable.*reify-agent-caches.service" "$1" | head -1 | cut -d: -f1)
        [ -n "$reload_ln" ] && [ -n "$enable_ln" ] && [ "$reload_ln" -lt "$enable_ln" ]
    ' _ "$CALLS_FILE"

# ── fail-open: no systemd --user bus (CI, a container) ────────────────────────
make_fake_home
make_cache_root
H_XDG_NOBUS="$(mktemp -d "$_TMPBASE/test-agent-cache-xdg-nobus-XXXXXX")"
_TMPDIRS+=("$H_XDG_NOBUS")

_REDIRECT_ENV=("XDG_CONFIG_HOME=$H_XDG_NOBUS" "REIFY_TEST_NO_USER_BUS=1")
reset_calls
run_redirect
_REDIRECT_ENV=()

assert "H12: exits 0 with no systemd --user bus (fail-open)" test "$RC" -eq 0
assert "H13: warns about the missing --user bus" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*(bus|systemd|skip)"' \
    _ "$OUT" "$ERR_OUT"
assert "H14: NO daemon-reload attempted without a bus" \
    bash -c '! grep -q "daemon-reload" "$1"' _ "$CALLS_FILE"
assert "H15: NO enable attempted without a bus" \
    bash -c '! grep -q "enable" "$1"' _ "$CALLS_FILE"

# ── mode separation, asserted from the OTHER direction ────────────────────────
# The boot unit depends on --seed-only being genuinely unprivileged: zero sudo,
# zero systemctl, no unit write, no conf write — while still seeding.  Asserting
# it here (rather than trusting the flag) is what makes H8's claim meaningful.
make_fake_home
make_cache_root
H_XDG_SEED="$(mktemp -d "$_TMPBASE/test-agent-cache-xdg-seed-XXXXXX")"
_TMPDIRS+=("$H_XDG_SEED")
H_SEED_CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"

_REDIRECT_ENV=("XDG_CONFIG_HOME=$H_XDG_SEED")
reset_calls
run_redirect --seed-only
_REDIRECT_ENV=()

assert "H16: --seed-only exits 0" test "$RC" -eq 0
assert "H17: --seed-only invoked ZERO sudo" \
    bash -c '! grep -q "^sudo " "$1"' _ "$CALLS_FILE"
assert "H18: --seed-only invoked ZERO systemctl" \
    bash -c '! grep -q "^systemctl " "$1"' _ "$CALLS_FILE"
assert "H19: --seed-only wrote no systemd unit" \
    test ! -e "$H_XDG_SEED/systemd/user/reify-agent-caches.service"
assert "H20: --seed-only wrote no tmpfiles conf" \
    test ! -e "$TMPFILES_DIR/reify-agent-caches.conf"
assert "H21: --seed-only DID still seed (the mode is not a no-op)" \
    test -f "$H_SEED_CARGO_DEST/registry/cache/alpha-1.0.0.crate"

# ──────────────────────────────────────────────────────────────────────────────
# Block I — setup-dev.sh wiring
#
# setup-dev.sh cannot be run in a test (it apts, installs rustup/micromamba,
# creates /opt/reify-deps and does a full release build), so this block is a
# structural grep — but a careful one.  It uses the two-step uncommented-line
# filter from test_setup_dev_no_ldconfig.sh, because every rationale comment
# this task adds to setup-dev.sh MENTIONS the script by name: a naive
# `grep -q setup-agent-cache-redirect.sh` would stay green even if the actual
# invocation were deleted.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: setup-dev.sh wiring ---"

assert "I1: setup-dev.sh exists and is executable" \
    bash -c 'test -f "$1" && test -x "$1"' _ "$SETUP_DEV"

assert "I2: an UNCOMMENTED line in setup-dev.sh invokes setup-agent-cache-redirect.sh" \
    bash -c "grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'setup-agent-cache-redirect.sh'"

# The referenced path must actually resolve, so the wiring can never point at a
# renamed or deleted file — a class of rot no mention-only grep can see.
assert "I3: the referenced script exists and is executable" \
    bash -c 'test -f "$1" && test -x "$1"' _ "$SCRIPT"

# Unconditional, per the recorded design decision: the ticket's goal for Fix B
# is that "a rebuilt host does not silently lose the exclusion", and an opt-in
# env var is precisely the thing a rebuilt host silently forgets to set.  The
# warm-lane opt-in exists because that step formats and mounts a 4096 GiB
# loopback volume; this one creates two directories and a two-line conf.
assert "I4: the invocation is NOT gated behind a REIFY_PROVISION_* opt-in" \
    bash -c '
        ln=$(grep -Ev "^[[:space:]]*#" "$1" | grep -n "setup-agent-cache-redirect.sh" | head -1 | cut -d: -f1)
        [ -n "$ln" ] || exit 1
        # Look back over the preceding uncommented lines for an unclosed
        # REIFY_PROVISION_* gate: an `if ... REIFY_PROVISION_*` with no `fi`
        # between it and the invocation would mean we sit inside that branch.
        gate=$(grep -Ev "^[[:space:]]*#" "$1" | head -n "$ln" \
               | grep -nE "^[[:space:]]*if .*REIFY_PROVISION_" | tail -1 | cut -d: -f1)
        [ -n "$gate" ] || exit 0
        closed=$(grep -Ev "^[[:space:]]*#" "$1" | sed -n "$((gate + 1)),$((ln - 1))p" \
                 | grep -cE "^fi$")
        [ "$closed" -ge 1 ]
    ' _ "$SETUP_DEV"

# setup-dev.sh runs under `set -euo pipefail`, so a BARE invocation whose exit
# is non-zero aborts a contributor's entire environment setup mid-way — before
# the cargo-nextest install, the npm ci and the smoke test.  This script is
# fail-open internally, but the call site must be structurally non-fatal too,
# and that is invisible to any assertion that only checks the script is
# mentioned.  Matches the E3 idiom in test_warm_lane_boot_persistence.sh.
assert "I5: the invocation is non-fatal (if/else+warn, no bare exit 1)" \
    bash -c '
        block=$(grep -A8 "setup-agent-cache-redirect.sh" "$1" | grep -Ev "^[[:space:]]*#")
        echo "$block" | grep -q "else" || exit 1
        echo "$block" | grep -q "warn" || exit 1
        ! echo "$block" | grep -qE "^[[:space:]]*exit[[:space:]]+1[[:space:]]*$" || exit 1
        exit 0
    ' _ "$SETUP_DEV"

# Regression guards: the sibling delegated steps this section is placed among
# must survive the edit.
assert "I6: setup-dev.sh still references build-manifold-deps.sh (regression guard)" \
    bash -c "grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'build-manifold-deps.sh'"
assert "I7: setup-dev.sh still references install-warm-lane-units.sh (regression guard)" \
    bash -c "grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'install-warm-lane-units.sh'"

test_summary
