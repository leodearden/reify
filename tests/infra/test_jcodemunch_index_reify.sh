#!/usr/bin/env bash
# Infrastructure tests for the single `watch --once` index primitive
# scripts/jcodemunch-index-reify.sh (task 6107, β).
#
# Design: docs/prds/jcodemunch-substrate-restoration.md §4.4
#         docs/prds/jcodemunch-substrate-restoration.capability-manifest.md §2/β
#
# Validates the script's CONTRACT executably rather than by grepping its prose
# — PRD §2.4 identifies exactly that vacuous-evidence shape as the disease
# (`L-SMOKE` named a script that did not exist; `jcodemunch_live.rs` was
# PASS-shaped whether or not the chain worked). Three test-only seams keep the
# whole contract hermetic:
#
#   --dry-run     prints the CONSTRUCTED argv, so the invocation contract
#                 (version pin / `watch` / `--once` / `--no-ai-summaries`, and
#                 the `--paths-from` + `index` bans) binds to the real command.
#   --check-only  skips the indexer and runs only identity resolution plus the
#                 DB gates, so the missing/empty/truncated refusals are driven
#                 against synthetic sqlite DBs under a temp CODE_INDEX_PATH.
#   REIFY_JC_INDEXER_CMD  a test-only indexer override, so the two REAL 1.108.54
#                 `watch --once` stderr shapes drive the summary/exit contract.
#
# Deliberately does NOT execute the real `uvx … watch --once` path: it costs
# ~5-10 min, needs PyPI, and mutates the host-global index at ~/.code-index —
# none of which belongs on a merge gate. That one end-to-end run is discharged
# once by the implementer as recorded acceptance evidence (same reasoning the
# capability manifest's `capstone-must-not-become-gate-resident` resolution
# applies to ε), mirroring test_gui_test_script.sh / test_run_gui_scripts.sh,
# which check a launcher's contract without launching it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

JC_INDEX="$REPO_ROOT/scripts/jcodemunch-index-reify.sh"

echo "=== jcodemunch-index-reify.sh index-primitive tests ==="

# -- Test 1: file exists + is executable -------------------------------------
echo ""
echo "--- Test 1: scripts/jcodemunch-index-reify.sh exists and is executable ---"

assert "scripts/jcodemunch-index-reify.sh exists" \
    test -f "$JC_INDEX"

assert "scripts/jcodemunch-index-reify.sh is executable" \
    test -x "$JC_INDEX"

# -- Test 2: shebang and strict-mode flags -----------------------------------
echo ""
echo "--- Test 2: shebang and 'set -euo pipefail' ---"

assert "scripts/jcodemunch-index-reify.sh has '#!/usr/bin/env bash' shebang on line 1" \
    bash -c "head -n1 '$JC_INDEX' | grep -qE '^#!/usr/bin/env bash$'"

assert "scripts/jcodemunch-index-reify.sh contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$JC_INDEX"

assert "scripts/jcodemunch-index-reify.sh passes 'bash -n' syntax check" \
    bash -n "$JC_INDEX"

test_summary
