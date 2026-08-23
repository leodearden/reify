#!/usr/bin/env bash
# Preflight guard for the workspace's native deps: THREE arms, all run BEFORE
# any expensive compile, each converting a silent or cryptic downstream
# failure into a fast, actionable message.
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
#   3. OCCT presence (task #6343). A missing OCCT is not merely cryptic, it is
#      SILENT: `reify_build_utils::find(NativeDep::Occt)` returns None,
#      `crates/reify-kernel-occt/build.rs` emits a `cargo:warning` and returns
#      without setting `has_occt`, and the crate degrades to stub types — which
#      also deletes its `#[cfg(all(test, has_occt))]` module and its ~25
#      `#![cfg(has_occt)]` integration binaries. The suite then reports ZERO
#      tests rather than zero failures, so the gate stays green over a kernel
#      nothing exercised. This arm makes that state red here.
#
# verify.sh runs this as the first plan entry when Rust work is in scope.
#
# Exit 0 when every arm passes; non-zero with a clear message otherwise. Fast:
# reads only Cargo.lock, the VERSION stamp, and a handful of stat()s (no
# registry / no cargo / no compile).
set -euo pipefail

err() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

# ---------- OCCT presence preflight (task #6343) ----------
#
# See arm 3 in the file header for why absence must be fatal HERE: nothing
# downstream can observe it. Note this gates the VERIFY PIPELINE, not
# `cargo build` — OCCT-free stub builds stay sanctioned, and
# crates/reify-kernel-occt/src/stubs.rs carries a real
# `#[cfg(all(test, not(has_occt)))] mod tests` contract suite for them.

# BEGIN occt-candidates — EXACT MIRROR of reify_build_utils::NativeDep::Occt
# (crates/reify-build-utils/src/lib.rs). Order is load-bearing: system paths
# come first, and /opt/reify-deps/lib appears in NEITHER list, because the
# conda env ships OCCT 7.9 as a transitive of gmsh=4.15.2 while reify links
# system OCCT 7.8. Rust stays the source of truth; this block is a declared
# mirror, pinned equal INCLUDING ORDER by
# tests/infra/test_occt_deps_preflight.sh.
OCCT_LIB_CANDIDATES=(
    /usr/lib/x86_64-linux-gnu
    /usr/lib
    /usr/local/lib
    /snap/freecad/current/usr/lib
)
OCCT_INCLUDE_CANDIDATES=(
    /usr/include/opencascade
    /usr/local/include/opencascade
    /snap/freecad/current/usr/include/opencascade
)
OCCT_LIB_SENTINEL=libTKernel.so
OCCT_INCLUDE_SENTINEL=Standard_Failure.hxx
# END occt-candidates

occt_hint() {
    err "Install the OCCT 7.8 dev packages — scripts/setup-dev.sh's OCCT block does exactly this:"
    err "    sudo add-apt-repository -y ppa:freecad-maintainers/occt-releases"
    err "    sudo apt-get install -y libocct-foundation-dev libocct-modeling-algorithms-dev \\"
    err "                            libocct-modeling-data-dev libocct-data-exchange-dev"
    err "Or point OCCT_INCLUDE_DIR / OCCT_LIB_DIR at an existing install."
    err "WHY THIS IS FATAL rather than a warning: without OCCT, reify-kernel-occt's"
    err " build.rs never emits has_occt, so its #[cfg(all(test, has_occt))] module and"
    err " its ~25 #![cfg(has_occt)] integration binaries are not compiled AT ALL — the"
    err " suite reports zero tests REPORTED, not zero tests FAILED, and the gate goes"
    err " green over a kernel nothing exercised."
}

# occt_find_dir <override> <sentinel> <candidate>...
#
# Mirrors reify_build_utils::find_dir_with_override
# (crates/reify-build-utils/src/lib.rs) rule for rule, with ONE deliberate,
# one-directional divergence: an override must still CONTAIN the sentinel
# here, whereas the build takes an override on trust. A guard's job is to
# reject every configuration that yields a stub or an unverified link, so
# being a strict superset of the build's silent-failure modes is correct — and
# it is what makes this arm hermetically testable through the same two env
# vars the build honours, with no test-only seam added to production code. The
# strictness never rejects a real install: anything shipping OCCT headers also
# ships the unversioned dev symlink.
#
# Prints the resolved dir on stdout and returns 0; returns 1 with no output
# when nothing resolves.
occt_find_dir() {
    local override="$1" sentinel="$2"
    shift 2
    if [ -n "$override" ]; then
        [ -e "$override/$sentinel" ] || return 1
        printf '%s' "$override"
        return 0
    fi
    local cand
    for cand in "$@"; do
        if [ -e "$cand/$sentinel" ]; then
            printf '%s' "$cand"
            return 0
        fi
    done
    # OCCT's numbered-snap fallback, mirroring find_dir_with_override's
    # snap_subdir match. Absolute-path-rooted, so this branch is not
    # hermetically testable and exists purely for parity with the build.
    local snap_subdir=""
    case "$sentinel" in
        Standard_Failure.hxx) snap_subdir="usr/include/opencascade" ;;
        libTKernel.so) snap_subdir="usr/lib" ;;
    esac
    if [ -n "$snap_subdir" ]; then
        local rev
        for rev in /snap/freecad/*/; do
            [ -d "$rev" ] || continue
            if [ -e "$rev$snap_subdir/$sentinel" ]; then
                printf '%s' "$rev$snap_subdir"
                return 0
            fi
        done
    fi
    return 1
}

# occt_searched_desc <override> <env-var-name> <candidate>...
# Human-readable rendering of WHERE the guard actually looked, so a red gate
# names the searched paths rather than leaving the reader to infer them.
occt_searched_desc() {
    local override="$1" envvar="$2"
    shift 2
    if [ -n "$override" ]; then
        printf '%s (from %s)' "$override" "$envvar"
    else
        printf '%s' "$*"
    fi
}

OCCT_INCLUDE_OVERRIDE="${OCCT_INCLUDE_DIR:-}"
OCCT_LIB_OVERRIDE="${OCCT_LIB_DIR:-}"

# `|| true` inside the substitution: a non-resolving arm must reach the named
# error below, not abort under `set -e` with no message at all.
OCCT_INCLUDE_RESOLVED="$(occt_find_dir "$OCCT_INCLUDE_OVERRIDE" "$OCCT_INCLUDE_SENTINEL" "${OCCT_INCLUDE_CANDIDATES[@]}" || true)"
OCCT_LIB_RESOLVED="$(occt_find_dir "$OCCT_LIB_OVERRIDE" "$OCCT_LIB_SENTINEL" "${OCCT_LIB_CANDIDATES[@]}" || true)"

# Report BOTH halves before exiting. find() is None when EITHER is unresolved,
# so a reader whose host is missing both should not have to fix one, re-run,
# and discover the other.
occt_failed=0

if [ -z "$OCCT_INCLUDE_RESOLVED" ]; then
    err "manifold-deps guard: OCCT headers not found — no $OCCT_INCLUDE_SENTINEL in:"
    err "                     $(occt_searched_desc "$OCCT_INCLUDE_OVERRIDE" OCCT_INCLUDE_DIR "${OCCT_INCLUDE_CANDIDATES[@]}")"
    occt_failed=1
fi

if [ -z "$OCCT_LIB_RESOLVED" ]; then
    err "manifold-deps guard: OCCT libraries not found — no $OCCT_LIB_SENTINEL in:"
    err "                     $(occt_searched_desc "$OCCT_LIB_OVERRIDE" OCCT_LIB_DIR "${OCCT_LIB_CANDIDATES[@]}")"
    occt_failed=1
fi

if [ "$occt_failed" -ne 0 ]; then
    occt_hint
    exit 1
fi

exit 0
