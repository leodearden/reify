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
#                   count equality; wall-time logged but not asserted — #4847).
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
_GATE_DIR=""            # set by detect_substrate to the reflink-capable dir
_GATE_DIR_CLEANUP=0     # 1 = we provisioned the mount; teardown on EXIT
_B11_GATE_DIR=""        # set by detect_private_substrate to B11's private dir
_B11_GATE_DIR_CLEANUP=0 # 1 = detect_private_substrate self-provisioned; teardown on EXIT
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
    if [ "${_GATE_DIR_CLEANUP:-0}" = "1" ] && [ -n "${_GATE_DIR:-}" ]; then
        ${REIFY_WARM_LANE_SUDO:-sudo} umount "${_GATE_DIR}" 2>/dev/null || true
    fi
    # Guarded `!=`: when detect_private_substrate's rung 3 reused the SAME
    # idempotent loopback the main gate already provisioned, _GATE_DIR_CLEANUP
    # above already tears it down — avoid a double-unmount.
    if [ "${_B11_GATE_DIR_CLEANUP:-0}" = "1" ] && [ -n "${_B11_GATE_DIR:-}" ] \
        && [ "${_B11_GATE_DIR}" != "${_GATE_DIR:-}" ]; then
        ${REIFY_WARM_LANE_SUDO:-sudo} umount "${_B11_GATE_DIR}" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Arm the shared-trash litter guard (task 5612). Sited immediately after
# `trap cleanup EXIT` because the helper registers its per-run root into
# _TMPDIRS and so must follow this file's own `_TMPDIRS=()`.
# Rationale, ordering contract, stem rules and honest scope: see the
# CANONICAL WIRING CONTRACT comment in tests/infra/test_helpers.sh.
init_isolated_lane_root test-warm-pool

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

# _seed_lane_capture <label> <target_dir> <seed invocation...>
#   Shared caller-obligation guard for every SEED-MODE (CoW-clone) call site in
#   this suite (scripts/seed-warm-lane.sh:63-79). Non-empty stdout (with exit
#   0) is the ONLY signal that a seed-mode invocation certifies <target_dir>
#   warm-safe to build in; its fail-closed post-condition guards
#   (inv.9/inv.12/inv.13) legitimately abort with a non-zero exit ONTO the
#   hazardous half-seeded state (target/ already CoW-cloned, sources already
#   bulk-stamped) rather than un-doing it themselves. Two obligations follow
#   for every caller, centralized here rather than copy-pasted per site
#   (task #5880 amendment; originally landed inline per-site by task #5875
#   [B13] and task #5880 [B6, B3+B4]):
#     1. rc + stdout MUST be captured via `|| rc=$?`, never a bare command
#        substitution assignment: under this file's `set -euo pipefail`, a
#        bare assignment aborts the ENTIRE SUITE with no PASS/FAIL lines and
#        no "Results:" tally the instant a guard fires.
#     2. On refusal, the half-seeded target/ must be discarded rather than
#        left for downstream code to build on — that would silently inherit
#        exactly the stale-artifact false green the guard fired to prevent.
#   Deliberately does NOT redirect the seed invocation's stderr: seed's `err`
#   diagnostics on a fail-closed abort are the diagnostic payload and must
#   stay visible in the run log.
#
#   Sets in caller scope (fixed names — callers copy these into their own
#   site-specific variables immediately after the call, same convention
#   _b13_build_fresh_counts below uses for _B13_FRESH_TOTAL/_B13_DEP_FRESH):
#     _SEED_CAP_RC  — seed's exit code
#     _SEED_CAP_OUT — seed's stdout (guaranteed empty on any refusal, by the
#                     caller-obligation contract itself)
#   Callers still decide, themselves, what to do on refusal beyond the target
#   discard performed here (e.g. skip a downstream build and record sentinels
#   so the affected asserts fail distinctly rather than measuring a
#   half-seeded/cold-rebuilt lane — see the call sites).
_seed_lane_capture() {
    local label="$1"
    local target_dir="$2"
    shift 2

    local rc=0
    local out=""
    out="$("$@")" || rc=$?
    _SEED_CAP_RC="$rc"
    _SEED_CAP_OUT="$out"

    if [ "$rc" -ne 0 ] || [ -z "$out" ]; then
        echo "$label: seed REFUSED to certify the lane warm-safe (rc=$rc, stdout=${out:-<empty>}) — removing the half-seeded target at $target_dir (caller obligation, scripts/seed-warm-lane.sh:63-79)" >&2
        rm -rf "$target_dir" 2>/dev/null || true
        if [ -d "$target_dir" ]; then
            echo "$label: WARNING — failed to remove the hazardous half-seeded target at $target_dir; it may still be present for a later step to (mis)measure" >&2
        fi
    fi
}

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

# Exercising the substrate-gated layer (Blocks B11/B13 etc.) on a pool host,
# without sudo/losetup: warm-lane worktrees live ON the XFS pool volume, so
# pointing TMPDIR at a scratch dir INSIDE the worktree makes rung 2's reflink
# probe below succeed —
#
#   mkdir -p .b11-scratch && TMPDIR="$PWD/.b11-scratch" bash tests/infra/test_warm_lane_pool.sh
#
# — and Blocks B11/B13 actually run for real. Under DEFAULT env (no TMPDIR
# override) both rung 1 and rung 2 fail on a pool host: /tmp is ext4 (no
# reflink support, so rung 2's default ${TMPDIR:-/tmp} probe fails), and the
# pool volume's ROOT is not writable from inside a lane's Landlock scope (so
# an explicit REIFY_WARM_LANE_MOUNT pointed at the volume root would fail
# rung 1 too). The whole substrate-gated layer then SKIPs ("SKIP: no XFS
# reflink substrate") and the suite reports an all-green result that proves
# NOTHING about B11/B13 — this is exactly the discoverability gap that let
# the B11 fixture bug (reader releasing flock -s before the flip's Step-6 GC
# ran) live on main undetected by the merge gate. Task #5866.

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

# detect_private_substrate() — B11-scoped substrate detector; sets _B11_GATE_DIR
#   on success. Mirrors detect_substrate() rung-for-rung, EXCEPT rung 2 (the
#   shared ${TMPDIR:-/tmp} scratch probe) is DELIBERATELY OMITTED: a
#   df --output=avail delta measured on shared /tmp is not immune to an
#   arbitrary concurrent disk-writer (OS process, other host tasks) diluting
#   the free-space reading between the before/after snapshots — that dilution
#   would reintroduce the exact flake this detector exists to remove
#   (task #4928). Only a private, dedicated mount (an explicit
#   REIFY_WARM_LANE_MOUNT override, or a loopback self-provisioned via
#   provision-warm-lane-fs.sh) is acceptable for B11's df-flatness measurement.
#   Returns 0 when a private reflink-capable directory is found, 1 otherwise.
#   Ladder (rung numbers mirror detect_substrate(); rung 2 omitted):
#     1. REIFY_WARM_LANE_MOUNT (env) — probe cp --reflink=always inside it.
#     3. REIFY_RUN_WARM_LANE_GATE=1 — provision ephemeral loopback via
#        provision-warm-lane-fs.sh; sets _B11_GATE_DIR_CLEANUP=1 for teardown.
detect_private_substrate() {
    local probe_src probe_dst
    probe_src=""
    probe_dst=""

    # 1. Caller-supplied mount
    if [ -n "${REIFY_WARM_LANE_MOUNT:-}" ] && [ -d "${REIFY_WARM_LANE_MOUNT}" ]; then
        probe_src="$(mktemp "${REIFY_WARM_LANE_MOUNT}/.reflink-probe-src-XXXXXX" 2>/dev/null)" || true
        if [ -n "$probe_src" ] && [ -f "$probe_src" ]; then
            probe_dst="${probe_src}.dst"
            if cp --reflink=always "$probe_src" "$probe_dst" 2>/dev/null; then
                rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
                _B11_GATE_DIR="${REIFY_WARM_LANE_MOUNT}"
                return 0
            fi
            rm -f "$probe_src" "$probe_dst" 2>/dev/null || true
        fi
        echo "detect_private_substrate: REIFY_WARM_LANE_MOUNT reflink probe failed" >&2
    fi

    # 2. Scratch-dir reflink probe in shared ${TMPDIR:-/tmp} — DELIBERATELY
    #    OMITTED here (see function header). detect_substrate() still runs it
    #    for the other substrate-gated blocks, which measure fresh-unit counts
    #    / coherence and are immune to free-space dilution.

    # 3. Opt-in ephemeral loopback via provision-warm-lane-fs.sh
    if [ "${REIFY_RUN_WARM_LANE_GATE:-}" = "1" ]; then
        local mount_out
        mount_out="$(bash "$PROVISION_SCRIPT" 2>/dev/null)" || {
            echo "detect_private_substrate: provision-warm-lane-fs.sh failed" >&2
            return 1
        }
        if [ -n "${mount_out:-}" ] && [ -d "${mount_out}" ]; then
            _B11_GATE_DIR="$mount_out"
            _B11_GATE_DIR_CLEANUP=1
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
#   3. Normalize lane_ws tree clean (provenance guard requires no WIP); compute
#      --landed-commit from lane_ws HEAD.
#   4. Run refresh-warm-base.sh to advance base_ws/target from lane_ws/target
#      (atomically re-points the base symlink; sibling_ws/target is NOT affected —
#      it holds CoW blocks referencing the ORIGINAL base gen, which stay live).
#
# After this call:
#   - sibling_ws/target  : original base gen clone (in-flight, independent)
#   - base_ws/target     : symlink to refreshed gen of lane_ws/target
#
# The sibling build should still find warm_dep fresh:true (sources 2020-01-01 <
# original base artifacts), proving the refresh did not touch the in-flight clone.
# `test -d base_ws/target` still holds: test -d follows symlinks.
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
    # base_ws/target may be a symlink; seed receives the path — seed resolves it.
    #
    # REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 is the semantically CORRECT
    # annotation here, not a workaround for §9.5 inv.13. This fixture builds its
    # base with REAL cargo, so target/<profile>/.fingerprint/ exists and the
    # clone crosses inv.13's hazard gate; and it deliberately exercises the D5
    # accept path — B6's in-flight-independence assertion DEPENDS on warm_dep's
    # sources staying at 2020-01-01 so the dep reports fresh:true against the
    # ORIGINAL base's artifacts. That is precisely the claim inv.13 refuses to
    # make unsubstantiated in production, and precisely what this arm is here to
    # measure. (Not git-init + --base-commit HEAD: see the note at the B3+B4
    # seed site for why re-ordering the commit would risk B7's own invariant.)
    # rc/stdout capture + refusal cleanup: see _seed_lane_capture (~line 168).
    _seed_lane_capture "B6" "$sibling_ws/target" \
        env REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 \
            RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
        bash "$SEED_SCRIPT" "$base_ws/target" "$sibling_ws" \
            --fresh-checkout \
            --touch "$sibling_ws/warm_leaf/src/lib.rs"
    _B6_SEED_RC="$_SEED_CAP_RC"
    _B6_SEED_OUT="$_SEED_CAP_OUT"

    # ── Step 2: Normalize lane_ws tree clean (required by provenance guard) ───
    # git checkout -- . resets tracked files; git clean -xfd -e target removes
    # untracked/ignored files (preserving target/ which is excluded).
    (cd "$lane_ws" && git checkout -- . 2>/dev/null && git clean -xfd -e target -q 2>/dev/null) || true

    # ── Step 3: Advance the base (refresh base/target from lane/target) ───────
    # Provenance guard (inv.9): pass --landed-commit with the current HEAD sha.
    # The sibling's target was cloned BEFORE this → its CoW blocks are independent.
    local _lane_head
    _lane_head="$(git -C "$lane_ws" rev-parse HEAD)"
    RUSTFLAGS="" \
        bash "$REFRESH_SCRIPT" "$lane_ws/target" "$base_ws/target" \
            --landed-commit "$_lane_head" >/dev/null
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

# _b11_concurrent_clone_during_flip(ws_root)
# SUBSTRATE-GATED: requires a reflink-capable filesystem at _GATE_DIR.
#
# Builds a warm at-head base (symlink-gen, gen.1) from a synth workspace on the
# substrate, then exercises torn-base coherence by running a concurrent
# CoW clone of the resolved gen.1 dir during a generation flip:
#
#   1. Build synth workspace in $ws_root/lane (cargo cold-build on substrate).
#   2. git-init the lane (committed, no target/) so the provenance guard passes.
#   3. First refresh: base.gen.1 created, $ws_root/base → base.gen.1 symlink.
#   4. Resolve $ws_root/base → concrete gen.1 path.
#   5. _spawn_pinning_reader: takes flock -s on gen.1.lock (D8 seam; holds
#      dir-entry refcount), clones gen.1_dir → clone/target, signals the
#      clone-done marker, then HOLDS flock -s until explicitly killed — the
#      lock's lifetime is the consumer's, not the copy's (task #5866; see
#      step 9).
#   6. Records _B11_DF_BEFORE_AVAIL (df --output=avail on substrate, MiB).
#   7. Flip: second refresh → base.gen.2, symlink re-pointed; GC attempt deferred
#      (reader still holds flock -s → flock -n -x fails → rm skipped).
#   8. Records _B11_DF_AFTER_AVAIL.
#   9. Waits for the clone-done marker (walk genuinely complete), then
#      kill+wait tears the reader down, releasing flock -s so the post-drain
#      refresh can reap gen.1.
#
# Sets in the caller's scope:
#   _B11_CLONE_DIR        — parent dir of the concurrent clone ($ws_root/clone)
#   _B11_PINNED_GEN       — absolute path of gen.1 (the concrete dir that was cloned)
#   _B11_DF_BEFORE_AVAIL  — df available MiB before the flip
#   _B11_DF_AFTER_AVAIL   — df available MiB after the flip
#
# Convention: creates $ws_root/lane (advancing lane, git-init'd and committed)
# and $ws_root/base (symlink-gen base); callers use these for post-drain ops.
_b11_concurrent_clone_during_flip() {
    local ws_root="$1"
    local _b11_lane="$ws_root/lane"
    local _b11_base="$ws_root/base"

    # Step 1: generate synth workspace in the lane dir and cold-build on substrate
    mkdir -p "$_b11_lane"
    gen_synth_workspace "$_b11_lane"
    echo "B11: cold build on substrate (this takes a moment)..." >&2
    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_b11_lane/Cargo.toml" >/dev/null 2>&1

    # Step 2: git-init the lane with the source tree committed (target/ excluded)
    # so the provenance guard (git status --porcelain --untracked-files=no) passes.
    git -C "$_b11_lane" init -q
    git -C "$_b11_lane" add -- . ':!target'
    git -C "$_b11_lane" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        commit -q -m "initial: B11 advancing lane"
    local _b11_head
    _b11_head="$(git -C "$_b11_lane" rev-parse HEAD)"

    # Step 3: first refresh — creates base.gen.1 on the substrate, symlink → gen.1
    RUSTFLAGS="" \
        bash "$REFRESH_SCRIPT" "$_b11_lane/target" "$_b11_base" \
            --landed-commit "$_b11_head" >/dev/null 2>&1

    # Step 4: resolve symlink → concrete gen.1 dir path
    local _b11_gen1
    _b11_gen1="$(readlink "$_b11_base")"
    local _b11_gen1_lock="${_b11_gen1}.lock"
    # _spawn_pinning_reader touches lock_file itself; no need to duplicate it here.

    # Step 5: _spawn_pinning_reader — holds flock -s on gen.1.lock across the
    # ENTIRE operation below (clone + flip + teardown), not just the cp -a walk.
    #
    # The reader signals copy completion via the clone-done marker (step 9
    # waits on it) but DELIBERATELY keeps flock -s held until explicitly
    # killed. This fixture's reader used to release flock -s the instant its
    # cp -a finished; on a reflink substrate that CoW copy (~120ms measured)
    # completes well before the flip's Step-6 GC sweep runs (~490ms measured
    # for the whole flip, refresh-warm-base.sh) — so the reader was always
    # gone by the time GC ran, GC correctly saw no active reader, and reaped
    # gen.1 out from under this test (task #5866). _wait_for_marker on the
    # ready marker only proves the lock was ACQUIRED, never that it is still
    # HELD later — the walk's duration is exactly what determines whether
    # the reader is still gone or still holding by the time GC fires.
    local _b11_clone_dir="$ws_root/clone"
    local _b11_ready="${_b11_gen1}.ready-marker"
    local _b11_clone_done="${_b11_gen1}.clone-done"
    mkdir -p "$_b11_clone_dir"
    # hold_s left as "" (falls through to the helper's default); reflink_mode
    # is pinned to "always" — B11 runs only once detect_substrate/
    # detect_private_substrate have confirmed a reflink-capable mount, so a
    # clone that silently degraded to a full byte copy here would be a real
    # substrate regression, not something to paper over with =auto (#5866).
    _spawn_pinning_reader "$_b11_gen1_lock" "$_b11_ready" "$_b11_clone_done" \
        "$_b11_gen1" "$_b11_clone_dir/target" "" always
    local _b11_reader_pid="$_PINNING_READER_PID"
    # Causal handshake: wait until reader has acquired flock -s (replaces fixed sleep 0.1 — #4847).
    _wait_for_marker "$_b11_ready" 30

    # Step 6: record df --output=avail before the flip (MiB)
    # Measured on ws_root (the block's actual workspace dir) rather than the
    # hard-coded gate dir: ws_root is placed on the private loopback when one
    # is available, so this always reports the fs the CoW clone/flip actually
    # consumed (task #4928).
    _B11_DF_BEFORE_AVAIL="$(df --output=avail -m "$ws_root" 2>/dev/null \
        | tail -1 | tr -d ' ' || echo 0)"

    # Step 7: flip — second refresh → base.gen.2, ln -sfn re-points symlink,
    # GC sweep: gen.1 lock is held (flock -n -x fails) → rm deferred.
    RUSTFLAGS="" \
        bash "$REFRESH_SCRIPT" "$_b11_lane/target" "$_b11_base" \
            --landed-commit "$_b11_head" >/dev/null 2>&1

    # Step 8: record df --output=avail after the flip (MiB)
    _B11_DF_AFTER_AVAIL="$(df --output=avail -m "$ws_root" 2>/dev/null \
        | tail -1 | tr -d ' ' || echo 0)"

    # Step 9: wait for the clone-done marker — a causal guarantee the cp -a
    # walk actually completed (technique R), not just that it started, so
    # the coherence diff never compares a partial tree — then tear the
    # reader down (kill+wait), releasing flock -s so the post-drain refresh
    # can reap gen.1. Generous anti-hang deadline (technique T); measured
    # slack is large (copy ~120ms vs. the flip alone ~490ms).
    _wait_for_marker "$_b11_clone_done" 300
    kill "$_b11_reader_pid" 2>/dev/null || true
    wait "$_b11_reader_pid" 2>/dev/null || true

    # Set output variables
    _B11_CLONE_DIR="$_b11_clone_dir"
    _B11_PINNED_GEN="$_b11_gen1"

    echo "B11: gen.1=$_b11_gen1 clone=$_b11_clone_dir df_before=${_B11_DF_BEFORE_AVAIL}MiB df_after=${_B11_DF_AFTER_AVAIL}MiB" >&2
}

# _b13_reseed_vs_resetinplace(ws_root)
# SUBSTRATE-GATED: requires a reflink-capable filesystem at _GATE_DIR.
#
# Tests the D10 always-re-seed-at-acquire contract: re-seeding a staled lane from
# an at-head base gives a WARMER first build than near-cold reset-in-place.
#
# Setup:
#   1. Build a warm at-head base: gen_synth_workspace in ws_root/base_ws, cold
#      build to populate base_ws/target, first refresh → ws_root/base (symlink-gen).
#   2. git-init the advancing lane (ws_root/base_ws, committed at mtime-2020-01-01
#      state via _b7_init_git_lane after seeding) so the provenance guard passes.
#   3. Create a staled lane in ws_root/stale_lane (cloned from base_ws, so it
#      shares git history with _b13_base_head): regress warm_dep to a single fn
#      and cold-build (target/ now holds artifacts for OLD dep code), restore
#      the at-head dep from git, then apply a non-trivial delta (one new fn in
#      warm_leaf) and commit on top of the clone. Reset-in-place ends up
#      near-cold because both the dep-restore and the leaf-delta land AFTER
#      the stale build — their mtimes out-date its recorded fingerprints, and
#      a clean committed tree means control's `git checkout -- .` cannot undo
#      that (it only preserves; it never re-stales).
#
# Control arm (reset-in-place):
#   4. git checkout -- . && git clean -xfd -e target in stale_lane (resets source
#      to the committed state, preserving the stale target/).
#   5. First build → _B13_CTRL_FRESH + _B13_CTRL_WALL_MS.
#
# Treatment arm (re-seed from at-head base):
#   6. Resolve ws_root/base symlink → concrete .gen.N path.
#   7. Call seed-warm-lane.sh --fresh-checkout <gen_dir> <stale_lane> (D10 path;
#      deliberately NO --touch — the leaf delta is already git-diff-visible
#      from the cloned history, so this exercises _touch_git_delta's
#      base-commit resolution directly instead of an explicit touch).
#   8. First build → _B13_TREAT_FRESH + _B13_TREAT_WALL_MS.
#
# Sets in caller scope:
#   _B13_CTRL_FRESH     — fresh compiler-artifact count for the control build
#   _B13_TREAT_FRESH    — fresh compiler-artifact count for the treatment build
#   _B13_CTRL_WALL_MS   — build wall-time for the control (ms)
#   _B13_TREAT_WALL_MS  — build wall-time for the treatment (ms)
#   _B13_SEED_RC        — exit code of the treatment seed-warm-lane.sh invocation
#   _B13_SEED_OUT       — stdout of the treatment seed-warm-lane.sh invocation
#                         (the caller-obligation contract: non-empty == warm-safe)
#   _B13_LEAF_MTIME     — post-seed mtime (epoch secs) of warm_leaf/src/lib.rs
#   _B13_DEP_MTIME      — post-seed mtime (epoch secs) of warm_dep/src/lib.rs
#   _B13_STALE_EPOCH    — epoch secs for the 2020-01-01 bulk-stamp (computed,
#                         TZ-robust; never the hardcoded 1577836800)
#   _B13_CTRL_DEP_FRESH  — tri-state "true"/"false"/"" for the control build's
#                          warm_dep unit freshness ("" when no warm_dep
#                          artifact line exists at all, e.g. the build failed)
#   _B13_TREAT_DEP_FRESH — tri-state "true"/"false"/"" for the treatment build's
#                          warm_dep unit freshness (also "" when the seed
#                          refused to certify and no treatment build ran)

# _b13_build_fresh_counts(lane_dir, json_out_path) — run `cargo build
# --message-format=json` on lane_dir's workspace (same env as every other
# fresh-unit measurement in this file: CARGO_INCREMENTAL=0 RUSTC_WRAPPER=""
# RUSTFLAGS=""), keep the compiler-artifact lines at json_out_path (callers
# place this under $ws_root so the suite's _TMPDIRS cleanup reclaims it), and
# set in caller scope:
#   _B13_FRESH_TOTAL — count of compiler-artifact lines with "fresh":true
#   _B13_DEP_FRESH   — tri-state "true"/"false"/"" (empty when no warm_dep
#                      artifact line exists at all — e.g. the build failed
#                      outright): NOT a 0/1 collapse, so a build that never
#                      reports on warm_dep cannot silently read the same as
#                      one that legitimately reports fresh:false. Mirrors B3's
#                      tri-state idiom (_B3_DEP_FRESH, ~line 2056).
# Used by BOTH the control and treatment arms so the two builds are measured
# identically; callers copy these two outputs into their own arm-specific
# _B13_*_FRESH / _B13_*_DEP_FRESH variables immediately after the call.
_b13_build_fresh_counts() {
    local lane_dir="$1"
    local json_out="$2"

    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$lane_dir/Cargo.toml" \
            --message-format=json 2>/dev/null \
        | grep '"reason":"compiler-artifact"' > "$json_out" || true

    _B13_FRESH_TOTAL="$(grep -c '"fresh":true' "$json_out" || true)"

    _B13_DEP_FRESH="$(grep '"name":"warm_dep"' "$json_out" | \
        grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"
}

_b13_reseed_vs_resetinplace() {
    local ws_root="$1"
    local _b13_base_ws="$ws_root/base_ws"
    local _b13_base="$ws_root/base"
    local _b13_stale_lane="$ws_root/stale_lane"

    # Step 1: build warm at-head base on the substrate
    mkdir -p "$_b13_base_ws"
    gen_synth_workspace "$_b13_base_ws"
    echo "B13: cold build for at-head base (this takes a moment)..." >&2
    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_b13_base_ws/Cargo.toml" >/dev/null 2>&1

    # Record base provenance sidecar (RUSTFLAGS guard for seed)
    # rc captured via `|| rc=$?` (record-base mode has no stdout-emptiness
    # caller obligation like seed mode — the sidecar path this prints is
    # discarded here, unused — but a bare failing command is still a plain
    # simple command under this file's `set -euo pipefail`, so an unguarded
    # non-zero exit here would abort the whole suite exactly like the
    # seed-mode call sites; task #5880 amendment).
    _B13_RECORD_BASE_RC=0
    RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
        bash "$SEED_SCRIPT" --record-base "$_b13_base_ws/target" >/dev/null \
            || _B13_RECORD_BASE_RC=$?

    # git-init base_ws at the mtime-2020-01-01 state so provenance guard passes
    _b7_init_git_lane "$_b13_base_ws"
    local _b13_base_head
    _b13_base_head="$(git -C "$_b13_base_ws" rev-parse HEAD)"

    # First refresh → ws_root/base (symlink) → ws_root/base.gen.1
    RUSTFLAGS="" \
        bash "$REFRESH_SCRIPT" "$_b13_base_ws/target" "$_b13_base" \
            --landed-commit "$_b13_base_head" >/dev/null 2>&1

    # Resolve base symlink → concrete gen path (D10: cp -a on symlink copies link)
    local _b13_gen
    _b13_gen="$(readlink "$_b13_base")"

    # Step 2: create the staled lane by CLONING the already-committed base
    # workspace, rather than an independent copy + standalone `git init`. The
    # lane needs GIT HISTORY that actually contains _b13_base_head: seed's
    # `_touch_git_delta` runs `git -C "$LANE_DIR" diff --name-only "$sha"`,
    # which fails closed (exit 128, "fatal: bad object") when `$sha` is not
    # reachable in the LANE's own repo — an independently `git init`-ed lane
    # never shares history with base_ws, no matter how the sha reaches it
    # (`.basecommit` or `--base-commit`). `--no-hardlinks` keeps the two object
    # stores independent so teardown order under ws_root cannot matter.
    # Confirmed properties: carries the committed Cargo.lock (base_ws is
    # committed after its own cold build, by _b7_init_git_lane above); never
    # carries target/ (untracked, excluded by _b7_init_git_lane's ':!target'
    # pathspec); lands at now-mtimes (irrelevant — seed's --fresh-checkout
    # bulk-stamp and this arm's own commit below are what set the mtimes that
    # matter).
    git clone --no-hardlinks --quiet "$_b13_base_ws" "$_b13_stale_lane"

    # Stale the target from a source state genuinely BEHIND head (not ahead of
    # it): regress the heavy dep to a single fn and cold-build — target/ now
    # holds artifacts for OLD dep code, and this is CHEAPER than building the
    # full dep_fns count. warm_leaf's only call is to fn_1 (gen_synth_workspace),
    # which the regressed dep still provides, so the build is valid.
    printf 'pub fn fn_1() -> u64 { 1 }\n' > "$_b13_stale_lane/warm_dep/src/lib.rs"
    echo "B13: cold build for stale lane (regressed dep)..." >&2
    CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_b13_stale_lane/Cargo.toml" >/dev/null 2>&1

    # Restore the at-head dep from git: content differs from the index (the
    # regression above), so checkout REWRITES the file — its mtime becomes now,
    # strictly newer than the stale build's just-recorded fingerprint. This is
    # what makes reset-in-place near-cold below: after the commit two lines
    # down, the tree is clean, so control's `git checkout -- .` is a no-op that
    # PRESERVES this mtime rather than re-staling it.
    git -C "$_b13_stale_lane" checkout -- warm_dep/src/lib.rs

    # Apply the head-ward delta to the LEAF (matches Setup-3 above) and commit
    # on top of the cloned base commit. warm_dep is already clean (the restore
    # above made it match the index), so this commit's -a picks up only the
    # leaf. Committing it here (rather than relying on an explicit --touch at
    # the treatment call below) means `git diff --name-only $_b13_base_head`
    # inside the lane already lists this path, so seed's _touch_git_delta
    # touches it on its own — the leaf-mtime assertion below then pins
    # base-commit resolution itself, not a trivially-satisfied explicit touch.
    printf '\npub fn delta_fn() -> u64 { 42 }\n' >> "$_b13_stale_lane/warm_leaf/src/lib.rs"
    git -C "$_b13_stale_lane" \
        -c user.email="warm-lane-test@localhost" \
        -c user.name="Warm Lane Test" \
        commit -qam "stale: B13 staled lane (head-ward delta in warm_leaf)"

    # ── CONTROL: reset-in-place rebuild ───────────────────────────────────────
    # Reset source to committed state; stale target/ preserved (-e target).
    # The stale target still lacks the at-head base artifacts → near-cold build.
    echo "B13: control — reset-in-place rebuild..." >&2
    (cd "$_b13_stale_lane" \
        && git checkout -- . 2>/dev/null \
        && git clean -xfd -e target -q 2>/dev/null) || true
    local _b13_ctrl_t0
    _b13_ctrl_t0="$(date +%s%3N)"
    _b13_build_fresh_counts "$_b13_stale_lane" "$ws_root/b13-ctrl-fresh.json"
    _B13_CTRL_FRESH="$_B13_FRESH_TOTAL"
    _B13_CTRL_DEP_FRESH="$_B13_DEP_FRESH"
    _B13_CTRL_WALL_MS=$(( $(date +%s%3N) - _b13_ctrl_t0 ))

    # ── TREATMENT: re-seed from at-head base ──────────────────────────────────
    # Resolve base → concrete gen dir (cp -a on symlink copies the link; must resolve)
    # Seed with --fresh-checkout (D10 always-re-seed-at-acquire path).
    # Remove the stale lane/target first so seed's clobber guard passes.
    echo "B13: treatment — re-seed from at-head base..." >&2
    rm -rf "$_b13_stale_lane/target" 2>/dev/null || true
    # rc/stdout capture + refusal cleanup: see _seed_lane_capture (~line 168).
    #
    # No --touch here (deliberately): the leaf delta is already committed on
    # top of the cloned base commit (see the commit above), so it is already
    # listed by `git diff --name-only $_b13_base_head` inside the lane. An
    # explicit --touch would touch that same path unconditionally, BEFORE base
    # resolution even runs — masking a base-commit-resolution regression
    # behind a leaf mtime that would look fine regardless. Leaving it out
    # makes the leaf-mtime assertion below a genuine pin on
    # _touch_git_delta's git-diff resolution. This weakens no guard: PRD
    # §9.5 inv.13 (_assert_delta_touch_base_substantiated) already keys only
    # on base-commit resolution, not on the explicit --touch list.
    _seed_lane_capture "B13" "$_b13_stale_lane/target" \
        env RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
        bash "$SEED_SCRIPT" "$_b13_gen" "$_b13_stale_lane" \
            --fresh-checkout
    _B13_SEED_RC="$_SEED_CAP_RC"
    _B13_SEED_OUT="$_SEED_CAP_OUT"

    # TZ-robust stale-bulk-stamp epoch (mirrors seed's own
    # _assert_no_stale_delta_stamp idiom, scripts/seed-warm-lane.sh:455) —
    # never the hardcoded 1577836800, which is only correct under TZ=UTC.
    _B13_STALE_EPOCH="$(date -d '2020-01-01T00:00:00' +%s)"

    # Honour the caller obligation: an empty stdout (or non-zero rc) means the
    # seed refused to certify the lane warm-safe, and it aborted ONTO the
    # hazardous state (CoW clone already in place, sources already bulk-stamped
    # to 2020-01-01). Building here would inherit exactly the stale-artifact
    # false green the guard fired to prevent, so skip the build entirely and
    # record a sentinel so the warmth comparison fails loudly too.
    if [ "$_B13_SEED_RC" -eq 0 ] && [ -n "$_B13_SEED_OUT" ]; then
        # Delta-touch set: pin WHAT the re-seed actually touched, immediately
        # after it returns and before the build can observe/alter anything.
        # The head-ward leaf must be newer than the 2020 bulk stamp — and,
        # since no --touch is passed for it (see the call above), that can
        # only come from _touch_git_delta's git-diff resolution, making this
        # a genuine pin on base-commit resolution. The unchanged heavy dep
        # must still BE at the 2020 bulk stamp — together, the delta-touch
        # set is exactly the head-ward delta, nothing more.
        _B13_LEAF_MTIME="$(stat -c '%Y' "$_b13_stale_lane/warm_leaf/src/lib.rs")"
        _B13_DEP_MTIME="$(stat -c '%Y' "$_b13_stale_lane/warm_dep/src/lib.rs")"

        local _b13_treat_t0
        _b13_treat_t0="$(date +%s%3N)"
        _b13_build_fresh_counts "$_b13_stale_lane" "$ws_root/b13-treat-fresh.json"
        _B13_TREAT_FRESH="$_B13_FRESH_TOTAL"
        _B13_TREAT_DEP_FRESH="$_B13_DEP_FRESH"
        _B13_TREAT_WALL_MS=$(( $(date +%s%3N) - _b13_treat_t0 ))
    else
        echo "B13: treatment seed REFUSED to certify the lane (rc=$_B13_SEED_RC, stdout=${_B13_SEED_OUT:-<empty>}) — skipping the treatment build rather than measuring a half-seeded lane (caller obligation, scripts/seed-warm-lane.sh:63-79)" >&2
        # Sentinels chosen so EVERY treatment-side assertion below FAILs
        # (rather than accidentally passing, or crashing reading a missing/
        # stale file): _B13_LEAF_MTIME equal to the stale epoch fails the
        # "-ne" leaf assertion; _B13_DEP_MTIME not equal to it fails the
        # "-eq" dep assertion; _B13_TREAT_FRESH=-1 fails the "-gt" aggregate
        # comparison; _B13_TREAT_DEP_FRESH="" (absent, matching the tri-state
        # "no build ran" case — never "false") fails the "= true" per-unit
        # dep-freshness check.
        _B13_LEAF_MTIME="$_B13_STALE_EPOCH"
        _B13_DEP_MTIME=-1
        _B13_TREAT_FRESH=-1
        _B13_TREAT_DEP_FRESH=""
        _B13_TREAT_WALL_MS=-1
    fi

    echo "B13: ctrl fresh=${_B13_CTRL_FRESH} wall=${_B13_CTRL_WALL_MS}ms  treat fresh=${_B13_TREAT_FRESH} wall=${_B13_TREAT_WALL_MS}ms" >&2
}

# _passset_normalize_nextest — pure stdin→stdout normalizer for `cargo nextest run`
# output.  Selects PASS/FAIL/SKIP lines, strips BOTH volatile columns nextest
# emits per result line — the bracketed duration (e.g. `[   0.012s]`) and the
# `(N/M)` progress counter — collapses internal whitespace, trims, and sorts →
# produces a byte-stable pass-set string suitable for comparison.
#
# The `(N/M)` counter must be stripped too, not just the timing (#5878): it
# encodes nondeterministic test-COMPLETION order (nextest runs test binaries
# in parallel), so leaving it in place makes the trailing `sort` order by
# completion rather than by test identifier — serializing an identical test
# set into two different byte strings depending on which test happened to
# finish first.
#
# nextest RIGHT-ALIGNS the counter's numerator to the width of the run
# total, so `(1/2)`, `( 1/12)`, and `(  1/105)` are all the same token with
# 0/1/2 leading spaces of padding — the strip regex must tolerate that
# padding. Without it, the strip silently no-ops on padded lines while
# still succeeding on unpadded ones in the very same run, yielding a
# partially-normalized, interleaved stream that is harder to diagnose than
# a clean failure (#5878).
#
# Used by run_passset's nextest branch and the PS-NORM always-run regression block.
_passset_normalize_nextest() {
    grep -E '^\s*(PASS|FAIL|SKIP)' \
    | sed -E 's/\[[^]]*\]//g' \
    | sed -E 's/\([[:space:]]*[0-9]+\/[0-9]+\)//' \
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
#     the volatile `[...]` timing column AND the `(N/M)` progress counter →
#     byte-stable across independent builds and parallel-run completion order).
#   - cargo test branch: `test ... ok/FAILED/ignored` lines (already timing-free
#     and counter-free) → grep + sort only (nothing volatile to strip).
# The output format is designed to be byte-comparable between two runs on
# semantically identical workspaces.
run_passset() {
    local manifest="$1"
    local test_output passed=0 failed=0 ignored=0

    if command -v cargo-nextest >/dev/null 2>&1 || \
       cargo nextest --version >/dev/null 2>&1; then
        # nextest: normalize via _passset_normalize_nextest (strips timing column + progress counter)
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

# _wait_for_marker <marker-file> <deadline-seconds>
# Generic causal-ordering (technique R) poll: waits for <marker-file> to
# appear, polling in 0.05s ticks, returning 0 as soon as it exists, or
# non-zero once the generous deadline elapses (technique T anti-hang guard).
# The deadline is deliberately generous — it is an anti-hang guard only,
# never a timing discriminator.
#
# Extracted from _wait_for_reader_lock (task #4847), which delegates to this
# for its flock-acquisition-specific marker; also used directly for a
# copy-completion handshake (_spawn_pinning_reader, task #5866).
_wait_for_marker() {
    local marker="$1"
    local deadline_s="$2"
    # Poll every 0.05s; max_ticks = deadline_s × 20
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R) for reader-readiness: polls for the READY
# marker file in 0.05s ticks, returning 0 as soon as it appears, or non-zero
# once the generous deadline elapses (technique T anti-hang guard).
#
# The READY marker is touched by the reader AFTER acquiring flock -s on the
# lock file, so returning 0 causally guarantees the reader holds flock -s at
# the caller's next statement.
#
# The deadline is deliberately generous (5-60s for normal scheduling) — it
# is an anti-hang guard only, never a timing discriminator.
#
# Used by Block RH (unit test) and the rewired SGSWAP3/SGSWAP4/GC/B11 fixtures.
# Task: #4847
_wait_for_reader_lock() {
    _wait_for_marker "$1" "$2"
}

# _spawn_pinning_reader <lock_file> <ready_marker> <clone_done_marker> <src> <dst> [hold_s] [reflink_mode]
# Backgrounds a reader that: takes flock -s on <lock_file>, signals
# <ready_marker>, runs `cp -a --reflink=<reflink_mode> <src> <dst>`, signals
# <clone_done_marker> — then DELIBERATELY keeps holding flock -s
# (`exec sleep hold_s`, default 3600) until the caller kills it.
#
# hold_s defaults to a large-but-bounded 3600s: a belt-and-braces anti-hang
# ceiling, not a timing dependency. The operation under test while the lock
# must stay held (a real cargo build + refresh-warm-base.sh flip, in B11's
# case) is not bounded by this file's design, so a short fixed window (the
# previous default of 120) can itself become a source of intermittent
# flakes on a loaded host — teardown is solely the caller's kill+wait.
# The subshell's final statement is `exec sleep`, not a bare `sleep`, so
# the PID returned to the caller is GUARANTEED — independent of bash's
# undocumented last-command fork-suppression optimization — to be the
# fd-205 holder: do not append a further command after it, or `kill` would
# reap an orphaned sleep that keeps flock -s (and thus the lock) held for
# the rest of the window. The subshell's stdout/stderr are redirected to
# /dev/null so a reader stranded by a caller bug (teardown skipped) cannot
# hold a capturing pipe (e.g. run_all.sh) open for up to hold_s.
#
# reflink_mode defaults to "auto": this helper is shared between the
# always-run Block BH (plain /tmp, no reflink support required or expected)
# and the substrate-gated B11 fixture (a confirmed reflink-capable mount).
# =auto opportunistically takes the CoW path whenever the underlying FS
# supports it and falls back to a normal copy elsewhere instead of
# hard-failing, which is what makes Block BH genuinely FS-agnostic. B11
# passes "always" explicitly so that a substrate which turns out not to be
# reflink-capable fails loud, rather than silently degrading to a full byte
# copy that would still pass every remaining B11 assertion.
#
# The shared lock models the D8 dir-entry refcount ("this gen must stay
# live"): its lifetime is the CONSUMER's, not the copy's. Signalling
# copy-completion via a separate marker (rather than releasing the lock
# there) is what lets a caller wait for "clone finished" and "lock
# released" as two independent, causally-ordered events instead of
# conflating them — the conflation is exactly today's B11 bug (task #5866).
#
# The caller is responsible for teardown: `kill "$_PINNING_READER_PID"
# 2>/dev/null || true; wait "$_PINNING_READER_PID" 2>/dev/null || true` —
# the same pattern as Block RH / SGSWAP3 / SGSWAP4 / Block GC.
#
# Sets in the caller's scope: _PINNING_READER_PID — the background PID.
# (A `pid=$(...)` capture is impossible here: command substitution would
# run the reader in a subshell that exits immediately, dropping the flock.)
#
# fd 205: 9, 200 and 201 are already in use elsewhere in this file.
# Task: #5866
_spawn_pinning_reader() {
    local lock_file="$1"
    local ready_marker="$2"
    local clone_done_marker="$3"
    local src="$4"
    local dst="$5"
    local hold_s="${6:-3600}"
    local reflink_mode="${7:-auto}"
    touch "$lock_file" 2>/dev/null || true
    (
        flock -s 205
        touch "$ready_marker"
        cp -a --reflink="$reflink_mode" "$src" "$dst"
        touch "$clone_done_marker"
        exec sleep "$hold_s"
    ) 205>"$lock_file" >/dev/null 2>&1 &
    _PINNING_READER_PID=$!
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
# FC_LANE/FC_LANE2 are LANE_DIR args passed to run_helper -> SEED_SCRIPT, whose
# --fresh-checkout acquire-time lane-lock flocks the SIBLING path
# "${LANE_DIR}.lock" (scripts/seed-warm-lane.sh) -- a path OUTSIDE whatever
# directory is registered in _TMPDIRS. A bare `mktemp -d /tmp/...` registered
# directly (as this used to do) leaves that sibling .lock unreachable by
# `rm -rf` on the registered dir, leaking it into machine-shared /tmp on every
# green run (task #5628; same shape task #5609 fixed for
# tests/infra/test_seed_warm_lane.sh). Nesting each lane under a private
# per-run parent -- and registering ONLY the parent -- makes the sibling lock
# a child of the registered dir, so `rm -rf "$FC_LANE_ROOT"` reclaims it.
FC_LANE_ROOT="$(mktemp -d /tmp/test-warm-pool-FC-lane-root-XXXXXX)"
_TMPDIRS+=("$FC_BASE_PARENT" "$FC_LANE_ROOT")
FC_LANE="$(mktemp -d "$FC_LANE_ROOT/FC-lane-XXXXXX")"
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
# Nested under FC_LANE_ROOT (see FC_LANE comment above) — NOT registered as
# its own _TMPDIRS entry — so its sibling ${FC_LANE2}.lock is reclaimed by
# the existing `rm -rf "$FC_LANE_ROOT"` rather than leaking into bare /tmp.
FC_LANE2="$(mktemp -d "$FC_LANE_ROOT/FC-lane2-XXXXXX")"
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
# Block PRIV — Private-substrate detector for B11's df measurement (ALWAYS-RUN)
#
# Unit-tests detect_private_substrate() (task #4928): a B11-scoped substrate
# detector that mirrors detect_substrate()'s rung 1 (REIFY_WARM_LANE_MOUNT env
# override) and rung 3 (provision-warm-lane-fs.sh self-provisioned loopback),
# but DELIBERATELY OMITS rung 2 (the shared ${TMPDIR:-/tmp} scratch probe). A
# df --output=avail delta measured on shared /tmp is not immune to a
# concurrent disk-writer (OS, other tasks) diluting the free-space reading;
# only a private, dedicated mount is acceptable for B11's df-flatness
# assertion.
#
# RED until step-2-impl: detect_private_substrate is undefined → command
# lookup fails (127) → the `if` condition sees non-zero → PRIV1 prints "FAIL"
# instead of the mount path → the assertion fails.
#
# No new numeric bound is introduced here (G6) — these assertions cover
# detector return-code/output behavior and the rung-2 bypass only.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PRIV: private-substrate detector (B11 df immune to shared /tmp) ---"

# ── PRIV1 (RED ANCHOR): rung-1 mount succeeds → returns 0, sets _B11_GATE_DIR
_PRIV1_FAKE_MOUNT="$(mktemp -d /tmp/test-warm-pool-PRIV1-XXXXXX)"
_TMPDIRS+=("$_PRIV1_FAKE_MOUNT")
_PRIV1_DIR="$(
    export REIFY_WARM_LANE_MOUNT="$_PRIV1_FAKE_MOUNT"
    export REIFY_RUN_WARM_LANE_GATE=""
    export REIFY_TEST_REFLINK_OK=1
    export PATH="$STUB_DIR:$PATH"
    if detect_private_substrate >/dev/null 2>&1; then
        printf '%s' "${_B11_GATE_DIR:-}"
    else
        printf 'FAIL'
    fi
)"

# ── PRIV2a (baseline contrast): unmodified detect_substrate succeeds via
# rung-2 shared ${TMPDIR:-/tmp} scratch probe when no mount/gate is given.
_PRIV2A_RC=1
(
    REIFY_WARM_LANE_MOUNT="" \
    REIFY_RUN_WARM_LANE_GATE="" \
    REIFY_TEST_REFLINK_OK=1 \
    PATH="$STUB_DIR:$PATH" \
        detect_substrate 2>/dev/null
) && _PRIV2A_RC=0 || _PRIV2A_RC=$?

# ── PRIV2b (rung-2 BYPASS): detect_private_substrate REJECTS the identical
# env — rung 2 is never attempted (only rung 1 / rung 3, both off here).
_PRIV2B_RC=0
(
    REIFY_WARM_LANE_MOUNT="" \
    REIFY_RUN_WARM_LANE_GATE="" \
    REIFY_TEST_REFLINK_OK=1 \
    PATH="$STUB_DIR:$PATH" \
        detect_private_substrate 2>/dev/null
) || _PRIV2B_RC=$?

assert "PRIV1: detect_private_substrate returns 0 via rung-1 mount and sets _B11_GATE_DIR" \
    test "$_PRIV1_DIR" = "$_PRIV1_FAKE_MOUNT"
assert "PRIV2a: baseline detect_substrate returns 0 via rung-2 shared /tmp scratch" \
    test "$_PRIV2A_RC" -eq 0
assert "PRIV2b: detect_private_substrate returns exactly 1 under the identical env (rung-2 bypassed, not a crash/127)" \
    test "$_PRIV2B_RC" -eq 1

# ─────────────────────────────────────────────────────────────────────────────
# Block HX — host-exclusive classification confirm (ALWAYS-RUN)
#
# Read-only preservation guard (task #4928): confirms test_warm_lane_pool.sh
# stays declared host-exclusive in the H1 manifest
# (tests/infra/run-all-classification.manifest). H7 keeps the file
# host-exclusive (PRD §3/H7) — this block does NOT edit the manifest, it only
# confirms the existing declaration so a future refinement that silently
# moves the file into the concurrent pool is caught here.
#
# Green-on-add: this should pass immediately on the base branch. If it is
# RED, the H1 manifest drifted and the task premise must be re-checked.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block HX: host-exclusive classification confirm ---"

# shellcheck source=tests/infra/run-all-classification-lib.sh
source "$REPO_ROOT/tests/infra/run-all-classification-lib.sh"
_HX_HOSTEXCL="$(classification_bucket host-exclusive 2>/dev/null || true)"

assert "HX: test_warm_lane_pool.sh stays declared host-exclusive in the H1 manifest" \
    bash -c 'printf "%s\n" "$1" | grep -qx "test_warm_lane_pool.sh"' _ "$_HX_HOSTEXCL"

# ─────────────────────────────────────────────────────────────────────────────
# Block RH — Reader-lock handshake unit tests (ALWAYS-RUN)
#
# Unit-tests _wait_for_reader_lock <ready-marker> <deadline-seconds>:
#   (RH-POS) positive case: background a flock -s reader that signals READY
#     after acquiring the lock, call the helper, then assert a foreground
#     flock -n -x probe FAILS — proving the reader genuinely holds the shared
#     lock once the helper returns (mirrors the real GC mechanism at
#     scripts/refresh-warm-base.sh:381).
#   (RH-NEG) anti-hang case: call with a never-created marker and a 1s
#     deadline; assert non-zero return (times out, does not hang forever).
#
# RED until step-2-impl-handshake-helper: _wait_for_reader_lock is undefined
# → command not found under set -euo pipefail → script aborts (non-zero exit).
# Task: #4847
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block RH: reader-lock handshake unit tests ---"

_RH_PARENT="$(mktemp -d /tmp/test-warm-pool-RH-XXXXXX)"
_TMPDIRS+=("$_RH_PARENT")
_RH_LOCK="$_RH_PARENT/rh-test.lock"
_RH_READY="$_RH_PARENT/rh-ready"
_RH_NONEXISTENT="$_RH_PARENT/rh-never-created"
touch "$_RH_LOCK" 2>/dev/null || true

# ── RH-POS: positive — helper returns only after reader holds flock -s ────────
# Background a reader: acquires flock -s, then signals READY by touching the
# marker file, then holds the lock for 60s to let the assertion window run.
( flock -s 9; touch "$_RH_READY"; sleep 60 ) 9>"$_RH_LOCK" &
_RH_READER_PID=$!
# Call the helper (undefined until step-2-impl-handshake-helper → command not
# found under set -euo pipefail → script aborts → RED)
_wait_for_reader_lock "$_RH_READY" 30
# Probe: foreground flock -n -x on the same lock file must FAIL (reader holds -s)
# Mirrors the real GC: flock -n -x "$lock" sh -c 'rm -rf ...' (refresh-warm-base.sh:381)
_RH_PROBE_RC=0
flock -n -x "$_RH_LOCK" true 2>/dev/null || _RH_PROBE_RC=$?
# Release the reader before the assertion (kill + reap)
kill "$_RH_READER_PID" 2>/dev/null || true
wait "$_RH_READER_PID" 2>/dev/null || true
assert "RH-POS: flock -n -x probe FAILS after handshake (reader provably holds flock -s)" \
    test "$_RH_PROBE_RC" -ne 0

# ── RH-NEG: anti-hang — helper returns non-zero when marker never appears ────
_RH_NEG_RC=0
_wait_for_reader_lock "$_RH_NONEXISTENT" 1 || _RH_NEG_RC=$?
assert "RH-NEG: _wait_for_reader_lock returns non-zero when marker never appears (anti-hang)" \
    test "$_RH_NEG_RC" -ne 0

# ─────────────────────────────────────────────────────────────────────────────
# Block BH — pinning-reader hold contract (ALWAYS-RUN)
#
# Unit-tests the _spawn_pinning_reader helper (from step-4 onward) that
# models a consumer holding a shared flock -s across an operation rather
# than releasing it the instant the operation completes — the seam B11's
# fixture was missing (task #5866).
#
# The generic marker-poll seam this helper is built on (_wait_for_marker,
# extracted from _wait_for_reader_lock, task #4847) is already unit-tested
# by Block RH's RH-POS/RH-NEG immediately above, via the delegating
# _wait_for_reader_lock — Block BH deliberately does NOT retest that same
# poll contract a second time under the new name (that would be the same
# code path covered twice under two names). The handshake waits below are
# plumbing to set up BH1-BH3, guarded so a stuck handshake FAILs by name
# instead of aborting the suite under set -euo pipefail.
#
# _spawn_pinning_reader <lock> <ready> <clone-done> <src> <dst> contract
# (task #5866 — the actual regression this block exists to guard):
#   (BH1) the concurrent clone lands and is byte-identical to src.
#   (BH2, REGRESSION ANCHOR) a foreground flock -n -x probe on the lock
#     FAILS even though the clone-done marker is already present — proving
#     the reader still holds flock -s after its copy finished. This is
#     exactly what today's B11 reader (whose subshell ends at the cp) does
#     NOT do, and exactly what the flip's Step-6 GC
#     (scripts/refresh-warm-base.sh:428) tests with the identical
#     flock -n -x idiom.
#   (BH3, negative control) after kill+wait teardown, the same probe
#     SUCCEEDS — proving teardown genuinely releases the lock, so a
#     post-drain GC still has a free lock to reap against.
#
# RED until step-4: _spawn_pinning_reader is undefined → command not found
# under set -euo pipefail → non-zero exit.
# Task: #5866
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block BH: pinning-reader hold contract ---"

_BH_PARENT="$(mktemp -d /tmp/test-warm-pool-BH-XXXXXX)"
_TMPDIRS+=("$_BH_PARENT")

# ── Setup: a tiny 3-file source dir + lock/ready/clone-done paths — plain
# flock, no reflink required, so this runs on every host (merge gate incl.).
_BH_SRC="$_BH_PARENT/src"
mkdir -p "$_BH_SRC"
echo "one"   > "$_BH_SRC/a"
echo "two"   > "$_BH_SRC/b"
echo "three" > "$_BH_SRC/c"
_BH_DST="$_BH_PARENT/dst"
_BH_LOCK="$_BH_PARENT/pin.lock"
_BH_READY="$_BH_PARENT/pin-ready"
_BH_CLONE_DONE="$_BH_PARENT/pin-clone-done"

# Call the helper (undefined until step-4 → command not found under
# set -euo pipefail → script aborts → RED).
_spawn_pinning_reader "$_BH_LOCK" "$_BH_READY" "$_BH_CLONE_DONE" "$_BH_SRC" "$_BH_DST"
_BH_READER_PID="$_PINNING_READER_PID"

# Guarded handshake waits: capture the rc via `|| var=$?` (as BH-NEG-style
# calls elsewhere in this file already do) rather than calling
# _wait_for_marker bare — a bare non-zero return here would trip
# set -euo pipefail and abort the whole suite mid-block, surfacing as an
# unattributable abort instead of a named FAIL line in run_all.sh's
# per-test summary.
_BH_HS_READY_RC=0
_wait_for_marker "$_BH_READY" 30 || _BH_HS_READY_RC=$?
assert "BH-HANDSHAKE: _spawn_pinning_reader signals ready within deadline" \
    test "$_BH_HS_READY_RC" -eq 0
_BH_HS_CLONE_RC=0
_wait_for_marker "$_BH_CLONE_DONE" 30 || _BH_HS_CLONE_RC=$?
assert "BH-HANDSHAKE: _spawn_pinning_reader signals clone-done within deadline" \
    test "$_BH_HS_CLONE_RC" -eq 0

# ── BH1: the concurrent clone actually landed and matches src ──
assert "BH1: _spawn_pinning_reader's clone lands (dst exists)" \
    test -d "$_BH_DST"
assert "BH1: _spawn_pinning_reader's clone is byte-identical to src" \
    diff -r "$_BH_SRC" "$_BH_DST"

# ── BH2 (REGRESSION ANCHOR): reader still holds flock -s AFTER its copy
# completed. Mirrors RH-POS's probe idiom (flock -n -x on the same lock).
_BH2_PROBE_RC=0
flock -n -x "$_BH_LOCK" true 2>/dev/null || _BH2_PROBE_RC=$?
assert "BH2: flock -n -x probe FAILS after clone-done (reader still holds flock -s post-copy)" \
    test "$_BH2_PROBE_RC" -ne 0

# ── BH3 (negative control): kill+wait teardown genuinely releases the lock ──
kill "$_BH_READER_PID" 2>/dev/null || true
wait "$_BH_READER_PID" 2>/dev/null || true
_BH3_PROBE_RC=1
flock -n -x "$_BH_LOCK" true 2>/dev/null && _BH3_PROBE_RC=0 || _BH3_PROBE_RC=$?
assert "BH3: flock -n -x probe SUCCEEDS after kill+wait teardown (lock released)" \
    test "$_BH3_PROBE_RC" -eq 0

# ─────────────────────────────────────────────────────────────────────────────
# Block PS-NORM — Pass-set normalizer volatile-column regression (ALWAYS-RUN)
#
# Exercises run_passset()'s nextest-branch normalization WITHOUT invoking cargo.
# Covers BOTH volatile columns real nextest output carries per result line:
# the bracketed per-test duration (`[   0.0NNs]`) and the `(N/M)` progress
# counter.
#
# Arm 1 (timing): feeds two canned `cargo nextest run` outputs that are
# byte-identical EXCEPT for the duration column through
# _passset_normalize_nextest and asserts:
#   (a) the two normalized outputs are BYTE-IDENTICAL;
#   (b) their derived PASS/FAIL counts match.
# Premise (exactness): the inputs differ ONLY inside the bracketed `[...]`
# token; the normalizer strips every `[...]` token so post-normalization byte
# streams are identical by construction.
#
# Arm 2 (progress counter, #5878): feeds two canned outputs shaped like REAL
# nextest 0.9.136 output — every line carries BOTH the `[...]` duration AND
# the `(N/M)` counter, the counter positioned after the bracket, matching a
# probe of actual nextest output — that differ in the counter-TO-TEST
# binding (i.e. which test won the completion race), mirroring the genuine
# nondeterminism of nextest's parallel test execution, and asserts the same
# (a)/(b) pair.
# Premise (exactness): the inputs differ ONLY inside the `[...]` and `(N/M)`
# tokens, both of which the normalizer strips; post-normalization byte
# streams contain the same token multiset and `sort` is a total order on
# test identifiers, so byte-identity is a theorem, not a tolerance.
#
# Arm 1 without timing-column stripping: the `[   0.0NNs]` column survives →
# strings differ → assertion (a) fails → RED. (Historically, before
# _passset_normalize_nextest existed at all, this block instead aborted the
# whole script under set -euo pipefail with "command not found".)
#
# Arm 2 without progress-counter stripping: the `(N/M)` counter survives, its
# binding to a test varies by completion order, and the trailing `sort` keys
# off it → strings differ → assertion (a) fails → RED (#5878). The function
# already exists at this point, so this arm's RED is an assertion failure,
# not a script abort.
#
# Also asserts cargo-test fallback lines (already timing-free) are sort-stable
# across different emission orderings (regression guard).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PS-NORM: pass-set normalizer volatile-column regression ---"

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

# ── Canned nextest outputs (#5878) — nextest right-aligns the `(N/M)` counter's
# numerator to the width of the run total, so the counter has THREE observed
# shapes: unpadded at ≤9 total (`(1/2)`), one-space-padded at 10-99 total
# (`( 1/12)`), and two-space-padded at 100+ total (`(  1/105)`). This arm pins
# the UNPADDED width — every line carries both the `[...]` duration AND the
# unpadded `(N/M)` counter (probed against real nextest 0.9.136). Differ in
# the counter-TO-TEST binding (which test won the completion race), mirroring
# nextest's genuine parallel-run nondeterminism — not just in counter values.
# Arm 3 (below) pins the padded widths.
_PSNORM_CTR_COLD_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.017s] (1/2) warm_leaf tests::leaf_smoke
  PASS [   0.024s] (2/2) warm_dep tests::dep_smoke
PSNORM_EOF
)"
_PSNORM_CTR_WARM_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.031s] (1/2) warm_dep tests::dep_smoke
  PASS [   0.052s] (2/2) warm_leaf tests::leaf_smoke
PSNORM_EOF
)"

# Normalize through the same helper. Discriminating power: a normalizer
# that leaves the `(N/M)` counter unstripped keeps a token that encodes
# nondeterministic test-completion order, so the trailing `sort` keys off
# it — swapping which test claims which counter slot then yields two
# different byte streams → assertion (a) fails (#5878). The shipped
# normalizer strips the counter → GREEN.
_PSNORM_CTR_COLD_NORM="$(printf '%s\n' "$_PSNORM_CTR_COLD_INPUT" | _passset_normalize_nextest)"
_PSNORM_CTR_WARM_NORM="$(printf '%s\n' "$_PSNORM_CTR_WARM_INPUT" | _passset_normalize_nextest)"

assert "PS-NORM: nextest normalized output is byte-identical across progress-counter assignment (#5878)" \
    test "$_PSNORM_CTR_COLD_NORM" = "$_PSNORM_CTR_WARM_NORM"

# ── Derived PASS count must be exactly 2 (the preceding byte-identity
# assertion already proves cold == warm, so cross-comparing the two counts
# would be tautological; asserting the absolute value instead genuinely
# discriminates a normalizer that drops or duplicates a result line) ──────
_PSNORM_CTR_COLD_PASS="$(printf '%s\n' "$_PSNORM_CTR_COLD_NORM" | grep -c 'PASS' || echo 0)"
_PSNORM_CTR_WARM_PASS="$(printf '%s\n' "$_PSNORM_CTR_WARM_NORM" | grep -c 'PASS' || echo 0)"
assert "PS-NORM: derived PASS count for progress-counter arm (cold) is exactly 2 (#5878)" \
    test "$_PSNORM_CTR_COLD_PASS" -eq 2
assert "PS-NORM: derived PASS count for progress-counter arm (warm) is exactly 2 (#5878)" \
    test "$_PSNORM_CTR_WARM_PASS" -eq 2

# ── Canned nextest outputs (#5878) — PADDED progress-counter shapes. nextest
# right-aligns the `(N/M)` numerator to the width of the run total: a 12-test
# run pads single-digit numerators with ONE leading space (`( 1/12)`) while
# double-digit numerators need no padding (`(12/12)`); a 105-test run pads
# single-digit numerators with TWO leading spaces (`(  1/105)`) while
# triple-digit numerators need no padding (`(105/105)`) — so a single run's
# output MIXES padded and unpadded counters. This arm mixes BOTH the
# one-space (10-99 total) and two-space (100+ total) widths on one fixture
# pair, differing in the counter-TO-TEST binding as arm 2 does — so all
# three documented counter widths (arm 2's unpadded plus this arm's two
# padded widths) are pinned by fixtures.
_PSNORM_PAD_COLD_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.009s] ( 1/12) warm_leaf tests::leaf_smoke
  PASS [   0.031s] (12/12) warm_dep tests::dep_smoke
  PASS [   0.011s] (  1/105) nxprobe2 tests::t001
  PASS [   0.044s] (105/105) nxprobe2 tests::t105
PSNORM_EOF
)"
_PSNORM_PAD_WARM_INPUT="$(cat << 'PSNORM_EOF'
  PASS [   0.014s] ( 1/12) warm_dep tests::dep_smoke
  PASS [   0.052s] (12/12) warm_leaf tests::leaf_smoke
  PASS [   0.017s] (  1/105) nxprobe2 tests::t105
  PASS [   0.061s] (105/105) nxprobe2 tests::t001
PSNORM_EOF
)"

# Normalize through the same helper. Discriminating power: a counter regex
# that anchors `(` directly against a digit leaves padded tokens like
# `( 1/12)` and `(  1/105)` intact while stripping unpadded tokens like
# `(12/12)` and `(105/105)` on the SAME fixture, producing a
# partially-normalized, interleaved stream whose sort order flips →
# assertion (a) fails (#5878). The shipped `[[:space:]]*`-tolerant regex
# strips every width uniformly → GREEN.
_PSNORM_PAD_COLD_NORM="$(printf '%s\n' "$_PSNORM_PAD_COLD_INPUT" | _passset_normalize_nextest)"
_PSNORM_PAD_WARM_NORM="$(printf '%s\n' "$_PSNORM_PAD_WARM_INPUT" | _passset_normalize_nextest)"

assert "PS-NORM: nextest normalized output is byte-identical across PADDED progress-counter assignment (#5878)" \
    test "$_PSNORM_PAD_COLD_NORM" = "$_PSNORM_PAD_WARM_NORM"

# ── Derived PASS count must be exactly 4 (the preceding byte-identity
# assertion already proves cold == warm, so cross-comparing the two counts
# would be tautological; asserting the absolute value instead genuinely
# discriminates a normalizer that drops or duplicates a result line) ──────
_PSNORM_PAD_COLD_PASS="$(printf '%s\n' "$_PSNORM_PAD_COLD_NORM" | grep -c 'PASS' || echo 0)"
_PSNORM_PAD_WARM_PASS="$(printf '%s\n' "$_PSNORM_PAD_WARM_NORM" | grep -c 'PASS' || echo 0)"
assert "PS-NORM: derived PASS count for padded progress-counter arm (cold) is exactly 4 (#5878)" \
    test "$_PSNORM_PAD_COLD_PASS" -eq 4
assert "PS-NORM: derived PASS count for padded progress-counter arm (warm) is exactly 4 (#5878)" \
    test "$_PSNORM_PAD_WARM_PASS" -eq 4

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
# Block PG — Provenance-guard refusal + rename-negative-control (ALWAYS-RUN)
#
# B12: provenance-guard refusal assertions — exercising the REAL
#   refresh-warm-base.sh under the passthrough cp stub:
#   (a) WIP dirty lane + any --landed-commit → REFUSED (WIP check fires first)
#   (b) missing --landed-commit on clean lane → REFUSED (provenance required)
#   (c) wrong --landed-commit sha on clean lane → REFUSED (head mismatch)
#   (+) positive: clean lane + correct sha → exits 0 (form-agnostic; NOT
#       asserting dir-vs-symlink — that is step-3's job)
# B11 NEGATIVE CONTROL (always-run, FS-agnostic): plain mv -T over a populated
#   dir fails with ENOTEMPTY — documents why symlink-gen is required.
#
# RED until step-2 (provenance guard); B11-NC is GREEN immediately.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PG: provenance-guard refusal + rename negative control ---"

# ── PG fixtures ──────────────────────────────────────────────────────────────
_PG_PARENT="$(mktemp -d /tmp/test-warm-pool-PG-XXXXXX)"
_TMPDIRS+=("$_PG_PARENT")

# Advancing lanes — each lives in its own subdir so their git repos don't nest.
# _mk_*_advancing_lane creates $parent/lane; outputs that path.
_PG_WIP_LANE="$(_mk_wip_advancing_lane "$_PG_PARENT/wip")"
_PG_CLEAN_LANE_B="$(_mk_clean_advancing_lane "$_PG_PARENT/cln-b")"
_PG_CLEAN_LANE_C="$(_mk_clean_advancing_lane "$_PG_PARENT/cln-c")"
_PG_CLEAN_LANE_POS="$(_mk_clean_advancing_lane "$_PG_PARENT/pos")"

# Correct HEAD sha for the WIP lane (WIP check fires before sha comparison)
# and for the positive control lane.
_PG_WIP_HEAD="$(git -C "$_PG_WIP_LANE" rev-parse HEAD)"
_PG_POS_HEAD="$(git -C "$_PG_CLEAN_LANE_POS" rev-parse HEAD)"

# Base dirs (absent initially; parent is $_PG_PARENT which exists)
_PG_BASE_A="$_PG_PARENT/base-a"
_PG_BASE_B="$_PG_PARENT/base-b"
_PG_BASE_C="$_PG_PARENT/base-c"
_PG_BASE_POS="$_PG_PARENT/base-pos"

# ── B12a: WIP dirty lane → refused ───────────────────────────────────────────
# Passes the lane's own HEAD as --landed-commit (correct sha, but WIP check
# fires first and must refuse regardless of the sha).
_refresh_capture "$_PG_WIP_LANE/target" "$_PG_BASE_A" \
    --landed-commit "$_PG_WIP_HEAD"
assert "B12a: WIP lane refresh exits non-zero (provenance refused)" \
    test "$RC" -ne 0
assert "B12a: stderr names WIP/provenance refusal (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "wip|dirty|uncommitted|provenance|tracked"' _ "$ERR_OUT"
assert "B12a: base NOT created after WIP refusal (no swap)" \
    bash -c '[ ! -e "$1" ]' _ "$_PG_BASE_A"

# ── B12b: missing --landed-commit on clean lane → refused ─────────────────────
# No --landed-commit at all; a clean lane that would otherwise succeed.
_refresh_capture "$_PG_CLEAN_LANE_B/target" "$_PG_BASE_B"
assert "B12b: missing --landed-commit exits non-zero (provenance refused)" \
    test "$RC" -ne 0
assert "B12b: stderr names landed-commit/provenance required (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "landed.commit|provenance"' _ "$ERR_OUT"
assert "B12b: base NOT created after missing --landed-commit (no swap)" \
    bash -c '[ ! -e "$1" ]' _ "$_PG_BASE_B"

# ── B12c: wrong --landed-commit sha → refused ─────────────────────────────────
_refresh_capture "$_PG_CLEAN_LANE_C/target" "$_PG_BASE_C" \
    --landed-commit "0000000000000000000000000000000000000000"
assert "B12c: wrong --landed-commit sha exits non-zero (head mismatch refused)" \
    test "$RC" -ne 0
assert "B12c: stderr names head mismatch (actionable)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "head|mismatch"' _ "$ERR_OUT"
assert "B12c: base NOT created after head mismatch refusal (no swap)" \
    bash -c '[ ! -e "$1" ]' _ "$_PG_BASE_C"

# ── Positive control: clean lane + correct --landed-commit → exits 0 ──────────
# Form-agnostic: asserts RC=0 only; NOT dir-vs-symlink (step-3's job).
_refresh_capture "$_PG_CLEAN_LANE_POS/target" "$_PG_BASE_POS" \
    --landed-commit "$_PG_POS_HEAD"
assert "PG-pos: clean lane + correct --landed-commit exits 0 (positive control)" \
    test "$RC" -eq 0

# ── B11 NEGATIVE CONTROL: mv -T over populated dir fails (ENOTEMPTY) ──────────
# Always-run, FS-agnostic (coreutils). Documents why symlink-gen is required:
# plain rename(2) / mv -T CANNOT replace a non-empty directory
# (host-confirmed ENOTEMPTY errno 39; test is immediately GREEN).
_B11_NC_SRC="$(mktemp -d /tmp/test-warm-pool-B11NC-src-XXXXXX)"
_B11_NC_DST="$(mktemp -d /tmp/test-warm-pool-B11NC-dst-XXXXXX)"
_TMPDIRS+=("$_B11_NC_SRC" "$_B11_NC_DST")
touch "$_B11_NC_DST/existing-file"
_B11_NC_RC=0
_B11_NC_ERR="$(mv -T "$_B11_NC_SRC" "$_B11_NC_DST" 2>&1)" || _B11_NC_RC=$?
assert "B11-NC: mv -T over populated dir exits non-zero (ENOTEMPTY — symlink-gen required)" \
    test "$_B11_NC_RC" -ne 0
assert "B11-NC: mv -T error names directory not empty (ENOTEMPTY message)" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "not empty|directory"' _ "$_B11_NC_ERR"

# ─────────────────────────────────────────────────────────────────────────────
# Block PG: symlink-gen swap mechanics (ALWAYS-RUN)
#
# Via _refresh_capture under the passthrough cp stub:
#   (SGSWAP1+2) First refresh on absent base → <base> is a SYMLINK whose
#     target is a real dir <base>.gen.<N> populated with the advancing content.
#   (SGSWAP3) Bootstrap: refreshing when <base> is a pre-existing REAL dir
#     converts it to the symlink-gen scheme (old content renamed intact to a
#     retired gen, <base> becomes a symlink to the NEW gen); a flock -s reader
#     is held on gen.1.lock during refresh so the in-refresh GC defers rm —
#     the retained retired gen is then asserted concretely (mirrors SGSWAP4).
#   (SGSWAP4) Second refresh: advances to <base>.gen.<N+1>, atomically re-points
#     the symlink (readlink changes), and the prior gen dir still exists on disk
#     immediately after the flip (retained, not yet GC'd).
#
# RED until step-4 (symlink-gen swap impl): current script does the .new/.old
# mv dance — <base> stays a REAL dir (never a symlink) → symlink assertions fail.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PG: symlink-gen swap mechanics ---"

# ── SGSWAP fixtures ──────────────────────────────────────────────────────────
_SGSWAP_PARENT="$(mktemp -d /tmp/test-warm-pool-SGSWAP-XXXXXX)"
_TMPDIRS+=("$_SGSWAP_PARENT")

# One clean advancing lane shared by all sub-tests
_SGSWAP_LANE="$(_mk_clean_advancing_lane "$_SGSWAP_PARENT/adv")"
_SGSWAP_LANE_HEAD="$(git -C "$_SGSWAP_LANE" rev-parse HEAD)"

# Base paths (all absent initially or set up per sub-test)
_SGSWAP_BASE_FRESH="$_SGSWAP_PARENT/base-fresh"    # absent → SGSWAP1+2
_SGSWAP_BASE_BOOT="$_SGSWAP_PARENT/base-boot"      # pre-existing real dir → SGSWAP3
_SGSWAP_BASE_SECOND="$_SGSWAP_PARENT/base-second"  # absent → two refreshes → SGSWAP4

# ── SGSWAP1+2: first refresh on absent base → symlink → populated gen dir ─────
_refresh_capture "$_SGSWAP_LANE/target" "$_SGSWAP_BASE_FRESH" \
    --landed-commit "$_SGSWAP_LANE_HEAD"
assert "SGSWAP1: first refresh on absent base exits 0" \
    test "$RC" -eq 0
assert "SGSWAP1: <base> is a SYMLINK after first refresh" \
    test "$REFRESH_BASE_IS_SYMLINK" = "1"
assert "SGSWAP1: symlink target is a real <base>.gen.<N> dir" \
    bash -c '[ -d "$1.gen.1" ] || ls -d "$1".gen.* 2>/dev/null | grep -qv partial' \
        _ "$_SGSWAP_BASE_FRESH"
assert "SGSWAP2: symlink resolves to populated dir (advancing content present)" \
    bash -c '[ -f "$(readlink -f "$1")/debug/placeholder" ]' _ "$_SGSWAP_BASE_FRESH"

# ── SGSWAP3: bootstrap — pre-existing REAL dir base → convert to symlink-gen ──
# Bootstrap renames the pre-existing real dir to a retired gen (old content
# preserved intact under the new name, not clobbered/recreated); a flock -s
# reader is held on gen.1.lock during the refresh so the in-refresh GC's
# flock -n -x fails → gen.1 is retained for the assertion (mirrors SGSWAP4).
mkdir -p "$_SGSWAP_BASE_BOOT/debug"
echo "pre-existing-content" > "$_SGSWAP_BASE_BOOT/debug/old-artifact"
# Hold a reader lock on gen.1 BEFORE the bootstrap refresh. Rationale: a fresh
# bootstrap (no pre-existing .gen.*) deterministically computes next_gen=1,
# renames the real dir to .gen.1, then creates .gen.2; holding flock -s on
# gen.1.lock makes the in-refresh GC's flock -n -x fail → gen.1 is retained.
_SGSWAP3_LOCK="${_SGSWAP_BASE_BOOT}.gen.1.lock"
_SGSWAP3_READY="$_SGSWAP_PARENT/sgswap3-reader-ready"
touch "$_SGSWAP3_LOCK" 2>/dev/null || true
( flock -s 201; touch "$_SGSWAP3_READY"; sleep 120 ) 201>"$_SGSWAP3_LOCK" &
_SGSWAP3_READER_PID=$!
# Causal handshake: wait until reader has acquired flock -s (replaces fixed sleep 0.1 — #4847)
_wait_for_reader_lock "$_SGSWAP3_READY" 30
_refresh_capture "$_SGSWAP_LANE/target" "$_SGSWAP_BASE_BOOT" \
    --landed-commit "$_SGSWAP_LANE_HEAD"
# Capture live gen + identify the retained retired gen before releasing the reader.
_SGSWAP3_LIVE_GEN="$(readlink "$_SGSWAP_BASE_BOOT" 2>/dev/null || echo "")"
_SGSWAP3_RETIRED_GEN=""
for _sgd in "${_SGSWAP_BASE_BOOT}.gen."*; do
    [ -d "$_sgd" ] || continue
    _sgn="${_sgd##*.gen.}"
    case "$_sgn" in *[!0-9]*) continue ;; esac
    [ "$_sgd" != "$_SGSWAP3_LIVE_GEN" ] && _SGSWAP3_RETIRED_GEN="$_sgd"
done
# Release the reader lock (was only needed to hold gen.1 alive during the refresh)
kill "$_SGSWAP3_READER_PID" 2>/dev/null || true; wait "$_SGSWAP3_READER_PID" 2>/dev/null || true
assert "SGSWAP3: bootstrap refresh on real-dir base exits 0" \
    test "$RC" -eq 0
assert "SGSWAP3: <base> is a SYMLINK after bootstrap refresh" \
    test "$REFRESH_BASE_IS_SYMLINK" = "1"
assert "SGSWAP3: a concrete retired gen was identified (not reaped during refresh)" \
    bash -c '[ -n "$1" ]' _ "$_SGSWAP3_RETIRED_GEN"
assert "SGSWAP3: retired gen dir still exists on disk" \
    bash -c '[ -d "$1" ]' _ "$_SGSWAP3_RETIRED_GEN"
assert "SGSWAP3: retired gen holds original old content (renamed intact, not clobbered)" \
    bash -c '[ -f "$1/debug/old-artifact" ] && [ "$(cat "$1/debug/old-artifact")" = "pre-existing-content" ]' \
        _ "$_SGSWAP3_RETIRED_GEN"
assert "SGSWAP3: retired gen is a different dir than the live gen (anti-tautology)" \
    bash -c '[ "$1" != "$2" ]' _ "$_SGSWAP3_RETIRED_GEN" "$_SGSWAP3_LIVE_GEN"
assert "SGSWAP3: <base> symlink points to new gen (advancing content present)" \
    bash -c '[ -f "$(readlink -f "$1")/debug/placeholder" ]' _ "$_SGSWAP_BASE_BOOT"

# ── SGSWAP4: second refresh → gen advances, prior gen retained (GC deferred) ──
# First refresh: creates gen.1
_refresh_capture "$_SGSWAP_LANE/target" "$_SGSWAP_BASE_SECOND" \
    --landed-commit "$_SGSWAP_LANE_HEAD"
_SGSWAP_GEN1_LINK="$(readlink "$_SGSWAP_BASE_SECOND" 2>/dev/null || echo "")"
# Hold a reader lock on gen.1 BEFORE the second refresh so the D10 GC defers
# the rm (flock -n -x on the gen's lock will fail → skip, reap on a later refresh).
# This demonstrates "retained for GC" behavior with the active reader.
_SGSWAP4_LOCK="${_SGSWAP_GEN1_LINK}.lock"
_SGSWAP4_READY="$_SGSWAP_PARENT/sgswap4-reader-ready"
touch "$_SGSWAP4_LOCK" 2>/dev/null || true
( flock -s 200; touch "$_SGSWAP4_READY"; sleep 120 ) 200>"$_SGSWAP4_LOCK" &
_SGSWAP4_READER_PID=$!
# Causal handshake: wait until reader has acquired flock -s (replaces fixed sleep 0.1 — #4847)
_wait_for_reader_lock "$_SGSWAP4_READY" 30
# Second refresh: creates gen.2, re-points symlink; gen.1 GC deferred (reader holds lock)
_refresh_capture "$_SGSWAP_LANE/target" "$_SGSWAP_BASE_SECOND" \
    --landed-commit "$_SGSWAP_LANE_HEAD"
_SGSWAP_GEN2_LINK="$(readlink "$_SGSWAP_BASE_SECOND" 2>/dev/null || echo "")"
# Release the reader lock (was only needed to hold gen.1 alive for the assertion)
kill "$_SGSWAP4_READER_PID" 2>/dev/null || true; wait "$_SGSWAP4_READER_PID" 2>/dev/null || true
assert "SGSWAP4: second refresh exits 0" \
    test "$RC" -eq 0
assert "SGSWAP4: <base> is still a SYMLINK after second refresh" \
    test "$REFRESH_BASE_IS_SYMLINK" = "1"
assert "SGSWAP4: symlink re-pointed (gen link changed between refreshes)" \
    bash -c '[ -n "$1" ] && [ "$1" != "$2" ]' _ "$_SGSWAP_GEN2_LINK" "$_SGSWAP_GEN1_LINK"
assert "SGSWAP4: prior gen dir still exists after flip (GC deferred while reader held lock)" \
    bash -c '[ -d "$1" ]' _ "$_SGSWAP_GEN1_LINK"

# ─────────────────────────────────────────────────────────────────────────────
# Block PG: reader-refcount GC (ALWAYS-RUN)
#
# With a symlink-gen base established (≥1 retired gen present):
#   (GC1) simulate an in-flight clone holding a shared lock on the retired gen's
#         lock file (flock -s <base>.gen.<K>.lock); run a refresh; assert the
#         retired gen dir is NOT removed while the reader holds the lock.
#   (GC2) release the reader lock; run another refresh; assert the now-unreferenced
#         retired gen IS reaped (GC'd) — the dir entry removed.
#
# FS-agnostic (flock + rm only); runs always-run in default CI.
# RED until step-6 (reader-refcount GC impl): the current refresh retains all
# retired gens but never GC's them → GC2 "is reaped" assertion fails.
# GC1 "not removed while reader holds lock" also fails (no flock guard at all —
# the retired gen would survive anyway, but for the WRONG reason; after step-4
# the script never deletes retired gens, so GC1 appears to pass by coincidence.
# However, step-6 is what makes GC2 fire correctly; until then GC2 is RED).
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block PG: reader-refcount GC ---"

# ── GC fixtures: establish a base with at least one retired gen ───────────────
_GC_PARENT="$(mktemp -d /tmp/test-warm-pool-GC-XXXXXX)"
_TMPDIRS+=("$_GC_PARENT")

_GC_LANE="$(_mk_clean_advancing_lane "$_GC_PARENT/adv")"
_GC_LANE_HEAD="$(git -C "$_GC_LANE" rev-parse HEAD)"
_GC_BASE="$_GC_PARENT/base"

# First refresh: creates gen.1, symlink → gen.1
_refresh_capture "$_GC_LANE/target" "$_GC_BASE" --landed-commit "$_GC_LANE_HEAD"
_GC_GEN1_PATH="$(readlink "$_GC_BASE" 2>/dev/null || echo "")"

# Acquire a persistent reader lock on gen.1 BEFORE the second refresh.
# This serves double duty:
#   (a) prevents the D10 GC from reaping gen.1 during the second refresh
#       (fixture integrity — without this, gen.1 would be immediately reaped
#       since no reader is present, leaving _GC_RETIRED_GEN empty)
#   (b) is the active reader flock -s we test GC1 against in the third refresh
_GC_LOCK_FILE="${_GC_GEN1_PATH}.lock"
_GC_READY="$_GC_PARENT/gc-reader-ready"
touch "$_GC_LOCK_FILE" 2>/dev/null || true
( flock -s 200; touch "$_GC_READY"; sleep 120 ) 200>"$_GC_LOCK_FILE" &
_GC_READER_PID=$!
# Causal handshake: wait until reader has acquired flock -s (replaces fixed sleep 0.2 — #4847)
_wait_for_reader_lock "$_GC_READY" 30

# Second refresh: creates gen.2, symlink → gen.2; gen.1 GC deferred (reader holds lock)
_refresh_capture "$_GC_LANE/target" "$_GC_BASE" --landed-commit "$_GC_LANE_HEAD"

# Identify the retired gen (gen.1, preserved due to reader lock)
_GC_LIVE_GEN="$(readlink "$_GC_BASE" 2>/dev/null || echo "")"
_GC_RETIRED_GEN=""
for _gd in "${_GC_BASE}.gen."*; do
    [ -d "$_gd" ] || continue
    _gn="${_gd##*.gen.}"
    case "$_gn" in *[!0-9]*) continue ;; esac
    [ "$_gd" != "$_GC_LIVE_GEN" ] && _GC_RETIRED_GEN="$_gd"
done

# ── GC1: retired gen NOT removed while reader holds shared flock ──────────────
# The persistent reader (started above) holds flock -s on _GC_LOCK_FILE.
# Run a third refresh; the GC attempt should find the lock held → skip rm.
_refresh_capture "$_GC_LANE/target" "$_GC_BASE" --landed-commit "$_GC_LANE_HEAD"
assert "GC1: third refresh exits 0 (GC attempt with active reader)" \
    test "$RC" -eq 0
assert "GC1: retired gen NOT removed while reader holds shared flock" \
    bash -c '[ -d "$1" ]' _ "$_GC_RETIRED_GEN"
# Release the persistent reader — gen.1 lock is now free for GC2
kill "$_GC_READER_PID" 2>/dev/null || true; wait "$_GC_READER_PID" 2>/dev/null || true

# ── GC2: retired gen IS reaped after reader releases lock ─────────────────────
# No reader holds the shared lock → GC should rm the retired gen dir.
_refresh_capture "$_GC_LANE/target" "$_GC_BASE" --landed-commit "$_GC_LANE_HEAD"
assert "GC2: fourth refresh exits 0 (GC should reap free retired gen)" \
    test "$RC" -eq 0
assert "GC2: retired gen IS reaped after reader releases lock (dir removed)" \
    bash -c '[ ! -d "$1" ]' _ "$_GC_RETIRED_GEN"

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
# ─────────────────────────────────────────────────────────────────────────────
# Block TRASH: shared-trash litter guard (task 5612). Two asserts, deliberately
# kept as two independently-reported signals: TRASH2 can realistically only ever
# report "clean", which is indistinguishable from a checker that stopped working
# — TRASH1 is the hermetic control proving the instrument still fires.
# Full rationale and honest scope: the CANONICAL WIRING CONTRACT comment in
# tests/infra/test_helpers.sh.
#
# Sited HERE, immediately BEFORE the top-level substrate gate below: _skip()
# calls test_summary and exits 0, so anything after the gate is DEAD in the
# default CI environment (no REIFY_WARM_LANE_MOUNT, /tmp ext4) — which is every
# ordinary run of this suite. TRASH1 is hermetic and substrate-independent, so
# it belongs on the always-runs path unconditionally. TRASH3 at the end of the
# file re-checks TRASH2 after the substrate-gated real seed runs.
# ─────────────────────────────────────────────────────────────────────────────
assert "TRASH1: shared-trash litter detector is live (self-test fires on a synthetic bare-/tmp lane)" \
    assert_shared_trash_litter_detector_live
assert "TRASH2: no lane in this suite littered the machine-shared /tmp/.reseed-trash" \
    assert_no_shared_trash_litter

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
# B3 wall: logged to stderr as a non-discriminating diagnostic (not asserted —
#   direction can invert under scheduling jitter; warm-skip proven structurally
#   by _B3_DEP_FRESH=true / _B3_LEAF_FRESH=false / B4 count-equality — #4847).
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
# rc captured via `|| rc=$?` (record-base mode has no stdout-emptiness caller
# obligation like seed mode — the sidecar path this prints is discarded here,
# unused — but a bare failing command is still a plain simple command under
# this file's `set -euo pipefail`, so an unguarded non-zero exit here would
# abort the whole suite exactly like the seed-mode call sites below; task
# #5880 amendment).
_B34_RECORD_BASE_RC=0
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" --record-base "$_WS_BASE/target" >/dev/null \
        || _B34_RECORD_BASE_RC=$?

assert "B3+B4: record-base stamped the base's provenance sidecar (exits 0)" \
    test "$_B34_RECORD_BASE_RC" -eq 0

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
#
# REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 is the semantically CORRECT annotation
# here, not a workaround for §9.5 inv.13. The base is built with REAL cargo, so
# target/<profile>/.fingerprint/ exists and the clone crosses inv.13's hazard
# gate; and this arm deliberately exercises the D5 accept path — the warm-skip
# assertion below depends on the 2020-01-01 bulk stamp making warm_dep report
# fresh:true, which is exactly the claim inv.13 refuses to make unsubstantiated
# in production.
#
# Deliberately NOT git-init + --base-commit HEAD: that would require committing
# $_WS_LANE BEFORE this seed, and _b7_init_git_lane below documents a
# load-bearing dependence on committing AFTER the seed so git's index stat-cache
# records the 2020-01-01 mtimes and a later `git checkout -- .` does not rewrite
# warm_dep. Re-ordering that would put B7's own stated critical invariant at
# risk for no coverage gain.
# rc/stdout capture + refusal cleanup: see _seed_lane_capture (~line 168).
_seed_lane_capture "B3+B4" "$_WS_LANE/target" \
    env REIFY_WARM_LANE_ALLOW_NO_BASE_COMMIT=1 \
        RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" "$_WS_BASE/target" "$_WS_LANE" \
        --fresh-checkout \
        --touch "$_WS_LANE/warm_leaf/src/lib.rs"
_B34_SEED_RC="$_SEED_CAP_RC"
_B34_SEED_OUT="$_SEED_CAP_OUT"

assert "B3+B4: lane seed certified the lane warm-safe (rc=0, non-empty stdout)" \
    bash -c '[ "$1" -eq 0 ] && [ -n "$2" ]' _ "$_B34_SEED_RC" "$_B34_SEED_OUT"

# ── Warm lane build: heavy dep reused via CoW (fresh:true), leaf rebuilt ──────
# Skipped entirely when the seed refused to certify the lane (mirrors B13's
# treatment-arm shape, task #5880 amendment): the verdict is already decided
# by the assert above, so building would either fail against the now-missing
# target/ or, worse, silently succeed as a full COLD build — which would make
# the "leaf delta-closure is fresh:false" assert below pass VACUOUSLY (a cold
# build reports every unit fresh:false regardless of mechanism) instead of
# testing the warm-skip mechanism it exists to test. Sentinels below are
# chosen so every downstream B3/B4 assertion FAILs distinctly on refusal
# rather than silently passing or crashing on a missing/stale JSON capture.
#
# rc is still captured on the run branch itself via `|| _B34_WARM_RC=$?`: a
# cargo failure unrelated to seeding (e.g. a genuine compile error) must not
# silently abort the suite either — same bare-command-under-errexit hazard as
# the seed call above, just for the build instead of the seed.
_B34_WARM_RC=0
if [ "$_B34_SEED_RC" -eq 0 ] && [ -n "$_B34_SEED_OUT" ]; then
    # Capture JSON to inspect per-crate freshness (B3 warm-skip) AND measure wall.
    _B3_WARM_T0="$(date +%s%3N)"
    _WARM_JSON="$(CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_WS_LANE/Cargo.toml" \
            --message-format=json 2>/dev/null)" || _B34_WARM_RC=$?
    _B3_WARM_WALL=$(( $(date +%s%3N) - _B3_WARM_T0 ))

    # ── Extract B3/B4 signals from warm lane build output ────────────────────
    _B3_DEP_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
        grep '"reason":"compiler-artifact"' | grep '"name":"warm_dep"' | \
        grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

    _B3_LEAF_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
        grep '"reason":"compiler-artifact"' | grep '"name":"warm_leaf"' | \
        grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"

    _B4_WARM_FRESH="$(printf '%s\n' "$_WARM_JSON" | \
        grep '"reason":"compiler-artifact"' | grep -c '"fresh":true' || true)"
else
    echo "B3+B4: skipping warm lane build — seed refused to certify the lane warm-safe (caller obligation, scripts/seed-warm-lane.sh:63-79)" >&2
    _B34_WARM_RC=-1
    _B3_WARM_WALL=-1
    _B3_DEP_FRESH="<seed-refused>"
    _B3_LEAF_FRESH="<seed-refused>"
    _B4_WARM_FRESH=-1
fi

# Record signals to stderr (direction-only; no frozen thresholds per G6/PRD §9)
echo "B3 wall: cold=${_B3_COLD_WALL}ms warm=${_B3_WARM_WALL}ms delta=$((${_B3_COLD_WALL} - ${_B3_WARM_WALL}))ms" >&2
echo "B4 fresh counts: inplace=${_B4_INPLACE_FRESH} warm=${_B4_WARM_FRESH}" >&2

assert "B3+B4: warm lane build exits 0" \
    test "$_B34_WARM_RC" -eq 0
assert "B3: heavy dep unit is fresh:true in warm lane (CoW-reused, not recompiled)" \
    test "$_B3_DEP_FRESH" = "true"
assert "B3: leaf delta-closure is fresh:false in warm lane (was rebuilt)" \
    test "$_B3_LEAF_FRESH" = "false"
assert "B4: fresh-unit count in warm lane == in-place control (path-independence)" \
    test "$_B4_INPLACE_FRESH" -eq "$_B4_WARM_FRESH"
# B3 wall-direction assert dropped (#4847 technique C): direction can invert under
# scheduling jitter; warm-skip is proven structurally by the three asserts above.

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

assert "B6: sibling seed certified the lane warm-safe (rc=0, non-empty stdout)" \
    bash -c '[ "$1" -eq 0 ] && [ -n "$2" ]' _ "$_B6_SEED_RC" "$_B6_SEED_OUT"

# After the refresh, build the sibling lane and check dep freshness — skipped
# entirely when the sibling seed refused to certify (mirrors the B3+B4
# treatment above, task #5880 amendment): the verdict is already decided by
# the assert above, and a build against the (already target-less) sibling
# would just waste a cold cargo build that has no chance of resolving warm.
_B6_SIBLING_RC=0
if [ "$_B6_SEED_RC" -eq 0 ] && [ -n "$_B6_SEED_OUT" ]; then
    _B6_SIBLING_JSON="$(CARGO_INCREMENTAL=0 RUSTC_WRAPPER="" RUSTFLAGS="" \
        cargo build --manifest-path "$_B6_SIBLING_LANE/Cargo.toml" \
            --message-format=json 2>/dev/null)" || _B6_SIBLING_RC=$?

    _B6_SIBLING_DEP_FRESH="$(printf '%s\n' "$_B6_SIBLING_JSON" | \
        grep '"reason":"compiler-artifact"' | grep '"name":"warm_dep"' | \
        grep -o '"fresh":[a-z]*' | head -1 | sed 's/"fresh"://;s/"//g')"
else
    echo "B6: skipping sibling lane build — sibling seed refused to certify the lane warm-safe (caller obligation, scripts/seed-warm-lane.sh:63-79)" >&2
    _B6_SIBLING_RC=-1
    _B6_SIBLING_DEP_FRESH="<seed-refused>"
fi

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

# ─────────────────────────────────────────────────────────────────────────────
# Block B11 — Torn-base coherence: concurrent reflink clone during flip
#             (SUBSTRATE-GATED; placed after detect_substrate gate)
#
# Uses helper `_b11_concurrent_clone_during_flip` (defined in step-8) to:
#   1. Build a warm at-head base (gen.1, symlink-gen) from a synth workspace.
#   2. Resolve <base> symlink → its concrete gen.1 dir (cp -a on a symlink copies
#      the link itself; the consumer must resolve to the concrete gen first).
#   3. Background a reader: holds flock -s on gen.1.lock AND runs
#      cp -a --reflink=always <gen.1_dir> <clone>/target concurrently (simulates
#      a pool acquire holding the dir-entry refcount through the walk).
#   4. WHILE the clone is in-flight, perform a generation flip (second refresh:
#      gen.2 built, ln -sfn re-pointed, GC attempt → reader holds flock-s → rm
#      deferred).
#   5. Join the reader (clone complete, flock-s released).
#
# On return the helper sets:
#   _B11_CLONE_DIR        — dir that received the concurrent clone
#   _B11_PINNED_GEN       — absolute path of gen.1 (the concrete gen that was cloned)
#   _B11_DF_BEFORE_AVAIL  — df --output=avail on the substrate before the flip (MiB)
#   _B11_DF_AFTER_AVAIL   — df --output=avail on the substrate after the flip (MiB)
#
# Assertions:
#   (a) Clone coherence: <clone>/target is byte-identical to gen.1 — single-gen,
#       no torn mix. Relies on POSIX held-open-dir reader coherence (host-verified
#       2026-06-18): files already open in the clone walk are not affected by
#       swapping the directory entry above them.
#   (b) Retired gen.1 survived the mid-clone GC (reader held flock -s → rm skipped).
#   (c) Retired gen.1 is reaped by a post-drain refresh (flock-s now released,
#       GC finds no reader → rm fires).
#   (d) df --output=avail is flat across the flip (CoW extent sharing; no full-size
#       duplication — direction recorded to stderr, no frozen threshold per PRD §9/G6).
#
# RED because `_b11_concurrent_clone_during_flip` is undefined until step-8.
# On non-substrate CI the substrate gate fires first → _skip → graceful exit 0.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B11: torn-base coherence (concurrent reflink clone during flip) ---"

# Route the workspace onto a PRIVATE loopback (rung 1 / rung 3) when one is
# obtainable, so the df-flatness measurement below is immune to a concurrent
# disk-writer diluting shared /tmp's free-space reading (task #4928). When no
# private mount is obtainable, fall back to the general gate dir: the
# coherence/GC/reap assertions below are privacy-independent and still run;
# only the df-flat assertion is gated on _B11_PRIVATE_OK.
_B11_PRIVATE_OK=0
if detect_private_substrate; then
    _B11_PRIVATE_OK=1
else
    _B11_GATE_DIR="$_GATE_DIR"
    echo "B11: no private loopback -> df-flat assertion will SKIP (shared-/tmp df is not immune to a concurrent disk-writer)" >&2
fi

_B11_WS_ROOT="$(mktemp -d "$_B11_GATE_DIR/warm-lane-b11-XXXXXX")"
_TMPDIRS+=("$_B11_WS_ROOT")

# Call the helper (undefined until step-8 → exits 127 under set -euo pipefail
# on a substrate host → RED; SKIPs gracefully off-substrate via the gate above).
_b11_concurrent_clone_during_flip "$_B11_WS_ROOT"

# ── (a) Clone coherence ────────────────────────────────────────────────────────
# The clone was made while a generation flip was in progress.  Because the reader
# held flock -s throughout the cp walk, the dir-entry for gen.1 remained live;
# the clone must see exactly gen.1's content, never a mix of gen.1/gen.2.
assert "B11: concurrent clone completed (clone/target dir exists)" \
    bash -c '[ -d "$1/target" ]' _ "$_B11_CLONE_DIR"
assert "B11: clone target is coherent (byte-identical to pinned gen, no torn mix)" \
    bash -c 'diff -rq "$1/target" "$2" >/dev/null 2>&1' _ "$_B11_CLONE_DIR" "$_B11_PINNED_GEN"

# ── (b) Retired gen survived mid-clone GC ─────────────────────────────────────
# After the flip the reader has joined (flock released).  gen.1 dir entry was
# NOT removed by the mid-flip GC (reader held flock -s → rm was deferred).
assert "B11: retired gen.1 survived mid-clone GC (reader held flock, GC deferred)" \
    bash -c '[ -d "$1" ]' _ "$_B11_PINNED_GEN"

# ── (c) Post-drain refresh reaps gen.1 ────────────────────────────────────────
# Reader has joined; gen.1 lock is now free.  Run a post-drain refresh; the GC
# acquires flock -n -x and rm's gen.1.
# Convention: the helper creates $_B11_WS_ROOT/lane as the advancing lane.
_B11_LANE_HEAD="$(git -C "$_B11_WS_ROOT/lane" rev-parse HEAD 2>/dev/null || echo "")"
_refresh_capture "$_B11_WS_ROOT/lane/target" "$_B11_WS_ROOT/base" \
    --landed-commit "$_B11_LANE_HEAD"
assert "B11: post-drain refresh exits 0" \
    test "$RC" -eq 0
assert "B11: retired gen.1 reaped by post-drain refresh (no reader → GC fires)" \
    bash -c '[ ! -d "$1" ]' _ "$_B11_PINNED_GEN"

# ── (d) df flatness: CoW extent sharing ───────────────────────────────────────
# gen.2 shares CoW extents with gen.1 → available space drops by at most the size
# of unique (modified) files, not the full workspace.
# Tolerance: ≤50 MiB consumed is consistent with CoW sharing; >50 MiB signals
# full-size duplication (bug).  Direction only — no frozen threshold per PRD §9/G6.
# Gated on _B11_PRIVATE_OK: a delta measured on shared /tmp is not immune to a
# concurrent disk-writer, so asserting it there would reintroduce the exact
# flake this task removes (task #4928) — skip gracefully instead (never
# false-RED), mirroring this file's established no-substrate skip convention.
echo "B11 df: before=${_B11_DF_BEFORE_AVAIL}MiB after=${_B11_DF_AFTER_AVAIL}MiB flip-cost=$(( _B11_DF_BEFORE_AVAIL - _B11_DF_AFTER_AVAIL ))MiB" >&2
if [ "$_B11_PRIVATE_OK" = "1" ]; then
    assert "B11: df flat across flip (private loopback; ≤50 MiB consumed, no full-size dup)" \
        bash -c '[ "$(( $1 - $2 ))" -le 50 ]' _ "$_B11_DF_BEFORE_AVAIL" "$_B11_DF_AFTER_AVAIL"
else
    echo "B11: SKIP df-flat assertion -- no private loopback (shared-/tmp df is not immune to a concurrent disk-writer)" >&2
fi

# ─────────────────────────────────────────────────────────────────────────────
# Block B13 — Re-seed-at-acquire rescue: warm vs near-cold control
#             (SUBSTRATE-GATED; placed after detect_substrate gate)
#
# Uses helper `_b13_reseed_vs_resetinplace` (defined in step-10) to:
#   1. Build a warm at-head base (symlink-gen, via gen_synth_workspace + refresh).
#   2. Create a lane deliberately STALED: its target/ is pinned far behind head
#      while head has advanced (non-trivial delta applied to source).
#   3. Acquire the staled lane TWO WAYS on the SAME lane dir:
#      CONTROL:   reset-in-place rebuild (git reset/clean -e target; cargo build)
#                 → near-cold first build.
#      TREATMENT: re-seed via seed-warm-lane.sh --fresh-checkout from the at-head
#                 base (D10 always-re-seed-at-acquire path) → warm first build.
#   4. Records fresh-unit counts + wall times for both paths.
#
# Sets on return:
#   _B13_CTRL_FRESH       — fresh-unit count for the control (reset-in-place)
#   _B13_TREAT_FRESH      — fresh-unit count for the treatment (re-seed)
#   _B13_CTRL_WALL_MS     — build wall-time for the control (ms)
#   _B13_TREAT_WALL_MS    — build wall-time for the treatment (ms)
#
# Assertions (improvement-direction only; no frozen thresholds per PRD §9/G6):
#   (a) Treatment fresh-unit count > control fresh-unit count
#       (re-seed rescue from at-head base yields warmer first build than near-cold
#        reset-in-place).
#   (b) Recorded delta logged to stderr (direction-only signal, not a gate).
#
# RED because `_b13_reseed_vs_resetinplace` is undefined until step-10.
# On non-substrate CI the substrate gate fires first → _skip → graceful exit 0.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B13: re-seed-at-acquire rescue (warm vs near-cold control) ---"

_B13_WS_ROOT="$(mktemp -d "$_GATE_DIR/warm-lane-b13-XXXXXX")"
_TMPDIRS+=("$_B13_WS_ROOT")

# Call the helper (undefined until step-10 → exits 127 under set -euo pipefail
# on a substrate host → RED; SKIPs gracefully off-substrate via the gate above).
_b13_reseed_vs_resetinplace "$_B13_WS_ROOT"

# ── (contract) Base provenance sidecar was recorded successfully ─────────────
# Guards the --record-base call inside the helper above (task #5880
# amendment): without this, a record-base failure would just silently
# proceed (missing sidecar → RUSTFLAGS guard below defaults to "", which may
# or may not matter) instead of surfacing as its own diagnosable FAIL line.
assert "B13: record-base stamped the at-head base's provenance sidecar (exits 0)" \
    test "$_B13_RECORD_BASE_RC" -eq 0

# ── (contract) Treatment honoured seed-warm-lane.sh's caller obligation ───────
# Non-empty stdout (with exit 0) is the ONLY signal the seed considers the lane
# warm-safe (scripts/seed-warm-lane.sh:63-79). Asserting this explicitly means a
# fail-closed abort in the treatment seed is now an isolated, diagnosable RED
# line instead of the whole-suite abort it used to be.
assert "B13: treatment re-seed honoured seed-warm-lane.sh's caller contract (exit 0 + non-empty stdout)" \
    bash -c '[ "$1" -eq 0 ] && [ -n "$2" ]' _ "$_B13_SEED_RC" "$_B13_SEED_OUT"

# ── (delta-touch set) The re-seed touched exactly the head-ward delta ────────
# Pins WHAT the re-seed actually delta-touched: the resolved base commit must
# produce a MEANINGFUL (leaf touched to now) yet MINIMAL (dep left alone)
# delta, so the aggregate comparison below can never pass for the wrong reason
# (e.g. a repair that silently widened or emptied the delta-touch set). The
# treatment call passes no --touch for the leaf (see _b13_reseed_vs_resetinplace
# above), so the leaf assertion below exercises _touch_git_delta's git-diff
# base-commit resolution directly rather than being trivially satisfied by an
# explicit touch that fires regardless of whether base resolution succeeded.
echo "B13 mtimes: leaf=${_B13_LEAF_MTIME} dep=${_B13_DEP_MTIME} stale_epoch=${_B13_STALE_EPOCH}" >&2
assert "B13: re-seed delta-touched the head-ward leaf to now (not the 2020 bulk stamp)" \
    bash -c '[ "$1" -ne "$2" ]' _ "$_B13_LEAF_MTIME" "$_B13_STALE_EPOCH"
assert "B13: re-seed left the unchanged heavy dep at the 2020 bulk stamp (delta-touch set == head-ward delta only)" \
    bash -c '[ "$1" -eq "$2" ]' _ "$_B13_DEP_MTIME" "$_B13_STALE_EPOCH"

# ── (warmth mechanism) Per-unit freshness explains the aggregate comparison ──
# The aggregate fresh-unit comparison below has only two compilation units, so
# a bare inequality is satisfiable by accident. Assert the MECHANISM directly:
# the treatment's unchanged heavy dep is CoW-reused from the at-head base
# (fresh:true), while the control's reset-in-place rebuild is genuinely
# near-cold for that same dep (fresh:false). Tri-state string comparison
# ("true"/"false"), not -eq 1/0: a build that emits no warm_dep
# compiler-artifact line at all (e.g. it failed outright) yields an EMPTY
# _B13_*_DEP_FRESH, which fails both checks below instead of silently
# matching the "rebuilt"/"not fresh" case.
echo "B13 dep-fresh: control=${_B13_CTRL_DEP_FRESH:-<absent>} treatment=${_B13_TREAT_DEP_FRESH:-<absent>}" >&2
assert "B13: treatment keeps the unchanged heavy dep warm (warm_dep fresh:true from the CoW base clone)" \
    test "$_B13_TREAT_DEP_FRESH" = "true"
assert "B13: control rebuilt the heavy dep (reset-in-place is genuinely near-cold)" \
    test "$_B13_CTRL_DEP_FRESH" = "false"

# ── (a) Treatment is warmer than control ──────────────────────────────────────
# Re-seed from at-head base gives a higher fresh-unit count than reset-in-place.
# Improvement-direction only; no frozen threshold (PRD §9/G6 convention).
echo "B13 fresh-units: control(reset-in-place)=${_B13_CTRL_FRESH} treatment(re-seed)=${_B13_TREAT_FRESH}" >&2
echo "B13 wall-ms: control=${_B13_CTRL_WALL_MS}ms treatment=${_B13_TREAT_WALL_MS}ms delta=$(( _B13_CTRL_WALL_MS - _B13_TREAT_WALL_MS ))ms" >&2
assert "B13: treatment fresh-unit count > control (re-seed warmer than reset-in-place)" \
    bash -c '[ "$1" -gt "$2" ]' _ "$_B13_TREAT_FRESH" "$_B13_CTRL_FRESH"

# TRASH3 re-checks TRASH2 after the substrate-gated blocks — the only part of
# this suite that drives the REAL seed, and therefore the only part that could
# actually litter. TRASH1/TRASH2 run before the top-level substrate gate so they
# cover every invocation including the `_skip` early exit (the default CI path);
# this one covers the lanes those blocks seeded, which they ran too early to see.
assert "TRASH3: ... and still none after the substrate-gated real seed runs" \
    assert_no_shared_trash_litter

test_summary
