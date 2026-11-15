#!/usr/bin/env bash
# scripts/release-alpha.sh <tag> — publish an alpha release from the main checkout.
#
# The `before_done` deploy action of the alpha-spine release gates (docs/prds/alpha-spine.md
# D7, tasks ρ1 #9735 and ρ2 #9738): runs unattended from the orchestrator's main checkout once
# every leaf of that release has landed. Steps, each loud on failure so the runner escalates:
#   1. preflight: on main, docs/releases/<tag>.md present, scripts/build-bundle.sh present (δ #9732),
#      gh authenticated;
#   2. scripts/build-bundle.sh — contract: prints the absolute path of every produced asset
#      (.deb and .AppImage) one per line on stdout, everything else on stderr;
#   3. annotated tag <tag> at HEAD (an existing tag must already point at HEAD), pushed to the
#      GitHub remote;
#   4. GitHub prerelease <tag> with the note as body and the assets attached; a re-run on an
#      existing release re-uploads the assets (--clobber) instead of failing;
#   5. close every issue the note names on a line starting `Closes:` (e.g. `Closes: #1, #2`),
#      commenting with the release URL; already-closed issues are skipped.
# Idempotent: a second run on the same tag reaches the same end state.
set -euo pipefail

TAG="${1:?usage: scripts/release-alpha.sh <tag>}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
NOTE="docs/releases/$TAG.md"

fail() { echo "release-alpha: $*" >&2; exit 2; }

# ---- 1. preflight ----------------------------------------------------------------
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || fail "HEAD is not main"
[ -f "$NOTE" ] || fail "missing migration note $NOTE"
[ -x scripts/build-bundle.sh ] || fail "scripts/build-bundle.sh missing or not executable (alpha-spine δ)"
command -v gh >/dev/null || fail "gh not on PATH"
gh auth status >/dev/null 2>&1 || fail "gh is not authenticated in this environment"
REMOTE="$(git remote -v | awk '/github\.com/ && /\(push\)/ {print $1; exit}')"
[ -n "$REMOTE" ] || fail "no github.com push remote"
REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
[ -n "$REPO" ] || fail "gh cannot resolve the repository"

# ---- 2. bundle --------------------------------------------------------------------
mapfile -t ASSETS < <(scripts/build-bundle.sh | grep -E '\.(deb|AppImage)$' || true)
[ "${#ASSETS[@]}" -ge 2 ] || fail "build-bundle.sh produced ${#ASSETS[@]} asset path(s); expected a .deb and an AppImage"
for a in "${ASSETS[@]}"; do [ -s "$a" ] || fail "asset missing or empty: $a"; done

# ---- 3. tag -----------------------------------------------------------------------
HEAD_SHA="$(git rev-parse HEAD)"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    [ "$(git rev-parse "$TAG^{commit}")" = "$HEAD_SHA" ] || fail "tag $TAG exists and does not point at HEAD $HEAD_SHA"
else
    git tag -a "$TAG" -m "$TAG" "$HEAD_SHA"
fi
# https remote with no stored git credential: borrow gh's token for this one push.
git -c credential.helper= -c "credential.helper=!gh auth git-credential" push "$REMOTE" "refs/tags/$TAG"

# ---- 4. release -------------------------------------------------------------------
if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "${ASSETS[@]}" --clobber
    gh release edit "$TAG" --notes-file "$NOTE" --prerelease >/dev/null
else
    gh release create "$TAG" "${ASSETS[@]}" --title "$TAG" --notes-file "$NOTE" --prerelease --target "$HEAD_SHA" >/dev/null
fi
URL="$(gh release view "$TAG" --json url -q .url)"

# ---- 5. close the issues the note names -----------------------------------------
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
