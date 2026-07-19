#!/usr/bin/env bash
# scripts/check-infra-classification-manifest.sh — mechanical early gate (task 5252).
#
# Fails the build EARLY (naming the offending file) whenever a
# tests/infra/test_*.sh exists on the tree with NO row in
# tests/infra/run-all-classification.manifest — or, the reverse direction, a
# manifest row whose test file no longer exists. This is the MECHANICAL
# enforcement the prompt-only /prd authoring reminder (task 5160) could not
# provide: it recurred twice post-landing (tasks 5247 & 5249; see esc-5252-1).
#
# Shared derivation (cannot silently diverge by construction): this gate
# sources tests/infra/run-all-classification-lib.sh and consumes
# classification_undeclared / classification_orphaned — the SAME library the
# pool drift test (tests/infra/test_run_all_classification.sh Test 5) derives
# from. Catching BOTH drift directions makes the gate a strict superset of that
# pool test, so this early, clearly-attributed check is always the FIRST to
# fail — the late pool test can never be the one to surface the drift. Same
# shared-lib pattern as scripts/release-scope-lib.sh (sourced by both
# scripts/verify.sh and tests/infra/test_release_scoped_scope.sh).
#
# verify.sh runs this as the FIRST plan entry when Rust work is in scope
# (RUN_RUST=1), before check-manifold-deps.sh and any compile. Fast: pure bash
# + filesystem, no cargo. Honors RUN_ALL_CLASSIFICATION_MANIFEST (via the lib)
# for testability.
#
# Exit 0 when clean; exit 1 with a clear per-file message otherwise.
set -euo pipefail

err() { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="$REPO_ROOT/tests/infra/run-all-classification-lib.sh"
MANIFEST_REL="tests/infra/run-all-classification.manifest"

if [ ! -f "$LIB" ]; then
    err "infra-classification gate: classification lib not found at $LIB"
    exit 1
fi
# shellcheck source=tests/infra/run-all-classification-lib.sh
source "$LIB"

# Refuse to run against a MISSING manifest instead of passing vacuously. The
# lib's accessors gracefully degrade to empty output when the manifest is absent
# (`[ -f ] || return 0`), so a DELETED or renamed manifest would make BOTH
# classification_undeclared and classification_orphaned empty — the gate would
# report "clean" precisely when EVERY discovered tests/infra/test_*.sh is
# undeclared (the single largest drift this gate exists to catch). Resolve the
# path THROUGH the lib (classification_manifest_path) so the
# RUN_ALL_CLASSIFICATION_MANIFEST override is honored — the check then targets
# the exact file the accessors will read, closing the hole under the override
# too, not only for the default path.
manifest_path="$(classification_manifest_path)"
if [ ! -f "$manifest_path" ]; then
    err "infra-classification gate: manifest not found at $manifest_path"
    err "  a deleted/renamed $MANIFEST_REL would let every tests/infra/test_*.sh"
    err "  pass this gate vacuously — refusing to run without the manifest."
    exit 1
fi

undeclared="$(classification_undeclared)"
orphaned="$(classification_orphaned)"

status=0

if [ -n "$undeclared" ]; then
    status=1
    err "tests/infra classification drift — test file(s) present on disk with NO $MANIFEST_REL row:"
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        err "  tests/infra/$f exists but has no $MANIFEST_REL row"
        err "    -> add a '$f <bucket>' row (bucket: pool | intra-run-serial | host-exclusive)"
    done <<< "$undeclared"
fi

if [ -n "$orphaned" ]; then
    status=1
    err "tests/infra classification drift — $MANIFEST_REL row(s) with NO matching test file:"
    while IFS= read -r f; do
        [ -z "$f" ] && continue
        err "  $MANIFEST_REL declares '$f' but tests/infra/$f does not exist"
        err "    -> remove the stale '$f' row, or restore the file"
    done <<< "$orphaned"
fi

if [ "$status" -ne 0 ]; then
    err "This mechanical gate (task 5252) derives from the same"
    err "tests/infra/run-all-classification-lib.sh as test_run_all_classification.sh."
    err "See $MANIFEST_REL and docs/prds/run-all-host-infra-partition.md §3."
    exit 1
fi

exit 0
