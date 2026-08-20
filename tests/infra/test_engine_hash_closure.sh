#!/usr/bin/env bash
# Drift guard (task 5272; PRD docs/prds/merge-gate-compile-cost.md §5 C4):
# the checked-in reify-eval engine-hash closure manifest
# (crates/reify-eval/engine_hash_closure.txt) must remain a SUPERSET (⊇) of
# reify-eval's actual build+normal (dev-excluded) transitive dependency closure.
#
# Why: ENGINE_VERSION_HASH narrows its Cargo.lock contribution to only the
# resolved (name, version) pins of the crate NAMES in that manifest. If a real
# closure member were MISSING from the manifest, a version bump of that crate
# would NOT invalidate the persistent FEA cache — a silent under-invalidation
# (stale, possibly-incorrect FEA results). This guard recomputes the live
# closure with the SAME canonical command used to author the manifest and fails
# if any actual member is absent from it. Over-approximation (manifest ⊋ actual)
# is deliberately allowed — the safe, over-invalidating direction (G6).
#
# The canonical closure command below MUST stay byte-identical to the one
# documented in the manifest header (and used to author it), so the ⊇ check is
# meaningful.
#
# Classified `pool` in run-all-classification.manifest: hermetic — `cargo tree
# --frozen` is resolve-only (reads the committed Cargo.lock + shared registry
# cache; no compile, no target/ mutation).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

MANIFEST="$REPO_ROOT/crates/reify-eval/engine_hash_closure.txt"

echo "=== engine_hash_closure drift guard ==="

# Canonical closure extraction — MUST stay byte-identical to the command in the
# engine_hash_closure.txt header and the pre-1 authoring command.
compute_closure() {
    cargo tree -p reify-eval -e no-dev --prefix none \
        --target x86_64-unknown-linux-gnu --frozen -f '{p}' \
        | awk '{print $1}' | sort -u
}

# Manifest data lines: drop #-comment and blank lines, then trim leading/trailing
# whitespace on each name so this shell parser matches parse_closure_manifest's
# per-line `.trim()` in src/engine_hash_algo.rs exactly (keeps the two closure
# parsers byte-consistent — otherwise a stray-whitespace name line would make the
# guard compare an untrimmed name against cargo tree's trimmed output and report
# spurious drift). NOT sorted here.
manifest_data() {
    grep -v '^[[:space:]]*#' "$MANIFEST" | sed '/^[[:space:]]*$/d' \
        | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'
}

WORK="$(mktemp -d "${TMPDIR:-/tmp}/engine-hash-closure.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

ACTUAL="$WORK/actual.txt"            # live closure, sorted-unique
RAW_NAMES="$WORK/manifest_raw.txt"   # manifest names, file order
MANIFEST_SORTED="$WORK/manifest.txt" # manifest names, sorted-unique

# Both operands are sorted with the SAME (this-environment) `sort`, so the comm
# comparison below is robust to whatever locale/collation CI runs under.
( cd "$REPO_ROOT" && compute_closure ) > "$ACTUAL"
manifest_data > "$RAW_NAMES"
sort -u "$RAW_NAMES" > "$MANIFEST_SORTED"

echo ""
echo "--- manifest + recomputed closure are present and non-empty ---"
assert "closure manifest exists at crates/reify-eval/engine_hash_closure.txt" \
    test -f "$MANIFEST"
assert "closure manifest has at least one crate name" \
    bash -c '[ -s "$1" ]' _ "$MANIFEST_SORTED"
assert "cargo tree recomputed a non-empty live closure" \
    bash -c '[ -s "$1" ]' _ "$ACTUAL"

echo ""
echo "--- manifest names are unique (no duplicates) ---"
assert "manifest has no duplicate crate names" \
    bash -c '[ "$(wc -l < "$1")" -eq "$(wc -l < "$2")" ]' _ "$RAW_NAMES" "$MANIFEST_SORTED"

echo ""
echo "--- manifest ⊇ live closure (no member missing) ---"
# comm -23 ACTUAL MANIFEST = names in ACTUAL but NOT in the manifest. Any such
# name is a real closure member the narrowed hash would silently ignore → FAIL.
assert "manifest is a superset of the live closure (no member missing)" \
    bash -c '[ -z "$(comm -23 "$1" "$2")" ]' _ "$ACTUAL" "$MANIFEST_SORTED"

echo ""
echo "--- non-vacuity self-check: dropping one real member IS flagged ---"
# Build a synthetic manifest missing exactly one real closure member and prove
# the SAME comm-based comparison reports drift — proves the guard is not vacuous
# (BT-8). Since manifest ⊇ actual, the dropped name is guaranteed present.
DROPPED="$WORK/manifest_missing_one.txt"
DROP_NAME="$(head -n 1 "$ACTUAL")"
grep -vxF "$DROP_NAME" "$MANIFEST_SORTED" > "$DROPPED"
assert "self-check: a manifest missing '$DROP_NAME' is flagged as drift" \
    bash -c '[ -n "$(comm -23 "$1" "$2")" ]' _ "$ACTUAL" "$DROPPED"

test_summary
