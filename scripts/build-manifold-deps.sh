#!/usr/bin/env bash
# Build the manifold C++ static libs once into /opt/reify-deps/manifold/lib.
#
# Reify links four native geometry kernels. OCCT, OpenVDB, and gmsh all link
# *prebuilt* static/shared libs from /opt/reify-deps. manifold was the lone
# exception: `manifold-csg-sys`'s build.rs clones elalish/manifold and
# cmake-builds the whole C++ tree (manifold + builtin TBB + builtin Clipper2 +
# manifoldc) from source on every cold worktree — ~4× per worktree, each doing
# its own 56 MB git clone + cmake configure + ~227 MB OUT_DIR. A fleet-wide
# sccache invalidation (e.g. a pin bump) then turned every fresh merge verify
# into a 60–75 min cold build, overrunning verify.sh's inner timeouts → exit 124.
#
# This script brings manifold in line with the other kernels: build its C++
# libs ONCE per host here, and let a `[target.<triple>.manifold]` build-script
# override in .cargo/config.toml link the prebuilt libs (skipping the sys
# crate's build.rs entirely). See `scripts/check-manifold-deps.sh` (the verify
# preflight guard) and the override in `.cargo/config.toml`.
#
# Idempotent — safe to re-run. A `manifold-csg-sys` pin bump (changing either
# the crate version in Cargo.lock or the MANIFOLD_VERSION tag it pins) requires
# re-running this script; the guard fails verify on version drift.
#
# This script is deliberately INDEPENDENT of `cargo build` (a cargo build would
# hit the override and skip the very build we want here). It mirrors the exact
# cmake args and link set from manifold-csg-sys's build/{build.rs,fetch.rs}.
#
# Usage: ./scripts/build-manifold-deps.sh [--force]
set -euo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*"; }

FORCE=0
for arg in "$@"; do
    case "$arg" in
        -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
        --force)   FORCE=1 ;;
        *) err "Unknown flag: $arg"; exit 2 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCKFILE="$REPO_ROOT/Cargo.lock"
PREFIX="/opt/reify-deps/manifold"
LIBDIR="$PREFIX/lib"
STAMP="$PREFIX/VERSION"
# The four static libs the override links, by their installed names.
REQUIRED_LIBS=(libmanifoldc.a libmanifold.a libClipper2.a libtbb.a)

# ---------- Resolve the pin from Cargo.lock + the sys crate's MANIFOLD_VERSION ----------

if [ ! -f "$LOCKFILE" ]; then
    err "Cargo.lock not found at $LOCKFILE"
    exit 1
fi

# `manifold-csg-sys` version (e.g. 3.5.101) — the [[package]] block whose
# `name = "manifold-csg-sys"` is immediately followed by its `version`.
CSG_SYS_VER="$(awk '
    /^name = "manifold-csg-sys"$/ { f=1; next }
    f && /^version = / { gsub(/[",]/,""); print $3; exit }
' "$LOCKFILE")"
if [ -z "${CSG_SYS_VER:-}" ]; then
    err "Could not find manifold-csg-sys version in $LOCKFILE"
    exit 1
fi
info "manifold-csg-sys pinned at $CSG_SYS_VER (per Cargo.lock)"

# Locate the crate's registry source so we can read MANIFOLD_VERSION (the
# upstream git tag it pins). Populate the registry with `cargo fetch` if a
# fresh checkout hasn't downloaded it yet.
find_reg_src() {
    ls -d "$HOME"/.cargo/registry/src/*/manifold-csg-sys-"${CSG_SYS_VER}" 2>/dev/null | head -1
}
REG_SRC="$(find_reg_src || true)"
if [ -z "${REG_SRC:-}" ]; then
    info "manifold-csg-sys-${CSG_SYS_VER} not in the cargo registry cache — running 'cargo fetch'..."
    (cd "$REPO_ROOT" && cargo fetch --locked) || (cd "$REPO_ROOT" && cargo fetch)
    REG_SRC="$(find_reg_src || true)"
fi
if [ -z "${REG_SRC:-}" ]; then
    err "Could not locate manifold-csg-sys-${CSG_SYS_VER} registry source under ~/.cargo/registry/src/."
    err "Run 'cargo fetch' in $REPO_ROOT and retry."
    exit 1
fi

# MANIFOLD_VERSION const, e.g.  pub(crate) const MANIFOLD_VERSION: &str = "v3.5.0";
MANIFOLD_TAG="$(grep -oP 'MANIFOLD_VERSION[^"]*"\K[^"]+' "$REG_SRC/build.rs" | head -1)"
if [ -z "${MANIFOLD_TAG:-}" ]; then
    err "Could not read MANIFOLD_VERSION from $REG_SRC/build.rs"
    exit 1
fi
info "upstream manifold tag: $MANIFOLD_TAG (per manifold-csg-sys-${CSG_SYS_VER}/build.rs)"

WANT_STAMP="${CSG_SYS_VER} ${MANIFOLD_TAG}"

# ---------- Idempotency ----------

all_libs_present() {
    local l
    for l in "${REQUIRED_LIBS[@]}"; do
        [ -f "$LIBDIR/$l" ] || return 1
    done
    return 0
}

if [ "$FORCE" -eq 0 ] && [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$WANT_STAMP" ] && all_libs_present; then
    ok "manifold prebuilt up to date ($WANT_STAMP) at $LIBDIR — nothing to do."
    ok "Re-run with --force to rebuild."
    exit 0
fi

# ---------- Preflight ----------

if ! command -v cmake >/dev/null 2>&1; then
    err "cmake not found on PATH. Install it (apt install cmake) and retry."
    exit 1
fi
if ! command -v git >/dev/null 2>&1; then
    err "git not found on PATH."
    exit 1
fi
# /opt/reify-deps is host-global and (per setup-dev.sh) chowned to the dev user.
if ! mkdir -p "$LIBDIR" 2>/dev/null; then
    err "Cannot create $LIBDIR. Is /opt/reify-deps writable by $USER?"
    err "setup-dev.sh creates it via: sudo mkdir -p /opt/reify-deps && sudo chown -R \$USER /opt/reify-deps"
    exit 1
fi

# ---------- Build ----------

BUILD_ROOT="$(mktemp -d -t reify-manifold-build-XXXXXX)"
cleanup() { rm -rf "$BUILD_ROOT"; }
trap cleanup EXIT

SRC="$BUILD_ROOT/manifold"
BUILD="$BUILD_ROOT/build"

info "Cloning elalish/manifold into $SRC ..."
git -c core.autocrlf=false clone --quiet https://github.com/elalish/manifold.git "$SRC"
git -C "$SRC" -c advice.detachedHead=false checkout --quiet "$MANIFOLD_TAG"

# cmake args — mirror manifold-csg-sys-${CSG_SYS_VER}/build/build.rs (host,
# parallel path). MANIFOLD_PAR=ON pulls builtin TBB; BUILD_SHARED_LIBS=OFF +
# PIC give the static .a's that the override links.
CMAKE_ARGS=(
    -S "$SRC"
    -B "$BUILD"
    -DCMAKE_BUILD_TYPE=Release
    -DMANIFOLD_TEST=OFF
    -DMANIFOLD_PYBIND=OFF
    -DMANIFOLD_JSBIND=OFF
    -DMANIFOLD_CBIND=ON
    -DMANIFOLD_CROSS_SECTION=ON
    -DMANIFOLD_USE_BUILTIN_CLIPPER2=ON
    -DBUILD_SHARED_LIBS=OFF
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON
    -DMANIFOLD_PAR=ON
    -DMANIFOLD_USE_BUILTIN_TBB=ON
)

# Route C/C++ compiles through sccache when present (same opt-out var as the
# sys crate's build.rs) so rebuilds reuse object files.
if [ -z "${MANIFOLD_CSG_NO_SCCACHE:-}" ] && command -v sccache >/dev/null 2>&1; then
    info "sccache detected — routing C/C++ compiles through it."
    CMAKE_ARGS+=(-DCMAKE_C_COMPILER_LAUNCHER=sccache -DCMAKE_CXX_COMPILER_LAUNCHER=sccache)
fi

info "Configuring (cmake) ..."
cmake "${CMAKE_ARGS[@]}"

info "Building (cmake --build --parallel) — this is the slow step (~5-10 min cold) ..."
cmake --build "$BUILD" --config Release --parallel

# ---------- Install ----------
#
# Mirror find_lib_recursive: depth-first search for lib<name>.a. TBB's static
# lib may be named libtbb.a / libtbb12.a / libtbb12_static.a depending on the
# bundled oneTBB version — the override links `static=tbb`, so install whichever
# we find AS libtbb.a.

find_one() {
    # find_one <root> <basename> — first match, or empty.
    find "$1" -name "$2" -type f -print -quit 2>/dev/null
}

install_lib() {
    # install_lib <src-basename> <installed-basename>
    local src; src="$(find_one "$BUILD" "$1")"
    if [ -z "$src" ]; then
        return 1
    fi
    cp -f "$src" "$LIBDIR/$2"
    info "installed $2  (from ${src#"$BUILD"/})"
    return 0
}

install_lib libmanifoldc.a libmanifoldc.a || { err "libmanifoldc.a not found under $BUILD"; exit 1; }
install_lib libmanifold.a  libmanifold.a  || { err "libmanifold.a not found under $BUILD"; exit 1; }
install_lib libClipper2.a  libClipper2.a  || { err "libClipper2.a not found under $BUILD"; exit 1; }

# TBB: try the known static names in build.rs's order, install as libtbb.a.
if   install_lib libtbb.a           libtbb.a; then :
elif install_lib libtbb12.a         libtbb.a; then :
elif install_lib libtbb12_static.a  libtbb.a; then :
else err "builtin TBB static lib (libtbb.a / libtbb12.a / libtbb12_static.a) not found under $BUILD"; exit 1
fi

# tbbmalloc is harmless if present and not linked by the override; copy it so a
# future tbbmalloc link need only edit the override, not rebuild.
install_lib libtbbmalloc.a libtbbmalloc.a || true

# ---------- Stamp ----------

printf '%s\n' "$WANT_STAMP" > "$STAMP"
ok "manifold prebuilt installed to $LIBDIR"
ok "VERSION stamp: $WANT_STAMP"
echo
info "Installed libs:"
ls -la "$LIBDIR"
