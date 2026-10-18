#!/usr/bin/env bash
# Preflight guard for the workspace's native deps: its arms all run BEFORE
# any expensive compile, each converting a silent or cryptic downstream
# failure into a fast, actionable message.
#
# THE SILENT-VACUITY RULE — why the native-dep arms exist. Stated ONCE here,
# and REFERENCED rather than restated by the kernel build.rs files and
# tests/infra/test_occt_deps_preflight.sh:
#
#   A missing native dep is not merely cryptic, it is SILENT.
#   `reify_build_utils::find(NativeDep::X)` returns None when EITHER the header
#   dir or the lib dir is unresolved; that kernel's build.rs answers with a
#   `cargo:warning` and a bare `return`, setting no `has_<dep>` cfg; and every
#   `#[cfg(has_<dep>)]` item — the crate's whole test surface included — then
#   stops being compiled AT ALL. The suite reports ZERO tests rather than zero
#   failures, so a passing suite and a DELETED suite are indistinguishable from
#   outside and the gate goes green over a kernel nothing exercised.
#
#   The build.rs files stay deliberately fail-OPEN — their stub modules are a
#   sanctioned, tested configuration — which is exactly why the GATE has to
#   live out here instead.
#
#   1. manifold prebuilt. The `[target.x86_64-unknown-linux-gnu.manifold]`
#      override in .cargo/config.toml makes Cargo link prebuilt static libs
#      from /opt/reify-deps/manifold/lib instead of building manifold from
#      source. If those libs are missing or stale (a `manifold-csg-sys` pin
#      bump rebuilt the crate but no one re-ran the deps script), the failure
#      is otherwise a cryptic linker error deep in a multi-minute build. This
#      arm names `scripts/build-manifold-deps.sh` instead.
#
#   2. tbb pin dir (task #5192, mechanism A''). See that arm's own banner.
#
#   3. ONE ARM PER ROW of crates/reify-build-utils/native-deps.toml, the table
#      reify_build_utils decodes for the build's own has_<dep> verdict. The
#      rows are read through scripts/native_deps_table.py and dispatched
#      fail-closed through NATIVE_DEP_ARMS: an unreadable table, a row with no
#      arm, or an arm with no row reds the guard before arm 1 runs.
#
#      occt (task #6343). The rule above, plus a version pin: reify pins OCCT
#      nowhere else in-tree, so a distro upgrade that moves the version
#      relinks the kernel with nothing louder than a `cargo:warning` — and the
#      has_occt suite that would have caught the regression is exactly what
#      disappears when OCCT goes missing. The XDE toolkits build.rs links for
#      the STEP assembly reader (the row's xde_toolkits) are fatal when absent
#      and recorded when present; gmsh loads its own 7.9 copies of them beside
#      these in-process (hazard: docs/prds/v0_6/step-assembly-import.md §3.1).
#
#      gmsh (task #6493). The rule again, one dep over. PRESENCE is fatal; the
#      resolved SONAME is RECORDED but deliberately NOT pinned to an accepted
#      set — see dep_presence_arm's recording half for why OCCT's pin does not
#      carry over.
#
#      openvdb (task #6493). The third instance of gmsh's shape. Presence
#      fatal, SONAME recorded and not pinned, same reasons.
#
# Arms run in order — manifold, tbb pin, then the table rows in table order —
# and the first failure exits, so an arm can only assume the arms ahead of it
# passed. tests/infra/test_occt_deps_preflight.sh drives each downstream arm
# with healthy fixtures for every arm ahead of it for exactly that reason.
#
# verify.sh runs this as the first plan entry when Rust work is in scope.
#
# Exit 0 when every arm passes; non-zero with a clear message otherwise. Fast:
# reads only Cargo.lock, the VERSION stamp, the native-deps table, and a
# handful of stat()s (no registry / no cargo / no compile).
set -euo pipefail

err() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
# stdout, not stderr: this is the RECORDING half of EVERY presence arm (OCCT,
# Gmsh, OpenVDB) — it puts the resolved version and dirs in the verify log so a
# reviewer reading a green reify-kernel-<dep> result can see WHICH install
# produced it. add_tool() only executes plan entries, so nothing parses this
# script's stdout.
ok() { printf '\033[1;32m[ok]\033[0m %s\n' "$*"; }

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ---------- native-deps table (task #8634) ----------
#
# Every per-dep datum the native-dep arms read — env names, cfg, ordered
# candidates, sentinels, the occt snap fallback and XDE toolkits — comes from
# this table, eval'd as the <ROW^^>_<COLUMN^^> declarations the reader prints.
# REIFY_NATIVE_DEPS_TABLE points the guard at another table (the test's
# fixture seam); the default IS the production table.
#
# Read and checked BEFORE the manifold arm: an unreadable table, or a row and
# an arm that do not pair up, is a defect in repo data, so it reds the same
# way whatever the host has installed.
NATIVE_DEPS_TABLE="${REIFY_NATIVE_DEPS_TABLE:-$REPO_ROOT/crates/reify-build-utils/native-deps.toml}"

# Only the reader's stdout is captured; its stderr, which names the defect,
# passes through.
if ! native_dep_decls="$(python3 -I "$REPO_ROOT/scripts/native_deps_table.py" "$NATIVE_DEPS_TABLE")"; then
    err "manifold-deps guard: native-deps table unreadable: $NATIVE_DEPS_TABLE"
    err "                     The reader's message above names the defect. This is a bug in"
    err "                     the table, NOT a missing install."
    exit 1
fi
eval "$native_dep_decls"

# Row -> arm. ONE map, checked in both directions here and read again by the
# dispatch at the end of this file.
declare -A NATIVE_DEP_ARMS=([occt]=occt_arm [gmsh]=gmsh_arm [openvdb]=openvdb_arm)

declare -A native_dep_row_set=()
dispatch_gaps=0
for row in "${NATIVE_DEP_ROWS[@]}"; do
    native_dep_row_set[$row]=1
    if [ -z "${NATIVE_DEP_ARMS[$row]+x}" ]; then
        err "manifold-deps guard: native-deps row '$row' has no gate arm, so it would ship"
        err "                     UNGATED (table: $NATIVE_DEPS_TABLE)."
        dispatch_gaps=1
    fi
done
for row in "${!NATIVE_DEP_ARMS[@]}"; do
    if [ -z "${native_dep_row_set[$row]+x}" ]; then
        err "manifold-deps guard: gate arm ${NATIVE_DEP_ARMS[$row]} has no '$row' row, so it"
        err "                     would never run (table: $NATIVE_DEPS_TABLE)."
        dispatch_gaps=1
    fi
done
if [ "$dispatch_gaps" -ne 0 ]; then
    err "Pair every table row with an arm in NATIVE_DEP_ARMS (scripts/check-manifold-deps.sh)."
    err "This is a bug in the table or in this script, NOT a missing install."
    exit 1
fi

LOCKFILE="$REPO_ROOT/Cargo.lock"
PREFIX="/opt/reify-deps/manifold"
LIBDIR="$PREFIX/lib"
STAMP="$PREFIX/VERSION"
REQUIRED_LIBS=(libmanifoldc.a libmanifold.a libClipper2.a libtbb.a)

hint() {
    err "Run:  ./scripts/build-manifold-deps.sh"
    err "(builds manifold's C++ libs once into $LIBDIR; see .cargo/config.toml's"
    err " [target.x86_64-unknown-linux-gnu.manifold] override and CLAUDE.md 'Local Dev Setup')."
}

# Crate version pinned in Cargo.lock.
CSG_SYS_VER="$(awk '
    /^name = "manifold-csg-sys"$/ { f=1; next }
    f && /^version = / { gsub(/[",]/,""); print $3; exit }
' "$LOCKFILE" 2>/dev/null || true)"
if [ -z "${CSG_SYS_VER:-}" ]; then
    err "manifold-deps guard: could not read manifold-csg-sys version from $LOCKFILE"
    exit 1
fi

if [ ! -f "$STAMP" ]; then
    err "manifold-deps guard: prebuilt missing — no $STAMP."
    hint
    exit 1
fi

for l in "${REQUIRED_LIBS[@]}"; do
    if [ ! -f "$LIBDIR/$l" ]; then
        err "manifold-deps guard: prebuilt incomplete — missing $LIBDIR/$l."
        hint
        exit 1
    fi
done

# Stamp format: "<crate-version> <upstream-tag>". Compare only the crate
# version — that is what determines the C ABI / link set the override targets.
STAMPED_VER="$(awk '{print $1}' "$STAMP")"
if [ "$STAMPED_VER" != "$CSG_SYS_VER" ]; then
    err "manifold-deps guard: version drift — prebuilt is for manifold-csg-sys $STAMPED_VER,"
    err "                     but Cargo.lock pins $CSG_SYS_VER. The prebuilt is stale."
    hint
    exit 1
fi

# ---------- TBB pin dir preflight (task #5192, mechanism A'') ----------
#
# Each workspace binary needs a direct NEEDED libtbb.so.12 resolved via this
# tbb-ONLY pin dir (prepended first in the binary's own RUNPATH) so it loads
# before the transitive libTKernel->libtbb edge — DT_RUNPATH is non-transitive
# and cannot redirect that edge otherwise. See CLAUDE.md "Native deps" and
# crates/reify-build-utils/src/lib.rs's emit_tbb_pin_for_bins/_for_tests.
TBB_PIN_DIR="/opt/reify-deps/tbb-pin"
TBB_PIN_LIB="$TBB_PIN_DIR/libtbb.so.12"
DEPS_LIBTBB="/opt/reify-deps/lib/libtbb.so.12"

if [ ! -L "$TBB_PIN_LIB" ]; then
    # Self-heal: a host whose /opt/reify-deps/lib was already populated
    # before task #5192 added the pin dir is missing only the symlink, not
    # the lib itself — recreate it here (mirrors build-manifold-deps.sh's
    # idempotent ensure_tbb_pin()) so the FIRST verify after this change
    # lands self-heals instead of hard-failing until someone re-runs the
    # build script on the shared deps host.
    if [ -e "$DEPS_LIBTBB" ]; then
        mkdir -p "$TBB_PIN_DIR"
        # Multiple worktree verify pipelines can hit this self-heal
        # concurrently on this shared host. `mkdir -p` and a direct
        # `ln -sfn` are each individually idempotent (so the race is
        # benign), but publishing via a unique temp name + `mv -T` makes
        # the final rename a single atomic syscall, so a concurrent
        # reader's `readlink -f` below can never observe a symlink
        # mid-recreation (which would otherwise risk a transient spurious
        # hard-fail under heavy parallel verify load).
        tmp_link="$TBB_PIN_DIR/.libtbb.so.12.tmp.$$"
        ln -sfn "$DEPS_LIBTBB" "$tmp_link"
        mv -T "$tmp_link" "$TBB_PIN_LIB"
        warn "manifold-deps guard: tbb-pin was missing — self-healed $TBB_PIN_LIB -> $DEPS_LIBTBB."
        warn "                     Run ./scripts/build-manifold-deps.sh to make this permanent."
    else
        err "manifold-deps guard: tbb-pin missing — no symlink at $TBB_PIN_LIB,"
        err "                     and $DEPS_LIBTBB does not exist to self-heal from."
        hint
        exit 1
    fi
fi

TBB_PIN_TARGET="$(readlink -f "$TBB_PIN_LIB" 2>/dev/null || true)"
case "$TBB_PIN_TARGET" in
    */libtbb.so.12.*)
        ;;
    *)
        err "manifold-deps guard: tbb-pin at $TBB_PIN_LIB resolves to"
        err "                     '${TBB_PIN_TARGET:-<broken symlink>}', expected a libtbb.so.12.<N> file."
        hint
        exit 1
        ;;
esac

# ---------- native-dep presence arms (tasks #6343, #6493, #8634) ----------
#
# See arm 3 in the file header for why absence must be fatal HERE: nothing
# downstream can observe it. Note these gate the VERIFY PIPELINE, not
# `cargo build` — dep-free stub builds stay sanctioned, and each kernel carries
# real `cfg(not(has_<dep>))` stub modules for them (reify-kernel-occt's
# src/stubs.rs has a `#[cfg(all(test, not(has_occt)))] mod tests` contract
# suite).
#
# Every helper takes a table ROW key (occt, gmsh, openvdb) and reads that
# row's declarations through indirect expansion of its ${ROW^^}_<COLUMN>
# names, so a row adds data and no resolution logic.

# Accepted OCCT versions, compared at MAJOR.MINOR. The value under test is the
# suffix of the FIRST-level `libTKernel.so` symlink target — exactly what
# reify_build_utils::read_soname_version() extracts and what
# crates/reify-kernel-occt/build.rs splices into its
# `dylib:+verbatim=libTK*.so.<ver>` link directives.
#
# WHY MAJOR.MINOR RATHER THAN THE VERBATIM SEGMENT: the segment's SHAPE is a
# packaging detail, not a compatibility one. Debian points the dev symlink at
# `libTKernel.so.7.8` (two hops), conda-forge points it straight at
# `libTKernel.so.7.9.3` (one hop), and a repackaging that moves Debian's link
# one hop further (`-> libTKernel.so.7.8.1`) changes the segment without
# changing the OCCT. Exact-matching the segment would turn that non-event into
# a hard stop on EVERY RUN_RUST=1 verify — instructing the operator to
# re-validate the STEP/geometry pins — while build.rs would have spliced
# `libTKernel.so.7.8.1`, a file that exists and links. So both sides are
# projected through occt_majmin() before comparison. The VERBATIM segment
# still appears in the [ok] line and in every error, because that is the exact
# string build.rs links against. The projection also frees this array to hold
# either shape: widening it with a conda-shaped `7.9.3` yields `7.9`, which is
# what a resolved `7.9.3` projects to as well.
#
# WHY THIS IS A HARD PIN, not a warning: reify pins OCCT nowhere else in-tree
# (there is no version constraint in any Cargo.toml, and nothing else in the
# verify plan looks at OCCT at all), so a plain distro `apt upgrade` silently
# relinks the kernel against a new OCCT. That is the exact event build.rs's
# read_soname_version fallback anticipates — and if the move instead lands
# reify in stub mode, the has_occt suite that would have caught the resulting
# geometry regressions is deleted by the same cfg (the silent-vacuity rule in
# this file's header), so it cannot be inferred from test results either.
#
# ON A LEGITIMATE BUMP: widening this array is the sanctioned response — but
# only AFTER re-validating the OCCT-sensitive pins against the new version
# (task 6184's STEP plane-angle unit pin, and the reify-kernel-occt geometry
# suites). An array rather than a scalar so an operator can accept two
# versions during a migration; either way, widening it is a one-line diff a
# reviewer sees. scripts/setup-dev.sh's OCCT block provisions from the same
# expectation (at major.minor — its dpkg parse is `grep -oP '\d+\.\d+'`), and
# tests/infra/test_occt_deps_preflight.sh asserts that value still projects
# into this set — bump the two together.
OCCT_ACCEPTED_SONAMES=(7.8)

# occt_majmin <version> — the MAJOR.MINOR projection acceptance compares on.
# BOTH the resolved SONAME and every OCCT_ACCEPTED_SONAMES entry go through it,
# so the two may be written in different shapes and still compare correctly. A
# version with fewer than two dot-separated segments projects to itself.
occt_majmin() {
    printf '%s' "$1" | cut -d. -f1,2
}

occt_install_hint() {
    err "Install the OCCT ${OCCT_ACCEPTED_SONAMES[0]} dev packages — scripts/setup-dev.sh's OCCT block does exactly this:"
    err "    sudo add-apt-repository -y ppa:freecad-maintainers/occt-releases"
    err "    sudo apt-get install -y libocct-foundation-dev libocct-modeling-algorithms-dev \\"
    err "                            libocct-modeling-data-dev libocct-data-exchange-dev"
    err "Or point $OCCT_INCLUDE_ENV / $OCCT_LIB_ENV at an existing install."
}

occt_hint() {
    occt_install_hint
    err "WHY THIS IS FATAL rather than a warning: without OCCT, reify-kernel-occt's"
    err " build.rs never emits $OCCT_CFG, so its #[cfg(all(test, $OCCT_CFG))] module and"
    err " its ~25 #![cfg($OCCT_CFG)] integration binaries are not compiled AT ALL — the"
    err " suite reports zero tests REPORTED, not zero tests FAILED, and the gate goes"
    err " green over a kernel nothing exercised."
}

# dep_require_row <row> — every name the helpers below read for <row> is
# declared, and both sentinels are non-empty. Exits 1 naming the row
# otherwise.
#
# The reader declares every column of every row, so a miss here means the
# reader and this script disagree on the <ROW>_<COLUMN> naming contract.
# Under `set -u` that would otherwise abort the WHOLE preflight with a bare
# "unbound variable", precisely the cryptic-failure mode this file exists to
# convert into an actionable message. `declare -p` rather than a `${!ref:-}`
# read, because indirect expansion of an UNDECLARED `FOO[@]` under `:-`
# yields one EMPTY element rather than none. An empty sentinel would match any
# existing dir.
dep_require_row() {
    local row="$1" up="${1^^}" col name bad=""
    for col in INCLUDE_ENV LIB_ENV CFG INCLUDE_SENTINEL LIB_SENTINEL \
        INCLUDE_CANDIDATES LIB_CANDIDATES XDE_TOOLKITS \
        SNAP_ROOT SNAP_INCLUDE_SUBDIR SNAP_LIB_SUBDIR; do
        name="${up}_${col}"
        declare -p "$name" >/dev/null 2>&1 || bad="$bad $name"
    done
    for col in INCLUDE_SENTINEL LIB_SENTINEL; do
        name="${up}_${col}"
        [ -n "${!name:-}" ] || bad="$bad $name(empty)"
    done
    [ -z "$bad" ] && return 0
    err "manifold-deps guard: internal error — the '$row' arm cannot run; the native-deps"
    err "                     reader left these undeclared or empty:$bad"
    err "                     scripts/native_deps_table.py and this script disagree on the"
    err "                     row's <ROW>_<COLUMN> names. This is a bug in those two files,"
    err "                     NOT a missing install — do not try to fix it by installing"
    err "                     anything or setting an override."
    exit 1
}

# dep_override <row> <INCLUDE|LIB> — the value of that half's override env
# var (the row's include_env / lib_env), EMPTY when unset or exported empty.
dep_override() {
    local env_ref="${1^^}_${2}_ENV"
    local env_name="${!env_ref}"
    printf '%s' "${!env_name:-}"
}

# dep_find_dir <row> <INCLUDE|LIB> <override>
#
# SHARED by every native-dep arm, so the resolution rule exists ONCE.
#
# The build's resolution core in reify_build_utils applies the same rule to
# the same row, with ONE deliberate, one-directional divergence: an override
# must still CONTAIN the sentinel here, whereas the build takes an override
# on trust. A guard's job is to reject every configuration that yields a stub
# or an unverified link, so being a strict superset of the build's
# silent-failure modes is correct — and it is what makes these arms
# hermetically testable through the same env vars the build honours. The
# strictness never rejects a real install: anything shipping the headers also
# ships the unversioned dev symlink.
#
# EMPTY-OVERRIDE RULE (shared, NOT a divergence): an exported-but-EMPTY
# override counts as UNSET on both sides and falls through to the candidate
# list; `[ -n "$override" ]` below is this side of it. Before the build
# applied it too, it resolved such a var to the EMPTY PATH, still set
# has_occt, and linked against nothing — while this guard, reading the same
# environment, went green describing a resolution the build would not
# perform.
#
# After the candidates, a row WITH a snap table scans <root>/*/<subdir> (the
# numbered /snap/freecad/<rev>/ installs); a row without one never scans.
#
# Prints the resolved dir on stdout and returns 0; returns 1 with no output
# when nothing resolves.
dep_find_dir() {
    local up="${1^^}" half="$2" override="$3"
    local sentinel_ref="${up}_${half}_SENTINEL" candidates_ref="${up}_${half}_CANDIDATES[@]"
    local snap_root_ref="${up}_SNAP_ROOT" snap_subdir_ref="${up}_SNAP_${half}_SUBDIR"
    local sentinel="${!sentinel_ref}" snap_root="${!snap_root_ref}" snap_subdir="${!snap_subdir_ref}"
    if [ -n "$override" ]; then
        [ -e "$override/$sentinel" ] || return 1
        printf '%s' "$override"
        return 0
    fi
    local cand
    for cand in "${!candidates_ref}"; do
        if [ -e "$cand/$sentinel" ]; then
            printf '%s' "$cand"
            return 0
        fi
    done
    if [ -n "$snap_root" ]; then
        local rev
        for rev in "$snap_root"/*/; do
            [ -d "$rev" ] || continue
            if [ -e "$rev$snap_subdir/$sentinel" ]; then
                printf '%s' "$rev$snap_subdir"
                return 0
            fi
        done
    fi
    return 1
}

# dep_soname_ver <lib_dir> <sentinel>
#
# The version segment of the dev symlink's FIRST-LEVEL target — the same rule
# reify_build_utils::read_soname_version() applies, shared by every arm so it
# exists ONCE here:
#   - `readlink`, NEVER `readlink -f`. Multi-hop resolution gives the wrong
#     answer on both live shapes: OCCT's Debian chain would yield 7.8.1 where
#     the build sees 7.8, and openvdb's would yield 13.0.0 where it sees 13.0.
#   - everything after the `<sentinel>.` prefix taken VERBATIM, so conda's
#     one-hop `libgmsh.so -> libgmsh.so.4.15.2` yields `4.15.2` and OCCT's
#     `:libTKernel.so.7.9.3` link directive names a file that exists.
#
# Prints the version on stdout, or NOTHING when it is undeterminable (the
# sentinel is not a symlink, or its target does not carry the prefix). Always
# exits 0 — whether an empty result is fatal is the CALLER's decision, and the
# arms differ: OCCT hard-fails (build.rs splices the version into link
# directives behind a hard-coded fallback, so an unread SONAME links something
# nobody verified), while Gmsh and OpenVDB record `unknown` and continue
# (they link the unversioned `dylib=gmsh` / `dylib=openvdb` symlink and splice
# no version anywhere, so there is no unverified-link hazard to gate on).
dep_soname_ver() {
    local lib_dir="$1" sentinel="$2"
    # `|| true`: a non-symlink makes readlink exit non-zero, and under `set -e`
    # that would abort the caller with no message at all — reporting the
    # undeterminable case is the whole point.
    local target base ver
    target="$(readlink "$lib_dir/$sentinel" 2>/dev/null || true)"
    base="${target##*/}"
    [ -n "$base" ] || return 0
    ver="${base#"$sentinel."}"
    # Unchanged => the prefix was absent, so there is no version to read. An
    # empty remainder (a bare `<sentinel>.` target) is equally unusable.
    [ "$ver" = "$base" ] && return 0
    printf '%s' "$ver"
}

# dep_searched_desc <row> <INCLUDE|LIB> <override>
# Human-readable rendering of WHERE the guard actually looked, so a red gate
# names the searched paths rather than leaving the reader to infer them.
dep_searched_desc() {
    local up="${1^^}" half="$2" override="$3"
    local env_ref="${up}_${half}_ENV" candidates_ref="${up}_${half}_CANDIDATES[*]"
    if [ -n "$override" ]; then
        printf '%s (from %s)' "$override" "${!env_ref}"
    else
        printf '%s' "${!candidates_ref}"
    fi
}

# dep_presence_arm <row> <Label> <hint_fn>
#
# The ENTIRE body of a presence-only arm — override read, both resolutions,
# report-both-halves, hint+exit, SONAME recording — so that logic exists once
# rather than once per dep. Every per-dep value comes from <row>.
#
# OCCT IS DELIBERATELY EXCLUDED, and occt_arm stays written out on its own:
# OCCT's SONAME pin is FATAL and sits BETWEEN resolution and the `ok` line
# (accepted-set comparison, two distinct multi-line diagnostics, a different
# install hint per failure mode), so folding it in would need a parameter per
# difference.
#
# Exits 1 (terminating the whole script, which is the contract — arms run in
# order and the first failure exits) when either half is unresolved. Returns
# 0 having printed the arm's `[ok]` recording line otherwise.
dep_presence_arm() {
    local row="$1" label="$2" hint_fn="$3" up="${1^^}"
    dep_require_row "$row"

    local inc_sent_ref="${up}_INCLUDE_SENTINEL" lib_sent_ref="${up}_LIB_SENTINEL"
    local inc_sent="${!inc_sent_ref}" lib_sent="${!lib_sent_ref}"
    local inc_ov lib_ov
    inc_ov="$(dep_override "$row" INCLUDE)"
    lib_ov="$(dep_override "$row" LIB)"

    # `|| true` inside the substitution: a non-resolving arm must reach the
    # named error below, not abort under `set -e` with no message at all.
    local inc_resolved lib_resolved
    inc_resolved="$(dep_find_dir "$row" INCLUDE "$inc_ov" || true)"
    lib_resolved="$(dep_find_dir "$row" LIB "$lib_ov" || true)"

    # Report BOTH halves before exiting, same rule as occt_arm: find() is None
    # when EITHER is unresolved, so a reader whose host is missing both should
    # not have to fix one, re-run, and discover the other.
    local failed=0

    if [ -z "$inc_resolved" ]; then
        err "manifold-deps guard: $row headers not found — no $inc_sent in:"
        err "                     $(dep_searched_desc "$row" INCLUDE "$inc_ov")"
        failed=1
    fi

    if [ -z "$lib_resolved" ]; then
        err "manifold-deps guard: $row libraries not found — no $lib_sent in:"
        err "                     $(dep_searched_desc "$row" LIB "$lib_ov")"
        failed=1
    fi

    if [ "$failed" -ne 0 ]; then
        "$hint_fn"
        exit 1
    fi

    # RECORDING half of the arm — stdout, so a reviewer reading a green
    # reify-kernel-<dep> result in the verify log can see WHICH install produced
    # it. Read through the shared dep_soname_ver(), so the first-level-only rule
    # (`readlink`, never `readlink -f`) is stated once for every arm.
    #
    # `unknown` is NOT fatal here, unlike the OCCT arm: these crates' build.rs
    # files link the unversioned `dylib=<dep>` dev symlink and splice no version
    # into any link directive, so an unreadable SONAME cannot make the build link
    # something nobody verified — it only costs this log line its specificity.
    # Hard-failing on it would red every RUN_RUST=1 verify over a packaging
    # detail with no correctness consequence.
    local ver
    ver="$(dep_soname_ver "$lib_resolved" "$lib_sent")"
    ok "$label ${ver:-unknown} at $lib_resolved (headers: $inc_resolved)"
}

# dep_hint <row> <conda-ver> <apt-ver> <subject> <surfaces-line>... — the
# complete diagnostic block a presence-only arm prints just before it exits:
# how to install the dep, then the silent-vacuity rule from this file's header,
# rendered for the operator staring at the red gate. The `<dep>_hint` wrappers
# below carry DATA ONLY. The dep and crate names are the row key, and the env
# and cfg names come from the row.
#
# NOT SHARED WITH OCCT, deliberately: occt_hint/occt_install_hint stay on
# their own. OCCT is reached from three failure paths (both halves
# unresolved, undeterminable SONAME, SONAME drift) with genuinely different
# prose per path, and it is apt-provisioned rather than conda-forge — it
# shares no sentence with these two.
#
# The surfaces clause is taken as TRAILING VARARGS, one per output line, so each
# dep keeps its own hand-wrapping instead of rendering as a single over-long
# line that a terminal re-wraps arbitrarily.
dep_hint() {
    local row="$1" conda_ver="$2" apt_ver="$3" subject="$4" up="${1^^}"
    shift 4
    local inc_env_ref="${up}_INCLUDE_ENV" lib_env_ref="${up}_LIB_ENV" cfg_ref="${up}_CFG"
    local cfg="${!cfg_ref}" line

    err "Provision the conda-forge reify-deps env — scripts/setup-dev.sh's"
    err "'conda-forge env: gmsh + openvdb' block does exactly this, installing"
    err "$row $conda_ver into /opt/reify-deps (apt's $row is stale at $apt_ver):"
    err "    ./scripts/setup-dev.sh"
    err "Or point ${!inc_env_ref} / ${!lib_env_ref} at an existing install."
    err "WHY THIS IS FATAL rather than a warning: without $row,"
    err " reify-kernel-$row's build.rs never emits $cfg, so every"
    err " #[cfg($cfg)]-gated item in the workspace is not compiled AT ALL —"
    for line in "$@"; do
        err " $line"
    done
    err " The suite then reports zero tests REPORTED, not zero tests FAILED, and the"
    err " gate goes green over $subject nothing exercised."
}

# occt_arm — presence, then the SONAME pin, then the XDE toolkits.
occt_arm() {
    dep_require_row occt

    local inc_ov lib_ov
    inc_ov="$(dep_override occt INCLUDE)"
    lib_ov="$(dep_override occt LIB)"

    # `|| true` inside the substitution: a non-resolving arm must reach the
    # named error below, not abort under `set -e` with no message at all.
    local inc_resolved lib_resolved
    inc_resolved="$(dep_find_dir occt INCLUDE "$inc_ov" || true)"
    lib_resolved="$(dep_find_dir occt LIB "$lib_ov" || true)"

    # Report BOTH halves before exiting. find() is None when EITHER is
    # unresolved, so a reader whose host is missing both should not have to
    # fix one, re-run, and discover the other.
    local failed=0

    if [ -z "$inc_resolved" ]; then
        err "manifold-deps guard: OCCT headers not found — no $OCCT_INCLUDE_SENTINEL in:"
        err "                     $(dep_searched_desc occt INCLUDE "$inc_ov")"
        failed=1
    fi

    if [ -z "$lib_resolved" ]; then
        err "manifold-deps guard: OCCT libraries not found — no $OCCT_LIB_SENTINEL in:"
        err "                     $(dep_searched_desc occt LIB "$lib_ov")"
        failed=1
    fi

    if [ "$failed" -ne 0 ]; then
        occt_hint
        exit 1
    fi

    # Both halves resolved, so has_occt WILL be set. Now pin which OCCT it is.
    # The first-level read itself lives in dep_soname_ver() above, shared with
    # the other arms; soname_target is kept only for the error text, which
    # names the raw link target the operator will see on disk.
    local soname_path="$lib_resolved/$OCCT_LIB_SENTINEL"
    local soname_prefix="$OCCT_LIB_SENTINEL."
    local soname_target soname_ver
    soname_target="$(readlink "$soname_path" 2>/dev/null || true)"
    soname_ver="$(dep_soname_ver "$lib_resolved" "$OCCT_LIB_SENTINEL")"

    if [ -z "$soname_ver" ]; then
        err "manifold-deps guard: could not determine the OCCT SONAME from $soname_path"
        err "                     (first-level symlink target: '${soname_target:-<not a symlink>}';"
        err "                      expected $soname_prefix<version>)."
        err "This is NOT a cosmetic gap. find() reports the dir as resolved — it only tests"
        err " existence — so has_occt IS set, and crates/reify-kernel-occt/build.rs falls"
        err " back to a HARD-CODED version for its dylib:+verbatim=libTK*.so.<ver> link"
        err " directives behind nothing but a cargo:warning. The build would link a version"
        err " nobody verified."
        occt_install_hint
        exit 1
    fi

    local soname_majmin v soname_accepted=0
    soname_majmin="$(occt_majmin "$soname_ver")"
    for v in "${OCCT_ACCEPTED_SONAMES[@]}"; do
        if [ "$(occt_majmin "$v")" = "$soname_majmin" ]; then
            soname_accepted=1
            break
        fi
    done

    if [ "$soname_accepted" -ne 1 ]; then
        err "manifold-deps guard: OCCT SONAME drift — resolved $soname_ver (major.minor"
        err "                     $soname_majmin) at $lib_resolved, but the accepted"
        err "                     set is: ${OCCT_ACCEPTED_SONAMES[*]}"
        err "                     (read from $soname_path -> $soname_target)."
        err "If this move was NOT intended, install an accepted version:"
        occt_install_hint
        err "If it WAS intended: re-validate the OCCT-sensitive pins against $soname_ver"
        err " (the STEP plane-angle unit pin, the reify-kernel-occt geometry suites), then"
        err " widen OCCT_ACCEPTED_SONAMES in scripts/check-manifold-deps.sh and bump"
        err " scripts/setup-dev.sh's OCCT block to match — tests/infra/"
        err " test_occt_deps_preflight.sh asserts the two agree."
        exit 1
    fi

    # The row's xde_toolkits — what crates/reify-kernel-occt/build.rs links for
    # the STEP assembly reader beyond the libTKernel.so sentinel — each by the
    # exact filename its `dylib:+verbatim=lib<TK>.so.<ver>` directive links;
    # `-e` follows the symlink.
    local -a xde_resolved=() xde_missing=()
    local tk tk_file
    for tk in "${OCCT_XDE_TOOLKITS[@]}"; do
        tk_file="lib$tk.so.$soname_ver"
        if [ -e "$lib_resolved/$tk_file" ]; then
            xde_resolved+=("$tk_file")
        else
            xde_missing+=("$tk_file")
        fi
    done

    if [ "${#xde_missing[@]}" -ne 0 ]; then
        for tk_file in "${xde_missing[@]}"; do
            err "manifold-deps guard: OCCT XDE toolkit missing — no $tk_file in $lib_resolved."
        done
        err "WHY THIS IS FATAL: crates/reify-kernel-occt/build.rs links it verbatim, so its absence is a"
        err " cryptic link failure of every OCCT-touching crate deep in the build; it carries the XDE"
        err " document framework the STEP assembly reader uses."
        err "TKXCAF ships in libocct-data-exchange-<ver>, TKLCAF in libocct-ocaf-<ver>."
        occt_install_hint
        exit 1
    fi

    ok "OCCT $soname_ver at $lib_resolved (headers: $inc_resolved; XDE toolkits: ${xde_resolved[*]})"
}

# Data only — the body is dep_hint() above, shared with openvdb_hint below.
gmsh_hint() {
    dep_hint gmsh 4.15.2 4.12.1 "a mesher" \
        "reify-kernel-gmsh's whole test surface, the occt_gmsh conformance suites," \
        "and the reify-eval FEA/mesh e2e binaries."
}

# The whole arm body lives in dep_presence_arm() above, driven by the gmsh row.
gmsh_arm() {
    dep_presence_arm gmsh Gmsh gmsh_hint
}

# Data only — same shared dep_hint() body as gmsh_hint above, so a change to the
# install instructions or the fatality rationale lands in both by construction.
openvdb_hint() {
    dep_hint openvdb 13.0.0 10.0.1 "a voxel kernel" \
        "the crate's whole sparse-SDF/voxel-grid test surface included."
}

# Same one-call arm as gmsh's, one dep over — the body is shared, so a future
# change to the error format or an added diagnostic lands in both.
openvdb_arm() {
    dep_presence_arm openvdb OpenVDB openvdb_hint
}

# ---------- dispatch: one arm per table row ----------
#
# Table order fixes the order of the [ok] lines and nothing else: coverage of
# NATIVE_DEP_ARMS was already checked in both directions, before arm 1.
for row in "${NATIVE_DEP_ROWS[@]}"; do
    "${NATIVE_DEP_ARMS[$row]}"
done

exit 0
