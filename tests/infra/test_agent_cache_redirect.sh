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
#   B — hardlink pre-seed (Fix A): seeded set, byte-identity, INODE identity,
#       and the accepted metadata write-through residue on the link-mode trees
#   C — idempotence + top-up across repeat runs
#   D — degenerate self-copy guard (script running under its own redirect)
#   E — fail-open on absent/unusable sources
#   F — tmpfiles age-clean exclusion (Fix B): content, idempotence, fail-open
#   G — desync guard: the conf's paths ARE the seeder's destinations, and the
#       PRODUCTION defaults ARE dark-factory-orchestrator.yaml's role_env_overrides
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

# ── generated-conf accessors ──────────────────────────────────────────────────
# One definition of "what a non-comment line is", shared by every assertion in
# Blocks F and G that looks inside the generated conf.  These are called from
# the MAIN shell at argument-expansion time (`assert ... "$(conf_paths "$C")"`)
# rather than from inside an `assert ... bash -c '...'` body, because a shell
# function is not visible to a `bash -c` subshell.
conf_paths() {
    awk '!/^[[:space:]]*#/ && NF { print $2 }' "$1"
}
conf_verbs() {
    awk '!/^[[:space:]]*#/ && NF { print $1 }' "$1" | sort -u
}
conf_body_line_count() {
    awk '!/^[[:space:]]*#/ && NF { n++ } END { print n + 0 }' "$1"
}

# ── fake $HOME with synthetic cargo + npm cache trees ─────────────────────────
# Sets the FAKE_HOME global (rather than echoing it) because the _TMPDIRS
# registration must land in the MAIN shell: a `_TMPDIRS+=(...)` performed
# inside a `$(...)` subshell is discarded when that subshell exits, leaking the
# tree for the whole run.  Same reason test_helpers.sh's make_isolated_lane
# keeps its registration out of the subshell.
#
# `registry/src`, `_npx` and `_cacache/tmp` are populated ON PURPOSE: they are
# the trees the seeder must NOT copy (registry/src is derived from the .crate
# tarballs in registry/cache; the other two are churn, not build inputs), so
# their presence at the source is what gives the "absent at the destination"
# assertions their teeth.
#
# The tree layout mirrors the two SEED MODES the script distinguishes, and the
# distinction is a containment property, not an optimisation:
#   link (immutable)      registry/cache/*.crate, _cacache/content-v2/**
#   copy (mutated in place) registry/index/**/.cache/**, _cacache/index-v5/**
# Both copy-mode trees are reproduced here in their real on-disk shape (cargo's
# sparse-index .cache/<a>/<b>/<crate> files, which cargo rewrites in place; a
# cacache index-v5 bucket, which cacache mutates by APPENDING an entry line) so
# the inode assertions below are asserting about the paths that actually carry
# the hazard rather than about stand-ins.
FAKE_HOME=""
make_fake_home() {
    FAKE_HOME="$(mktemp -d "$_TMPBASE/test-agent-cache-home-XXXXXX")"
    _TMPDIRS+=("$FAKE_HOME")
    mkdir -p "$FAKE_HOME/.cargo/registry/cache" \
             "$FAKE_HOME/.cargo/registry/index/index.crates.io-1949cf8c/.cache/al/ph" \
             "$FAKE_HOME/.cargo/registry/src/github.com-1ecc/alpha-1.0.0" \
             "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab" \
             "$FAKE_HOME/.npm/_cacache/index-v5/5c/05" \
             "$FAKE_HOME/.npm/_cacache/tmp" \
             "$FAKE_HOME/.npm/_npx"
    printf 'crate-tarball-alpha\n' > "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate"
    printf 'crate-tarball-beta\n'  > "$FAKE_HOME/.cargo/registry/cache/beta-2.0.0.crate"
    printf 'index-entry-alpha\n'   > "$FAKE_HOME/.cargo/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha"
    printf 'extracted-lib-rs\n'    > "$FAKE_HOME/.cargo/registry/src/github.com-1ecc/alpha-1.0.0/lib.rs"
    printf 'cacache-blob\n'        > "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob"
    printf '\ndeadbeef\t{"key":"alpha"}\n' > "$FAKE_HOME/.npm/_cacache/index-v5/5c/05/bucket"
    printf 'cacache-scratch\n'     > "$FAKE_HOME/.npm/_cacache/tmp/scratch"
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
# passed straight through to `env`, and appear AFTER these defaults so they
# override them).
#
# REIFY_MAIN_CHECKOUT is pinned at a NONEXISTENT path for the same reason: it
# defaults to /home/leo/src/reify, so once this task lands, the unit installed
# by an unpinned run would resolve to the real main checkout and this suite's
# behaviour would silently change on the landing commit.  Block H sets it
# explicitly for both the stable-path and the fallback case.
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
            REIFY_MAIN_CHECKOUT="$STUB_DIR/no-such-main-checkout" \
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

# distinct_inode <a> <b> — the mirror probe, for the copy-mode trees.  NOT
# spelled `! same_inode`: BOTH files must exist and differ, so a regression that
# skipped the tree entirely (no destination file at all) FAILS here rather than
# satisfying a negation vacuously.
distinct_inode() {
    local a b
    a="$(stat -c %i "$1" 2>/dev/null)" || return 1
    b="$(stat -c %i "$2" 2>/dev/null)" || return 1
    [ -n "$a" ] && [ -n "$b" ] && [ "$a" != "$b" ]
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
assert "B7: sparse-index .cache entry present at destination" \
    test -f "$B_CARGO_DEST/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha"
assert "B8: nested _cacache content-v2 blob present at destination" \
    test -f "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"
assert "B8b: _cacache index-v5 bucket present at destination" \
    test -f "$B_NPM_DEST/_cacache/index-v5/5c/05/bucket"

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
assert "B12b: _cacache/tmp is NOT seeded (cacache scratch, not a build input)" \
    test ! -e "$B_NPM_DEST/_cacache/tmp"

# ── the load-bearing assertions: inode identity, in BOTH directions ───────────
# Inode identity is what distinguishes the near-free `cp -al` hardlink from a
# real byte copy — and the correct answer differs per tree, so asserting only
# one direction would license the wrong mechanism on half the seed.
#
# B13/B15 (link mode, MUST share): without them a plain `cp -a` implementation
# is indistinguishable from the intended one on every other assertion here,
# while silently doubling the host's cache footprint on every boot.
#
# B14/B16 (copy mode, MUST NOT share) are the sandbox-containment assertions.
# A shared inode on a tree that is mutated IN PLACE — cargo rewrites its
# sparse-index .cache files on every index revalidation; cacache APPENDS to its
# index-v5 buckets — means a landlock-sandboxed agent writing under /tmp writes
# THROUGH to the host's ~/.cargo / ~/.npm, outside the write set the sandbox
# exists to enforce.  A `cp -al` regression on these two trees passes every
# other assertion in this block and is invisible until it has already escaped
# the sandbox.
assert "B13: seeded .crate SHARES AN INODE with its source (link mode)" \
    same_inode "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate" \
               "$B_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "B14: sparse-index .cache entry does NOT share an inode (copy mode: cargo rewrites it in place)" \
    distinct_inode "$FAKE_HOME/.cargo/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha" \
                   "$B_CARGO_DEST/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha"
assert "B15: seeded _cacache content blob SHARES AN INODE with its source (link mode)" \
    same_inode "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob" \
               "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"
assert "B16: _cacache index-v5 bucket does NOT share an inode (copy mode: cacache appends to it)" \
    distinct_inode "$FAKE_HOME/.npm/_cacache/index-v5/5c/05/bucket" \
                   "$B_NPM_DEST/_cacache/index-v5/5c/05/bucket"

# Copy mode must still be a faithful copy — a "not shared" that was reached by
# not copying at all would satisfy B14/B16 vacuously.
assert "B17: the copied index entry is byte-identical to its source" \
    cmp -s "$FAKE_HOME/.cargo/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha" \
           "$B_CARGO_DEST/registry/index/index.crates.io-1949cf8c/.cache/al/ph/alpha"
assert "B18: the copied index-v5 bucket is byte-identical to its source" \
    cmp -s "$FAKE_HOME/.npm/_cacache/index-v5/5c/05/bucket" \
           "$B_NPM_DEST/_cacache/index-v5/5c/05/bucket"

# The end-to-end containment property, asserted by observation rather than by
# reasoning about inodes: mutate the seeded copy the way the real tool would
# (cacache appends an entry line to a bucket) and confirm the host's cache is
# untouched.  This is the assertion that would have caught the original
# `cp -al` seed of index-v5, stated in the terms the sandbox actually cares
# about.
printf 'cafebabe\t{"key":"beta"}\n' >> "$B_NPM_DEST/_cacache/index-v5/5c/05/bucket"
assert "B19: appending to the seeded bucket does NOT write through to the host cache" \
    bash -c '! grep -q "cafebabe" "$1"' _ "$FAKE_HOME/.npm/_cacache/index-v5/5c/05/bucket"

# The OK line must carry a COUNT, and the count must be nonzero.  Paired with
# E4b/E4c below: every guard in seed_one is fail-open, so "exit 0" alone says
# nothing about whether anything was actually seeded.
assert "B20: a real seed reports OK with a nonzero seeded count" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qE "OK:.*seed pass complete: [1-9]"' \
    _ "$OUT" "$ERR_OUT"

# ── B21/B22: the ACCEPTED RESIDUE on the link-mode trees, pinned ──────────────
# B19 asserts containment for a COPY-mode tree, and states it in terms of file
# CONTENT.  A hardlink shares the whole INODE though, so the link-mode trees
# leak a second channel that no assertion above can see: mode, owner and mtime.
# A sandboxed agent that chmods or touches a seeded .crate or content-v2 blob
# changes it in the host's ~/.cargo / ~/.npm too — outside the landlock write
# set, exactly like an in-place content write would.
#
# These two assert that the leak HAPPENS.  That is deliberate and is the point:
# it is a knowingly accepted trade (see the ACCEPTED RESIDUE paragraph in the
# script's seed-sources note — the alternative is copying 4.8G of content-v2 at
# every boot), and an accepted boundary that lives only in prose is one nobody
# can tell has moved.  Written this way the residue is a characterised fact, and
# a future switch of either tree to copy mode turns these RED — which is the
# correct outcome: it is a real behaviour change, and it should be noticed and
# re-costed rather than absorbed silently.
#
# Asserted on the seeded destination's SOURCE, i.e. the direction that matters:
# the write is issued under the redirect root and observed in the host cache.
chmod 0600 "$B_CARGO_DEST/registry/cache/alpha-1.0.0.crate"
assert "B21: chmod on a link-mode seeded .crate DOES reach the host cache (accepted inode-sharing residue)" \
    bash -c '[ "$(stat -c %a "$1")" = "600" ]' \
    _ "$FAKE_HOME/.cargo/registry/cache/alpha-1.0.0.crate"

touch -d '2001-02-03 04:05:06' "$B_NPM_DEST/_cacache/content-v2/sha512/ab/blob"
assert "B22: touch on a link-mode seeded blob DOES reach the host cache (same residue, mtime channel)" \
    bash -c '[ "$(stat -c %Y "$1")" = "$(date -d "2001-02-03 04:05:06" +%s)" ]' \
    _ "$FAKE_HOME/.npm/_cacache/content-v2/sha512/ab/blob"

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
# Matched on GUARD 2(a)'s own wording.  A `skip` alternative would match any
# warn at all (every one of them ends in "skipping"), so this would stay green
# with the equality branch deleted and an unrelated guard firing instead.
assert "D2: warns specifically that the destination IS the source" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*destination is the source itself"' \
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
# This is the ONLY assertion in the suite that observes GUARD 2(b), so it has
# to name that branch's own wording: with `degenerate|itself|skip` in the
# alternation it matched the equality branch — or any other warn — and would
# have passed with the nesting branch deleted outright.
assert "D6: warns specifically that the destination is NESTED INSIDE the source" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*destination is nested inside the source"' \
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
# Matched on the wording GUARD 1 actually emits, not on a generic "skip":
# every warn in the script ends in "skipping", so a `skip` alternative would
# match ANY warn and this assertion would pass even if the missing-source
# branch were deleted and some unrelated guard fired instead.
assert "E2: warns specifically about the missing/unreadable seed source" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*seed source missing or unreadable"' \
    _ "$OUT" "$ERR_OUT"
# No partial tree: a `mkdir -p` performed before checking the source would leave
# an empty destination that looks seeded to a later reader (and to `test -d`).
assert "E3: leaves no partial cargo destination tree behind" \
    test ! -e "$E_CARGO_DEST/registry/cache"
assert "E4: leaves no partial npm destination tree behind" \
    test ! -e "$E_NPM_DEST/_cacache"

# ── E4b/E4c: no SILENT GREEN when nothing was seeded ──────────────────────────
# Every guard in seed_one is fail-open, so an all-skipped run still exits 0.
# Reporting "OK: seed pass complete" there is the failure this script exists to
# close, one level up: on a host where /tmp was remounted onto another device,
# `systemctl --user status reify-agent-caches.service` would show a clean
# oneshot whose last line reads OK while every agent run cold-starts forever.
assert "E4b: an all-skipped run WARNS that nothing was seeded" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*NOTHING was seeded"' \
    _ "$OUT" "$ERR_OUT"
assert "E4c: an all-skipped run does NOT report a successful seed pass" \
    bash -c '! printf "%s\n%s\n" "$1" "$2" | grep -qE "OK:.*seed pass complete"' \
    _ "$OUT" "$ERR_OUT"

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
    test "$(conf_body_line_count "$F_CONF")" -eq 2

assert "F4: conf excludes the cargo destination" \
    bash -c 'printf "%s\n" "$1" | grep -Fxq -- "$2"' \
    _ "$(conf_paths "$F_CONF")" "$F_CARGO_DEST"
assert "F5: conf excludes the npm destination" \
    bash -c 'printf "%s\n" "$1" | grep -Fxq -- "$2"' \
    _ "$(conf_paths "$F_CONF")" "$F_NPM_DEST"

# The verb matters and is easy to get wrong.  Only lowercase `x` excludes a
# directory AND ITS CONTENTS from age-cleaning; `X` excludes the directory
# itself but leaves its contents cleanable — which is precisely the partial-
# gutting failure this block exists to prevent — and `d`/`D` would create or
# empty the tree rather than protect it.
assert "F6: every non-comment line uses the 'x' verb (not X, not d/D)" \
    test "$(conf_verbs "$F_CONF")" = "x"

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

# F16 — no apply step.  `x` is IGNORE_PATH: it creates nothing, and the timer's
# `--clean` pass re-reads /etc/tmpfiles.d on every invocation, so writing the
# conf IS the activation.  An earlier revision ran `systemd-tmpfiles --create`
# here "so the exclusion takes effect now": a no-op that ALSO fired a second
# sudo immediately after the install on the production path (/etc/tmpfiles.d is
# not user-writable), defeating the compare-before-write check F9 pins.
#
# Asserted against the run that ACTUALLY WROTE the conf — i.e. here, in the
# first-run group, not after the idempotent second run.  install_tmpfiles_
# exclusion() returns early when the conf is already current, so the same
# assertion placed after F7-F10 would pass on a script that still ran the apply
# step: it would never have reached it.  The stub records argv without acting,
# so this asserts on the CALL, not on its effect.
assert "F16: the run invoked NO systemd-tmpfiles (writing the conf is the activation)" \
    bash -c '! grep -q "^systemd-tmpfiles " "$1"' _ "$CALLS_FILE"

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
    test "$(conf_body_line_count "$F_CONF")" -eq 2

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
    bash -c '[ -n "$1" ] && [ "$1" = "$2" ]' \
    _ "$(conf_paths "$G_CONF" | sort)" \
      "$(find "$CACHE_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)"

# ── G4-G9: the CROSS-FILE half of the desync guard ────────────────────────────
# G1-G3 close the INTRA-script desync only, and they close it by construction:
# both sides are derived from the same CACHE_ROOT the harness injects, so they
# cannot disagree even if the constant itself is wrong.  The constant that is
# actually load-bearing is the PRODUCTION default — `/tmp` plus the two
# basenames — because it must equal what dark-factory-orchestrator.yaml's
# role_env_overrides points a sandboxed agent's CARGO_HOME / npm_config_cache
# at.  Every other assertion in this suite runs with REIFY_AGENT_CACHE_ROOT
# pointed at a temp dir, so before these, renaming either side left all of them
# green while the seeder deposited ~1G at a path no agent would ever read: the
# seeder reporting "4 of 4 source pairs seeded" IS the silent failure.
#
# `--print-paths` exists for this: it resolves the same constants and exits
# without touching anything, so the production defaults can be observed without
# a test run writing into the real /tmp redirect.
#
# The yaml side is parsed with PyYAML rather than grepped — a grep for
# `/tmp/reify-agent-cargo-home` would match the long rationale comment above the
# block and stay green after the value itself changed.  The sibling suite
# tests/infra/test_sandbox_cache_writability_seam.sh has an equivalent parser,
# but it is inline in that file rather than a shared helper, and that file is
# outside this task's locked modules; extracting it into test_helpers.sh is the
# right cleanup and is left as a follow-up rather than done from here.
G_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
# Hardcoded copy of the sibling suite's CACHE_REDIRECT_ROLES
# (tests/infra/test_sandbox_cache_writability_seam.sh) — since task 5858 that
# set is no longer "roles DF sandboxes today" (roles.py sandboxed=True), it is
# "roles reify's yaml redirects ahead of DF sandboxing them": implementer,
# debugger and simple_task are sandboxed today; architect,
# reviewer_comprehensive, judge and merger are redirected pre-emptively per
# the sibling suite's header (merger added by #6275, which re-evaluated an
# exclusion whose rationale task 5836 had inverted). Not reify-observable, so
# a change to either side's copy needs a manual update in both places — and
# note that is now doubly true: this is the THIRD hand-maintained mirror of
# the set, after the sibling suite's CACHE_REDIRECT_ROLES and the yaml's
# role_env_overrides, and this enumeration is the copy that silently went
# stale when merger was added.
#
# That last sentence used to be the ONLY thing holding this copy in sync.
# G10-G11 near the end of this block now pin it against the sibling's
# declaration as an order-insensitive set, so forgetting this line is a red
# suite rather than a silent gap. The literal is deliberately kept (not
# derived from the sibling) so the comparison is between two independent
# statements; see the G10-G11 header for that argument and for what the pin
# still cannot observe.
G_CACHE_REDIRECT_ROLES="implementer debugger simple_task architect reviewer_comprehensive judge merger"

# REIFY_AGENT_CACHE_ROOT is UNSET here, unlike in run_redirect: observing the
# `/tmp` default is the entire point of these assertions.
G_PRINT="$(env -u CARGO_HOME -u npm_config_cache -u REIFY_AGENT_CACHE_ROOT \
               -u REIFY_TMPFILES_DIR -u XDG_CONFIG_HOME \
               PATH="$STUB_DIR:$PATH" bash "$SCRIPT" --print-paths 2>/dev/null)" || true
G_CARGO_DEFAULT="$(printf '%s\n' "$G_PRINT" | sed -n 's/^CARGO_DEST=//p')"
G_NPM_DEFAULT="$(printf '%s\n' "$G_PRINT" | sed -n 's/^NPM_DEST=//p')"

assert "G4: --print-paths reports a CARGO_DEST default" test -n "$G_CARGO_DEFAULT"
assert "G5: --print-paths reports an NPM_DEST default" test -n "$G_NPM_DEFAULT"
# Non-vacuity: the two must differ, or a single wrong constant would satisfy
# both role comparisons below at once.
assert "G6: the two defaults are distinct paths" \
    bash -c '[ "$1" != "$2" ]' _ "$G_CARGO_DEFAULT" "$G_NPM_DEFAULT"

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available — skipping the yaml cross-check (G7-G9)"
elif [ ! -f "$G_YAML" ]; then
    echo "SKIP: $G_YAML not found — skipping the yaml cross-check (G7-G9)"
else
    _G_PARSE_PY="$(mktemp "$_TMPBASE/test-agent-cache-yaml-XXXXXX.py")"
    _TMPDIRS+=("$_G_PARSE_PY")
    cat > "$_G_PARSE_PY" << 'PYEOF'
"""Read dark-factory-orchestrator.yaml's landlock cache-redirect contract.

Usage:
    <yaml> sandbox_enabled            -- exit 0 iff sandbox.enabled is true
    <yaml> role_key <role> <key>      -- print role_env_overrides[role][key],
                                         exit 1 if absent/empty
"""
import sys

import yaml

with open(sys.argv[1]) as fh:
    doc = yaml.safe_load(fh) or {}
check = sys.argv[2]

if check == "sandbox_enabled":
    sandbox = doc.get("sandbox")
    sys.exit(0 if isinstance(sandbox, dict) and sandbox.get("enabled") is True else 1)

if check == "role_key":
    role, key = sys.argv[3], sys.argv[4]
    overrides = doc.get("role_env_overrides")
    if not isinstance(overrides, dict):
        sys.exit(1)
    entry = overrides.get(role)
    if not isinstance(entry, dict):
        sys.exit(1)
    value = entry.get(key)
    if not isinstance(value, str) or not value:
        sys.exit(1)
    print(value)
    sys.exit(0)

print(f"unknown check: {check}", file=sys.stderr)
sys.exit(2)
PYEOF

    # Gated on sandbox.enabled, mirroring the sibling suite: with the sandbox
    # off there is no redirect for the defaults to agree WITH, so asserting
    # would make this suite red for a decision taken elsewhere.  With it on,
    # the coupling is live and these are hard assertions.
    if python3 "$_G_PARSE_PY" "$G_YAML" sandbox_enabled; then
        echo "sandbox.enabled == true; cross-checking the seeder's defaults against role_env_overrides ($G_CACHE_REDIRECT_ROLES)"
        for _g_role in $G_CACHE_REDIRECT_ROLES; do
            _g_yaml_cargo="$(python3 "$_G_PARSE_PY" "$G_YAML" role_key "$_g_role" CARGO_HOME)" || _g_yaml_cargo=""
            _g_yaml_npm="$(python3 "$_G_PARSE_PY" "$G_YAML" role_key "$_g_role" npm_config_cache)" || _g_yaml_npm=""

            assert "G7[$_g_role]: role_env_overrides.$_g_role defines both CARGO_HOME and npm_config_cache" \
                bash -c '[ -n "$1" ] && [ -n "$2" ]' _ "$_g_yaml_cargo" "$_g_yaml_npm"
            assert "G8[$_g_role]: the seeder's default CARGO_DEST == role_env_overrides.$_g_role.CARGO_HOME" \
                bash -c '[ "$1" = "$2" ]' _ "$G_CARGO_DEFAULT" "$_g_yaml_cargo"
            assert "G9[$_g_role]: the seeder's default NPM_DEST == role_env_overrides.$_g_role.npm_config_cache" \
                bash -c '[ "$1" = "$2" ]' _ "$G_NPM_DEFAULT" "$_g_yaml_npm"
        done
    else
        echo "SKIP: sandbox.enabled is not true — the redirect is not live, skipping G7-G9"
    fi
fi

# ──────────────────────────────────────────────────────────────────────────────
# G10-G11 — CROSS-FILE MIRROR PIN (#6275 review amendment)
#
# G_CACHE_REDIRECT_ROLES above is the THIRD hand-maintained copy of the
# redirect set, and it is the copy that silently went stale when `merger` was
# added.  Nothing mechanically tied it to the other two: the sibling suite's
# (C) complement pin lives INSIDE test_sandbox_cache_writability_seam.sh and
# never reads this file, and G7-G9 above simply iterate whatever this list
# happens to hold.  So a role added to the yaml AND to the sibling but
# forgotten HERE leaves both suites green while that role's
# seeder-default-vs-yaml agreement is never asserted at all — the exact
# failure mode #6275 was filed to repair, one file over, still open after the
# intra-file pin landed.  G10-G11 close it.
#
# WHY THE LITERAL IS KEPT rather than derived from the sibling by the same
# sed (the alternative the review offered): deriving makes this file
# vacuously agree with whatever the sibling says, including a wrong value,
# and erases the independent second statement that is the only thing a
# two-copy comparison can be evidence OF.  Comparing two independent literals
# is strictly stronger than deriving one from the other; the cost is that an
# intentional role change must be typed in both places, which is the point.
#
# Deliberately OUTSIDE the PyYAML / sandbox.enabled gate above — same posture
# as the sibling's block (C) ("plain bash, always runs, does not read the yaml
# at all").  A drifted mirror must go red on a host without PyYAML too.
#
# WHAT THIS PIN CANNOT OBSERVE, stated so it is not read as more coverage than
# it has: it pins reify's two TEST-SIDE copies against each other.  It does
# NOT pin either against the yaml's role_env_overrides keys (G7-G9 do that,
# but only for roles this list already names, so a role missing from BOTH
# mirrors is still invisible here), and it does NOT read DF's roles.py.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- G10-G11: cross-file redirect-role mirror pin ---"

G_SEAM_SUITE="$REPO_ROOT/tests/infra/test_sandbox_cache_writability_seam.sh"
G_SEAM_ROLES="$(sed -n 's/^CACHE_REDIRECT_ROLES="\([^"]*\)"$/\1/p' "$G_SEAM_SUITE" 2>/dev/null || true)"

# Non-vacuity guard for G11: if the sibling's variable is renamed, reshaped
# (say into an array), or gains a second declaration, the extraction stops
# meaning what G11 assumes and G11 would otherwise compare against an empty
# or ambiguous string.  Fail LOUD here instead of silently weakening G11.
assert "G10: the sibling suite declares CACHE_REDIRECT_ROLES exactly once and it is extractable (a rename/reshape there goes RED here rather than vacuously green)" \
    bash -c '
        [ -f "$1" ] || { echo "sibling suite not found: $1" >&2; exit 1; }
        [ -n "$2" ] || { echo "no ^CACHE_REDIRECT_ROLES=\"...\" declaration found in $1 -- renamed or reshaped?" >&2; exit 1; }
        n=$(printf "%s\n" "$2" | wc -l)
        [ "$n" -eq 1 ] || { echo "expected exactly 1 CACHE_REDIRECT_ROLES declaration in $1, found $n" >&2; exit 1; }
    ' _ "$G_SEAM_SUITE" "$G_SEAM_ROLES"

# Order-insensitive on BOTH sides (sorted token streams, same idiom as the
# sibling's (C) complement assertion), so merely reordering either literal is
# not a spurious FAIL.  The diagnostics name the drifted role in whichever
# direction it drifted, so the failure says what to edit.
assert "G11: this file's G_CACHE_REDIRECT_ROLES == the sibling suite's CACHE_REDIRECT_ROLES as an ORDER-INSENSITIVE set -- the cross-file drift that let merger go stale in this copy can no longer stay green" \
    bash -c '
        l_sorted=$(printf "%s\n" $1 | sort | tr "\n" " ")
        r_sorted=$(printf "%s\n" $2 | sort | tr "\n" " ")
        [ "$l_sorted" = "$r_sorted" ] && exit 0
        echo "the two hand-maintained redirect-role mirrors drifted" >&2
        echo "  here    (G_CACHE_REDIRECT_ROLES): $l_sorted" >&2
        echo "  sibling (CACHE_REDIRECT_ROLES):   $r_sorted" >&2
        for r in $1; do
            case " $2 " in
                *" $r "*) ;;
                *) echo "  named here but NOT in the sibling: $r" >&2 ;;
            esac
        done
        for r in $2; do
            case " $1 " in
                *" $r "*) ;;
                *) echo "  in the sibling but NOT named here: $r" >&2 ;;
            esac
        done
        exit 1
    ' _ "$G_CACHE_REDIRECT_ROLES" "$G_SEAM_ROLES"

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

# A stand-in for the stable main checkout, holding its own copy of the script
# at the canonical scripts/ path.  The distinction it makes observable: this
# path is NOT the one the suite invokes ($SCRIPT, a warm-lane worktree path),
# so H8/H8b can tell "pinned to the stable checkout" apart from "pinned to
# whatever ran setup-dev.sh".
H_MAIN="$(mktemp -d "$_TMPBASE/test-agent-cache-main-XXXXXX")"
_TMPDIRS+=("$H_MAIN")
mkdir -p "$H_MAIN/scripts"
cp "$SCRIPT" "$H_MAIN/scripts/setup-agent-cache-redirect.sh"
chmod +x "$H_MAIN/scripts/setup-agent-cache-redirect.sh"
H_STABLE="$H_MAIN/scripts/setup-agent-cache-redirect.sh"

_REDIRECT_ENV=("XDG_CONFIG_HOME=$H_XDG" "REIFY_MAIN_CHECKOUT=$H_MAIN")
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

# THE assertion of this block: ExecStart is the STABLE MAIN-CHECKOUT absolute
# path plus --seed-only.
#
# --seed-only is what makes the boot path unprivileged — a boot-time unit that
# tried to sudo from a non-interactive --user context would hang or fail on
# every boot.
#
# The stable path is the other half, and H8b is what gives it teeth.
# setup-dev.sh is routinely run from a warm-lane worktree, so a
# ${BASH_SOURCE[0]}-derived ExecStart bakes in a path that disappears the next
# time that lane is reclaimed — after which the oneshot fails 203/EXEC at every
# boot and, because nothing Requires= it, nothing ever says so.  Host
# convention, matching the inline comments on deploy/systemd/
# reify-warm-lane.service and reify-warm-lane-gc.service.
assert "H8: ExecStart is the STABLE main-checkout path plus --seed-only" \
    bash -c '[ "$(grep "^ExecStart=" "$1" | head -1)" = "ExecStart=$2 --seed-only" ]' \
    _ "$H_UNIT" "$H_STABLE"
assert "H8b: ExecStart is NOT the invoking (warm-lane worktree) copy" \
    bash -c '! grep -qF -- "ExecStart=$2 " "$1"' _ "$H_UNIT" "$SCRIPT"
assert "H8c: Documentation= also points at the stable path, not the invoking copy" \
    bash -c 'grep -qF -- "Documentation=file://$2" "$1"' _ "$H_UNIT" "$H_STABLE"

# ── H8d-H8f: STATIC pins on the migration onto scripts/lib_main_checkout.sh ────
# These three are greps rather than behavioural runs, and that is forced rather
# than lazy: every behavioural case in this block pins REIFY_MAIN_CHECKOUT
# explicitly (run_redirect hardcodes it, H8 and H11b override it), so layer 1 of
# reify_main_checkout short-circuits the git derivation and NO run in THIS suite
# can observe which resolver produced the answer.  The behavioural
# discrimination — the derivation running with REIFY_MAIN_CHECKOUT UNSET, from a
# real linked worktree — lives in tests/infra/test_host_global_unit_pinning.sh
# Part C.  What these pin is that the script's private hardcoded resolver is
# gone and the shared lib is what replaced it (task 6864).
#
# The uncommented-line filter is load-bearing, not pedantry: the migration adds
# rationale comments that name the lib, so a naive `grep -q lib_main_checkout.sh`
# would stay green with the hardcoded resolver still in place — the exact failure
# mode Block I's banner documents for setup-dev.sh.
assert "H8d: an UNCOMMENTED line sources scripts/lib_main_checkout.sh" \
    bash -c "grep -Ev '^[[:space:]]*#' \"$SCRIPT\" | grep -qE '(^|[[:space:]])(\.|source)[[:space:]].*lib_main_checkout\.sh'"

# What makes the migration irreversible: restoring
# `${REIFY_MAIN_CHECKOUT:-/home/leo/src/reify}` reds this.  Scoped to uncommented
# lines so the header prose may still name the host path in passing.
assert "H8e: NO uncommented line hardcodes the /home/leo/src/reify main checkout" \
    bash -c "! grep -Ev '^[[:space:]]*#' \"$SCRIPT\" | grep -q '/home/leo/src/reify'"

# NON-VACUITY for H8d: a lib deleted or renamed out from under that source line
# would leave H8d certifying a path that resolves to nothing.
assert "H8f: the sourced lib exists and is readable" \
    bash -c 'test -f "$1" && test -r "$1"' _ "$REPO_ROOT/scripts/lib_main_checkout.sh"

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

# ── H11b-H11d: fallback when this is NOT the main checkout ────────────────────
# A contributor clone (or this host before the script has landed on main) has
# no executable at the configured stable path.  Pinning the unit there anyway
# would install a unit that fails 203/EXEC at every boot, so the script falls
# back to the invoking copy — and must SAY so, because that unit now carries
# exactly the fragile-path property H8b rejects for the main-checkout host.
make_fake_home
make_cache_root
H_XDG_FB="$(mktemp -d "$_TMPBASE/test-agent-cache-xdg-fb-XXXXXX")"
_TMPDIRS+=("$H_XDG_FB")

_REDIRECT_ENV=("XDG_CONFIG_HOME=$H_XDG_FB" "REIFY_MAIN_CHECKOUT=$STUB_DIR/no-such-checkout")
reset_calls
run_redirect
_REDIRECT_ENV=()

assert "H11b: still installs a unit when the stable path has no executable" \
    test -f "$H_XDG_FB/systemd/user/reify-agent-caches.service"
assert "H11c: falls back to the invoking copy's absolute path" \
    bash -c '[ "$(grep "^ExecStart=" "$1" | head -1)" = "ExecStart=$2 --seed-only" ]' \
    _ "$H_XDG_FB/systemd/user/reify-agent-caches.service" "$SCRIPT"
assert "H11d: warns that it pinned the unit at the invoking copy" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*stable main-checkout path"' \
    _ "$OUT" "$ERR_OUT"

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
# The bus-probe branch's own wording — `skip` alone would match the seed-side
# warns this fixture also emits and would pass with the probe removed.
assert "H13: warns specifically about the missing systemd --user bus" \
    bash -c 'printf "%s\n%s\n" "$1" "$2" | grep -qiE "WARN.*no systemd --user bus"' \
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
