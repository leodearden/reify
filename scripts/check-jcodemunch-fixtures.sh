#!/usr/bin/env bash
# scripts/check-jcodemunch-fixtures.sh
#
# OFFLINE validator for crates/reify-audit/tests/fixtures/jcodemunch/.
# Checks that each fixture file exists, is non-empty, parses as JSON, and
# carries its documented top-level key encoding the real wire SHAPE.
#
# Does NOT assert array non-emptiness: a clean repo may legitimately yield
# empty dead_symbols or violations; the contract these fixtures encode for
# L-CLIENT's decode boundary test is the wire shape, not cardinality.
#
# See also:
#   crates/reify-audit/tests/fixtures/jcodemunch/README.md — capture provenance
#   scripts/smoke-jcodemunch-serve.sh — live serve smoke (requires running serve)
#   docs/architecture-audit/jcodemunch-serve-activation.md — activation runbook
#
# Requires: jq

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/check-jcodemunch-fixtures.sh [-h|--help]

Offline shape-validation for crates/reify-audit/tests/fixtures/jcodemunch/.
Checks existence, JSON parse, and documented top-level key for each fixture.
Exits 0 on success, 1 on first failure.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

FIXTURE_DIR="crates/reify-audit/tests/fixtures/jcodemunch"

# Navigate to repo root relative to script location.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

check_fixture() {
    local name="$1"
    local key="$2"
    local path="$FIXTURE_DIR/$name"

    echo "check-jcodemunch-fixtures: [$name] ..."

    # Existence check.
    if [[ ! -f "$path" ]]; then
        echo "FAIL [$name]: file does not exist: $path" >&2
        echo "  Capture it with the serve running (see README.md and step-6 in the plan)." >&2
        return 1
    fi

    # Non-empty check.
    if [[ ! -s "$path" ]]; then
        echo "FAIL [$name]: file is empty: $path" >&2
        return 1
    fi

    # JSON parse check.
    if ! jq -e '.' "$path" >/dev/null 2>&1; then
        echo "FAIL [$name]: not valid JSON: $path" >&2
        echo "  $(head -c 200 "$path")" >&2
        return 1
    fi

    # Top-level key presence check.
    if ! jq -e --arg k "$key" 'has($k)' "$path" >/dev/null 2>&1; then
        echo "FAIL [$name]: missing top-level key '$key' in $path" >&2
        echo "  Top-level keys present: $(jq -r 'keys[]' "$path" 2>/dev/null | tr '\n' ' ')" >&2
        return 1
    fi

    echo "check-jcodemunch-fixtures: $name OK (key='$key' present)"
}

# Fixture: get_changed_symbols → key: changed_symbols
check_fixture "get_changed_symbols.json" "changed_symbols"

# Fixture: get_dead_code_v2 → key: dead_symbols
check_fixture "get_dead_code_v2.json" "dead_symbols"

# Fixture: get_untested_symbols → key: symbols
check_fixture "get_untested_symbols.json" "symbols"

# Fixture: get_layer_violations → key: violations
check_fixture "get_layer_violations.json" "violations"

echo "check-jcodemunch-fixtures: OK  all 4 fixtures present, valid JSON, correct keys"
