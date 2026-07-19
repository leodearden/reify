#!/usr/bin/env bash
# Infrastructure test for task 5247.
# Guards the review-amendment stopgap removal (capstone task 5247, undeploying
# task 5246's override now that dark_factory:2749/2750 have landed):
#   (A) PyYAML-gated SKIP:
#       (a) dark-factory-orchestrator.yaml still parses as valid YAML (an
#           edit-corruption regression guard for the removal edit).
#       (b) the top-level 'max_amendment_rounds' key is ABSENT from the
#           parsed config, so dark-factory's built-in default (1) applies.
#           Checked via dict MEMBERSHIP, not truthiness — the removed value
#           was literally 0 (falsy), so a truthiness check (`if d.get(key):`)
#           would be satisfied by a present-but-0 key and never distinguish
#           "absent" from "present with 0".
#   (B) Always-runs (python-free) guard: no active `max_amendment_rounds:`
#       key-assignment line remains anywhere in the file.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== max_amendment_rounds default-restored guard (task 5247) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"

# ---------------------------------------------------------------------------
# (A) PyYAML-gated: file still parses + max_amendment_rounds key absent
# ---------------------------------------------------------------------------
echo ""
echo "--- (A) parsed-config assertions via PyYAML ---"

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML parse assertions"
else
    assert "dark-factory-orchestrator.yaml still parses as valid YAML" \
        python3 -c 'import yaml,sys; yaml.safe_load(open(sys.argv[1]))' "$ORCH_YAML"

    # Membership check, NOT truthiness: the removed value was literally 0
    # (falsy), so `if d.get("max_amendment_rounds"):` would already read as
    # "absent" against current main and make this assertion vacuously pass
    # before the key is actually removed.
    assert "'max_amendment_rounds' key is absent from parsed config (DF default of 1 applies)" \
        python3 -c 'import yaml,sys; d = yaml.safe_load(open(sys.argv[1])); sys.exit(0 if "max_amendment_rounds" not in d else 1)' "$ORCH_YAML"
fi

# ---------------------------------------------------------------------------
# (B) No active max_amendment_rounds key-assignment line remains (python-free)
# ---------------------------------------------------------------------------
echo ""
echo "--- (B) no active key-assignment line remains ---"

assert "no active max_amendment_rounds key line remains" \
    bash -c "! grep -Eq '^[[:space:]]*max_amendment_rounds[[:space:]]*:' '$ORCH_YAML'"

test_summary
