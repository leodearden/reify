#!/usr/bin/env bash
# scripts/check-jcodemunch-fixtures.sh
#
# OFFLINE validator for crates/reify-audit/tests/fixtures/jcodemunch/.
# Checks that each fixture file exists, is non-empty, parses as JSON, and
# carries the correct wire-level signature.
#
# Two fixture shapes are possible:
#
#   MUNCH-encoded (get_changed_symbols, get_dead_code_v2, get_untested_symbols):
#     The fixture is the JSON-RPC result object {"content":[{"type":"text","text":
#     "#MUNCH/1 tool=<name> enc=gen1\n..."}],"isError":false}.  The check asserts
#     the "content" key is present and content[0].text starts with
#     "#MUNCH/1 tool=<name>".
#
#   Plain-JSON (get_layer_violations):
#     The fixture is the tool's text response parsed as JSON — an object with a
#     documented top-level key ("violations").  The check asserts that key exists.
#
# Does NOT assert array non-emptiness: a clean repo may legitimately yield
# empty dead_symbols or violations; the contract these fixtures encode for
# L-CLIENT's decode boundary test is the real wire SHAPE, not cardinality.
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
MUNCH fixtures: checks content[0].text starts with "#MUNCH/1 tool=<name>".
JSON fixtures:  checks documented top-level key is present.
Exits 0 on success, 1 on first failure.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

FIXTURE_DIR="crates/reify-audit/tests/fixtures/jcodemunch"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

_check_common() {
    local name="$1"; local path="$FIXTURE_DIR/$name"
    if [[ ! -f "$path" ]]; then
        echo "FAIL [$name]: file does not exist: $path" >&2
        echo "  Capture with serve running — see README.md and step-6 in the plan." >&2
        return 1
    fi
    if [[ ! -s "$path" ]]; then
        echo "FAIL [$name]: file is empty: $path" >&2
        return 1
    fi
    if ! jq -e '.' "$path" >/dev/null 2>&1; then
        echo "FAIL [$name]: not valid JSON: $path" >&2
        echo "  $(head -c 200 "$path")" >&2
        return 1
    fi
}

# check_munch_fixture NAME TOOL_NAME
# Validates a MUNCH-encoded fixture (result object with content[].text header).
check_munch_fixture() {
    local name="$1"; local tool_name="$2"; local path="$FIXTURE_DIR/$name"
    echo "check-jcodemunch-fixtures: [$name] (MUNCH) ..."
    _check_common "$name" || return 1

    # Must have a "content" array at top level (JSON-RPC result object shape).
    if ! jq -e '.content' "$path" >/dev/null 2>&1; then
        echo "FAIL [$name]: missing top-level 'content' key (expected JSON-RPC result object)." >&2
        echo "  Keys present: $(jq -r 'keys[]' "$path" 2>/dev/null | tr '\n' ' ')" >&2
        return 1
    fi

    # content[0].text must start with "#MUNCH/1 tool=<tool_name>".
    expected_header="#MUNCH/1 tool=$tool_name"
    actual_first_line=$(jq -r '.content[0].text' "$path" 2>/dev/null | head -1 || true)
    if [[ "$actual_first_line" != "$expected_header"* ]]; then
        echo "FAIL [$name]: content[0].text does not start with '$expected_header'." >&2
        echo "  Actual first line: $actual_first_line" >&2
        return 1
    fi

    echo "check-jcodemunch-fixtures: $name OK (MUNCH header='$expected_header')"
}

# check_json_fixture NAME TOP_LEVEL_KEY
# Validates a plain-JSON fixture (tool text response with a documented top-level key).
check_json_fixture() {
    local name="$1"; local key="$2"; local path="$FIXTURE_DIR/$name"
    echo "check-jcodemunch-fixtures: [$name] (JSON) ..."
    _check_common "$name" || return 1

    if ! jq -e --arg k "$key" 'has($k)' "$path" >/dev/null 2>&1; then
        echo "FAIL [$name]: missing top-level key '$key' in $path" >&2
        echo "  Top-level keys present: $(jq -r 'keys[]' "$path" 2>/dev/null | tr '\n' ' ')" >&2
        return 1
    fi

    echo "check-jcodemunch-fixtures: $name OK (key='$key' present)"
}

# Fixture: get_changed_symbols → MUNCH-encoded result object
check_munch_fixture "get_changed_symbols.json" "get_changed_symbols"

# Fixture: get_dead_code_v2 → MUNCH-encoded result object
check_munch_fixture "get_dead_code_v2.json" "get_dead_code_v2"

# Fixture: get_untested_symbols → MUNCH-encoded result object
check_munch_fixture "get_untested_symbols.json" "get_untested_symbols"

# Fixture: get_layer_violations → plain JSON with "violations" key (empty array; clean-repo capture)
check_json_fixture "get_layer_violations.json" "violations"

# Fixture: get_layer_violations_populated → synthetic hand-authored fixture with one violation record
# Validates the populated decode path that the real wire capture could not exercise (clean repo).
check_json_fixture "get_layer_violations_populated.json" "violations"

echo "check-jcodemunch-fixtures: OK  all 5 fixtures present, valid JSON, correct shapes"
