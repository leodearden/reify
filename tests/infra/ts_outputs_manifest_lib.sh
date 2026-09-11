#!/usr/bin/env bash
# tests/infra/ts_outputs_manifest_lib.sh — the ONE test-side verifier for
# tree-sitter-reify/src/.generated_outputs.stamp.
#
# Designed to be sourced, not executed directly.
#
# WHY THIS IS SHARED (`#6992` amendment pass).  The manifest format —
# `<sha256>  <relpath>` lines sorted by relpath — has TWO independent WRITERS:
# `_render_outputs_manifest` in scripts/tree-sitter-generate.sh and
# `outputs_manifest_render` in tree-sitter-reify/build_support.rs.  Two writers
# is deliberate, one per language, and test_shell_written_manifest_satisfies_build_rs
# pins them against each other.  Two independent test-side VERIFIERS is not: each
# would quietly accept whatever shape its own writer happens to emit, which is
# exactly the drift the cross-check exists to catch.  So the verifier lives here
# once, and both scripts/test_tree_sitter_generate.sh and
# tests/infra/test_tree_sitter_pipeline.sh source it.
#
# NO FUNCTION DEFINED HERE MAY CONTAIN THE SUBSTRING `test_` IN ITS NAME.
# test_tree_sitter_pipeline.sh's run_tests discovers cases with
# `declare -F | awk '/test_/{print $3}'`, which matches anywhere on the line, so
# any such function would be executed as a test case by every suite that sources
# this file.

# Source guard — prevent double-sourcing.
if [ "${_TS_OUTPUTS_MANIFEST_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_TS_OUTPUTS_MANIFEST_LIB_SOURCED=1

# The generated outputs, one per line in LC_ALL=C sort order — the order both
# writers emit, and the order build_support.rs's EXPECTED_OUTPUTS is compared as.
TS_MANIFEST_EXPECTED_RELS='grammar.json
node-types.json
parser.c'

# ts_sha256 <file>
#
# Bare sha256 of one file, or return 1 having printed nothing.
#
# Mirrors portable_sha256 (scripts/lib_portable.sh) rather than sourcing it, for
# the namespace reason in this file's header: a sourced library's functions land
# in the caller's namespace, and run_tests would execute any of them whose name
# contains `test_`.
ts_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        return 1
    fi
}

# ts_outputs_manifest_check <src-dir>
#
# Verify <src-dir>/.generated_outputs.stamp against the bytes sitting in
# <src-dir> RIGHT NOW: the manifest exists and is non-empty, it names EXACTLY
# the three expected outputs sorted by relpath, and every recorded hash equals
# the file's current sha256.
#
# TRI-STATE, so a host gap never reads as a defect and a defect never reads as a
# host gap — the distinction suggestion (3) of the `#6992` review turned on:
#   0 — verified
#   1 — verification FAILED (diagnostic already printed)
#   2 — cannot verify: neither sha256sum nor shasum is on PATH (message printed)
#
# A named output that will not hash is a FAIL, not a 2: the hasher was proven
# present up front, so the fault is the file — and a missing/unreadable output is
# precisely the partial-generate residue this manifest exists to detect.
#
# Nothing a COMMENT can satisfy: the subject is the manifest's bytes on disk,
# never the source text of whatever wrote them.
ts_outputs_manifest_check() {
    local src="$1" stamp="$1/.generated_outputs.stamp"

    # Probed ONCE, before any per-file hashing, so that every later failure can
    # be attributed to the file rather than to the environment.
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        echo "  SKIP: no sha256sum/shasum on PATH"
        return 2
    fi

    if [ ! -s "$stamp" ]; then
        echo ""
        echo "  ASSERTION FAILED: outputs manifest is empty or missing: $stamp"
        return 1
    fi

    local actual_rels
    actual_rels=$(awk '{print $2}' "$stamp")
    if [ "$actual_rels" != "$TS_MANIFEST_EXPECTED_RELS" ]; then
        echo ""
        echo "  ASSERTION FAILED: manifest must name exactly the three generated outputs, sorted"
        echo "  manifest: $stamp"
        echo "  --- expected ---"; printf '%s\n' "$TS_MANIFEST_EXPECTED_RELS"
        echo "  --- actual ---";   printf '%s\n' "$actual_rels"
        return 1
    fi

    local rel recorded actual
    while read -r recorded rel; do
        [ -n "$rel" ] || continue
        # Guarded substitution, and an empty result treated as a mismatch: an
        # unguarded `actual=$(ts_sha256 ...)` propagates the hasher's exit status
        # under `set -euo pipefail` and kills the whole suite mid-run, turning
        # one assertion's failure into a script crash that reports nothing.
        actual=$(ts_sha256 "$src/$rel" 2>/dev/null) || actual=""
        if [ -z "$actual" ]; then
            echo ""
            echo "  ASSERTION FAILED: manifest names $rel, but it would not hash"
            echo "  manifest: $stamp"
            echo "  path:     $src/$rel"
            return 1
        fi
        if [ "$recorded" != "$actual" ]; then
            echo ""
            echo "  ASSERTION FAILED: manifest hash for $rel does not match the file on disk"
            echo "  manifest: $stamp"
            echo "  recorded: $recorded"
            echo "  actual:   $actual"
            return 1
        fi
    done < "$stamp"

    return 0
}
