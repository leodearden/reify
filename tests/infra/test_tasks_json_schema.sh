#!/usr/bin/env bash
# tests/infra/test_tasks_json_schema.sh
# Schema-invariant regression tests for .taskmaster/tasks/tasks.json.
#
# Drives scripts/validate_tasks_json.py with inline JSON fixtures via mktemp
# to verify that invariants 1-3 fire on bad input and pass on good input.
# Also validates the real tasks.json to catch any future drift.
#
# Part of Task 1888: tasks.json schema-invariant validator.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VALIDATOR="$REPO_ROOT/scripts/validate_tasks_json.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# -- Temp dir setup -----------------------------------------------------------
TMPDIR_FIXTURES="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_FIXTURES"' EXIT

echo "=== tasks.json schema-invariant tests ==="

# -- Prerequisite: validator script exists ------------------------------------
echo ""
echo "--- Test: validator script exists ---"

assert "validator script exists" \
    test -f "$VALIDATOR"

# -- Invariant 1: id must be a string matching ^\d+$ -------------------------
echo ""
echo "--- Test: invariant 1 (id type) ---"

# (a) Minimal valid fixture: id is a string.
VALID_ID="$TMPDIR_FIXTURES/valid_id.json"
cat >"$VALID_ID" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]}}
EOF

# (b) Bad fixture: id is an integer.
BAD_ID_TYPE="$TMPDIR_FIXTURES/bad_id_type.json"
cat >"$BAD_ID_TYPE" <<'EOF'
{"master":{"tasks":[{"id":1,"dependencies":[]}]}}
EOF

# (c) Bad fixture: id is a non-numeric string slug (e.g. "task-1").
#     Invariant 1 requires ^\d+$ so a slug must be rejected even though it's a str.
BAD_ID_SLUG="$TMPDIR_FIXTURES/bad_id_slug.json"
cat >"$BAD_ID_SLUG" <<'EOF'
{"master":{"tasks":[{"id":"task-1","dependencies":[]}]}}
EOF

assert "valid id passes validator" \
    python3 "$VALIDATOR" "$VALID_ID"

assert "int id fails validator" \
    bash -c "! python3 '$VALIDATOR' '$BAD_ID_TYPE'"

assert "int id error mentions 'id'" \
    bash -c "python3 '$VALIDATOR' '$BAD_ID_TYPE' 2>&1 | grep -q 'id'"

assert "slug id fails validator (numeric-only regex enforced)" \
    bash -c "! python3 '$VALIDATOR' '$BAD_ID_SLUG'"

# -- Invariant 2: deps must be strings referencing existing ids ---------------
echo ""
echo "--- Test: invariant 2 (dep type and orphan) ---"

# (a) Dep references a non-existent id (orphan).
ORPHAN_DEP="$TMPDIR_FIXTURES/orphan_dep.json"
cat >"$ORPHAN_DEP" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":["999"]}]}}
EOF

# (b) Dep is an integer (type drift).
INT_DEP="$TMPDIR_FIXTURES/int_dep.json"
cat >"$INT_DEP" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]},{"id":"2","dependencies":[1]}]}}
EOF

# (c) Valid non-empty dep: task "2" depends on existing task "1" — must pass.
#     This guards against a bug where all deps are flagged orphan regardless of
#     membership (regression not caught by failure-only fixtures).
VALID_DEP="$TMPDIR_FIXTURES/valid_dep.json"
cat >"$VALID_DEP" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]},{"id":"2","dependencies":["1"]}]}}
EOF

assert "orphan dep fails validator" \
    bash -c "! python3 '$VALIDATOR' '$ORPHAN_DEP'"

assert "orphan dep error mentions '999' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' '$ORPHAN_DEP' 2>&1 | grep -qE '999|orphan'"

assert "int dep fails validator" \
    bash -c "! python3 '$VALIDATOR' '$INT_DEP'"

assert "int dep error mentions dep or type" \
    bash -c "python3 '$VALIDATOR' '$INT_DEP' 2>&1 | grep -qiE 'dep|type|str'"

assert "valid non-empty dep passes validator" \
    python3 "$VALIDATOR" "$VALID_DEP"

# -- Invariant 3: no duplicate ids -------------------------------------------
echo ""
echo "--- Test: invariant 3 (duplicate ids) ---"

DUPLICATE_IDS="$TMPDIR_FIXTURES/duplicate_ids.json"
cat >"$DUPLICATE_IDS" <<'EOF'
{"master":{"tasks":[{"id":"5","dependencies":[]},{"id":"5","dependencies":[]}]}}
EOF

assert "duplicate ids fail validator" \
    bash -c "! python3 '$VALIDATOR' '$DUPLICATE_IDS'"

assert "duplicate ids error mentions 'duplicate' and '5'" \
    bash -c "python3 '$VALIDATOR' '$DUPLICATE_IDS' 2>&1 | grep -q 'duplicate' && python3 '$VALIDATOR' '$DUPLICATE_IDS' 2>&1 | grep -q '5'"

# -- Real-world sanity: current tasks.json must pass --------------------------
echo ""
echo "--- Test: real tasks.json passes schema ---"

assert "current .taskmaster/tasks/tasks.json passes schema" \
    python3 "$VALIDATOR" "$REPO_ROOT/.taskmaster/tasks/tasks.json"

# -- Subtask path: default-off behaviour (invariant 4 guarded) ----------------
echo ""
echo "--- Test: subtask checks (--check-subtasks flag, default off) ---"

# Valid subtask with string id — passes both with and without --check-subtasks.
VALID_SUBTASK="$TMPDIR_FIXTURES/valid_subtask.json"
cat >"$VALID_SUBTASK" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

# Bad subtask with numeric id — should PASS without --check-subtasks (default off),
# and FAIL with --check-subtasks.
BAD_SUBTASK="$TMPDIR_FIXTURES/bad_subtask.json"
cat >"$BAD_SUBTASK" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[],"subtasks":[{"id":1,"dependencies":[]}]}]}}
EOF

assert "valid subtask passes without --check-subtasks" \
    python3 "$VALIDATOR" "$VALID_SUBTASK"

assert "valid subtask passes with --check-subtasks" \
    python3 "$VALIDATOR" --check-subtasks "$VALID_SUBTASK"

# Default-off: bad subtask id passes WITHOUT the flag.
assert "bad subtask int id passes without --check-subtasks (default-off verified)" \
    python3 "$VALIDATOR" "$BAD_SUBTASK"

# Enabled: bad subtask id fails WITH the flag.
assert "bad subtask int id fails with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$BAD_SUBTASK'"

# Subtask invariant 2: orphan dep under --check-subtasks.
# Subtask "1" references dep "999" which does not exist as a sibling or parent
# task id.  This exercises the subtask branch of inv-2 (previously untested).
SUBTASK_ORPHAN_DEP="$TMPDIR_FIXTURES/subtask_orphan_dep.json"
cat >"$SUBTASK_ORPHAN_DEP" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[],"subtasks":[{"id":"1","dependencies":["999"]}]}]}}
EOF

assert "subtask orphan dep passes without --check-subtasks (default-off)" \
    python3 "$VALIDATOR" "$SUBTASK_ORPHAN_DEP"

assert "subtask orphan dep fails with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$SUBTASK_ORPHAN_DEP'"

assert "subtask orphan dep error mentions '999' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$SUBTASK_ORPHAN_DEP' 2>&1 | grep -qE '999|orphan'"

# -- Multi-tag support --------------------------------------------------------
echo ""
echo "--- Test: multi-tag support ---"

# (a) Valid multi-tag fixture: master + feature-x, both with valid tasks.
#     Should pass even though there are two tags.
MULTI_TAG_VALID="$TMPDIR_FIXTURES/multi_tag_valid.json"
cat >"$MULTI_TAG_VALID" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"feature-x":{"tasks":[{"id":"2","dependencies":[]}]}}
EOF

assert "valid multi-tag fixture passes validator" \
    python3 "$VALIDATOR" "$MULTI_TAG_VALID"

# (b) Multi-tag fixture: feature-x has a numeric id (invariant-1 violation).
#     Validator should fail and stderr should mention 'feature-x' so the tag
#     is identifiable in the error output.
MULTI_TAG_BAD_FX="$TMPDIR_FIXTURES/multi_tag_bad_fx.json"
cat >"$MULTI_TAG_BAD_FX" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"feature-x":{"tasks":[{"id":2,"dependencies":[]}]}}
EOF

assert "bad id in feature-x tag fails validator" \
    bash -c "! python3 '$VALIDATOR' '$MULTI_TAG_BAD_FX'"

assert "bad id error mentions 'feature-x' tag name" \
    bash -c "python3 '$VALIDATOR' '$MULTI_TAG_BAD_FX' 2>&1 | grep -q 'feature-x'"

# (c) Cross-tag dep fails as orphan: master task "2" depends on "99" which
#     only exists in feature-x.  Tags are independent namespaces — so "99" is
#     not a valid dep target within master.
#     Validator should fail and stderr should mention 'master' so the tag is
#     identifiable in the error output.
MULTI_TAG_CROSS_DEP="$TMPDIR_FIXTURES/multi_tag_cross_dep.json"
cat >"$MULTI_TAG_CROSS_DEP" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]},{"id":"2","dependencies":["99"]}]},"feature-x":{"tasks":[{"id":"99","dependencies":[]}]}}
EOF

assert "cross-tag orphan dep fails validator" \
    bash -c "! python3 '$VALIDATOR' '$MULTI_TAG_CROSS_DEP'"

assert "cross-tag orphan dep error mentions 'master' tag name" \
    bash -c "python3 '$VALIDATOR' '$MULTI_TAG_CROSS_DEP' 2>&1 | grep -q 'master'"

# -- Summary ------------------------------------------------------------------
test_summary
