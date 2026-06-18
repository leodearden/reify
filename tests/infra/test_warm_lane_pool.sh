#!/usr/bin/env bash
# tests/infra/test_warm_lane_pool.sh
# End-to-end integration gate for the warm-lane CoW pool mechanism.
# Task: #4662
#
# Architecture — two layers:
#
#   ALWAYS-RUN layer (no substrate needed, runs everywhere):
#     Block A  — script-presence / CLI-stability preconditions for all 4
#                warm-lane scripts (provision/seed/refresh/preflight).
#     Block FC — fail-closed wiring (B2 non-reflink-loud, B5 RUSTFLAGS-mismatch,
#                B5 preflight against unmounted mount) via the PATH-stub idiom.
#
#   SUBSTRATE-GATED real end-to-end layer (skips gracefully when no reflink
#   substrate or no cargo; runs on the provisioned host or with opt-in):
#     Block B3+B4 — warm-skip + path-independence (heavy dep fresh:true, B4 fresh
#                   count equality, B3 wall direction).
#     Block PS    — identical test pass-set warm vs cold.
#     Block B7    — reset-in-place stability over K cycles.
#     Block B6+B1 — lifecycle: in-flight clone independence + provision idempotency.
#
# Env knobs:
#   REIFY_WARM_LANE_MOUNT        — pre-existing XFS-reflink mount to use as
#                                  substrate (skips provision step).
#   REIFY_RUN_WARM_LANE_GATE     — set to 1 to opt-in to provisioning a small
#                                  ephemeral loopback via provision-warm-lane-fs.sh
#                                  when no mount is available.
#   REIFY_WARM_LANE_GATE_DEP_FNS — number of trivial fns in the heavy dep crate
#                                  (default: 200; tune for timing signal).
#   REIFY_WARM_LANE_GATE_RESET_CYCLES — number of reset-in-place cycles for B7
#                                  (default: 3).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== warm-lane pool end-to-end integration gate (task #4662) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Resolved paths for the four warm-lane scripts (systems-under-test; read-only)
# ─────────────────────────────────────────────────────────────────────────────
PROVISION_SCRIPT="$REPO_ROOT/scripts/provision-warm-lane-fs.sh"
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"
REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"
PREFLIGHT_SCRIPT="$REPO_ROOT/scripts/warm-lane-preflight.sh"

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup trap
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_GATE_DIR=""           # set by detect_substrate to the reflink-capable dir
_GATE_DIR_CLEANUP=0    # 1 = we provisioned the mount; teardown on EXIT
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
    if [ "${_GATE_DIR_CLEANUP:-0}" = "1" ] && [ -n "${_GATE_DIR:-}" ]; then
        ${REIFY_WARM_LANE_SUDO:-sudo} umount "${_GATE_DIR}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# ─────────────────────────────────────────────────────────────────────────────
# PATH-stub infrastructure (reused from test_seed_warm_lane.sh)
# Used by Block FC to exercise the fail-closed guards of seed-warm-lane.sh
# without a real reflink substrate.
#
# Stubs record every invocation to CALLS_FILE. Behaviour knobs:
#   REIFY_TEST_REFLINK_OK   — cp stub: "1" → exit 0; else print error + exit 1
#   REIFY_TEST_GIT_DIFF_FILES — git stub: emitted on diff --name-only
#   REIFY_TEST_GIT_HEAD     — git stub: emitted on rev-parse HEAD
# ─────────────────────────────────────────────────────────────────────────────
STUB_DIR="$(mktemp -d /tmp/test-warm-pool-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-warm-pool-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-warm-pool-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# cp stub: record argv; REIFY_TEST_REFLINK_OK=1 → exit 0, else error + exit 1
cat > "$STUB_DIR/cp" << 'STUB_EOF'
#!/usr/bin/env bash
echo "cp $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    exit 0
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# find stub: record argv, exit 0 (no-op; real mtime tests use real find)
cat > "$STUB_DIR/find" << 'STUB_EOF'
#!/usr/bin/env bash
echo "find $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/find"

# touch stub: record argv, exit 0 (no-op)
cat > "$STUB_DIR/touch" << 'STUB_EOF'
#!/usr/bin/env bash
echo "touch $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
exit 0
STUB_EOF
chmod +x "$STUB_DIR/touch"

# git stub: record argv; controlled diff/rev-parse output via env vars
cat > "$STUB_DIR/git" << 'STUB_EOF'
#!/usr/bin/env bash
echo "git $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
for arg in "$@"; do
    if [ "$arg" = "--name-only" ]; then
        printf "%s\n" "${REIFY_TEST_GIT_DIFF_FILES:-}"
        exit 0
    fi
done
for arg in "$@"; do
    if [ "$arg" = "rev-parse" ]; then
        echo "${REIFY_TEST_GIT_HEAD:-abc1234}"
        exit 0
    fi
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/git"

# run_helper — invoke SEED_SCRIPT under stub PATH; capture OUT/ERR_OUT/RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SEED_SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() { > "$CALLS_FILE"; }

# ─────────────────────────────────────────────────────────────────────────────
# Passthrough cp stub — Block PG scaffolding (added by task #4667)
#
# Strips --reflink=always / --reflink=auto then execs /bin/cp so gen dirs
# materialize on the non-reflink CI FS.  Distinct from the Block FC cp stub
# above which never copies (exit 0 only).  flock/ln/mv/git/readlink stay REAL.
# ─────────────────────────────────────────────────────────────────────────────
PASSTHROUGH_STUB_DIR="$(mktemp -d /tmp/test-warm-pool-pass-XXXXXX)"
_TMPDIRS+=("$PASSTHROUGH_STUB_DIR")

cat > "$PASSTHROUGH_STUB_DIR/cp" << 'PASS_STUB_EOF'
#!/usr/bin/env bash
# Strip --reflink=* flags then exec real /bin/cp
args=()
for arg in "$@"; do
    case "$arg" in
        --reflink=*|--reflink) ;;
        *) args+=("$arg") ;;
    esac
done
exec /bin/cp "${args[@]}"
PASS_STUB_EOF
chmod +x "$PASSTHROUGH_STUB_DIR/cp"

# ─────────────────────────────────────────────────────────────────────────────
# Substrate helper functions
# ─────────────────────────────────────────────────────────────────────────────

# detect_substrate() — Substrate acquisition ladder; sets _GATE_DIR on success.
#   Returns 0 when a reflink-capable directory is found, 1 otherwise.
#   Ladder:
#     1. REIFY_WARM_LANE_MOUNT (env) — probe cp --reflink=always inside it.
#     2. Scratch-dir reflink probe in ${TMPDIR:-/tmp}.
#     3. REIFY_RUN_WARM_LANE_GATE=1 — provision ephemeral loopback via
#        provision-warm-lane-fs.sh; sets _GATE_DIR_CLEANUP=1 for teardown.
detect_substrate() {
    local probe_src probe_dst probe_tmp
    probe_src=""
    probe_dst=""
    probe_tmp=""

    # 1. Caller-supplied mount
    if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ] && [ -d "${REIFY_WARM_LANE_MOUNT}" ]; then
        probe_src="$(mktemp "${REIFY_WARM_LANE_MOUNT}/.reflink-probe-src-XXXXXX" 2>/dev/null)" || true
        if [ -n "$probe_src" ] && [ -f "$probe_src" ]; then
            probe_dst="${probe_src}.dst"
            if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
                rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
                _GATE_DIR="${REIFY_WARM_LANE_MOUNT}"
                return 0
            fi
            rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
        fi
        echo "detect_substrate: REIFY_WARM_LANE_MOUNT reflink probe failed" >&2
    fi

    # 2. Scratch-dir reflink probe in TMPDIR (usually /tmp)
    probe_tmp="$(mktemp -d "${TMPDIR:-/tmp}/warm-lane-scratch-XXXXXX" 2>/dev/null)" || true
    if [ -n "$probe_tmp" ] && [ -d "$probe_tmp" ]; then
        probe_src="$probe_tmp/probe.src"
        probe_dst="$probe_tmp/probe.dst"
        : > "$probe_src"
        if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
            _GATE_DIR="$(dirname "$probe_tmp")"
            rm -rf "$probe_tmp" 2>/dev/null || true
            return 0
        fi
        rm -rf "$probe_tmp" 2>/dev/null || true
    fi

    # 3. Opt-in ephemeral loopback via provision-warm-lane-fs.sh
    if [ "${REIFY_RUN_WARM_LANE_GATE:-}" = "1" ]; then
        local mount_out
        mount_out="$(bash "$PROVISION_SCRIPT" 2>/dev/null)" || {
            echo "detect_substrate: provision-warm-lane-fs.sh failed" >&2
            return 1
        }
        if [ -n "${mount_out:-}" ] && [ -d "${mount_out}" ]; then
            _GATE_DIR="$mount_out"
            _GATE_DIR_CLEANUP=1
            return 0
        fi
    fi

    return 1
}

# _skip(reason) — emit SKIP on stderr, call test_summary (counts so far), exit 0.
_skip() {
    echo "SKIP: $*" >&2
    test_summary
    exit 0
}

# gen_synth_workspace(dir) — writes a path-clean cargo [workspace] to dir/:
#   warm_dep/  — REIFY_WARM_LANE_GATE_DEP_FNS trivial pub fns (default 500) +
#                one #[test]; NO build.rs / NO absolute-path codegen.
#   warm_leaf/ — one fn using warm_dep + one #[test].
#   Cargo.toml — [workspace] table (halts upward traversal into reify).
#   No prior build — caller performs the cold build.
gen_synth_workspace() {
    local dir="$1"
    local dep_fns="${REIFY_WARM_LANE_GATE_DEP_FNS:-500}"
    local i

    mkdir -p "$dir/warm_dep/src" "$dir/warm_leaf/src"

    cat > "$dir/Cargo.toml" << 'TOML_EOF'
[workspace]
members = ["warm_dep", "warm_leaf"]
resolver = "2"
TOML_EOF

    cat > "$dir/warm_dep/Cargo.toml" << 'TOML_EOF'
[package]
name = "warm_dep"
version = "0.1.0"
edition = "2021"
TOML_EOF

    {
        for i in $(seq 1 "$dep_fns"); do
            printf 'pub fn fn_%d() -> u64 { %d }\n' "$i" "$i"
        done
        printf '\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn dep_smoke() { assert_eq!(super::fn_1(), 1); }\n}\n'
    } > "$dir/warm_dep/src/lib.rs"

    cat > "$dir/warm_leaf/Cargo.toml" << 'TOML_EOF'
[package]
name = "warm_leaf"
version = "0.1.0"
edition = "2021"

[dependencies]
warm_dep = { path = "../warm_dep" }
TOML_EOF

    cat > "$dir/warm_leaf/src/lib.rs" << 'RUST_EOF'
pub fn leaf_fn() -> u64 { warm_dep::fn_1() }

#[cfg(test)]
mod tests {
    #[test]
    fn leaf_smoke() { assert_eq!(super::leaf_fn(), 1); }
}
RUST_EOF
}

# _b6_clone_and_refresh(base_ws, lane_ws, sibling_ws)
#   B6 in-flight independence setup:
#   1. Copy source tree from base_ws (without target/) into sibling_ws.
#   2. CoW-seed sibling_ws from base_ws/target BEFORE the refresh (sibling becomes
#      an in-flight snapshot of the original warm base).
#   3. Run refresh-warm-base.sh to advance base_ws/target from lane_ws/target
#      (atomically replaces the base; sibling_ws/target is NOT affected — it holds
#      CoW blocks referencing the ORIGINAL base, which stay live after the mv).
#
# After this call:
#   - sibling_ws/target  : original base clone (in-flight, independent of refresh)
#   - base_ws/target     : refreshed copy of lane_ws/target
#
# The sibling build should still find warm_dep fresh:true (sources 2020-01-01 <
# original base artifacts), proving the refresh did not touch the in-flight clone.
_b6_clone_and_refresh() {
    local base_ws="$1"
    local lane_ws="$2"
    local sibling_ws="$3"

    # ── Copy workspace sources into the sibling (no target/) ──────────────────
    mkdir -p "$sibling_ws"
    cp "$base_ws/Cargo.toml" "$sibling_ws/Cargo.toml"
    cp "$base_ws/Cargo.lock" "$sibling_ws/Cargo.lock" 2>/dev/null || true
    cp -a "$base_ws/warm_dep" "$sibling_ws/"
    cp -a "$base_ws/warm_leaf" "$sibling_ws/"

    # ── Step 1: CoW-seed sibling from base/target (in-flight snapshot) ────────
    # --fresh-checkout: bulk-stamps sibling sources to 2020-01-01, touches leaf now.
    RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
        bash "$SEED_SCRIPT" "$base_ws/target" "$sibling_ws" \
            --fresh-checkout \
            --touch "$sibling_ws/warm_leaf/src/lib.rs" >/dev/null

    # ── Step 2: Advance the base (refresh base/target from lane/target) ───────
    # The sibling's target was cloned BEFORE this mv → its CoW blocks are independent.
    RUSTFLAGS="" \
        bash "$REFRESH_SCRIPT" "$lane_ws/target" "$base_ws/target" >/dev/null
}

# _b7_init_git_lane(lane_dir) — git-initialize a synth lane directory so that
# reset-in-place (git checkout -- . && git clean -xfd -e target) is faithful.
#
# After the CoW seed + warm lane build, source files are at mtime=2020-01-01
# (bulk-stamped by --fresh-checkout) except the touched leaf.  Committing them
# at those mtimes causes git's index stat-cache to record mtime=2020-01-01 for
# warm_dep sources.  On subsequent `git checkout -- .`, git sees the warm_dep
# files are unmodified (content + cached-mtime match) and does NOT rewrite them,
# preserving their 2020-01-01 mtime.  Only the mutated leaf (content differs)
# gets rewritten → only the leaf's mtime updates to now.
#
# This is the critical invariant for B7: warm_dep sources stay at 2020-01-01
# (older than artifacts → dep appears fresh) while the leaf gets a fresh mtime
# (newer than its artifact → leaf rebuilds).
_b7_init_git_lane() {
    local lane="$1"
    # Guard: only init if not already a git repo
    if [ -d "$lane/.git" ]; then
        return 0
    fi
    git -C "$lane" init -q
    # Add all sources EXCLUDING target/ (too large + not source)
    git -C "$lane" add -- . ':!target'
    git -C "$lane" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        commit -q -m "initial: synth lane at mtime-2020-01-01 state"
}

# _passset_normalize_nextest — pure stdin→stdout normalizer for `cargo nextest run`
# output.  Selects PASS/FAIL/SKIP lines, strips the volatile bracketed duration
# column (e.g. `[   0.012s]`), collapses internal whitespace, trims, and sorts →
# produces a timing-free, byte-stable pass-set string suitable for comparison.
#
# Used by run_passset's nextest branch and the PS-NORM always-run regression block.
_passset_normalize_nextest() {
    grep -E '^\s*(PASS|FAIL|SKIP)' \
    | sed -E 's/\[[^]]*\]//g' \
    | sed -E 's/[[:space:]]+/ /g' \
    | sed -E 's/^ //;s/ $//' \
    | sort
}

# _passset_normalize_cargo_test — pure stdin→stdout normalizer for `cargo test`
# output.  Selects `test ... ok/FAILED/ignored` lines and sorts → produces a
# stable pass-set string for comparison.  Used by run_passset's cargo-test branch
# and the PS-NORM always-run regression block.
_passset_normalize_cargo_test() {
    grep -E '^test .+ \.\.\. (ok|FAILED|ignored)' \
    | sort
}

# run_passset(manifest) — run the workspace tests (cargo nextest run if available,
# else cargo test) and produce a normalized, deterministic string capturing the
# sorted test identifiers plus the pass/fail counts.  Output is on stdout.
# Env: CARGO_INCREMENTAL=0, RUSTC_WRAPPER="", RUSTFLAGS="" (matches build env).
#
# Normalization:
#   - nextest branch: PASS/FAIL/SKIP lines → _passset_normalize_nextest (strips
#     the volatile `[...]` timing column → byte-stable across independent builds).
#   - cargo test branch: `test ... ok/FAILED/ignored` lines (already timing-free)
#     → grep + sort only (no timing column to strip).
# The output format is designed to be byte-comparable between two runs on
# semantically identical workspaces.
run_passset() {
    local manifest="$1"
    local test_output passed=0 failed=0 ignored=0

    if command -v cargo-nextest >/dev/null 2>&1 || \
       cargo nextest --version >/dev/null 2>&1; then
        # nextest: normalize via _passset_normalize_nextest (strips timing column)
        test_output="$(
            CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
                cargo nextest run \
                    --manifest-path "$manifest" \
                    --no-fail-fast 2>&1 \
            | _passset_normalize_nextest \
            || true
        )"
        # Count outcomes from the NORMALIZED (timing-free) lines
        passed="$(printf '%s\n' "$test_output" | grep -c '^PASS' || true)"
        failed="$(printf '%s\n' "$test_output" | grep -c '^FAIL' || true)"
        printf 'passed=%s failed=%s\n%s\n' "$passed" "$failed" "$test_output"
    else
        # cargo test: capture test names and the summary line
        test_output="$(
            CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
                cargo test \
                    --manifest-path "$manifest" \
                    -- --test-output immediate-fail 2>&1 \
            || true
        )"
        # Extract sorted test identifiers via _passset_normalize_cargo_test
        local test_lines
        test_lines="$(printf '%s\n' "$test_output" \
            | _passset_normalize_cargo_test \
            || true)"
        passed="$(printf '%s\n' "$test_lines" | grep -c '\.\.\. ok$' || true)"
        failed="$(printf '%s\n' "$test_lines" | grep -c '\.\.\. FAILED$' || true)"
        ignored="$(printf '%s\n' "$test_lines" | grep -c '\.\.\. ignored$' || true)"
        printf 'passed=%s failed=%s ignored=%s\n%s\n' \
            "$passed" "$failed" "$ignored" "$test_lines"
    fi
}

# build_count_fresh(manifest) — run `cargo build --message-format=json` on the
# given workspace manifest and count compiler-artifact lines reporting "fresh":true.
# Outputs the integer count on stdout.
# Env: CARGO_INCREMENTAL=0, RUSTC_WRAPPER="", RUSTFLAGS="" (deterministic; must
# match the RUSTFLAGS recorded by seed-warm-lane.sh --record-base for the guards
# to pass on the seeded lane).
build_count_fresh() {
    local manifest="$1"
    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$manifest" \
            --message-format=json 2>/dev/null \
        | grep '"reason":"compiler-artifact"' \
        | grep -c '"fresh":true' \
        || true
}

# build_walltime(manifest) — time a full cargo build on the given manifest.
# Outputs elapsed wall-clock milliseconds on stdout (date +%s%3N for sub-second
# resolution — avoids spurious direction failures when synthetic builds round to 0s).
# Env: same as build_count_fresh (CARGO_INCREMENTAL=0, RUSTC_WRAPPER="", RUSTFLAGS="").
build_walltime() {
    local manifest="$1" t0 t1
    t0="$(date +%s%3N)"
    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$manifest" >/dev/null 2>&1
    t1="$(date +%s%3N)"
    echo $(( t1 - t0 ))
}

# ─────────────────────────────────────────────────────────────────────────────
# Block PG scaffolding — git-fixture builders + _refresh_capture runner
# (ALWAYS-RUN; added by task #4667 for D10 base-coherence hardening)
#
# These helpers drive the REAL refresh-warm-base.sh under the passthrough cp
# stub so that provenance-guard + symlink-gen + refcount-GC mechanics can be
# exercised in default CI without a reflink FS.
# ─────────────────────────────────────────────────────────────────────────────

# _mk_clean_advancing_lane(parent_dir)
# git-init a lane dir (parent_dir/lane) with a committed source tree + a real
# target/ subdir. git status --porcelain --untracked-files=no is empty (clean).
# Prints the lane dir path to stdout.
_mk_clean_advancing_lane() {
    local parent_dir="$1"
    local lane_dir="$parent_dir/lane"
    mkdir -p "$lane_dir/src" "$lane_dir/target/debug"
    cat > "$lane_dir/Cargo.toml" <<'TOML_EOF'
[package]
name = "advancing_crate"
version = "0.1.0"
edition = "2021"
TOML_EOF
    printf 'pub fn hello() -> u64 { 42 }\n' > "$lane_dir/src/lib.rs"
    # target/ stays untracked; the committed tree is Cargo.toml + src/
    printf 'artifact-placeholder\n' > "$lane_dir/target/debug/placeholder"
    git -C "$lane_dir" init -q
    git -C "$lane_dir" add -- . ':!target'
    git -C "$lane_dir" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        commit -q -m "initial: clean advancing lane"
    echo "$lane_dir"
}

# _mk_wip_advancing_lane(parent_dir)
# Like _mk_clean_advancing_lane but appends an uncommitted TRACKED edit to
# src/lib.rs so git status --porcelain --untracked-files=no is non-empty (WIP).
# Prints the lane dir path to stdout.
_mk_wip_advancing_lane() {
    local parent_dir="$1"
    local lane_dir
    lane_dir="$(_mk_clean_advancing_lane "$parent_dir")"
    printf '// WIP edit — uncommitted tracked change\n' >> "$lane_dir/src/lib.rs"
    echo "$lane_dir"
}

# _refresh_capture <advancing_dir> <base_dir> [options...]
# Invoke the REAL refresh-warm-base.sh under the passthrough cp stub, passing
# all arguments unchanged.  Sets global variables:
#   RC                      — exit code of refresh-warm-base.sh
#   OUT                     — stdout of refresh-warm-base.sh
#   ERR_OUT                 — stderr of refresh-warm-base.sh
#   REFRESH_BASE_IS_SYMLINK — "1" if <base_dir> is a symlink after the call,
#                             "0" otherwise
#   REFRESH_BASE_LINK       — readlink <base_dir> if symlink, else ""
# Precondition: $2 is the base_dir positional (not an option value).
_refresh_capture() {
    local _rc=0
    local _base_dir="$2"
    > "$ERR_FILE"
    OUT="$(
        PATH="$PASSTHROUGH_STUB_DIR:$PATH" \
            bash "$REFRESH_SCRIPT" "$@" 2>"$ERR_FILE"
    )" || _rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$_rc
    REFRESH_BASE_IS_SYMLINK="0"
    REFRESH_BASE_LINK=""
    if [ -L "$_base_dir" ]; then
        REFRESH_BASE_IS_SYMLINK="1"
        REFRESH_BASE_LINK="$(readlink "$_base_dir")"
    fi
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — Script-presence / CLI-stability preconditions (ALWAYS-RUN)
# Each of the 4 warm-lane scripts must exist as an executable, and --help must
# exit 0 and print "usage" or "Usage" on stderr.
# The verify-pipeline-infra-tests.txt map must contain a drift-guard row that
# routes a warm-lane script edit to this gate.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: script-presence / CLI-stability ---"

_VP_INFRA_MAP="$REPO_ROOT/scripts/verify-pipeline-infra-tests.txt"

# ── A1: provision-warm-lane-fs.sh ────────────────────────────────────────────
assert "A1: provision-warm-lane-fs.sh exists and is executable" \
    test -x "$PROVISION_SCRIPT"
_A1_ERR="$(bash "$PROVISION_SCRIPT" --help 2>&1 >/dev/null)" || true
_A1_RC=0; bash "$PROVISION_SCRIPT" --help >/dev/null 2>&1 || _A1_RC=$?
assert "A1: provision-warm-lane-fs.sh --help exits 0" \
    test "$_A1_RC" -eq 0
assert "A1: provision-warm-lane-fs.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A1_ERR"

# ── A2: seed-warm-lane.sh ─────────────────────────────────────────────────────
assert "A2: seed-warm-lane.sh exists and is executable" \
    test -x "$SEED_SCRIPT"
_A2_ERR="$(bash "$SEED_SCRIPT" --help 2>&1 >/dev/null)" || true
_A2_RC=0; bash "$SEED_SCRIPT" --help >/dev/null 2>&1 || _A2_RC=$?
assert "A2: seed-warm-lane.sh --help exits 0" \
    test "$_A2_RC" -eq 0
assert "A2: seed-warm-lane.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A2_ERR"

# ── A3: refresh-warm-base.sh ──────────────────────────────────────────────────
assert "A3: refresh-warm-base.sh exists and is executable" \
    test -x "$REFRESH_SCRIPT"
_A3_ERR="$(bash "$REFRESH_SCRIPT" --help 2>&1 >/dev/null)" || true
_A3_RC=0; bash "$REFRESH_SCRIPT" --help >/dev/null 2>&1 || _A3_RC=$?
assert "A3: refresh-warm-base.sh --help exits 0" \
    test "$_A3_RC" -eq 0
assert "A3: refresh-warm-base.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A3_ERR"

# ── A4: warm-lane-preflight.sh ───────────────────────────────────────────────
assert "A4: warm-lane-preflight.sh exists and is executable" \
    test -x "$PREFLIGHT_SCRIPT"
_A4_ERR="$(bash "$PREFLIGHT_SCRIPT" --help 2>&1 >/dev/null)" || true
_A4_RC=0; bash "$PREFLIGHT_SCRIPT" --help >/dev/null 2>&1 || _A4_RC=$?
assert "A4: warm-lane-preflight.sh --help exits 0" \
    test "$_A4_RC" -eq 0
assert "A4: warm-lane-preflight.sh --help prints usage on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$_A4_ERR"

# ── A5: drift-guard map contains a row for a warm-lane script → this gate ────
# At least one row in verify-pipeline-infra-tests.txt must map a warm-lane
# script artifact to a glob that matches tests/infra/test_warm_lane_pool.sh.
# This ensures that a future edit to provision/seed/refresh/preflight will
# re-exercise this integration gate at task-scope verify time.
assert "A5: verify-pipeline-infra-tests.txt exists" \
    test -f "$_VP_INFRA_MAP"
assert "A5: drift-guard map has a warm-lane-script → test_warm_lane_pool.sh row" \
    bash -c '
        map="$1"
        # Look for any non-comment row whose artifact column matches a warm-lane script
        # and whose test-glob column would fnmatch tests/infra/test_warm_lane_pool.sh.
        while IFS= read -r line; do
            [[ "$line" =~ ^[[:space:]]*# ]] && continue
            [[ -z "${line// }" ]] && continue
            artifact=$(awk "{print \$1}" <<< "$line")
            glob=$(awk "{print \$2}" <<< "$line")
            case "$artifact" in
                scripts/*warm-lane*.sh|scripts/*warm_lane*.sh|scripts/provision-warm-lane-fs.sh|\
scripts/seed-warm-lane.sh|scripts/refresh-warm-base.sh|scripts/warm-lane-preflight.sh) ;;
                *) continue ;;
            esac
            # Check if the glob matches this gate file
            case "tests/infra/test_warm_lane_pool.sh" in
                $glob) exit 0 ;;
            esac
        done < "$map"
        exit 1
    ' _ "$_VP_INFRA_MAP"

# ─────────────────────────────────────────────────────────────────────────────
# Block FC — Fail-closed wiring (ALWAYS-RUN; no real substrate needed)
#
# Exercises the integration-level guards via the PATH-stub idiom reused from
# test_seed_warm_lane.sh:  STUB_DIR with cp/find/touch/git stubs recording
# argv to CALLS_FILE, run_helper capturing OUT/ERR_OUT/RC separately.
#
# Stubs + run_helper + reset_calls are defined in impl-failclosed (impl step).
# Referencing them here without prior definition → immediate error under
# set -euo pipefail → RED until the impl step defines them.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block FC: fail-closed wiring (B5/B2/preflight) ---"

# ── FC fixture: a base dir whose .warm-base-meta records a DIFFERENT RUSTFLAGS
FC_BASE_PARENT="$(mktemp -d /tmp/test-warm-pool-FC-base-XXXXXX)"
FC_BASE="$FC_BASE_PARENT/target"
FC_LANE="$(mktemp -d /tmp/test-warm-pool-FC-lane-XXXXXX)"
_TMPDIRS+=("$FC_BASE_PARENT" "$FC_LANE")
mkdir -p "$FC_BASE"
cat > "$FC_BASE_PARENT/.warm-base-meta" <<'SIDECAR_EOF'
RUSTFLAGS=original-flags
INVOCATION=
SIDECAR_EOF

# ── FC1: B5 — RUSTFLAGS mismatch → non-zero exit, actionable stderr, empty stdout, cp not called
reset_calls
RUSTFLAGS="different-flags" run_helper "$FC_BASE" "$FC_LANE" --fresh-checkout
assert "FC1: RUSTFLAGS mismatch exits non-zero (B5)" test "$RC" -ne 0
assert "FC1: stderr names RUSTFLAGS mismatch (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qi "RUSTFLAGS"' _ "$ERR_OUT"
assert "FC1: STDOUT empty on RUSTFLAGS mismatch (fail-closed)" \
    bash -c '[ -z "$1" ]' _ "$OUT"
assert "FC1: cp never invoked on RUSTFLAGS mismatch (guard fires first)" \
    bash -c '! grep -q "^cp" "$1"' _ "$CALLS_FILE"

# ── FC2: B2 — reflink-failure → non-zero exit with actionable message
FC_LANE2="$(mktemp -d /tmp/test-warm-pool-FC-lane2-XXXXXX)"
_TMPDIRS+=("$FC_LANE2")
reset_calls
RUSTFLAGS="original-flags" REIFY_TEST_REFLINK_OK=0 \
    run_helper "$FC_BASE" "$FC_LANE2" --fresh-checkout
assert "FC2: cp failure (non-reflink FS) exits non-zero (B2)" test "$RC" -ne 0
assert "FC2: stderr names reflink failure (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# ── FC3: preflight — unmounted mount → non-zero exit with actionable hint
FC_FAKE_MOUNT="$(mktemp -d /tmp/test-warm-pool-FC-mnt-XXXXXX)"
_TMPDIRS+=("$FC_FAKE_MOUNT")
# The fake mount dir exists but is NOT a real mountpoint → preflight check 1 fails.
FC_PF_RC=0
bash "$PREFLIGHT_SCRIPT" --mount "$FC_FAKE_MOUNT" 2>/dev/null || FC_PF_RC=$?
assert "FC3: preflight fails on unmounted dir (non-zero)" test "$FC_PF_RC" -ne 0
FC_PF_ERR="$(bash "$PREFLIGHT_SCRIPT" --mount "$FC_FAKE_MOUNT" 2>&1 >/dev/null)" || true
assert "FC3: preflight stderr names mount/provision remediation (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "mount|provision"' _ "$FC_PF_ERR"

# ─────────────────────────────────────────────────────────────────────────────
# Block SG — Substrate detector + skip path (ALWAYS-RUN)
#
# Unit-tests detect_substrate() and _skip() which are defined in the
# impl-substrate-gate step. Until then, placeholder values make every
# assertion RED.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block SG: substrate detection + skip path ---"

# ── SG1: detect_substrate returns non-zero when no reflink substrate is available
_SG_DETECT_NO_SUB_RC=0
(
    REIFY_WARM_LANE_MOUNT="" \
    REIFY_RUN_WARM_LANE_GATE="" \
    PATH="$STUB_DIR:$PATH" \
    REIFY_TEST_REFLINK_OK=0 \
        detect_substrate 2>/dev/null
) || _SG_DETECT_NO_SUB_RC=$?

# ── SG2: detect_substrate returns 0 when a valid mount + probe succeeds
_SG2_FAKE_MOUNT="$(mktemp -d /tmp/test-warm-pool-SG2-XXXXXX)"
_TMPDIRS+=("$_SG2_FAKE_MOUNT")
_SG_DETECT_WITH_SUB_RC=1
(
    REIFY_WARM_LANE_MOUNT="$_SG2_FAKE_MOUNT" \
    PATH="$STUB_DIR:$PATH" \
    REIFY_TEST_REFLINK_OK=1 \
        detect_substrate 2>/dev/null
) && _SG_DETECT_WITH_SUB_RC=0 || _SG_DETECT_WITH_SUB_RC=$?

# ── SG3: _skip exits 0 and emits SKIP on stderr (invoked in subshell)
_SG_SKIP_RC=0
_SG_SKIP_ERR="$( _skip "unit-test-sentinel" 2>&1 1>/dev/null )" || _SG_SKIP_RC=$?

# ── SG4: command -v cargo returns non-zero when cargo is absent from PATH
_SG_CARGO_MISS_RC=0
( PATH="/nonexistent_path_for_cargo_test_xyz" command -v cargo >/dev/null 2>&1 ) \
    || _SG_CARGO_MISS_RC=$?

assert "SG1: detect_substrate returns non-zero when no substrate available" \
    test "$_SG_DETECT_NO_SUB_RC" -ne 0
assert "SG2: detect_substrate returns 0 when valid mount+reflink provided" \
    test "$_SG_DETECT_WITH_SUB_RC" -eq 0
assert "SG3: _skip exits 0 (graceful skip, not hard abort)" \
    test "$_SG_SKIP_RC" -eq 0
assert "SG3: _skip emits a SKIP line on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "SKIP"' _ "$_SG_SKIP_ERR"
assert "SG4: gate detects absent cargo (command -v cargo in empty PATH)" \
    test "$_SG_CARGO_MISS_RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block PS-NORM — Pass-set normalizer timing-strip regression (ALWAYS-RUN)
#
# Exercises run_passset()'s nextest-branch normalization WITHOUT invoking cargo.
# Feeds two canned `cargo nextest run` outputs that are byte-identical EXCEPT
# for the volatile per-test duration column (the `[   0.0NNs]` token) through
# _passset_normalize_nextest and asserts:
#   (a) the two normalized outputs are BYTE-IDENTICAL;
#   (b) their derived PASS/FAIL counts match.
#
# Premise (exactness): the inputs differ ONLY inside the bracketed `[...]`
# token; the normalizer strips every `[...]` token so post-normalization byte
# streams are identical by construction.
#
# Without timing stripping the `[   0.0NNs]` column is retained → strings
# differ → assertion fails → RED.  _passset_normalize_nextest is defined in
# impl-passset-timing-strip; until then, calling it under set -euo pipefail
# aborts the script → RED.
#
# Also asserts cargo-test fallback lines (already timing-free) are sort-stable
# across different emission orderings (regression guard).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PS-NORM: pass-set normalizer timing-strip regression ---"

# ── Canned nextest outputs — differ ONLY in the [   0.0NNs] timing column ─────
_PSNORM_COLD_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.012s] warm_dep tests::dep_smoke
  PASS [   0.008s] warm_leaf tests::leaf_smoke
PSNORM_EOF
)"
_PSNORM_WARM_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.034s] warm_dep tests::dep_smoke
  PASS [   0.019s] warm_leaf tests::leaf_smoke
PSNORM_EOF
)"

# Normalize through the timing-strip helper.
# _passset_normalize_nextest is undefined until impl-passset-timing-strip → RED.
_PSNORM_COLD_NORM="$(printf '%s\n' "$_PSNORM_COLD_INPUT" | _passset_normalize_nextest)"
_PSNORM_WARM_NORM="$(printf '%s\n' "$_PSNORM_WARM_INPUT" | _passset_normalize_nextest)"

assert "PS-NORM: nextest normalized output is byte-identical across timing differences" \
    test "$_PSNORM_COLD_NORM" = "$_PSNORM_WARM_NORM"

# ── Derived PASS counts must also match ──────────────────────────────────────
_PSNORM_COLD_PASS="$(printf '%s\n' "$_PSNORM_COLD_NORM" | grep -c 'PASS' || echo 0)"
_PSNORM_WARM_PASS="$(printf '%s\n' "$_PSNORM_WARM_NORM" | grep -c 'PASS' || echo 0)"
assert "PS-NORM: derived PASS count matches between cold and warm normalized outputs" \
    test "$_PSNORM_COLD_PASS" -eq "$_PSNORM_WARM_PASS"

# ── Cargo-test fallback regression guard ─────────────────────────────────────
# Cargo-test `... ok/FAILED/ignored` lines carry no timing column; they are
# normalized by the cargo branch (grep + sort only, no sed strip).  Assert that
# two different emission orderings of the same tests sort to byte-identical
# output — regression guard confirming the cargo-test branch is unaffected.
_PSNORM_CT_FWD="$(printf 'test a::smoke ... ok\ntest b::smoke ... ok\n' | \
    _passset_normalize_cargo_test)"
_PSNORM_CT_REV="$(printf 'test b::smoke ... ok\ntest a::smoke ... ok\n' | \
    _passset_normalize_cargo_test)"
assert "PS-NORM: cargo-test lines normalize stably via _passset_normalize_cargo_test" \
    test "$_PSNORM_CT_FWD" = "$_PSNORM_CT_REV"

# ─────────────────────────────────────────────────────────────────────────────
# Top-level substrate gate — guards all real substrate-gated blocks below.
#
# In the default CI environment (REIFY_WARM_LANE_MOUNT unset, /tmp is ext4,
# REIFY_RUN_WARM_LANE_GATE unset) detect_substrate returns 1 → _skip is called
# → harness exits 0 with "SKIP: …" on stderr. The real blocks never run.
#
# To arm the real blocks:
#   REIFY_WARM_LANE_MOUNT=/path/to/xfs-mount   — use an existing XFS volume
#   REIFY_RUN_WARM_LANE_GATE=1                 — self-provision an ephemeral loop
# ─────────────────────────────────────────────────────────────────────────────
if ! detect_substrate 2>/dev/null; then
    _skip "no XFS reflink substrate; set REIFY_WARM_LANE_MOUNT or REIFY_RUN_WARM_LANE_GATE=1"
fi
if ! command -v cargo >/dev/null 2>&1; then
    _skip "cargo not in PATH; substrate-gated real blocks skipped"
fi
# _GATE_DIR is now set to the reflink-capable directory for the real blocks.

# ─────────────────────────────────────────────────────────────────────────────
# Block B3+B4 — Warm-skip + path-independence (SUBSTRATE-GATED)
#
# B3 warm-skip: in the seeded-lane build the heavy dep unit is fresh:true
#   (reused via CoW, NOT recompiled) and the leaf delta-closure is fresh:false.
# B4 path-independence: fresh-unit count in warm lane == in-place control count.
# B3 wall: warm lane build wall-time < cold-control build wall-time (direction).
#
# Helpers gen_synth_workspace/build_count_fresh/build_walltime are defined in
# impl-warmskip-pathindep. Until then, placeholder values make assertions RED on
# a real XFS host (assertions are never reached on non-XFS via the skip path).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B3+B4: warm-skip + path-independence ---"

# ── Generate a synthetic cargo workspace on the XFS substrate ────────────────
_GATE_WS_ROOT="$(mktemp -d "$_GATE_DIR/warm-lane-ws-XXXXXX")"
_TMPDIRS+=("$_GATE_WS_ROOT")
_WS_BASE="$_GATE_WS_ROOT/synth-base"
_WS_LANE="$_GATE_WS_ROOT/synth-lane"
gen_synth_workspace "$_WS_BASE"
echo "B3+B4: workspace generated at $_WS_BASE (${REIFY_WARM_LANE_GATE_DEP_FNS:-500} dep fns)" >&2

# ── Stamp base provenance so seed-warm-lane.sh RUSTFLAGS guard passes ─────────
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" --record-base "$_WS_BASE/target" >/dev/null

# ── Cold control build: empty target → all from scratch (B3 wall baseline) ───
_B3_COLD_WALL="$(build_walltime "$_WS_BASE/Cargo.toml")"

# ── Apply leaf delta (one new fn in warm_leaf) ────────────────────────────────
printf '\npub fn delta_fn() -> u64 { 42 }\n' >> "$_WS_BASE/warm_leaf/src/lib.rs"

# ── In-place control rebuild: dep stays fresh, leaf rebuilds ──────────────────
# Count fresh units — this is the B4 baseline (same count expected in warm lane).
_B4_INPLACE_FRESH="$(build_count_fresh "$_WS_BASE/Cargo.toml")"

# ── Copy workspace sources (with delta applied) to the lane dir ───────────────
# Excludes target/ — the lane's target/ comes from the CoW seed below.
mkdir -p "$_WS_LANE"
cp "$_WS_BASE/Cargo.toml" "$_WS_LANE/Cargo.toml"
cp "$_WS_BASE/Cargo.lock" "$_WS_LANE/Cargo.lock" 2>/dev/null || true
cp -a "$_WS_BASE/warm_dep" "$_WS_LANE/"
cp -a "$_WS_BASE/warm_leaf" "$_WS_LANE/"

# ── CoW-seed the lane: clone base/target → lane/target ───────────────────────
# --fresh-checkout: bulk-stamps all lane sources to 2020-01-01 (older than any
# artifact → dep appears fresh), then touches the leaf to NOW (newer than its
# artifact → cargo rebuilds just the leaf).
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" "$_WS_BASE/target" "$_WS_LANE" \
        --fresh-checkout \
        --touch "$_WS_LANE/warm_leaf/src/lib.rs" >/dev/null

# ── Warm lane build: heavy dep reused via CoW (fresh:true), leaf rebuilt ──────
# Capture JSON to inspect per-crate freshness (B3 warm-skip) AND measure wall.
_B3_WARM_T0="$(date +%s%3N)"
_WARM_JSON="$(CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
    cargo build --manifest-path "$_WS_LANE/Cargo.toml" \
        --message-format=json 2>/dev/null)"
_B3_WARM_WALL=$(( $(date +%s%3N) - _B3_WARM_T0 ))

# ── Extract B3/B4 signals from warm lane build output ────────────────────────
_B3_DEP_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
    grep '"reason":"compiler-artifact"' | grep '"name":"warm_dep"' | \
    grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

_B3_LEAF_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
    grep '"reason":"compiler-artifact"' | grep '"name":"warm_leaf"' | \
    grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

_B4_WARM_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
    grep '"reason":"compiler-artifact"' | grep -c '"fresh":true' || true)"

# Record signals to stderr (direction-only; no frozen thresholds per G6/PRD §9)
echo "B3 wall: cold=${_B3_COLD_WALL}ms warm=${_B3_WARM_WALL}ms delta=$((${_B3_COLD_WALL} - ${_B3_WARM_WALL}))ms" >&2
echo "B4 fresh counts: inplace=${_B4_INPLACE_FRESH} warm=${_B4_WARM_FRESH}" >&2

assert "B3: heavy dep unit is fresh:true in warm lane (CoW-reused, not recompiled)" \
    test "$_B3_DEP_FRESH" = "true"
assert "B3: leaf delta-closure is fresh:false in warm lane (was rebuilt)" \
    test "$_B3_LEAF_FRESH" = "false"
assert "B4: fresh-unit count in warm lane == in-place control (path-independence)" \
    test "$_B4_INPLACE_FRESH" -eq "$_B4_WARM_FRESH"
assert "B3: warm lane build wall-time < cold-control build wall-time (direction)" \
    test "$_B3_WARM_WALL" -lt "$_B3_COLD_WALL"

# ─────────────────────────────────────────────────────────────────────────────
# Block PS — Identical test pass-set: warm lane vs cold control (SUBSTRATE-GATED)
#
# Asserts that the sorted set of test identifiers AND the pass/fail counts
# produced by running the synth workspace's tests in the warm lane equal those
# from the cold control.  Since the CoW lane has byte-identical source the
# tests are trivially identical (spike §6 confirmation at synthetic scale).
#
# run_passset(manifest) is defined in impl-passset. Until then, calling it
# errors under set -euo pipefail → RED on a substrate (SKIP on non-substrate
# because the substrate gate fires first).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PS: identical test pass-set (warm vs cold) ---"

_PS_COLD="$(run_passset "$_WS_BASE/Cargo.toml")"
_PS_WARM="$(run_passset "$_WS_LANE/Cargo.toml")"

assert "PS: warm-lane test identifiers == cold control (byte-identical source)" \
    test "$_PS_COLD" = "$_PS_WARM"

# ─────────────────────────────────────────────────────────────────────────────
# Block B7 — Reset-in-place stability: K cycles (SUBSTRATE-GATED)
#
# Over K cycles (REIFY_WARM_LANE_GATE_RESET_CYCLES, default 3) of:
#   1. mutate the leaf (add a new fn)
#   2. git checkout -- . && git clean -xfd -e target
#   3. rebuild
# Assertions each cycle:
#   - build exits 0
#   - warm_dep stays fresh:true (warmth preserved — git checkout only rewrites the
#     modified leaf; warm_dep sources untouched → stay at 2020-01-01 < artifacts)
#   - refresh-warm-base.sh --check-frag <lane>/target returns "ok N" (extents bounded)
#   - du of lane/target stays bounded (no space leak)
#
# The lane must be git-initialized (committed at the 2020-01-01 mtime state) so
# that git checkout only rewrites modified files and leaves warm_dep sources at
# their staged mtime.
#
# _b7_init_git_lane is defined in impl-reset-in-place — RED until then.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B7: reset-in-place stability (${REIFY_WARM_LANE_GATE_RESET_CYCLES:-3} cycles) ---"

_B7_K="${REIFY_WARM_LANE_GATE_RESET_CYCLES:-3}"

# git-init the lane so that git checkout only rewrites MODIFIED files.
# Until _b7_init_git_lane is defined (impl step): command-not-found → RED on substrate.
_b7_init_git_lane "$_WS_LANE"

_B7_DU_BASE="$(du -sb "$_WS_LANE/target" 2>/dev/null | awk '{print $1}' || echo 0)"

_B7_i=1
while [ "$_B7_i" -le "$_B7_K" ]; do
    echo "B7: cycle $_B7_i/$_B7_K" >&2

    # Mutate leaf (different fn per cycle)
    printf '\npub fn b7_fn_%d() -> u64 { %d }\n' "$_B7_i" "$_B7_i" \
        >> "$_WS_LANE/warm_leaf/src/lib.rs"

    # Reset in place: reverts the leaf; warm_dep sources untouched (→ stay 2020-01-01)
    (cd "$_WS_LANE" && git checkout -- . && git clean -xfd -e target 2>/dev/null)

    # Rebuild and capture per-crate freshness
    _B7_CYCLE_RC=0
    _B7_CYCLE_JSON="$(CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_WS_LANE/Cargo.toml" \
            --message-format=json 2>/dev/null)" || _B7_CYCLE_RC=$?

    _B7_DEP_FRESH="$(printf '%s\n' "$_B7_CYCLE_JSON" | \
        grep '"reason":"compiler-artifact"' | grep '"name":"warm_dep"' | \
        grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

    # check-frag (stdout: "ok N" or "reseed-due N")
    _B7_FRAG="$(bash "$REFRESH_SCRIPT" --check-frag "$_WS_LANE/target" 2>/dev/null || true)"

    # du check
    _B7_DU_NOW="$(du -sb "$_WS_LANE/target" 2>/dev/null | awk '{print $1}' || echo 0)"

    assert "B7[cycle $_B7_i/$_B7_K]: build exits 0" \
        test "$_B7_CYCLE_RC" -eq 0
    assert "B7[cycle $_B7_i/$_B7_K]: warm_dep stays fresh:true after reset (warmth preserved)" \
        test "$_B7_DEP_FRESH" = "true"
    assert "B7[cycle $_B7_i/$_B7_K]: check-frag returns ok (extents bounded below threshold)" \
        bash -c 'printf "%s\n" "$1" | grep -qi "^ok"' _ "$_B7_FRAG"
    assert "B7[cycle $_B7_i/$_B7_K]: lane/target du stays bounded (≤ 2× baseline; no space leak)" \
        test "$_B7_DU_NOW" -le "$(( _B7_DU_BASE * 2 ))"

    _B7_i=$(( _B7_i + 1 ))
done

# ─────────────────────────────────────────────────────────────────────────────
# Block B6+B1 — Lifecycle: in-flight clone independence + provision idempotency
#               (SUBSTRATE-GATED)
#
# B6 — in-flight clone independence:
#   1. CoW-clone lane/target → sibling_lane/target (a 2nd pool lane, in-flight)
#   2. Run refresh-warm-base.sh BASE/target BASE2/target (advance the base)
#   3. Assert: the sibling lane's target is byte-identical to what it was before
#      the refresh (the refresh did NOT affect the in-flight clone).
#   4. Assert: building the sibling lane still has warm_dep fresh:true.
#
# B1 — provision idempotency (ONLY when the gate self-provisioned the substrate):
#   A second call to provision-warm-lane-fs.sh against the already-mounted
#   substrate exits 0, prints the same mount path, and does NOT reformat/remount.
#   Skipped (with a logged note) when substrate was supplied externally.
#
# Both assertions reference _b6_clone_and_refresh (undefined until impl-lifecycle)
# → RED on substrate; SKIP on non-substrate.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B6+B1: lifecycle (in-flight independence + provision idempotency) ---"

# B6: in-flight CoW clone independence
# Create a sibling lane by CoW-cloning base/target into sibling/target.
_B6_SIBLING_LANE="$_GATE_WS_ROOT/synth-sibling"

# Snapshot sibling/target BEFORE the refresh so we can compare after.
# _b6_clone_and_refresh is defined in impl-lifecycle → RED until then.
_b6_clone_and_refresh "$_WS_BASE" "$_WS_LANE" "$_B6_SIBLING_LANE"

# After the refresh, build the sibling lane and check dep freshness.
_B6_SIBLING_RC=0
_B6_SIBLING_JSON="$(CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
    cargo build --manifest-path "$_B6_SIBLING_LANE/Cargo.toml" \
        --message-format=json 2>/dev/null)" || _B6_SIBLING_RC=$?

_B6_SIBLING_DEP_FRESH="$(printf '%s\n' "$_B6_SIBLING_JSON" | \
    grep '"reason":"compiler-artifact"' | grep '"name":"warm_dep"' | \
    grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

assert "B6: sibling lane build exits 0 after base refresh (in-flight independence)" \
    test "$_B6_SIBLING_RC" -eq 0
assert "B6: sibling lane warm_dep still fresh:true after base refresh" \
    test "$_B6_SIBLING_DEP_FRESH" = "true"
assert "B6: base-refreshed target exists (refresh completed)" \
    test -d "$_WS_BASE/target"

# B1: provision idempotency (only when we self-provisioned the substrate)
if [ "${_GATE_DIR_CLEANUP:-0}" = "1" ]; then
    _B1_RC=0
    _B1_OUT="$(bash "$PROVISION_SCRIPT" 2>/dev/null)" || _B1_RC=$?
    assert "B1: 2nd provision of already-mounted substrate exits 0 (idempotent no-op)" \
        test "$_B1_RC" -eq 0
    assert "B1: 2nd provision prints the same mount path (idempotent)" \
        test "$_B1_OUT" = "$_GATE_DIR"
else
    echo "B1: skipping provision idempotency (substrate was externally supplied, not self-provisioned)" >&2
fi

test_summary
