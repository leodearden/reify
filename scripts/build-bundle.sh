#!/usr/bin/env bash
# scripts/build-bundle.sh — build Reify's Linux installers: a .deb and an AppImage.
#
# STDOUT CONTRACT. On success stdout carries exactly two lines, the absolute
# paths of the .deb and of the .AppImage, and nothing else; every other byte
# goes to stderr. Consumer: scripts/release-alpha.sh step 2, which reads stdout
# through a process substitution that cannot see this script's exit status, so
# the paths are printed LAST, after every check has passed. A failed preflight
# or post-check exits 2; a failed build step exits with that step's status.
# tests/infra/test_bundle_clean_install.py (armed) asserts the contract on a
# real build and proves both assets in a clean ubuntu:24.04 container.
#
# What ships (docs/prds/alpha-spine.md §5.2): reify-gui, the reify CLI on PATH,
# the sidecar and the landlock resources, the desktop entry with the .ri
# association (all from gui/src-tauri/tauri.conf.json), and the shared-library
# closure a stock Ubuntu 24.04 lacks. scripts/bundle_closure.py derives that
# closure against a reference container and stages it with patched RUNPATHs
# (/usr/bin/* -> /usr/lib/<productName>/lib, each library -> $ORIGIN); the deb
# ships the staged tree through a generated tauri config overlay, and the
# AppImage's libraries are deployed by linuxdeploy, which tauri runs.
#
# Runs unattended (the release gates run it from the orchestrator's systemd
# user service), so nothing prompts. Needs: the native deps
# (scripts/check-manifold-deps.sh), tauri-cli, docker, npm, dpkg-deb, cmp,
# python3; the first AppImage build downloads linuxdeploy into ~/.cache/tauri.
set -euo pipefail

exec 3>&1 1>&2

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"
export CI=true

TAURI_CONF="gui/src-tauri/tauri.conf.json"

fail() { echo "build-bundle: $*" >&2; exit 2; }
step() { echo "==> build-bundle: $*" >&2; }

# ---- 1. preflight -------------------------------------------------------------
step "preflight"
# Absent native deps compile a kernel-less binary silently (that script's
# header, THE SILENT-VACUITY RULE), which must never become a release asset.
bash scripts/check-manifold-deps.sh || fail "native deps preflight failed (scripts/check-manifold-deps.sh)"
cargo tauri --version >/dev/null 2>&1 || fail "cargo tauri (tauri-cli) is not on PATH"
docker info >/dev/null 2>&1 || fail "docker is unusable; scripts/bundle_closure.py probes a reference container"
for tool in npm python3 dpkg-deb tar cmp; do
    command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done

# ---- 2. frontend and sidecar --------------------------------------------------
# gui and gui/sidecar are separate npm roots; lockfile-exact installs.
step "npm ci (gui, gui/sidecar)"
(cd gui && npm ci --no-audit --no-fund)
(cd gui/sidecar && npm ci --no-audit --no-fund)
step "sidecar"
bash gui/sidecar/build-sidecar.sh
step "frontend (gui/dist, embedded by the embedded-frontend feature)"
(cd gui && npm run build)

# ---- 3. binaries --------------------------------------------------------------
step "reify-gui: cargo tauri build --no-bundle"
# --ignore-version-mismatches: tauri-cli refuses the build because the npm
# @tauri-apps/* packages trail the Rust crates by a minor release. That pairing
# is what scripts/run-gui.sh builds and launches today (plain cargo, no check),
# so the flag ships the same binary rather than a new combination. `-- --locked`
# reaches cargo build, so a stale Cargo.lock fails here instead of being rewritten.
(cd gui/src-tauri && cargo tauri build --ci --no-bundle --ignore-version-mismatches \
    --features embedded-frontend -- --locked)
step "reify CLI"
cargo build --release --locked -p reify-cli --bin reify

METADATA="$(cargo metadata --format-version 1 --no-deps)"
TARGET_DIR="$(python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' <<<"$METADATA")"
RELEASE="$TARGET_DIR/release"
STAGE="$TARGET_DIR/bundle-stage"
OVERLAY="$STAGE/overlay.json"
for bin in reify-gui reify; do
    [ -x "$RELEASE/$bin" ] || fail "cargo produced no $RELEASE/$bin"
done

# ---- 4. shared-library closure --------------------------------------------------
step "shared-library closure (scripts/bundle_closure.py)"
rm -rf "$STAGE"
python3 scripts/bundle_closure.py --tauri-conf "$TAURI_CONF" --stage "$STAGE" \
    --overlay "$OVERLAY" --main-binary "reify-gui=$RELEASE/reify-gui" \
    --extra-binary "reify=$RELEASE/reify"

# ---- 5. bundle ------------------------------------------------------------------
# LD_LIBRARY_PATH removed so linuxdeploy resolves the AppImage's libraries the
# way bundle_closure.py resolved the deb's: through each binary's own RUNPATH.
step "cargo tauri bundle (deb, appimage)"
rm -rf "$RELEASE/bundle/deb" "$RELEASE/bundle/appimage"
(cd gui/src-tauri && env -u LD_LIBRARY_PATH cargo tauri bundle --ci --bundles deb,appimage \
    --config "$OVERLAY" --features embedded-frontend)

# ---- 6. post-checks -------------------------------------------------------------
step "post-checks"
shopt -s nullglob
DEBS=("$RELEASE"/bundle/deb/*.deb)
APPIMAGES=("$RELEASE"/bundle/appimage/*.AppImage)
shopt -u nullglob
[ "${#DEBS[@]}" -eq 1 ] || fail "expected one .deb under $RELEASE/bundle/deb, found ${#DEBS[@]}"
[ "${#APPIMAGES[@]}" -eq 1 ] || fail "expected one .AppImage under $RELEASE/bundle/appimage, found ${#APPIMAGES[@]}"
DEB="${DEBS[0]}"
APPIMAGE="${APPIMAGES[0]}"
[ -s "$DEB" ] || fail "$DEB is empty"
[ -s "$APPIMAGE" ] || fail "$APPIMAGE is empty"

CHECK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/build-bundle-check.XXXXXX")"
trap 'rm -rf "$CHECK_DIR"' EXIT
DEB_ROOT="$CHECK_DIR/root"
dpkg-deb --extract "$DEB" "$DEB_ROOT"

for member in usr/bin/reify-gui usr/bin/reify usr/bin/reify-sidecar; do
    [ -f "$DEB_ROOT/$member" ] || fail "$DEB does not contain /$member"
done

# Every deb.files entry of the overlay ships as staged: the RUNPATH-patched
# copies, not cargo's, which tauri copies first and deb.files must overwrite.
DEB_FILES="$(python3 -c 'import json, sys; files = json.load(open(sys.argv[1]))["bundle"]["linux"]["deb"]["files"]; print("\n".join(f"{dest}\t{src}" for dest, src in files.items()))' "$OVERLAY")"
LIB_COUNT=0
shopt -s nullglob
while IFS=$'\t' read -r dest src; do
    if [ -d "$src" ]; then
        for lib in "$src"/*; do
            installed="$dest/${lib##*/}"
            cmp -s "$lib" "$DEB_ROOT$installed" || fail "$installed in $DEB is not the staged $lib"
            LIB_COUNT=$((LIB_COUNT + 1))
        done
    else
        cmp -s "$src" "$DEB_ROOT$dest" || fail "$dest in $DEB is not the staged $src"
    fi
done <<<"$DEB_FILES"
shopt -u nullglob
[ "$LIB_COUNT" -ge 1 ] || fail "$DEB ships no bundled library"
step "deb ships $LIB_COUNT bundled libraries, each byte-identical to its staged copy"

# ---- 7. the contract ------------------------------------------------------------
printf '%s\n' "$DEB" "$APPIMAGE" >&3
