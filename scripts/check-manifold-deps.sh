#!/usr/bin/env bash
# Preflight guard: verify the prebuilt manifold C++ libs are present and match
# the pinned `manifold-csg-sys` version BEFORE any expensive compile.
#
# The `[target.x86_64-unknown-linux-gnu.manifold]` override in
# .cargo/config.toml makes Cargo link prebuilt static libs from
# /opt/reify-deps/manifold/lib instead of building manifold from source. If
# those libs are missing or stale (a `manifold-csg-sys` pin bump rebuilt the
# crate but no one re-ran the deps script), the failure is otherwise a cryptic
# linker error deep in a multi-minute build. This guard converts that into a
# fast, actionable message naming `scripts/build-manifold-deps.sh`.
#
# verify.sh runs this as the first plan entry when Rust work is in scope.
#
# Exit 0 on match; non-zero with a clear message otherwise. Fast: reads only
# Cargo.lock + the VERSION stamp (no registry / no cargo).
set -euo pipefail

err() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

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

exit 0
