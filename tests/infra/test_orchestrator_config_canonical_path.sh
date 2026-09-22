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
#   (C) The scan's charter (task 7788), pinned against a throwaway repo: a
#       mention inside the machine-written agent-confusion corpus is ignored,
#       while the identical mention in any other file is still reported.
#
# The match pattern is a PCRE negative-lookbehind
# `(?<!dark-factory-)orchestrator\.yaml` so the canonical filename
# `dark-factory-orchestrator.yaml` (which CONTAINS the legacy substring) is
# NOT a false match. The symlink's git blob content is literally
# `dark-factory-orchestrator.yaml`, so it is excluded too — this guard stays
# green whether the symlink is later deleted or retained. This test file's
# own body necessarily contains the pattern (to search for it), so it
# excludes itself via a `:(exclude)` pathspec — as it does
# docs/legibility/confusion-codebook.yaml, which only quotes the filename as
# what confused an agent.

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

# legacy_config_ref_exclusions — the pathspecs the scan skips, one per line.
legacy_config_ref_exclusions() {
    printf '%s\n' ':(exclude)tests/infra/test_orchestrator_config_canonical_path.sh'
    # Mention, not use: dark-factory's machine-written agent-confusion corpus records
    # the filename as what confused an agent, and nothing reads it. No baseline heals
    # a red here. Same ruling as cited test paths: docs/legibility/landing-contract.md
    printf '%s\n' ':(exclude)docs/legibility/confusion-codebook.yaml'
}

# legacy_config_refs <root> — print `file:line:text` for every tracked line
# under <root> that names the legacy config filename; nothing when none does.
legacy_config_refs() {
    local root="$1"
    local -a excl
    mapfile -t excl < <(legacy_config_ref_exclusions)
    git -C "$root" grep -nP '(?<!dark-factory-)orchestrator\.yaml' -- . "${excl[@]}" || true
}

# PASSES iff the scan finds nothing on the real tree. Any match is echoed so
# the assert() harness dumps the offending file:line list on FAIL.
assert_no_legacy_config_refs() {
    local matches
    matches="$(legacy_config_refs "$REPO_ROOT")"
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

# ---------------------------------------------------------------------------
# (C) The machine-written confusion corpus is out of charter
# ---------------------------------------------------------------------------
echo ""
echo "--- (C) the machine-written confusion corpus is out of charter ---"

# Hermetic, the test_cited_test_paths_resolve.sh idiom: one run-private parent
# removed by a single EXIT trap, and a fixture repo built with user/system git
# config nulled. docs/elsewhere.md carries the identical mention, so (C)(b)
# cannot pass on a scan that finds nothing.
_RUN_TMP="$(mktemp -d "${TMPDIR:-/tmp}/reify-canonical-path-run.XXXXXX")"
trap 'rm -rf "$_RUN_TMP"' EXIT

_gitf() { GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null git "$@"; }

FIXTURE="$_RUN_TMP/repo"
LEGACY_MENTION='cat orchestrator.yaml -> No such file or directory'
_gitf init -q -b main "$FIXTURE"
_gitf -C "$FIXTURE" config user.email t@e.x
_gitf -C "$FIXTURE" config user.name t
mkdir -p "$FIXTURE/docs/legibility"
printf "    evidence_quote: '%s'\n" "$LEGACY_MENTION" \
    > "$FIXTURE/docs/legibility/confusion-codebook.yaml"
printf '%s\n' "$LEGACY_MENTION" > "$FIXTURE/docs/elsewhere.md"
_gitf -C "$FIXTURE" add -A
_gitf -C "$FIXTURE" commit -qm fixture

# _scan_reports <scan-output> <path> — PASSES iff the scan reported <path>.
_scan_reports() {
    local scan="$1" path="$2"
    grep -qF -- "$path" <<<"$scan" && return 0
    printf 'expected %s to be reported; the scan printed:\n%s\n' "$path" "${scan:-<nothing>}"
    return 1
}

# _scan_omits <scan-output> <path> — PASSES iff the scan did NOT report <path>.
_scan_omits() {
    local scan="$1" path="$2"
    grep -qF -- "$path" <<<"$scan" || return 0
    printf 'expected %s to be out of charter; the scan printed:\n%s\n' "$path" "$scan"
    return 1
}

FIXTURE_REFS="$(legacy_config_refs "$FIXTURE")"

assert "(C)(a) a legacy-filename mention in docs/elsewhere.md is reported" \
    _scan_reports "$FIXTURE_REFS" docs/elsewhere.md
assert "(C)(b) the identical mention inside docs/legibility/confusion-codebook.yaml is ignored" \
    _scan_omits "$FIXTURE_REFS" docs/legibility/confusion-codebook.yaml

test_summary
