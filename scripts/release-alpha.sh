#!/usr/bin/env bash
# scripts/release-alpha.sh <tag> — publish an alpha release from the main checkout.
#
# The `before_done` deploy action of the alpha-spine release gates (docs/prds/alpha-spine.md
# D7, tasks ρ1 #9735 and ρ2 #9738): runs unattended from the orchestrator's main checkout once
# every leaf of that release has landed. Steps, each loud on failure so the runner escalates:
#   1. preflight: <tag> names a version, v<major>.<minor>[.<patch>][-<prerelease>], which maps
#      to the bundle version <major>.<minor>.<patch, else 0>[-<prerelease>] (v0.2-alpha ->
#      0.2.0-alpha; tauri requires semver), and any other tag is refused first; on main, an
#      existing tag <tag> already points at HEAD, docs/releases/<tag>.md present,
#      scripts/build-bundle.sh and the B7 proof tests/infra/test_bundle_clean_install.py
#      present (δ #9732), gh authenticated — all before the cold bundle build and the proof;
#   2. scripts/build-bundle.sh --version <bundle version> — contract: prints the absolute path
#      of every produced asset (.deb and .AppImage) one per line on stdout, everything else on
#      stderr; exactly one .deb and one .AppImage are required, each named `_<bundle version>_`;
#   3. B7 (docs/prds/alpha-spine.md §6), the clean-container proof, armed over exactly the
#      assets step 5 publishes; a red proof exits 2 before any tag, push, release or issue
#      close, on a re-run too, so it never --clobbers good assets;
#   4. annotated tag <tag> at HEAD, created when absent, pushed to the GitHub remote;
#   5. GitHub prerelease <tag> with the note as body and the assets attached; a re-run on an
#      existing release re-uploads the assets (--clobber) instead of failing;
#   6. close every issue the note names on a line starting `Closes:` (e.g. `Closes: #1, #2`),
#      commenting with the release URL; already-closed issues are skipped.
# Idempotent: a second run on the same tag reaches the same end state.
set -euo pipefail

fail() { echo "release-alpha: $*" >&2; exit 2; }

TAG="${1:?usage: scripts/release-alpha.sh <tag>}"
# Semver's grammar: no leading zeros; the prerelease is dot-separated non-empty
# identifiers, each numeric without a leading zero or alphanumeric with a letter or `-`.
NUMBER='(0|[1-9][0-9]*)'
PRERELEASE_ID="($NUMBER|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION_TAG="^v$NUMBER\\.$NUMBER(\\.$NUMBER)?(-$PRERELEASE_ID(\\.$PRERELEASE_ID)*)?\$"
[[ "$TAG" =~ $VERSION_TAG ]] \
    || fail "tag $TAG does not name a version: expected v<major>.<minor>[.<patch>][-<prerelease>] (semver)"
BUNDLE_VERSION="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.${BASH_REMATCH[4]:-0}${BASH_REMATCH[5]}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
NOTE="docs/releases/$TAG.md"
PROOF="tests/infra/test_bundle_clean_install.py"

# ---- 1. preflight ----------------------------------------------------------------
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || fail "HEAD is not main"
HEAD_SHA="$(git rev-parse HEAD)"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    [ "$(git rev-parse "$TAG^{commit}")" = "$HEAD_SHA" ] || fail "tag $TAG exists and does not point at HEAD $HEAD_SHA"
fi
[ -f "$NOTE" ] || fail "missing migration note $NOTE"
[ -x scripts/build-bundle.sh ] || fail "scripts/build-bundle.sh missing or not executable (alpha-spine δ)"
[ -f "$PROOF" ] || fail "$PROOF missing (the B7 clean-container proof, alpha-spine δ)"
command -v gh >/dev/null || fail "gh not on PATH"
gh auth status >/dev/null 2>&1 || fail "gh is not authenticated in this environment"
REMOTE="$(git remote -v | awk '/github\.com/ && /\(push\)/ {print $1; exit}')"
[ -n "$REMOTE" ] || fail "no github.com push remote"
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
[ -n "$REPO" ] || fail "gh cannot resolve the repository"

# ---- 2. bundle --------------------------------------------------------------------
mapfile -t ASSETS < <(scripts/build-bundle.sh --version "$BUNDLE_VERSION" \
    | grep -E '\.(deb|AppImage)$' || true)
DEBS=()
APPIMAGES=()
for a in "${ASSETS[@]}"; do
    [ -s "$a" ] || fail "asset missing or empty: $a"
    case "${a##*/}" in
        *"_${BUNDLE_VERSION}_"*) ;;
        *) fail "asset $a is not named for version $BUNDLE_VERSION (tag $TAG)" ;;
    esac
    case "$a" in
        *.deb) DEBS+=("$a") ;;
        *.AppImage) APPIMAGES+=("$a") ;;
    esac
done
[ "${#DEBS[@]}" -eq 1 ] && [ "${#APPIMAGES[@]}" -eq 1 ] \
    || fail "build-bundle.sh produced ${#DEBS[@]} .deb and ${#APPIMAGES[@]} .AppImage path(s); expected one .deb and one .AppImage"
ASSETS=("${DEBS[0]}" "${APPIMAGES[0]}")

# ---- 3. B7: the clean-container proof over the assets about to be published -------
REIFY_RUN_BUNDLE_CLEAN_INSTALL=1 REIFY_BUNDLE_DEB="${DEBS[0]}" REIFY_BUNDLE_APPIMAGE="${APPIMAGES[0]}" \
    python3 "$PROOF" -v >&2 \
    || fail "B7 clean-container proof failed on ${DEBS[0]} and ${APPIMAGES[0]}; nothing tagged or published"

# ---- 4. tag -----------------------------------------------------------------------
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || git tag -a "$TAG" -m "$TAG" "$HEAD_SHA"
# https remote with no stored git credential: borrow gh's token for this one push.
git -c credential.helper= -c "credential.helper=!gh auth git-credential" push "$REMOTE" "refs/tags/$TAG"

# ---- 5. release -------------------------------------------------------------------
if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "${ASSETS[@]}" --clobber
    gh release edit "$TAG" --notes-file "$NOTE" --prerelease >/dev/null
else
    gh release create "$TAG" "${ASSETS[@]}" --title "$TAG" --notes-file "$NOTE" --prerelease --target "$HEAD_SHA" >/dev/null
fi
URL="$(gh release view "$TAG" --json url -q .url)"

# ---- 6. close the issues the note names -----------------------------------------
ISSUES="$(grep -iE '^Closes:' "$NOTE" | grep -oE '#[0-9]+' | tr -d '#' | sort -un || true)"
for n in $ISSUES; do
    state="$(gh issue view "$n" --json state -q .state 2>/dev/null || echo MISSING)"
    case "$state" in
        OPEN) gh issue close "$n" --comment "Fixed in $TAG: $URL" >/dev/null; echo "closed #$n" ;;
        CLOSED) echo "already closed #$n" ;;
        *) fail "issue #$n named in $NOTE does not exist in $REPO" ;;
    esac
done

echo "released $TAG at $HEAD_SHA: $URL"
printf '%s\n' "${ASSETS[@]}"
