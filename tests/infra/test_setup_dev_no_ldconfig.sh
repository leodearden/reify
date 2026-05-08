#!/usr/bin/env bash
# Tests that scripts/setup-dev.sh has no UNCOMMENTED lines that write
# /etc/ld.so.conf.d/reify-deps.conf or call `sudo ldconfig`.
#
# Background: that conf-file approach was replaced by per-crate RPATH in
# crates/reify-kernel-{gmsh,openvdb}/build.rs.  Active code that writes
# the conf file or calls ldconfig must not exist; explanatory comments
# that reference the old design are still allowed.
#
# Check 0: setup-dev.sh exists and is executable (sanity).
# Check 1: no uncommented line references ld.so.conf.d/reify-deps.conf.
# Check 2: no uncommented line declares the ldso_conf variable.
# Check 3: no uncommented line invokes `sudo ldconfig`.
#
# "Uncommented" means: after stripping lines whose first non-whitespace
# character is `#`.  The two-step filter approach (grep -v then grep) is
# used rather than an anchored regex so that patterns appearing at the
# very start of a non-comment line are still caught.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== setup-dev.sh: no ld.so.conf.d wiring tests ==="

# ==============================================================================
# Check 0: sanity — setup-dev.sh exists and is executable
# ==============================================================================
echo ""
echo "--- Check 0: setup-dev.sh exists and is executable ---"

assert "scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "scripts/setup-dev.sh is executable" \
    test -x "$SETUP_DEV"

# ==============================================================================
# Check 1: no uncommented line references ld.so.conf.d/reify-deps.conf
#
# Two-step filter: grep -v strips comment lines (first non-whitespace is #),
# then grep -q searches for the literal string.  Negation via ! means the
# assertion passes only when no match is found.
# ==============================================================================
echo ""
echo "--- Check 1: no uncommented 'ld.so.conf.d/reify-deps.conf' reference ---"

assert "no uncommented line references ld.so.conf.d/reify-deps.conf" \
    bash -c "! grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'ld.so.conf.d/reify-deps.conf'"

# ==============================================================================
# Check 2: no uncommented line declares the ldso_conf variable
# ==============================================================================
echo ""
echo "--- Check 2: no uncommented 'ldso_conf=' declaration ---"

assert "no uncommented line contains 'ldso_conf='" \
    bash -c "! grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'ldso_conf='"

# ==============================================================================
# Check 3: no uncommented line invokes sudo ldconfig
# ==============================================================================
echo ""
echo "--- Check 3: no uncommented 'sudo ldconfig' invocation ---"

assert "no uncommented line invokes 'sudo ldconfig'" \
    bash -c "! grep -Ev '^[[:space:]]*#' \"$SETUP_DEV\" | grep -qF 'sudo ldconfig'"

# -- Summary ------------------------------------------------------------------
test_summary
