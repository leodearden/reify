#!/usr/bin/env bash
# tests/infra/test_occt_deps_preflight.sh
#
# Guard for the OCCT arm of scripts/check-manifold-deps.sh (task #6343).
#
# WHY this guard exists. `reify_build_utils::find(NativeDep::Occt)` returns
# `None` when EITHER the header dir or the lib dir is unresolved, and
# `crates/reify-kernel-occt/build.rs` responds with a `cargo:warning` plus a
# bare `return` — no `has_occt` cfg. That silently DELETES the crate's
# `#[cfg(all(test, has_occt))]` modules and its ~25 `#![cfg(has_occt)]`
# integration binaries: the suite reports ZERO tests, not zero failures, and
# the verify gate stays green. This test pins the preflight that converts that
# vacuity into a red gate.
#
# Script under test: scripts/check-manifold-deps.sh (emitted by
# scripts/verify.sh as a plan entry whenever RUN_RUST=1, so the arm it guards
# runs on every --scope all / merge-gate verify).
#
# Assertions:
#   1. scripts/check-manifold-deps.sh exists and is executable.
#   2. ABSENCE: both override dirs empty => non-zero, output names OCCT.
#   3. LIB-ONLY MISSING: headers present, libs absent => non-zero, output
#      names libTKernel.so and the offending lib dir.
#   4. INCLUDE-ONLY MISSING: libs present, headers absent => non-zero, output
#      names Standard_Failure.hxx and the offending include dir. This is the
#      mixed case that silently produces a stub build.
#   5. PARITY, DATA (anti-drift): the marker-delimited OCCT declarations in
#      scripts/check-manifold-deps.sh equal `NativeDep::Occt`'s arms in
#      crates/reify-build-utils/src/lib.rs — both candidate lists INCLUDING
#      ORDER (system paths must stay ahead of /opt/reify-deps' OCCT 7.9) and
#      both sentinel names. Rust is the source of truth; bash is a declared
#      mirror. Both parses must yield a non-empty result, so a renamed anchor
#      fails loudly instead of passing vacuously.
#   6. PARITY, SNAP FALLBACK: the same mirror one layer down — the default of
#      the guard's OCCT_SNAP_ROOT equals the literal in
#      find_dir_with_override's `read_dir(..)`, and the guard's
#      sentinel -> subdir `case` equals that fn's `match sentinel` arms, order
#      included. Declaration-level, because on a host that HAS system OCCT the
#      candidate loop short-circuits before either side's fallback ever runs.
#   7. ACCEPTED SONAME + RECORDING: a Debian-shaped chain whose first-level
#      link target carries the FIRST value of OCCT_ACCEPTED_SONAMES => exit 0,
#      AND the guard prints the resolved version and both resolved dirs. That
#      [ok] line is the arm's "which OCCT produced this green result" half, so
#      it is asserted rather than left to `>/dev/null`.
#   8. PATCH-SHAPED SONAME: `libTKernel.so -> libTKernel.so.<accepted>.1` — a
#      repackaging that moves the dev symlink one hop further on a
#      functionally identical OCCT => still exit 0, and the verbatim segment
#      is still recorded. The pin is on MAJOR.MINOR precisely so this
#      non-event cannot hard-stop every RUN_RUST=1 verify; build.rs splices
#      the verbatim segment, which names a file that exists.
#   9. UNACCEPTED SONAME: version 0.0 (never a real OCCT release, so this case
#      survives any future pin bump) => non-zero, output names OCCT, the
#      resolved version, and the accepted set.
#  10. CONDA-SHAPED ONE-LEVEL SYMLINK: `libTKernel.so -> libTKernel.so.7.9.3`,
#      the exact layout live at /opt/reify-deps/lib => resolves to `7.9.3`,
#      whose major.minor 7.9 is not accepted, so non-zero naming 7.9.3
#      VERBATIM. Pins that the guard takes the trailing segment as-is for the
#      record, exactly as read_soname_version documents, and projects only for
#      the comparison.
#  11. UNDETERMINABLE SONAME: `libTKernel.so` as a REGULAR FILE => non-zero.
#      This is the state where find() still reports the dir resolved (it only
#      tests .exists()), has_occt IS set, and build.rs silently falls back to
#      the literal string "7.8" — i.e. links a version nobody verified.
#  12. CROSS-ARTIFACT PIN: the version scripts/setup-dev.sh's OCCT block
#      expects from dpkg projects (major.minor) into OCCT_ACCEPTED_SONAMES.
#      Both sides are projected, so the accepted set stays free to hold a
#      three-segment SONAME even though setup-dev.sh's `grep -oP '\d+\.\d+'`
#      can only ever yield major.minor.
#
# The accepted-SONAME value is DERIVED from the guard, never hardcoded here, so
# a legitimate future pin bump stays a one-line diff in one file. Every derived
# parse asserts non-empty first.
#
# Hermeticity: `pool`. Pure bash + filesystem — no cargo, no npm, no network.
# Every OCCT case is driven through the OCCT_LIB_DIR / OCCT_INCLUDE_DIR
# overrides the BUILD already honours, pointed at `mktemp -d` fixtures under
# $_TMPDIR, so no bespoke test-only env seam is added to production code. The
# guard is deliberately stricter than `find_dir_with_override` here (it demands
# the sentinel inside an override rather than trusting the path), which is
# exactly what makes those cases drivable.
#
# KNOWN, DELIBERATE CAVEAT: check-manifold-deps.sh is ONE script, and its
# manifold-prebuilt and tbb-pin arms run ahead of the OCCT arm on every
# invocation — including the tbb arm's `mkdir -p /opt/reify-deps/tbb-pin`
# self-heal, which writes outside $_TMPDIR. So the two positive controls below
# also depend on a healthy /opt/reify-deps, and a broken one surfaces here as
# an OCCT-preflight failure. Every NEGATIVE case pairs its exit-code assert
# with an output assert naming an OCCT-specific string, so those stay
# attributable. There is deliberately no unqualified live-host probe in this
# file: scripts/verify.sh already emits this guard as a plan entry on every
# RUN_RUST=1 verify, which is where "is OCCT actually installed on this host"
# is answered for real.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

GUARD="$REPO_ROOT/scripts/check-manifold-deps.sh"
RUST_SRC="$REPO_ROOT/crates/reify-build-utils/src/lib.rs"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_TMPDIR"' EXIT

echo "=== OCCT deps preflight tests ==="

# ---------------------------------------------------------------------------
# Fixture + invocation helpers
# ---------------------------------------------------------------------------

# _mk_include_fixture <name> [<sentinel>] — dir under $_TMPDIR containing the
# named include sentinel (default: OCCT's Standard_Failure.hxx). Prints the
# path.
#
# The sentinel may be a NESTED path — OpenVDB's is `openvdb/openvdb.h`, not a
# bare filename — so its parent dirs are created too. A flat fixture would make
# the OpenVDB positive control fail for entirely the wrong reason.
_mk_include_fixture() {
    local d="$_TMPDIR/$1" sentinel="${2:-Standard_Failure.hxx}"
    local parent="$d"
    case "$sentinel" in */*) parent="$d/${sentinel%/*}" ;; esac
    mkdir -p "$parent"
    : > "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_lib_fixture <name> <version> [<sentinel>] — dir under $_TMPDIR
# reproducing the Debian TWO-HOP chain that reify_build_utils' own unit fixture
# models (crates/reify-build-utils/src/lib.rs, read_soname_version tests):
#   <sentinel> -> <sentinel>.<v> -> <sentinel>.<v>.1
# so the FIRST-level link target's suffix is exactly <version> (which is the
# whole point: `readlink -f` would yield <v>.1 instead). Sentinel defaults to
# OCCT's libTKernel.so. Prints the path.
_mk_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2" sentinel="${3:-libTKernel.so}"
    mkdir -p "$d"
    : > "$d/$sentinel.$v.1"
    ln -sfn "$sentinel.$v.1" "$d/$sentinel.$v"
    ln -sfn "$sentinel.$v" "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_patchlink_lib_fixture <name> <majmin> — a repackaging that points the
# dev symlink ONE HOP FURTHER than Debian's, `libTKernel.so ->
# libTKernel.so.<majmin>.1`, so the first-level target's suffix is
# `<majmin>.1` on a functionally identical OCCT. The pin is on major.minor
# exactly so this shape stays green.
_mk_patchlink_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2"
    mkdir -p "$d"
    : > "$d/libTKernel.so.$v.1"
    ln -sfn "libTKernel.so.$v.1" "$d/libTKernel.so"
    printf '%s' "$d"
}

# _mk_conda_lib_fixture <name> <version> [<sentinel>] — dir under $_TMPDIR
# reproducing the conda-forge / /opt/reify-deps layout: ONE hop,
# `<sentinel> -> <sentinel>.<v>` where <v> is itself the full version. The
# first-level target's suffix is therefore the whole version verbatim. This is
# also gmsh's live shape at /opt/reify-deps/lib
# (libgmsh.so -> libgmsh.so.4.15.2). Sentinel defaults to OCCT's libTKernel.so.
_mk_conda_lib_fixture() {
    local d="$_TMPDIR/$1" v="$2" sentinel="${3:-libTKernel.so}"
    mkdir -p "$d"
    : > "$d/$sentinel.$v"
    ln -sfn "$sentinel.$v" "$d/$sentinel"
    printf '%s' "$d"
}

# _mk_plainfile_lib_fixture <name> — dir whose libTKernel.so is a REGULAR FILE,
# not a symlink. The sentinel exists (so find() resolves the dir and has_occt
# IS set) but no SONAME can be read from it.
_mk_plainfile_lib_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    : > "$d/libTKernel.so"
    printf '%s' "$d"
}

# _mk_empty_fixture <name> — empty dir under $_TMPDIR. Prints the path.
_mk_empty_fixture() {
    local d="$_TMPDIR/$1"
    mkdir -p "$d"
    printf '%s' "$d"
}

# --- dep-generic guard invocation ------------------------------------------
#
# check-manifold-deps.sh is ONE script with SEQUENTIAL arms — manifold
# prebuilt, tbb pin, OCCT, Gmsh, OpenVDB — and any arm exiting non-zero means
# every arm after it never runs. A `_guard_env_exits_nonzero` assert on a
# DOWNSTREAM dep would then PASS for entirely the wrong reason (the upstream
# arm's exit) and test nothing at all — the same vacuity class this whole file
# exists to close. So the env-list form below takes the FULL override set
# explicitly, and each dep's section supplies healthy fixtures for every arm
# ahead of it rather than relying on live host state.

# _guard_run <VAR=VALUE>... — run the guard under exactly these overrides.
# Combined stdout+stderr on stdout; the guard's own exit status is returned.
_guard_run() {
    env "$@" bash "$GUARD" 2>&1
}

# _guard_env_exits_zero <VAR=VALUE>...
_guard_env_exits_zero() {
    [ -x "$GUARD" ] || return 1
    _guard_run "$@" >/dev/null
}

# _guard_env_exits_nonzero <VAR=VALUE>...
#
# Guarded on `-x "$GUARD"` first: a missing or unrunnable script also exits
# non-zero, which would otherwise false-pass every negation below.
_guard_env_exits_nonzero() {
    [ -x "$GUARD" ] || return 1
    ! _guard_run "$@" >/dev/null
}

# _guard_env_output_names <VAR=VALUE>... -- <needle>...
# Combined stdout+stderr of the guard must contain every needle (literal).
#
# ON A MISS it ECHOES the offending needle, the guard's exit status, the full
# override set it ran under, and the entire captured guard output, then returns
# 1. WHY: test_helpers.sh's assert() dumps its per-assert tmpfile only when that
# file is non-empty (`[ -s "$_f" ]`), so a helper that swallows the guard output
# into a shell variable and returns 1 silently produces a FAIL line with NO
# evidence attached — the reader cannot distinguish "the guard printed the wrong
# thing" from "the guard printed nothing" from "the guard was right and the
# harness misreported it". That gap is what kept the pipefail/SIGPIPE defect
# (see _out_contains) unroot-caused for a full task cycle.
#
# Emission is on the FAILURE path ONLY, so an all-green suite stays
# byte-for-byte unchanged — run_all.sh's cause_hint and dark-factory's
# classifier both parse this file's green output shape. Every continuation line
# carries the NON-whitespace `  | ` prefix test_helpers.sh documents:
# dark-factory's slot-timeout classifier is `^[ \t]*`-anchored, so a captured
# @@REIFY_SLOT_TIMEOUT@@ sentinel reproduced at column 0 (or merely indented)
# would misclassify the whole merge verify as semaphore starvation.
_guard_env_output_names() {
    [ -x "$GUARD" ] || return 1
    local -a envs=()
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
        envs+=("$1")
        shift
    done
    [ "${1:-}" = "--" ] && shift
    local out needle rc=0 e line
    out="$(_guard_run "${envs[@]}")" || rc=$?
    for needle in "$@"; do
        if ! _out_contains "$out" "$needle"; then
            echo "  | needle NOT FOUND in the guard output: $needle"
            echo "  | guard exit status: $rc"
            for e in "${envs[@]}"; do
                echo "  | override: $e"
            done
            echo "  | ---- captured guard output ----"
            # Fork-free, and deliberately NOT `printf | sed`: this is the
            # failure path, where losing the dump to a pipefail surprise is
            # worst. A herestring is not a pipeline, so nothing here is
            # exposed to the hazard _out_contains documents.
            while IFS= read -r line; do
                echo "  | $line"
            done <<< "$out"
            echo "  | ---- end captured guard output ----"
            return 1
        fi
    done
    return 0
}

# --- OCCT-positional wrappers ----------------------------------------------
# Thin adapters over the env-list forms above, kept so the OCCT sections read
# as they did before the file grew two more deps.

# _guard_exits_zero <lib_dir> <include_dir>
_guard_exits_zero() {
    _guard_env_exits_zero OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2"
}

# _guard_exits_nonzero <lib_dir> <include_dir>
_guard_exits_nonzero() {
    _guard_env_exits_nonzero OCCT_LIB_DIR="$1" OCCT_INCLUDE_DIR="$2"
}

# _out_contains <haystack> <needle> — literal (grep -F semantics) substring
# containment, used by every output assert in this file.
#
# FORK-FREE BY CONSTRUCTION, and it must stay that way. The obvious
# `printf '%s' "$haystack" | grep -qF -- "$needle"` is WRONG under this file's
# `set -euo pipefail` (line 96): `grep -q` exits the instant it matches and
# closes the pipe, the bash-builtin `printf` writer is then killed by SIGPIPE
# (141), and pipefail makes the PIPELINE report printf's 141 even though grep
# exited 0 — so a MATCH is reported as a MISS. Measured over 20000 iterations
# on the real ~1.1KB guard payload: 28 misses, PIPESTATUS "141 0" (writer
# killed, grep succeeded) EVERY time — 0.14% per call, ~2.8% per run across
# this file's ~20 output asserts. That is the entire observed flake, and
# because the miss is silent it named a DIFFERENT assertion on each run.
#
# The `case` form has no pipeline, so no pipefail exposure at all, and no fork.
# The needle is QUOTED inside the pattern, which is what keeps matching LITERAL
# (grep -F semantics) rather than glob. Section 0's
# `_containment_has_no_grep_pipeline` pins this deterministically — do not
# "simplify" it back into a pipeline.
_out_contains() {
    case "$1" in
        *"$2"*) return 0 ;;
        *)      return 1 ;;
    esac
}

# _lines_contain_exact <newline-separated-lines> <needle> — whole-LINE exact
# containment, i.e. `grep -qxF` semantics, with the same fork-free
# no-pipeline construction and for the same reason as _out_contains above.
# Both sides are wrapped in a newline so the pattern can only match a COMPLETE
# line, never a substring of one (which is exactly what `-x` buys).
_lines_contain_exact() {
    case $'\n'"$1"$'\n' in
        *$'\n'"$2"$'\n'*) return 0 ;;
        *)                return 1 ;;
    esac
}

# _guard_output_names <lib_dir> <include_dir> <needle>...
_guard_output_names() {
    local libdir="$1" incdir="$2"
    shift 2
    _guard_env_output_names OCCT_LIB_DIR="$libdir" OCCT_INCLUDE_DIR="$incdir" -- "$@"
}

# _majmin_lines — MAJOR.MINOR projection of each non-empty line on stdin,
# mirroring the guard's occt_majmin(). Used for the cross-artifact pin so both
# sides are compared the way the guard itself compares them.
_majmin_lines() {
    local _v
    while IFS= read -r _v; do
        [ -n "$_v" ] || continue
        printf '%s\n' "$_v" | cut -d. -f1,2
    done
}

# ---------------------------------------------------------------------------
# Parity parsers
#
# Both sides must FAIL LOUDLY (empty result -> a named assert failure) when
# their anchor is not found. A parity test that silently degrades to
# "" == "" would recreate the exact class of vacuity this task fixes.
# ---------------------------------------------------------------------------

# _rust_occt_list <fn-name> — the ordered `NativeDep::Occt => &[...]` string
# literals from the named fn block in crates/reify-build-utils/src/lib.rs, one
# per line. Anchored to `fn <name>` and bounded by that fn's closing brace, so
# the four `NativeDep::Occt =>` arms in the file can never be confused.
_rust_occt_list() {
    awk -v fname="fn $1" '
        index($0, fname) { infn = 1; next }
        infn && /^    }$/ { exit }
        infn && index($0, "NativeDep::Occt =>") { inarm = 1 }
        inarm {
            n = split($0, parts, "\"")
            for (i = 2; i <= n; i += 2) print parts[i]
            if (index($0, "]")) exit
        }
    ' "$RUST_SRC"
}

# _rust_occt_scalar <fn-name> — the single string literal on the
# `NativeDep::Occt =>` arm of the named fn block. Same anchoring rules.
_rust_occt_scalar() {
    awk -v fname="fn $1" '
        index($0, fname) { infn = 1; next }
        infn && /^    }$/ { exit }
        infn && index($0, "NativeDep::Occt =>") {
            n = split($0, parts, "\"")
            if (n >= 2) print parts[2]
            exit
        }
    ' "$RUST_SRC"
}

# _rust_snap_root — the literal dir find_dir_with_override's snap fallback
# scans (`std::fs::read_dir("/snap/freecad")`). Anchored INSIDE the
# snap_subdir block so an unrelated read_dir elsewhere cannot satisfy it.
_rust_snap_root() {
    awk '
        index($0, "let snap_subdir = match sentinel") { insnap = 1; next }
        insnap && /^}/ { exit }
        insnap && index($0, "read_dir(") {
            n = split($0, parts, "\"")
            if (n >= 2) print parts[2]
            exit
        }
    ' "$RUST_SRC"
}

# _rust_snap_map — `<sentinel> <subdir>` pairs from that same `match sentinel`,
# one per line, in arm order.
_rust_snap_map() {
    awk '
        index($0, "let snap_subdir = match sentinel") { insnap = 1; next }
        insnap && /^[[:space:]]*};/ { exit }
        insnap {
            n = split($0, parts, "\"")
            if (n >= 4) print parts[2], parts[4]
        }
    ' "$RUST_SRC"
}

# _bash_snap_root — the DEFAULT of the guard's OCCT_SNAP_ROOT assignment.
_bash_snap_root() {
    sed -n 's/^OCCT_SNAP_ROOT="${OCCT_SNAP_ROOT:-\(.*\)}"$/\1/p' "$GUARD" | head -1
}

# _bash_snap_map — `<sentinel> <subdir>` pairs from occt_find_dir's
# `case "$sentinel"`, one per line, in arm order.
_bash_snap_map() {
    awk '
        index($0, "case \"$sentinel\" in") { incase = 1; next }
        incase && index($0, "esac") { exit }
        incase && index($0, "snap_subdir=") {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            p = index(line, ")")
            if (p == 0) next
            pat = substr(line, 1, p - 1)
            rest = substr(line, p + 1)
            n = split(rest, parts, "\"")
            if (n >= 2) print pat, parts[2]
        }
    ' "$GUARD"
}

# _extract_bash_array <VAR> — elements of the named bash array, one per line,
# in declaration order, from shell source on stdin. Handles both the one-line
# `VAR=(a b c)` and the multi-line form.
_extract_bash_array() {
    awk -v var="$1" '
        !inarr && $0 ~ ("^[[:space:]]*" var "=\\(") {
            inarr = 1
            sub(/^[^(]*\(/, "")
        }
        inarr {
            line = $0
            sub(/#.*/, "", line)
            closed = (index(line, ")") > 0)
            sub(/\).*/, "", line)
            gsub(/"/, "", line)
            n = split(line, toks, /[[:space:]]+/)
            for (i = 1; i <= n; i++) if (toks[i] != "") print toks[i]
            if (closed) exit
        }
    '
}

# _bash_guard_array <VAR> — the named array declared anywhere in
# scripts/check-manifold-deps.sh.
_bash_guard_array() {
    _extract_bash_array "$1" < "$GUARD"
}

# _bash_occt_array <VAR> — the named array as declared INSIDE the
# `occt-candidates` marker block, so a same-named array elsewhere in the file
# can never satisfy the parity parse.
_bash_occt_array() {
    sed -n '/# BEGIN occt-candidates/,/# END occt-candidates/p' "$GUARD" \
        | _extract_bash_array "$1"
}

# _bash_occt_scalar <VAR> — value of the named scalar assignment inside the
# `occt-candidates` marker block.
_bash_occt_scalar() {
    awk -v var="$1" '
        index($0, "# BEGIN occt-candidates") { inblk = 1; next }
        index($0, "# END occt-candidates") { exit }
        !inblk { next }
        $0 ~ ("^[[:space:]]*" var "=") {
            line = $0
            sub(/^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*=/, "", line)
            sub(/[[:space:]]*#.*/, "", line)
            gsub(/"/, "", line)
            gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
            print line
            exit
        }
    ' "$GUARD"
}

# _setup_dev_occt_version — the version scripts/setup-dev.sh's OCCT block
# expects back from dpkg. Anchored to that section's `# ---------- OCCT`
# banner and bounded by the next banner, so the parse cannot drift onto an
# unrelated `installed_ver` comparison elsewhere in the script.
_setup_dev_occt_version() {
    awk '
        index($0, "# ---------- OCCT") { insec = 1; next }
        insec && /^# ---------- / { exit }
        insec && index($0, "installed_ver") {
            if (match($0, /"\$installed_ver"[[:space:]]*=[[:space:]]*"[^"]+"/)) {
                seg = substr($0, RSTART, RLENGTH)
                n = split(seg, parts, "\"")
                print parts[4]
                exit
            }
        }
    ' "$SETUP_DEV"
}

# Derived once, up here, because section 5's positive-control fixture also has
# to carry an ACCEPTED version once the SONAME pin exists — hardcoding it in
# two places would turn a legitimate pin bump into a multi-file edit. The
# non-empty assert lives with the SONAME section below.
_ACCEPTED_SONAMES="$(_bash_guard_array OCCT_ACCEPTED_SONAMES)"
_ACCEPTED_FIRST="$(printf '%s\n' "$_ACCEPTED_SONAMES" | head -1)"

# ---------------------------------------------------------------------------
# 0. SELF-CHECK — the needle-containment primitive itself.
#
# Placed AHEAD of every guard case deliberately: a broken containment primitive
# would otherwise surface as a random guard assertion failing, and be
# attributed to the guard (or to /opt/reify-deps contention, or to arm
# ordering) rather than to the test harness. That misattribution is exactly
# what kept this flake unroot-caused for a full task cycle.
#
# WHAT IS UNDER TEST: `_out_contains` must report containment DETERMINISTICALLY.
# A `printf | grep -q` implementation does not: `grep -q` exits the instant it
# matches and closes the pipe, the bash-builtin `printf` writer is killed by
# SIGPIPE (141), and `set -o pipefail` (line 96 of this file) makes the
# PIPELINE report 141 — so a MATCH is returned as a MISS. Measured over 20000
# iterations on the real ~1.1KB guard payload: 28 misses, PIPESTATUS "141 0"
# (writer killed, grep succeeded) every time — 0.14% per call, ~2.8% per run
# across this file's ~20 output asserts.
#
# WHY THE LOOP IS STATISTICAL AND NOT A SINGLE DETERMINISTIC CASE: an
# oversized (>64KiB) payload with the needle at byte 0 was MEASURED not to
# reproduce it (0 misses / 50 at 200011 bytes), so no deterministic behavioural
# reproduction exists. At the measured 0.0014/call rate, N=5000 gives
# P(false pass | defect present) = (1-0.0014)^5000 ~= 9e-4. The loop breaks on
# the FIRST miss, so RED is cheap; the fixed form is fork-free, so GREEN pays
# ~0.7s for the full 5000.
#
# The paired STRUCTURAL assert makes regression detection deterministic even on
# a statistically lucky run: the primitive's body must contain no `| grep`
# pipeline at all, which is the hazard CLASS rather than one instance of it.
# ---------------------------------------------------------------------------
echo ""
echo "--- 0: self-check — the needle-containment primitive is not itself flaky ---"

# A real guard payload (lib resolves, headers do not), captured ONCE — the same
# ~1.1KB stdout+stderr every output assert in this file is matched against.
_SELF_LIB_OK="$(_mk_lib_fixture selfcheck-lib "$_ACCEPTED_FIRST")"
_SELF_INC_MISSING="$(_mk_empty_fixture selfcheck-include)"
_SELF_PAYLOAD="$(OCCT_LIB_DIR="$_SELF_LIB_OK" OCCT_INCLUDE_DIR="$_SELF_INC_MISSING" bash "$GUARD" 2>&1 || true)"
_SELF_NEEDLE="Standard_Failure.hxx"
_SELF_STRESS_N=5000

assert "self-check captured a non-empty real guard payload to match against" \
    test -n "$_SELF_PAYLOAD"

# Reference oracle, asserted BEFORE the stress loop so that loop can only ever
# fail for the SIGPIPE reason and never because the needle is genuinely absent.
# Run under `bash -c` (default shell options — no pipefail) using bash's own
# fork-free `==` pattern match, which has no pipeline and cannot take SIGPIPE.
assert "self-check needle '$_SELF_NEEDLE' is genuinely PRESENT in that payload (bash [[ == * ]] oracle)" \
    bash -c '[[ "$1" == *"$2"* ]]' _ "$_SELF_PAYLOAD" "$_SELF_NEEDLE"

# _containment_is_deterministic — $_SELF_STRESS_N calls of the primitive on a
# payload/needle pair already proven to match. Breaks on the FIRST miss and
# reports the iteration index; the echoes land in assert's per-assert tmpfile,
# so a failure carries its own evidence in the archived verify log.
_containment_is_deterministic() {
    local i
    for ((i = 1; i <= _SELF_STRESS_N; i++)); do
        if ! _out_contains "$_SELF_PAYLOAD" "$_SELF_NEEDLE"; then
            echo "_out_contains reported a MISS at iteration $i of $_SELF_STRESS_N"
            echo "needle: $_SELF_NEEDLE"
            echo "payload bytes: ${#_SELF_PAYLOAD}"
            echo "the needle IS present (asserted above), so this is the"
            echo "pipefail + SIGPIPE defect, not a real miss."
            return 1
        fi
    done
    return 0
}

assert "_out_contains reports a PRESENT needle deterministically over $_SELF_STRESS_N calls" \
    _containment_is_deterministic

# Negative control, run in THIS shell (the primitive is a shell function, not
# an exported command, so it is not reachable from a `bash -c` child): the
# primitive must still MISS a needle that is genuinely absent. Without this, a
# bare `return 0` would satisfy the stress assert above.
_containment_negative_control() {
    ! _out_contains "$_SELF_PAYLOAD" "__no_such_needle_6493__"
}

assert "_out_contains still reports a genuinely ABSENT needle as a miss (negative control)" \
    _containment_negative_control

# _containment_has_no_grep_pipeline — DETERMINISTIC structural pin on the
# hazard class. `declare -f` re-renders the live function body, so this reads
# the primitive actually in force rather than a grep of the file. It is the
# half that still reds on a statistically lucky run of the loop above, and it
# is what stops the fork-free form being "simplified" back into a pipeline.
_containment_has_no_grep_pipeline() {
    local body
    body="$(declare -f _out_contains)" || return 1
    [ -n "$body" ] || return 1
    case "$body" in
        *'| grep'* | *'|grep'*)
            echo "_out_contains' body still contains a grep pipeline:"
            printf '%s\n' "$body"
            return 1
            ;;
    esac
    return 0
}

assert "_out_contains' body contains no '| grep' pipeline (no pipefail/SIGPIPE exposure)" \
    _containment_has_no_grep_pipeline

# --- A MISS must be DIAGNOSABLE, not silent.
#
# tests/infra/test_helpers.sh's assert() dumps captured evidence only when the
# checker actually wrote to its per-assert tmpfile (`[ -s "$_f" ]`). A
# `_guard_output_names` that swallows the guard output into a shell variable
# and then `return 1`s writes NOTHING, so the FAIL line reads
# "  FAIL: guard output NAMES <x>" with no captured-output block at all — the
# reader cannot tell whether the guard printed the wrong thing, printed
# nothing, or (as it turned out) printed exactly the right thing and the
# harness misreported it. That evidence gap is precisely why the SIGPIPE defect
# above survived a full task cycle unroot-caused.
#
# Behavioural, not prose: WHETHER evidence is emitted and whether it carries
# the two things a reader needs (which needle was missing, and what the guard
# actually said). Exact wording is deliberately not pinned.
_MISS_NEEDLE="__absent_needle_6493__"
_SELF_PAYLOAD_FIRST_LINE="${_SELF_PAYLOAD%%$'\n'*}"

assert "self-check has a non-empty first payload line to look for in the diagnostic" \
    test -n "$_SELF_PAYLOAD_FIRST_LINE"

# stdout AND stderr — the helper is free to use either; what matters is that
# assert()'s tmpfile (which captures both) ends up non-empty.
_MISS_DIAG="$(_guard_output_names "$_SELF_LIB_OK" "$_SELF_INC_MISSING" "$_MISS_NEEDLE" 2>&1 || true)"

assert "_guard_output_names EMITS evidence on a needle miss (assert's on-FAIL dump has something to show)" \
    test -n "$_MISS_DIAG"

assert "that evidence NAMES the needle that was missing ('$_MISS_NEEDLE')" \
    _out_contains "$_MISS_DIAG" "$_MISS_NEEDLE"

assert "that evidence carries the CAPTURED guard output (at least its first line)" \
    _out_contains "$_MISS_DIAG" "$_SELF_PAYLOAD_FIRST_LINE"

# The other half of the contract: an all-green run must stay byte-for-byte
# unchanged, because run_all.sh's cause_hint and dark-factory's classifier both
# parse this file's output shape. So the helper must emit on the FAILURE path
# only.
_no_emission_on_match() {
    local out
    out="$(_guard_output_names "$_SELF_LIB_OK" "$_SELF_INC_MISSING" "$_SELF_NEEDLE" 2>&1)" || return 1
    [ -z "$out" ] || {
        echo "_guard_output_names emitted on the SUCCESS path, which would change"
        echo "the green output shape run_all.sh and dark-factory parse:"
        printf '%s\n' "$out"
        return 1
    }
}

assert "_guard_output_names emits NOTHING when every needle matches (green shape unchanged)" \
    _no_emission_on_match

# ---------------------------------------------------------------------------
# 1. Guard script exists and is executable
# ---------------------------------------------------------------------------
echo ""
echo "--- 1: scripts/check-manifold-deps.sh exists and is executable ---"

assert "scripts/check-manifold-deps.sh exists" \
    test -f "$GUARD"

assert "scripts/check-manifold-deps.sh is executable" \
    test -x "$GUARD"

# ---------------------------------------------------------------------------
# 2. ABSENCE — neither headers nor libs resolvable
# ---------------------------------------------------------------------------
echo ""
echo "--- 2: absence — both OCCT dirs empty => red gate naming OCCT ---"

_EMPTY_LIB="$(_mk_empty_fixture absent-lib)"
_EMPTY_INC="$(_mk_empty_fixture absent-include)"

assert "guard exits NON-zero when both OCCT_LIB_DIR and OCCT_INCLUDE_DIR lack their sentinels" \
    _guard_exits_nonzero "$_EMPTY_LIB" "$_EMPTY_INC"

assert "guard output NAMES OCCT when neither half resolves" \
    _guard_output_names "$_EMPTY_LIB" "$_EMPTY_INC" "OCCT"

# ---------------------------------------------------------------------------
# 3. LIB-ONLY MISSING — headers resolve, libs do not
# ---------------------------------------------------------------------------
echo ""
echo "--- 3: lib-only missing => red gate naming libTKernel.so and the dir ---"

_INC_OK="$(_mk_include_fixture libonly-include)"
_LIB_MISSING="$(_mk_empty_fixture libonly-lib)"

assert "guard exits NON-zero when only the OCCT lib dir lacks libTKernel.so" \
    _guard_exits_nonzero "$_LIB_MISSING" "$_INC_OK"

assert "guard output NAMES libTKernel.so and the offending lib dir" \
    _guard_output_names "$_LIB_MISSING" "$_INC_OK" "libTKernel.so" "$_LIB_MISSING"

# ---------------------------------------------------------------------------
# 4. INCLUDE-ONLY MISSING — libs resolve, headers do not.
#    find() returns None when EITHER half is unresolved, so this mixed case
#    is a silent stub build today.
# ---------------------------------------------------------------------------
echo ""
echo "--- 4: include-only missing => red gate naming Standard_Failure.hxx ---"

# Built at an ACCEPTED version: the SONAME pin is never reached by the
# include-only case (presence resolution fails first), but the positive control
# at the end of this section runs the guard to completion and must stay green
# across a legitimate future pin bump.
_LIB_OK="$(_mk_lib_fixture inconly-lib "$_ACCEPTED_FIRST")"
_INC_MISSING="$(_mk_empty_fixture inconly-include)"

assert "guard exits NON-zero when only the OCCT include dir lacks Standard_Failure.hxx" \
    _guard_exits_nonzero "$_LIB_OK" "$_INC_MISSING"

assert "guard output NAMES Standard_Failure.hxx and the offending include dir" \
    _guard_output_names "$_LIB_OK" "$_INC_MISSING" "Standard_Failure.hxx" "$_INC_MISSING"

assert "guard exits 0 when BOTH override dirs carry their sentinels (positive control)" \
    _guard_exits_zero "$_LIB_OK" "$_INC_OK"

# ---------------------------------------------------------------------------
# 5. PARITY — the bash mirror equals NativeDep::Occt, order included
# ---------------------------------------------------------------------------
echo ""
echo "--- 5: parity — bash occt-candidates block mirrors NativeDep::Occt ---"

_RUST_LIB_CANDS="$(_rust_occt_list lib_candidates)"
_RUST_INC_CANDS="$(_rust_occt_list include_candidates)"
_RUST_LIB_SENT="$(_rust_occt_scalar lib_sentinel)"
_RUST_INC_SENT="$(_rust_occt_scalar include_sentinel)"

_BASH_LIB_CANDS="$(_bash_occt_array OCCT_LIB_CANDIDATES)"
_BASH_INC_CANDS="$(_bash_occt_array OCCT_INCLUDE_CANDIDATES)"
_BASH_LIB_SENT="$(_bash_occt_scalar OCCT_LIB_SENTINEL)"
_BASH_INC_SENT="$(_bash_occt_scalar OCCT_INCLUDE_SENTINEL)"

# Anchor-integrity asserts FIRST: without these, a renamed fn or a dropped
# marker block degrades every comparison below to "" == "" and the whole
# parity section passes while guarding nothing.
assert "Rust parse of NativeDep::Occt lib_candidates is non-empty (anchor 'fn lib_candidates' found)" \
    test -n "$_RUST_LIB_CANDS"
assert "Rust parse of NativeDep::Occt include_candidates is non-empty (anchor 'fn include_candidates' found)" \
    test -n "$_RUST_INC_CANDS"
assert "Rust parse of NativeDep::Occt lib_sentinel is non-empty (anchor 'fn lib_sentinel' found)" \
    test -n "$_RUST_LIB_SENT"
assert "Rust parse of NativeDep::Occt include_sentinel is non-empty (anchor 'fn include_sentinel' found)" \
    test -n "$_RUST_INC_SENT"
assert "bash parse of OCCT_LIB_CANDIDATES is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_LIB_CANDS"
assert "bash parse of OCCT_INCLUDE_CANDIDATES is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_INC_CANDS"
assert "bash parse of OCCT_LIB_SENTINEL is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_LIB_SENT"
assert "bash parse of OCCT_INCLUDE_SENTINEL is non-empty (occt-candidates marker block found)" \
    test -n "$_BASH_INC_SENT"

# Order-sensitive comparison: the priority order IS the invariant (system
# paths ahead of /opt/reify-deps/lib, which ships gmsh's transitive OCCT 7.9).
_parity_diff() {
    diff <(printf '%s\n' "$1") <(printf '%s\n' "$2") 2>&1 || true
}

_LIB_CAND_DIFF="$(_parity_diff "$_RUST_LIB_CANDS" "$_BASH_LIB_CANDS")"
if [ -n "$_LIB_CAND_DIFF" ]; then
    echo "  OCCT lib-candidate drift (< reify-build-utils, > check-manifold-deps.sh):"
    printf '%s\n' "$_LIB_CAND_DIFF" | sed 's/^/    /'
fi
assert "bash OCCT_LIB_CANDIDATES equals NativeDep::Occt lib_candidates, order included" \
    test -z "$_LIB_CAND_DIFF"

_INC_CAND_DIFF="$(_parity_diff "$_RUST_INC_CANDS" "$_BASH_INC_CANDS")"
if [ -n "$_INC_CAND_DIFF" ]; then
    echo "  OCCT include-candidate drift (< reify-build-utils, > check-manifold-deps.sh):"
    printf '%s\n' "$_INC_CAND_DIFF" | sed 's/^/    /'
fi
assert "bash OCCT_INCLUDE_CANDIDATES equals NativeDep::Occt include_candidates, order included" \
    test -z "$_INC_CAND_DIFF"

assert "bash OCCT_LIB_SENTINEL ('$_BASH_LIB_SENT') equals NativeDep::Occt lib_sentinel ('$_RUST_LIB_SENT')" \
    test "$_BASH_LIB_SENT" = "$_RUST_LIB_SENT"

assert "bash OCCT_INCLUDE_SENTINEL ('$_BASH_INC_SENT') equals NativeDep::Occt include_sentinel ('$_RUST_INC_SENT')" \
    test "$_BASH_INC_SENT" = "$_RUST_INC_SENT"

# --- 6: the same mirror one layer down — the snap-fallback ALGORITHM.
#
# The candidate lists above are DATA; the numbered-/snap/freecad fallback is
# duplicated LOGIC, and on any host with system OCCT the candidate loop
# short-circuits before either side's fallback runs, so nothing exercises it.
# These asserts pin it by declaration instead: a change to the scanned root, to
# the sentinel -> subdir mapping, or to arm order on the Rust side fails here
# rather than silently diverging.
_RUST_SNAP_ROOT="$(_rust_snap_root)"
_BASH_SNAP_ROOT="$(_bash_snap_root)"
_RUST_SNAP_MAP="$(_rust_snap_map)"
_BASH_SNAP_MAP="$(_bash_snap_map)"

assert "Rust parse of the snap-fallback root is non-empty (anchor 'let snap_subdir = match sentinel' found)" \
    test -n "$_RUST_SNAP_ROOT"
assert "bash parse of OCCT_SNAP_ROOT's default is non-empty (defaulted assignment found)" \
    test -n "$_BASH_SNAP_ROOT"
assert "Rust parse of the snap sentinel -> subdir map is non-empty" \
    test -n "$_RUST_SNAP_MAP"
assert "bash parse of the snap sentinel -> subdir case is non-empty (occt_find_dir's case found)" \
    test -n "$_BASH_SNAP_MAP"

assert "OCCT_SNAP_ROOT's default ('$_BASH_SNAP_ROOT') equals find_dir_with_override's read_dir literal ('$_RUST_SNAP_ROOT')" \
    test "$_BASH_SNAP_ROOT" = "$_RUST_SNAP_ROOT"

_SNAP_MAP_DIFF="$(_parity_diff "$_RUST_SNAP_MAP" "$_BASH_SNAP_MAP")"
if [ -n "$_SNAP_MAP_DIFF" ]; then
    echo "  OCCT snap sentinel->subdir drift (< reify-build-utils, > check-manifold-deps.sh):"
    printf '%s\n' "$_SNAP_MAP_DIFF" | sed 's/^/    /'
fi
assert "bash snap sentinel->subdir case equals find_dir_with_override's match arms, order included" \
    test -z "$_SNAP_MAP_DIFF"

# The override-precedence rule the two sides also share (an exported-but-EMPTY
# override counts as UNSET) is pinned on the Rust side by
# find_dir_ignores_exported_but_empty_override; it is not drivable from here
# without falling back to live host state, which this file deliberately avoids.

# ---------------------------------------------------------------------------
# 6. SONAME pin — the resolved version must be a declared, accepted one.
#
# All fixtures pair their lib dir with an include dir that DOES carry the
# header sentinel, so the presence arm is satisfied and the SONAME rule is the
# only thing under test.
# ---------------------------------------------------------------------------
echo ""
echo "--- 6: SONAME pin — accepted / unaccepted / conda-shaped / undeterminable ---"

assert "OCCT_ACCEPTED_SONAMES parses non-empty from the guard (anchor found, pin not vacuous)" \
    test -n "$_ACCEPTED_SONAMES"

_SON_INC="$(_mk_include_fixture soname-include)"

# 7 — accepted, and RECORDED.
_SON_OK="$(_mk_lib_fixture soname-accepted "$_ACCEPTED_FIRST")"
assert "guard exits 0 for an accepted SONAME ('$_ACCEPTED_FIRST', Debian two-hop chain)" \
    _guard_exits_zero "$_SON_OK" "$_SON_INC"

# The RECORDING half of the arm. Every other green-path assert runs through
# _guard_exits_zero, which discards stdout — so without this one, deleting the
# guard's [ok] line (or regressing it to name the wrong dir) leaves the whole
# suite passing. That is the same "a passing suite and a deleted suite are
# indistinguishable from outside" failure this task exists to close.
assert "guard RECORDS the resolved OCCT version and both resolved dirs on the green path" \
    _guard_output_names "$_SON_OK" "$_SON_INC" "OCCT $_ACCEPTED_FIRST" "$_SON_OK" "$_SON_INC"

# 8 — patch-shaped: the dev symlink points one hop further on a functionally
# identical OCCT. Exact-matching the verbatim segment would make this a red
# gate on every RUN_RUST=1 verify; the major.minor projection keeps it green,
# and build.rs links the verbatim `libTKernel.so.<v>.1`, which exists.
_SON_PATCH="$(_mk_patchlink_lib_fixture soname-patchlink "$_ACCEPTED_FIRST")"
assert "guard exits 0 for a patch-shaped link target ('$_ACCEPTED_FIRST.1' — same OCCT, different packaging)" \
    _guard_exits_zero "$_SON_PATCH" "$_SON_INC"

assert "guard RECORDS the verbatim patch-shaped segment ('$_ACCEPTED_FIRST.1'), not just the accepted major.minor" \
    _guard_output_names "$_SON_PATCH" "$_SON_INC" "OCCT $_ACCEPTED_FIRST.1"

# 9 — unaccepted. 0.0 can never be a legitimate OCCT release, so this case
# survives any future widening of the accepted set.
_SON_BAD="$(_mk_lib_fixture soname-unaccepted 0.0)"
assert "guard exits NON-zero for an unaccepted SONAME (0.0)" \
    _guard_exits_nonzero "$_SON_BAD" "$_SON_INC"

assert "guard output NAMES OCCT, the resolved version (0.0) and the accepted set" \
    _guard_output_names "$_SON_BAD" "$_SON_INC" "OCCT" "0.0" "$_ACCEPTED_FIRST"

# 10 — conda-shaped one-level symlink, the exact layout at /opt/reify-deps/lib.
_SON_CONDA="$(_mk_conda_lib_fixture soname-conda 7.9.3)"
assert "guard exits NON-zero for the conda one-level layout (libTKernel.so -> libTKernel.so.7.9.3)" \
    _guard_exits_nonzero "$_SON_CONDA" "$_SON_INC"

assert "guard output NAMES the verbatim trailing segment 7.9.3 (not 7.9, not 7)" \
    _guard_output_names "$_SON_CONDA" "$_SON_INC" "7.9.3"

# 11 — undeterminable: sentinel present but not a symlink. find() resolves the
# dir, has_occt IS set, and build.rs silently substitutes a hard-coded version.
_SON_PLAIN="$(_mk_plainfile_lib_fixture soname-plainfile)"
assert "guard exits NON-zero when libTKernel.so is a regular file (no readable SONAME)" \
    _guard_exits_nonzero "$_SON_PLAIN" "$_SON_INC"

assert "guard output says the SONAME could not be determined and names the path" \
    _guard_output_names "$_SON_PLAIN" "$_SON_INC" "could not determine" "$_SON_PLAIN/libTKernel.so"

# ---------------------------------------------------------------------------
# 7. CROSS-ARTIFACT PIN — setup-dev.sh's dpkg expectation vs. the accepted set.
#
# setup-dev.sh installs OCCT out of band and is not part of the verify plan, so
# nothing else forces the two to agree. If they drift, setup-dev.sh provisions
# a version the gate will then reject.
# ---------------------------------------------------------------------------
echo ""
echo "--- 7: cross-artifact pin — setup-dev.sh's OCCT version is in the accepted set ---"

_SETUP_DEV_VER="$(_setup_dev_occt_version)"

assert "scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "setup-dev.sh's OCCT block yields a version (anchor '# ---------- OCCT' + installed_ver found)" \
    test -n "$_SETUP_DEV_VER"

# Compared the way the GUARD compares: both sides projected to major.minor.
# setup-dev.sh's dpkg parse is `grep -oP '\d+\.\d+'`, so its value can only
# ever be major.minor — matching it against a raw accepted set would forbid the
# set from ever holding a three-segment SONAME (the conda-shaped `7.9.3` the
# guard itself demonstrates) without editing setup-dev.sh to a string its regex
# cannot produce.
_ACCEPTED_MAJMIN="$(printf '%s\n' "$_ACCEPTED_SONAMES" | _majmin_lines)"
_SETUP_DEV_MAJMIN="$(printf '%s\n' "$_SETUP_DEV_VER" | _majmin_lines)"

if ! _lines_contain_exact "$_ACCEPTED_MAJMIN" "$_SETUP_DEV_MAJMIN"; then
    echo "  OCCT version drift: setup-dev.sh provisions '$_SETUP_DEV_VER' (major.minor"
    echo "  $_SETUP_DEV_MAJMIN), accepted set projects to:"
    printf '%s\n' "$_ACCEPTED_MAJMIN" | sed 's/^/    /'
fi
# Deliberately left as a pipeline: unlike the diagnostic above, this one runs
# in a FRESH `bash -c` child, which starts with default shell options — VERIFIED
# `pipefail off` there, and shell options are not inherited across a `bash -c`.
# With pipefail off the pipeline reports grep's status, so a SIGPIPE'd printf
# cannot turn a match into a miss. Routing it through _lines_contain_exact is
# not possible anyway: that is a shell function, not reachable from the child.
assert "setup-dev.sh's OCCT version ('$_SETUP_DEV_VER') projects (major.minor) into OCCT_ACCEPTED_SONAMES" \
    bash -c 'printf "%s\n" "$1" | grep -qxF -- "$2"' _ "$_ACCEPTED_MAJMIN" "$_SETUP_DEV_MAJMIN"

# ---------------------------------------------------------------------------
# 8. GMSH PRESENCE — the same silent vacuity, one dep over (task #6493).
#
# reify_build_utils::find(NativeDep::Gmsh) returns None when EITHER half is
# unresolved, and crates/reify-kernel-gmsh/build.rs answers with a
# `cargo:warning` plus a bare `return` — byte-for-byte the same fail-OPEN shape
# OCCT had before task #6343. 81 has_gmsh-gated items then vanish: the suite
# reports zero tests REPORTED, not zero tests FAILED, and the gate goes green
# over a mesher nothing exercised.
#
# build.rs stays deliberately fail-OPEN (its cfg(not(has_gmsh)) stub modules
# are a sanctioned, tested configuration). The GATE lives here, in
# check-manifold-deps.sh, exactly as it does for OCCT.
#
# EVERY case supplies healthy OCCT overrides: the OCCT arm runs AHEAD of the
# Gmsh arm in the same script, so without them a `_guard_env_exits_nonzero`
# assert would pass on the OCCT arm's exit and test nothing. Every negative
# case pairs its exit-code assert with an output assert naming a GMSH-specific
# string, so a failure that really came from upstream stays attributable.
# ---------------------------------------------------------------------------
echo ""
echo "--- 8: gmsh presence — missing libs / headers => red gate naming gmsh ---"

# Healthy upstream (OCCT), built at the DERIVED accepted version rather than a
# hardcoded 7.8, so a legitimate future pin bump stays a one-line diff in one
# file.
_UPSTREAM_OCCT_LIB="$(_mk_lib_fixture upstream-occt-lib "$_ACCEPTED_FIRST")"
_UPSTREAM_OCCT_INC="$(_mk_include_fixture upstream-occt-include)"
_OCCT_OK=(OCCT_LIB_DIR="$_UPSTREAM_OCCT_LIB" OCCT_INCLUDE_DIR="$_UPSTREAM_OCCT_INC")

# Gmsh's live shape at /opt/reify-deps/lib is ONE hop
# (libgmsh.so -> libgmsh.so.4.15.2), so the fixture uses that layout.
_GMSH_LIB_OK="$(_mk_conda_lib_fixture gmsh-lib-ok 4.15.2 libgmsh.so)"
_GMSH_INC_OK="$(_mk_include_fixture gmsh-include-ok gmshc.h)"
_GMSH_LIB_MISSING="$(_mk_empty_fixture gmsh-lib-missing)"
_GMSH_INC_MISSING="$(_mk_empty_fixture gmsh-include-missing)"

assert "guard exits NON-zero when the Gmsh lib dir lacks libgmsh.so (headers present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_MISSING" GMSH_INCLUDE_DIR="$_GMSH_INC_OK"

assert "guard output NAMES libgmsh.so and the offending Gmsh lib dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_MISSING" GMSH_INCLUDE_DIR="$_GMSH_INC_OK" \
        -- "libgmsh.so" "$_GMSH_LIB_MISSING"

assert "guard exits NON-zero when the Gmsh include dir lacks gmshc.h (libs present)" \
    _guard_env_exits_nonzero "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_MISSING"

assert "guard output NAMES gmshc.h and the offending Gmsh include dir" \
    _guard_env_output_names "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_MISSING" \
        -- "gmshc.h" "$_GMSH_INC_MISSING"

assert "guard exits 0 when BOTH Gmsh override dirs carry their sentinels (positive control)" \
    _guard_env_exits_zero "${_OCCT_OK[@]}" \
        GMSH_LIB_DIR="$_GMSH_LIB_OK" GMSH_INCLUDE_DIR="$_GMSH_INC_OK"

test_summary
