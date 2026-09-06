#!/usr/bin/env bash
# Preflight guard for the workspace's native deps: FIVE arms, all run BEFORE
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
#   3. OCCT presence + SONAME (task #6343). A missing OCCT is not merely cryptic, it is
#      SILENT: `reify_build_utils::find(NativeDep::Occt)` returns None,
#      `crates/reify-kernel-occt/build.rs` emits a `cargo:warning` and returns
#      without setting `has_occt`, and the crate degrades to stub types — which
#      also deletes its `#[cfg(all(test, has_occt))]` module and its ~25
#      `#![cfg(has_occt)]` integration binaries. The suite then reports ZERO
#      tests rather than zero failures, so the gate stays green over a kernel
#      nothing exercised. This arm makes that state red here. It also pins the
#      resolved SONAME, because reify pins OCCT nowhere else in-tree: a distro
#      upgrade that moves the version relinks the kernel with nothing louder
#      than a `cargo:warning`, and the has_occt suite that would have caught
#      the regression is exactly what disappears when OCCT goes missing.
#
#   4. Gmsh presence (task #6493). Byte-for-byte the same silent vacuity as
#      arm 3: `reify_build_utils::find(NativeDep::Gmsh)` returns None,
#      `crates/reify-kernel-gmsh/build.rs` emits a `cargo:warning` and returns
#      without setting `has_gmsh`, and every `#[cfg(has_gmsh)]`-gated item in
#      the workspace stops being compiled at all. PRESENCE is fatal here;
#      the resolved SONAME is RECORDED but deliberately NOT pinned to an
#      accepted set — see that arm's own banner for why the OCCT pin's
#      justification does not carry over.
#
#   5. OpenVDB presence (task #6493). The third instance of arm 4's shape,
#      one dep over: `crates/reify-kernel-openvdb/build.rs` is byte-for-byte
#      the same fail-OPEN find()/warning/return, and every
#      `#[cfg(has_openvdb)]`-gated item disappears with it. Presence fatal,
#      SONAME recorded and not pinned, for the same reasons.
#
# Arms run in DECLARATION ORDER and the first failure exits, so an arm can
# only assume the arms above it passed. tests/infra/test_occt_deps_preflight.sh
# drives each downstream arm with healthy fixtures for every arm ahead of it
# for exactly that reason.
#
# verify.sh runs this as the first plan entry when Rust work is in scope.
#
# Exit 0 when every arm passes; non-zero with a clear message otherwise. Fast:
# reads only Cargo.lock, the VERSION stamp, and a handful of stat()s (no
# registry / no cargo / no compile).
set -euo pipefail

err() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
# stdout, not stderr: this is the RECORDING half of the OCCT arm — it puts the
# resolved version in the verify log so a reviewer reading a green
# reify-kernel-occt result can see WHICH OCCT produced it. add_tool() only
# executes plan entries, so nothing parses this script's stdout.
ok() { printf '\033[1;32m[ok]\033[0m %s\n' "$*"; }

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
# tests/infra/test_occt_deps_preflight.sh — so an edit on either side fails
# that guard rather than silently leaving the gate and the build disagreeing.
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
# geometry regressions is itself deleted by the same cfg. A passing suite and
# a DELETED suite are indistinguishable from outside, which is why this has to
# be caught before the compile rather than inferred from test results.
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
    err "Or point OCCT_INCLUDE_DIR / OCCT_LIB_DIR at an existing install."
}

# Root of OCCT's numbered-snap fallback, mirroring the literal in
# find_dir_with_override's `std::fs::read_dir("/snap/freecad")`. A DEFAULTED
# variable rather than an inline literal so the branch is reachable from a
# fixture on a host that has no system OCCT to short-circuit it, and so the
# root itself is a named declaration the parity test can compare against the
# Rust literal. The default IS the production value — overriding it changes
# nothing any pipeline caller does.
OCCT_SNAP_ROOT="${OCCT_SNAP_ROOT:-/snap/freecad}"

occt_hint() {
    occt_install_hint
    err "WHY THIS IS FATAL rather than a warning: without OCCT, reify-kernel-occt's"
    err " build.rs never emits has_occt, so its #[cfg(all(test, has_occt))] module and"
    err " its ~25 #![cfg(has_occt)] integration binaries are not compiled AT ALL — the"
    err " suite reports zero tests REPORTED, not zero tests FAILED, and the gate goes"
    err " green over a kernel nothing exercised."
}

# dep_find_dir <override> <sentinel> <candidate>...
#
# SHARED by all three native-dep arms below, so the resolution rule exists
# ONCE rather than three drifting times.
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
# EMPTY-OVERRIDE RULE (shared, NOT a divergence): an exported-but-EMPTY
# <DEP>_LIB_DIR / <DEP>_INCLUDE_DIR counts as UNSET on both sides and falls
# through to the candidate list. `[ -n "$override" ]` below is the bash half;
# `override_dir.filter(|d| !d.is_empty())` in find_dir_with_override is the
# Rust half, pinned by its `find_dir_ignores_exported_but_empty_override` unit
# test. Before that filter existed, the build resolved such a var to the EMPTY
# PATH, still set has_occt, and linked against nothing — while this guard,
# reading the same environment, went green describing a resolution the build
# would not perform.
#
# Prints the resolved dir on stdout and returns 0; returns 1 with no output
# when nothing resolves.
dep_find_dir() {
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
    # snap_subdir match. On any host that HAS system OCCT the candidate loop
    # above short-circuits before this runs, so its runtime behaviour is
    # unexercised there; what IS pinned by
    # tests/infra/test_occt_deps_preflight.sh is the DECLARATION parity of
    # both halves — OCCT_SNAP_ROOT's default against the Rust read_dir literal,
    # and the sentinel -> subdir mapping below against the Rust match arms,
    # order included.
    #
    # SCOPED TO OCCT BY CONSTRUCTION, and it must stay that way: the `case`
    # below is keyed on the SENTINEL, and has arms only for OCCT's two. Gmsh
    # and OpenVDB sentinels fall through to an empty $snap_subdir and never
    # scan, which is exactly what find_dir_with_override's `_ => None` arm
    # does. Do not add arms for them here — the build has none, and this
    # helper being shared is not a licence to widen it.
    local snap_subdir=""
    case "$sentinel" in
        Standard_Failure.hxx) snap_subdir="usr/include/opencascade" ;;
        libTKernel.so) snap_subdir="usr/lib" ;;
    esac
    if [ -n "$snap_subdir" ]; then
        local rev
        for rev in "$OCCT_SNAP_ROOT"/*/; do
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
# The version segment of the dev symlink's FIRST-LEVEL target, mirroring
# reify_build_utils::read_soname_version() (crates/reify-build-utils/src/lib.rs)
# rule for rule, shared by all three arms so that rule exists ONCE:
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
# three arms differ: OCCT hard-fails (build.rs splices the version into link
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

# dep_searched_desc <override> <env-var-name> <candidate>...
# Human-readable rendering of WHERE the guard actually looked, so a red gate
# names the searched paths rather than leaving the reader to infer them.
# Shared by all three arms, same as dep_find_dir above.
dep_searched_desc() {
    local override="$1" envvar="$2"
    shift 2
    if [ -n "$override" ]; then
        printf '%s (from %s)' "$override" "$envvar"
    else
        printf '%s' "$*"
    fi
}

# dep_presence_arm <PREFIX> <Label> <hint_fn>
#
# The ENTIRE body of a presence-only arm — override read, both resolutions,
# report-both-halves, hint+exit, SONAME recording — for the deps whose gate is
# presence and nothing more. Driven by DATA: every per-dep value is read out of
# the `<PREFIX>_*` names its `# BEGIN <dep>-candidates` marker block already
# declares, via indirect expansion, so an arm is one call and there is exactly
# one copy of the logic.
#
# WHY THIS IS A FUNCTION AND NOT TWO COPIES: the Gmsh and OpenVDB arms were
# near-verbatim duplicates of each other (~45 lines of executable shell apiece,
# differing only in prefix, human label and hint fn). A later change — a bypass
# env, a different error format, an extra diagnostic — then has to be applied
# twice and can silently be applied once, which is the drift class the leaf
# primitives above (dep_find_dir / dep_soname_ver / dep_searched_desc) already
# exist to prevent one level down. The DIAGNOSTICS this body invokes are shared
# for the same reason and by the same rule — see dep_hint() below, of which each
# dep's `<dep>_hint` is now a data-only wrapper.
#
# WHAT IT DOES NOT COVER, deliberately: the OCCT arm stays written out inline
# below. Its SONAME pin is FATAL and sits BETWEEN resolution and the `ok` line
# (accepted-set comparison, two distinct multi-line diagnostics, a different
# install hint per failure mode), so folding it in here would mean a parameter
# for every one of those differences — a worse trade than the duplication this
# removes. Presence-only arms share a body; OCCT's does not exist twice.
#
# <PREFIX> is UPPERCASE (GMSH, OPENVDB) and is the same token the marker block
# and the override env vars use. The lowercase form used in the error text is
# DERIVED from it rather than passed, so the two can never disagree.
#
# Exits 1 (terminating the whole script, which is the contract — arms run in
# declaration order and the first failure exits) when either half is
# unresolved. Returns 0 having printed the arm's `[ok]` recording line
# otherwise.
dep_presence_arm() {
    local prefix="$1" label="$2" hint_fn="$3"
    local lower="${prefix,,}"

    local inc_env="${prefix}_INCLUDE_DIR" lib_env="${prefix}_LIB_DIR"
    local inc_sent_ref="${prefix}_INCLUDE_SENTINEL" lib_sent_ref="${prefix}_LIB_SENTINEL"
    local inc_cands_name="${prefix}_INCLUDE_CANDIDATES" lib_cands_name="${prefix}_LIB_CANDIDATES"
    local inc_cands_ref="${inc_cands_name}[@]" lib_cands_ref="${lib_cands_name}[@]"

    # `:-` on the override reads, same EMPTY-OVERRIDE RULE dep_find_dir
    # documents: an exported-but-empty var counts as UNSET and falls through to
    # the candidate list, matching find_dir_with_override's
    # `.filter(|d| !d.is_empty())`.
    #
    # `:-` on the SENTINEL reads too, for a different reason: this script runs
    # under `set -u`, so a bare `${!lib_sent_ref}` for a dep whose marker block
    # declares its names slightly differently — or a typo in the
    # `dep_presence_arm <PREFIX>` argument — aborts the WHOLE preflight with a
    # bare `check-manifold-deps.sh: line NNN: FOO_LIB_SENTINEL: unbound
    # variable`. That is precisely the cryptic-failure mode this file exists to
    # convert into an actionable message, so it must not be this file's own
    # failure mode. The explicit check below turns it into one.
    local inc_ov="${!inc_env:-}" lib_ov="${!lib_env:-}"
    local inc_sent="${!inc_sent_ref:-}" lib_sent="${!lib_sent_ref:-}"

    # The candidate lists are probed with `declare -p` rather than a `${!ref:-}`
    # read: indirect expansion of an UNDECLARED `FOO[@]` under `:-` yields ONE
    # EMPTY element rather than none, which would silently hand dep_find_dir a
    # bogus "" candidate instead of failing. `declare -p` is also correct for a
    # declared-but-empty array, which a `${!name+x}` probe would misreport.
    local undeclared=""
    [ -n "$inc_sent" ] || undeclared="$undeclared $inc_sent_ref"
    [ -n "$lib_sent" ] || undeclared="$undeclared $lib_sent_ref"
    declare -p "$inc_cands_name" >/dev/null 2>&1 || undeclared="$undeclared $inc_cands_name"
    declare -p "$lib_cands_name" >/dev/null 2>&1 || undeclared="$undeclared $lib_cands_name"

    if [ -n "$undeclared" ]; then
        err "manifold-deps guard: internal error — dep_presence_arm $prefix cannot run;"
        err "                     these names are not declared:$undeclared"
        err "                     The '# BEGIN $lower-candidates' block must declare"
        err "                     ${prefix}_{LIB,INCLUDE}_SENTINEL and"
        err "                     ${prefix}_{LIB,INCLUDE}_CANDIDATES, and the prefix passed"
        err "                     to dep_presence_arm must be the same token those names"
        err "                     use. This is a bug in scripts/check-manifold-deps.sh"
        err "                     itself, NOT a missing install — do not try to fix it by"
        err "                     installing anything or setting ${prefix}_LIB_DIR."
        exit 1
    fi

    local -a inc_cands=("${!inc_cands_ref}") lib_cands=("${!lib_cands_ref}")

    # `|| true` inside the substitution: a non-resolving arm must reach the
    # named error below, not abort under `set -e` with no message at all.
    local inc_resolved lib_resolved
    inc_resolved="$(dep_find_dir "$inc_ov" "$inc_sent" "${inc_cands[@]}" || true)"
    lib_resolved="$(dep_find_dir "$lib_ov" "$lib_sent" "${lib_cands[@]}" || true)"

    # Report BOTH halves before exiting, same rule as the OCCT arm: find() is
    # None when EITHER is unresolved, so a reader whose host is missing both
    # should not have to fix one, re-run, and discover the other.
    local failed=0

    if [ -z "$inc_resolved" ]; then
        err "manifold-deps guard: $lower headers not found — no $inc_sent in:"
        err "                     $(dep_searched_desc "$inc_ov" "$inc_env" "${inc_cands[@]}")"
        failed=1
    fi

    if [ -z "$lib_resolved" ]; then
        err "manifold-deps guard: $lower libraries not found — no $lib_sent in:"
        err "                     $(dep_searched_desc "$lib_ov" "$lib_env" "${lib_cands[@]}")"
        failed=1
    fi

    if [ "$failed" -ne 0 ]; then
        "$hint_fn"
        exit 1
    fi

    # RECORDING half of the arm — stdout, so a reviewer reading a green
    # reify-kernel-<dep> result in the verify log can see WHICH install produced
    # it. Read through the shared dep_soname_ver(), so the first-level-only rule
    # (`readlink`, never `readlink -f`) is stated once for all three arms.
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

# dep_hint <PREFIX> <conda-ver> <apt-ver> <subject> <surfaces-line>... — the
# complete diagnostic block a presence-only arm prints just before it exits:
# how to install the dep, then WHY its absence is fatal rather than a warning.
#
# WHY THIS IS ONE FUNCTION AND NOT FOUR: gmsh_install_hint/openvdb_install_hint
# and gmsh_hint/openvdb_hint were four near-verbatim bodies (~28 lines) whose
# only differences were the dep name, two version numbers, which gated surfaces
# disappear, and the trailing noun. That is the same drift class
# dep_presence_arm's own banner argues against one level down — a change to the
# install instructions (a new setup-dev.sh entry point, a bypass env var) had to
# be applied twice and could silently be applied once. The executable body was
# deduplicated while the diagnostics it invokes were left as copies; this closes
# that gap. The per-dep wrappers below now carry DATA ONLY.
#
# NOT SHARED WITH OCCT, deliberately: occt_hint/occt_install_hint stay written
# out inline. OCCT is called from three different failure paths (both halves
# unresolved, undeterminable SONAME, SONAME drift) with genuinely different
# prose per path, and it is an apt-provisioned system dep rather than a
# conda-forge one — so it shares no sentence with these two.
#
# <PREFIX> is UPPERCASE, the same token the marker block and the override env
# vars use; the lowercase dep name, the crate name and the cfg name are all
# DERIVED from it rather than passed, so they can never disagree with the arm
# that printed them.
#
# The surfaces clause is taken as TRAILING VARARGS, one per output line, so each
# dep keeps its own hand-wrapping instead of rendering as a single over-long
# line that a terminal re-wraps arbitrarily.
dep_hint() {
    local prefix="$1" conda_ver="$2" apt_ver="$3" subject="$4"
    shift 4
    local lower="${prefix,,}"
    local line

    err "Provision the conda-forge reify-deps env — scripts/setup-dev.sh's"
    err "'conda-forge env: gmsh + openvdb' block does exactly this, installing"
    err "$lower $conda_ver into /opt/reify-deps (apt's $lower is stale at $apt_ver):"
    err "    ./scripts/setup-dev.sh"
    err "Or point ${prefix}_INCLUDE_DIR / ${prefix}_LIB_DIR at an existing install."
    err "WHY THIS IS FATAL rather than a warning: without $lower,"
    err " reify-kernel-$lower's build.rs never emits has_$lower, so every"
    err " #[cfg(has_$lower)]-gated item in the workspace is not compiled AT ALL —"
    for line in "$@"; do
        err " $line"
    done
    err " The suite then reports zero tests REPORTED, not zero tests FAILED, and the"
    err " gate goes green over $subject nothing exercised."
}

OCCT_INCLUDE_OVERRIDE="${OCCT_INCLUDE_DIR:-}"
OCCT_LIB_OVERRIDE="${OCCT_LIB_DIR:-}"

# `|| true` inside the substitution: a non-resolving arm must reach the named
# error below, not abort under `set -e` with no message at all.
OCCT_INCLUDE_RESOLVED="$(dep_find_dir "$OCCT_INCLUDE_OVERRIDE" "$OCCT_INCLUDE_SENTINEL" "${OCCT_INCLUDE_CANDIDATES[@]}" || true)"
OCCT_LIB_RESOLVED="$(dep_find_dir "$OCCT_LIB_OVERRIDE" "$OCCT_LIB_SENTINEL" "${OCCT_LIB_CANDIDATES[@]}" || true)"

# Report BOTH halves before exiting. find() is None when EITHER is unresolved,
# so a reader whose host is missing both should not have to fix one, re-run,
# and discover the other.
occt_failed=0

if [ -z "$OCCT_INCLUDE_RESOLVED" ]; then
    err "manifold-deps guard: OCCT headers not found — no $OCCT_INCLUDE_SENTINEL in:"
    err "                     $(dep_searched_desc "$OCCT_INCLUDE_OVERRIDE" OCCT_INCLUDE_DIR "${OCCT_INCLUDE_CANDIDATES[@]}")"
    occt_failed=1
fi

if [ -z "$OCCT_LIB_RESOLVED" ]; then
    err "manifold-deps guard: OCCT libraries not found — no $OCCT_LIB_SENTINEL in:"
    err "                     $(dep_searched_desc "$OCCT_LIB_OVERRIDE" OCCT_LIB_DIR "${OCCT_LIB_CANDIDATES[@]}")"
    occt_failed=1
fi

if [ "$occt_failed" -ne 0 ]; then
    occt_hint
    exit 1
fi

# Both halves resolved, so has_occt WILL be set. Now pin which OCCT it is.
# The first-level read itself lives in dep_soname_ver() above, shared with the
# Gmsh and OpenVDB arms; OCCT_SONAME_TARGET is kept only for the error text,
# which names the raw link target the operator will see on disk.
OCCT_SONAME_PATH="$OCCT_LIB_RESOLVED/$OCCT_LIB_SENTINEL"
OCCT_SONAME_TARGET="$(readlink "$OCCT_SONAME_PATH" 2>/dev/null || true)"
OCCT_SONAME_PREFIX="$OCCT_LIB_SENTINEL."
OCCT_SONAME_VER="$(dep_soname_ver "$OCCT_LIB_RESOLVED" "$OCCT_LIB_SENTINEL")"

if [ -z "$OCCT_SONAME_VER" ]; then
    err "manifold-deps guard: could not determine the OCCT SONAME from $OCCT_SONAME_PATH"
    err "                     (first-level symlink target: '${OCCT_SONAME_TARGET:-<not a symlink>}';"
    err "                      expected $OCCT_SONAME_PREFIX<version>)."
    err "This is NOT a cosmetic gap. find() reports the dir as resolved — it only tests"
    err " existence — so has_occt IS set, and crates/reify-kernel-occt/build.rs falls"
    err " back to a HARD-CODED version for its dylib:+verbatim=libTK*.so.<ver> link"
    err " directives behind nothing but a cargo:warning. The build would link a version"
    err " nobody verified."
    occt_install_hint
    exit 1
fi

OCCT_SONAME_MAJMIN="$(occt_majmin "$OCCT_SONAME_VER")"
occt_soname_accepted=0
for v in "${OCCT_ACCEPTED_SONAMES[@]}"; do
    if [ "$(occt_majmin "$v")" = "$OCCT_SONAME_MAJMIN" ]; then
        occt_soname_accepted=1
        break
    fi
done

if [ "$occt_soname_accepted" -ne 1 ]; then
    err "manifold-deps guard: OCCT SONAME drift — resolved $OCCT_SONAME_VER (major.minor"
    err "                     $OCCT_SONAME_MAJMIN) at $OCCT_LIB_RESOLVED, but the accepted"
    err "                     set is: ${OCCT_ACCEPTED_SONAMES[*]}"
    err "                     (read from $OCCT_SONAME_PATH -> $OCCT_SONAME_TARGET)."
    err "If this move was NOT intended, install an accepted version:"
    occt_install_hint
    err "If it WAS intended: re-validate the OCCT-sensitive pins against $OCCT_SONAME_VER"
    err " (the STEP plane-angle unit pin, the reify-kernel-occt geometry suites), then"
    err " widen OCCT_ACCEPTED_SONAMES in scripts/check-manifold-deps.sh and bump"
    err " scripts/setup-dev.sh's OCCT block to match — tests/infra/"
    err " test_occt_deps_preflight.sh asserts the two agree."
    exit 1
fi

ok "OCCT $OCCT_SONAME_VER at $OCCT_LIB_RESOLVED (headers: $OCCT_INCLUDE_RESOLVED)"

# ---------- Gmsh presence preflight (task #6493) ----------
#
# See arm 4 in the file header. Same fail-OPEN build.rs, same silent deletion
# of the gated test surface, same reason it has to be caught before the
# compile rather than inferred from test results.
#
# Note this gates the VERIFY PIPELINE, not `cargo build` — gmsh-free stub
# builds stay sanctioned, and crates/reify-kernel-gmsh carries real
# `cfg(not(has_gmsh))` stub modules (src/kernel.rs, src/lib.rs,
# src/mesh_profile_2d.rs) for them, exactly as reify-kernel-occt does.

# BEGIN gmsh-candidates — EXACT MIRROR of reify_build_utils::NativeDep::Gmsh
# (crates/reify-build-utils/src/lib.rs). Order is load-bearing and is the
# OPPOSITE of OCCT's: /opt/reify-deps comes FIRST here, because the conda-forge
# env is where reify's gmsh 4.15.2 actually lives and Ubuntu's apt gmsh (4.12.1)
# must not win. Rust stays the source of truth; this block is a declared mirror,
# pinned equal INCLUDING ORDER by tests/infra/test_occt_deps_preflight.sh — so
# an edit on either side fails that guard rather than silently leaving the gate
# and the build disagreeing.
GMSH_LIB_CANDIDATES=(
    /opt/reify-deps/lib
    /usr/lib/x86_64-linux-gnu
    /usr/lib
    /usr/local/lib
)
GMSH_INCLUDE_CANDIDATES=(
    /opt/reify-deps/include
    /usr/include
    /usr/local/include
)
GMSH_LIB_SENTINEL=libgmsh.so
GMSH_INCLUDE_SENTINEL=gmshc.h
# END gmsh-candidates

# Data only — the body is dep_hint() above, shared with openvdb_hint below.
gmsh_hint() {
    dep_hint GMSH 4.15.2 4.12.1 "a mesher" \
        "reify-kernel-gmsh's whole test surface, the occt_gmsh conformance suites," \
        "and the reify-eval FEA/mesh e2e binaries."
}

# The whole arm body — override read, both resolutions, report-both-halves,
# hint+exit, SONAME recording — lives in dep_presence_arm() above, shared with
# the OpenVDB arm below and driven entirely from the GMSH_* names the marker
# block declares.
dep_presence_arm GMSH Gmsh gmsh_hint

# ---------- OpenVDB presence preflight (task #6493) ----------
#
# See arm 5 in the file header. Same fail-OPEN build.rs, same silent deletion
# of the gated surface, same reason it has to be caught before the compile.
#
# Note this gates the VERIFY PIPELINE, not `cargo build` — openvdb-free stub
# builds stay sanctioned, and crates/reify-kernel-openvdb carries real
# `cfg(not(has_openvdb))` stub modules (src/kernel.rs, src/ingest.rs) for them.

# BEGIN openvdb-candidates — EXACT MIRROR of
# reify_build_utils::NativeDep::OpenVdb (crates/reify-build-utils/src/lib.rs).
# Order is load-bearing and is NOT the same as Gmsh's: OpenVdb puts
# /usr/local/lib ahead of /usr/lib/x86_64-linux-gnu where Gmsh does the
# reverse, so this list is copied PER-DEP and must never be "deduplicated"
# against the gmsh block above. /opt/reify-deps leads both, because that is
# where the conda-forge openvdb 13.0.0 lives. Rust stays the source of truth;
# this block is a declared mirror, pinned equal INCLUDING ORDER by
# tests/infra/test_occt_deps_preflight.sh — so an edit on either side fails
# that guard rather than silently leaving the gate and the build disagreeing.
OPENVDB_LIB_CANDIDATES=(
    /opt/reify-deps/lib
    /usr/local/lib
    /usr/lib/x86_64-linux-gnu
    /usr/lib
)
OPENVDB_INCLUDE_CANDIDATES=(
    /opt/reify-deps/include
    /usr/local/include
    /usr/include
)
OPENVDB_LIB_SENTINEL=libopenvdb.so
OPENVDB_INCLUDE_SENTINEL=openvdb/openvdb.h
# END openvdb-candidates

# Data only — same shared dep_hint() body as gmsh_hint above, so a change to the
# install instructions or the fatality rationale lands in both by construction.
openvdb_hint() {
    dep_hint OPENVDB 13.0.0 10.0.1 "a voxel kernel" \
        "the crate's whole sparse-SDF/voxel-grid test surface included."
}

# Same one-call arm as Gmsh's above, one dep over — the body is shared, so a
# future change to the error format or an added diagnostic lands in both.
dep_presence_arm OPENVDB OpenVDB openvdb_hint

exit 0
