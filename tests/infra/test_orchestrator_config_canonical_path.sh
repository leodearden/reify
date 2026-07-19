#!/usr/bin/env bash
# Infrastructure test for task 5242.
# Guards the canonical orchestrator-config path migration:
#   (A) The canonical top-level config exists at
#       <repo>/dark-factory-orchestrator.yaml and parses as valid YAML
#       (PyYAML-gated SKIP, mirroring test_warm_lane_pool_config.sh).
#   (B) ABSENCE INVARIANT — no tracked file references the legacy top-level
#       config path any more. Dark Factory standardized every project's
#       top-level config on <root>/dark-factory-orchestrator.yaml; the legacy
#       ./orchestrator.yaml symlink is being retired, so any remaining bare
#       reference would orphan a deleted path (~13 executed tests/infra/*.sh
#       read it; .envrc exports it). This git-grep guard is the executable
#       form of that acceptance invariant AND the safe-deletion gate: it
#       proves nothing reads the about-to-be-removed path.
#
# The match pattern is a PCRE negative-lookbehind
# `(?<!dark-factory-)orchestrator\.yaml` so the canonical filename
# `dark-factory-orchestrator.yaml` (which CONTAINS the legacy substring) is
# NOT a false match. The symlink's git blob content is literally
# `dark-factory-orchestrator.yaml`, so it is excluded too — this guard stays
# green whether the symlink is later deleted or retained. This test file's
# own body necessarily contains the pattern (to search for it), so it
# excludes itself via a `:(exclude)` pathspec.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== orchestrator config canonical-path guard (task 5242) ==="

CANONICAL_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# ---------------------------------------------------------------------------
# (A) Canonical config present + valid YAML
# ---------------------------------------------------------------------------
echo ""
echo "--- (A) canonical config present + valid YAML ---"

assert "dark-factory-orchestrator.yaml exists" \
    test -f "$CANONICAL_YAML"

# SKIP guard: require python3 + PyYAML (mirrors test_warm_lane_pool_config.sh).
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML parse assertion"
else
    assert "canonical config parses as valid YAML" \
        python3 -c 'import yaml,sys; yaml.safe_load(open(sys.argv[1]))' "$CANONICAL_YAML"
fi

# ---------------------------------------------------------------------------
# (B) No legacy top-level orchestrator.yaml reference remains (tracked content)
# ---------------------------------------------------------------------------
echo ""
echo "--- (B) no legacy top-level config reference remains ---"

# PASSES iff the git grep finds nothing (prints nothing / exits 1). Any match
# is echoed so the assert() harness dumps the offending file:line list on FAIL.
assert_no_legacy_config_refs() {
    local matches
    matches="$(git -C "$REPO_ROOT" grep -nP '(?<!dark-factory-)orchestrator\.yaml' \
        -- . ':(exclude)tests/infra/test_orchestrator_config_canonical_path.sh' || true)"
    if [ -n "$matches" ]; then
        echo "Legacy top-level config references still present (expected: none):"
        echo "$matches"
        echo "Retarget each to 'dark-factory-orchestrator.yaml' (see task 5242)."
        return 1
    fi
    return 0
}

assert "no legacy top-level config reference remains in tracked content" \
    assert_no_legacy_config_refs

test_summary
