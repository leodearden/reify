#!/usr/bin/env bash
# cargo_inputs_lib.sh — obtain a suite's locked cargo registry inputs before
# it reads them with --offline/--frozen (task 9993).
#
# Scope the fetch like the read it feeds: --manifest-path, plus --target
# <triple> only when that read filters to one platform. An unfiltered
# `cargo metadata` needs every target's crates.
#
# cargo_inputs_fetch [cargo fetch args...] returns:
#   0                              the inputs are in CARGO_HOME (silent)
#   $CARGO_INPUTS_UNAVAILABLE_RC   the registry could not be reached: a column-0
#                                  `@@REIFY_INFRA_UNAVAILABLE@@ kind=cargo_fetch`
#                                  line on stderr
#   1                              any other refusal: a defect in the tree
#
# Sourced only. It does not source test_helpers.sh and defines no `test_*`
# function, so test_tree_sitter_pipeline.sh's `declare -F` discovery can source
# it too. Rationale: tests/infra/README.md, "Cargo registry inputs: a verdict
# never depends on the cache".

CARGO_INPUTS_UNAVAILABLE_RC=75

# Measured, cargo 1.96: 'unable to update registry' and 'download of
# config.json failed' with no network; 'offline mode (via `--offline`)' with
# CARGO_NET_OFFLINE on an empty home. 'failed to download' and 'spurious
# network error' are cargo's crate-download wrappers.
_CARGO_INPUTS_NETWORK_SIGNATURES='unable to update registry|failed to download|download of .* failed|spurious network error|offline mode \(via'

cargo_inputs_fetch() {
    local out rc=0
    out=$(cargo fetch --locked "$@" 2>&1) || rc=$?
    [ "$rc" -ne 0 ] || return 0
    if grep -qE "$_CARGO_INPUTS_NETWORK_SIGNATURES" <<<"$out"; then
        echo "@@REIFY_INFRA_UNAVAILABLE@@ kind=cargo_fetch rc=$rc cmd=cargo fetch --locked $*" >&2
        _cargo_inputs_tail "$out"
        return "$CARGO_INPUTS_UNAVAILABLE_RC"
    fi
    echo "cargo fetch --locked $* refused (rc $rc): a defect in the tree, not the registry" >&2
    _cargo_inputs_tail "$out"
    return 1
}

_cargo_inputs_tail() {
    tail -n 20 <<<"$1" | sed 's/^/  | /' >&2
}
