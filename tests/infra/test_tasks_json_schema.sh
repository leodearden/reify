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

# -- Invariant 2 (dotted form): <parent>.<subtask> deps -----------------------
# Top-level deps may take the form "<parent>.<subtask>" iff parent is a known
# top-level id AND subtask exists under that parent's subtasks[].  This
# tolerance accommodates tm-core legacy data where parents were briefly listed
# as depending on their own subtasks.
echo ""
echo "--- Test: invariant 2 (dotted <parent>.<subtask> deps) ---"

# (a) Dotted dep where parent and subtask both resolve → must PASS.
DOTTED_DEP_VALID="$TMPDIR_FIXTURES/dotted_dep_valid.json"
cat >"$DOTTED_DEP_VALID" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":["200.1"],"subtasks":[]},{"id":"200","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

assert "valid dotted <parent>.<subtask> dep passes validator" \
    python3 "$VALIDATOR" "$DOTTED_DEP_VALID"

# (b) Dotted dep where parent does not exist → must FAIL as orphan.
DOTTED_DEP_BAD_PARENT="$TMPDIR_FIXTURES/dotted_dep_bad_parent.json"
cat >"$DOTTED_DEP_BAD_PARENT" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":["999.1"],"subtasks":[]}]}}
EOF

assert "dotted dep with missing parent fails validator" \
    bash -c "! python3 '$VALIDATOR' '$DOTTED_DEP_BAD_PARENT'"

assert "dotted dep with missing parent error mentions '999.1' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' '$DOTTED_DEP_BAD_PARENT' 2>&1 | grep -qE '999\\.1|orphan'"

# (c) Dotted dep where subtask does not exist under parent → must FAIL as orphan.
DOTTED_DEP_BAD_SUBTASK="$TMPDIR_FIXTURES/dotted_dep_bad_subtask.json"
cat >"$DOTTED_DEP_BAD_SUBTASK" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":["200.99"],"subtasks":[]},{"id":"200","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

assert "dotted dep with missing subtask fails validator" \
    bash -c "! python3 '$VALIDATOR' '$DOTTED_DEP_BAD_SUBTASK'"

assert "dotted dep with missing subtask error mentions '200.99' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' '$DOTTED_DEP_BAD_SUBTASK' 2>&1 | grep -qE '200\\.99|orphan'"

# (d) Malformed dotted dep with extra dot ("100.1.2") → must FAIL as orphan.
#     Locks the re.fullmatch boundary in _dotted_dep_resolves: the dotted-dep
#     regex must reject multi-dot forms so they fall through to the orphan
#     branch instead of being silently accepted as a parent.subtask reference.
DOTTED_DEP_MALFORMED="$TMPDIR_FIXTURES/dotted_dep_malformed.json"
cat >"$DOTTED_DEP_MALFORMED" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":["100.1.2"],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

assert "malformed dotted dep '100.1.2' fails validator (fullmatch boundary)" \
    bash -c "! python3 '$VALIDATOR' '$DOTTED_DEP_MALFORMED'"

assert "malformed dotted dep error mentions '100.1.2' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' '$DOTTED_DEP_MALFORMED' 2>&1 | grep -qE '100\\.1\\.2|orphan'"

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

assert "current .taskmaster/tasks/tasks.json passes validator subtask invariants (explicit --check-subtasks)" \
    python3 "$VALIDATOR" --check-subtasks "$REPO_ROOT/.taskmaster/tasks/tasks.json"

# -- Subtask path: default-on behaviour (--no-check-subtasks escape hatch) ----
echo ""
echo "--- Test: subtask checks (--check-subtasks default on, --no-check-subtasks escape hatch) ---"

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

# Default-on: bad subtask id FAILS without any flag (default-on guard).
assert "bad subtask int id FAILS by default (default-on)" \
    bash -c "! python3 '$VALIDATOR' '$BAD_SUBTASK'"

assert "bad subtask int id passes with --no-check-subtasks (escape hatch)" \
    python3 "$VALIDATOR" --no-check-subtasks "$BAD_SUBTASK"

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

assert "subtask orphan dep FAILS by default (default-on)" \
    bash -c "! python3 '$VALIDATOR' '$SUBTASK_ORPHAN_DEP'"

assert "subtask orphan dep passes with --no-check-subtasks (escape hatch)" \
    python3 "$VALIDATOR" --no-check-subtasks "$SUBTASK_ORPHAN_DEP"

assert "subtask orphan dep fails with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$SUBTASK_ORPHAN_DEP'"

assert "subtask orphan dep error mentions '999' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$SUBTASK_ORPHAN_DEP' 2>&1 | grep -qE '999|orphan'"

# -- Subtask invariant 2 (dotted form): <parent>.<subtask> deps ---------------
# A subtask's dep may also take the dotted ``<parent>.<subtask>`` form iff the
# parent is a known top-level id AND the subtask id exists under that parent.
# These fixtures exercise the dotted-dep branch in _validate_subtasks, which
# is a different code path from the top-level dotted-dep tests above (those
# tests only reach _validate_tasks).
echo ""
echo "--- Test: subtask dotted <parent>.<subtask> deps ---"

# (a) Dotted dep from a subtask where parent and subtask both resolve → must PASS.
SUBTASK_DOTTED_DEP_VALID="$TMPDIR_FIXTURES/subtask_dotted_dep_valid.json"
cat >"$SUBTASK_DOTTED_DEP_VALID" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":[],"subtasks":[{"id":"1","dependencies":["200.1"]}]},{"id":"200","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

assert "valid subtask dotted <parent>.<subtask> dep passes with --check-subtasks" \
    python3 "$VALIDATOR" --check-subtasks "$SUBTASK_DOTTED_DEP_VALID"

# (b) Dotted dep from subtask where parent does not exist → must FAIL as orphan.
SUBTASK_DOTTED_DEP_BAD_PARENT="$TMPDIR_FIXTURES/subtask_dotted_dep_bad_parent.json"
cat >"$SUBTASK_DOTTED_DEP_BAD_PARENT" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":[],"subtasks":[{"id":"1","dependencies":["999.1"]}]}]}}
EOF

assert "subtask dotted dep with missing parent fails validator" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$SUBTASK_DOTTED_DEP_BAD_PARENT'"

assert "subtask dotted dep with missing parent error mentions '999.1' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$SUBTASK_DOTTED_DEP_BAD_PARENT' 2>&1 | grep -qE '999\\.1|orphan'"

# (c) Dotted dep from subtask where subtask id does not exist under parent → must FAIL.
SUBTASK_DOTTED_DEP_BAD_SUBTASK="$TMPDIR_FIXTURES/subtask_dotted_dep_bad_subtask.json"
cat >"$SUBTASK_DOTTED_DEP_BAD_SUBTASK" <<'EOF'
{"master":{"tasks":[{"id":"100","dependencies":[],"subtasks":[{"id":"1","dependencies":["200.99"]}]},{"id":"200","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]}}
EOF

assert "subtask dotted dep with missing subtask fails validator" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$SUBTASK_DOTTED_DEP_BAD_SUBTASK'"

assert "subtask dotted dep with missing subtask error mentions '200.99' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$SUBTASK_DOTTED_DEP_BAD_SUBTASK' 2>&1 | grep -qE '200\\.99|orphan'"

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

# (d) Same task id reused across sibling tags is NOT a duplicate.  Tags are
#     independent namespaces, so id "1" appearing in both master and feature-x
#     must pass.  This is the positive twin of case (c) above.
CROSS_TAG_ID_REUSE="$TMPDIR_FIXTURES/cross_tag_id_reuse.json"
cat >"$CROSS_TAG_ID_REUSE" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"feature-x":{"tasks":[{"id":"1","dependencies":[]}]}}
EOF

assert "same id in sibling tags passes validator (independent namespaces)" \
    python3 "$VALIDATOR" "$CROSS_TAG_ID_REUSE"

assert "same id in sibling tags does NOT produce 'duplicate' on stderr" \
    bash -c "! python3 '$VALIDATOR' '$CROSS_TAG_ID_REUSE' 2>&1 | grep -qi 'duplicate'"

# -- Unexpected top-level keys emit warnings ----------------------------------
echo ""
echo "--- Test: unexpected top-level keys emit warnings ---"

# (a) Top-level value is a non-dict (string).  Validator should exit 0 (no
#     task errors) but print a WARN line on stderr mentioning the key.
WARN_NON_DICT="$TMPDIR_FIXTURES/warn_non_dict.json"
cat >"$WARN_NON_DICT" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"bad_key":"not-a-dict"}
EOF

assert "non-dict top-level value passes validator (exit 0)" \
    python3 "$VALIDATOR" "$WARN_NON_DICT"

assert "non-dict top-level value emits WARN on stderr" \
    bash -c "python3 '$VALIDATOR' '$WARN_NON_DICT' 2>&1 | grep -q 'WARN'"

assert "non-dict top-level value WARN mentions bad_key" \
    bash -c "python3 '$VALIDATOR' '$WARN_NON_DICT' 2>&1 | grep -q 'bad_key'"

# (b) Top-level value is a dict but has no 'tasks' field.
WARN_NO_TASKS="$TMPDIR_FIXTURES/warn_no_tasks.json"
cat >"$WARN_NO_TASKS" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"bad_key":{"no_tasks_field":true}}
EOF

assert "dict without tasks field passes validator (exit 0)" \
    python3 "$VALIDATOR" "$WARN_NO_TASKS"

assert "dict without tasks field emits WARN on stderr" \
    bash -c "python3 '$VALIDATOR' '$WARN_NO_TASKS' 2>&1 | grep -q 'WARN'"

assert "dict without tasks field WARN mentions bad_key" \
    bash -c "python3 '$VALIDATOR' '$WARN_NO_TASKS' 2>&1 | grep -q 'bad_key'"

# (c) Top-level value is a dict with tasks set to a non-list value.
WARN_TASKS_NOT_LIST="$TMPDIR_FIXTURES/warn_tasks_not_list.json"
cat >"$WARN_TASKS_NOT_LIST" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"bad_key":{"tasks":"not-a-list"}}
EOF

assert "tasks-not-list top-level value passes validator (exit 0)" \
    python3 "$VALIDATOR" "$WARN_TASKS_NOT_LIST"

assert "tasks-not-list top-level value emits WARN on stderr" \
    bash -c "python3 '$VALIDATOR' '$WARN_TASKS_NOT_LIST' 2>&1 | grep -q 'WARN'"

assert "tasks-not-list top-level value WARN mentions bad_key" \
    bash -c "python3 '$VALIDATOR' '$WARN_TASKS_NOT_LIST' 2>&1 | grep -q 'bad_key'"

# (d) Top-level key is underscore-prefixed (_meta).  Validator should exit 0
#     and emit NOTHING on stderr — silent skip, not warn-and-skip.  This pins
#     the negative space of cases (a)-(c): those all WARN; this does NOT.
META_UNDERSCORE="$TMPDIR_FIXTURES/meta_underscore.json"
cat >"$META_UNDERSCORE" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"_meta":{"version":"1.0"}}
EOF

assert "_meta key passes validator (exit 0)" \
    python3 "$VALIDATOR" "$META_UNDERSCORE"

assert "_meta key does NOT emit WARN on stderr (silent skip)" \
    bash -c "! python3 '$VALIDATOR' '$META_UNDERSCORE' 2>&1 | grep -q 'WARN'"

assert "_meta key does NOT mention _meta on stderr" \
    bash -c "! python3 '$VALIDATOR' '$META_UNDERSCORE' 2>&1 | grep -q '_meta'"

# -- Multi-tag --check-subtasks support --------------------------------------
echo ""
echo "--- Test: multi-tag --check-subtasks support ---"

# (a) master has valid subtasks, feature-x has a subtask with a numeric id.
#     Without --check-subtasks: passes (default-off preserved).
#     With --check-subtasks: fails and stderr mentions 'feature-x' so the tag
#     is identifiable in the subtask error output.
MULTI_TAG_BAD_SUBTASK_FX="$TMPDIR_FIXTURES/multi_tag_bad_subtask_fx.json"
cat >"$MULTI_TAG_BAD_SUBTASK_FX" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[],"subtasks":[{"id":"1","dependencies":[]}]}]},"feature-x":{"tasks":[{"id":"2","dependencies":[],"subtasks":[{"id":99,"dependencies":[]}]}]}}
EOF

assert "multi-tag bad subtask (feature-x) FAILS by default" \
    bash -c "! python3 '$VALIDATOR' '$MULTI_TAG_BAD_SUBTASK_FX'"

assert "multi-tag bad subtask passes with --no-check-subtasks (escape hatch)" \
    python3 "$VALIDATOR" --no-check-subtasks "$MULTI_TAG_BAD_SUBTASK_FX"

assert "multi-tag bad subtask (feature-x) fails with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$MULTI_TAG_BAD_SUBTASK_FX'"

assert "multi-tag bad subtask error mentions 'feature-x' tag name" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$MULTI_TAG_BAD_SUBTASK_FX' 2>&1 | grep -q 'feature-x'"

# (b) feature-x has a subtask with an orphan dep "999" (not a sibling subtask
#     id or parent task id).  With --check-subtasks: fails and stderr mentions
#     'feature-x' and '999' or 'orphan'.
MULTI_TAG_SUBTASK_ORPHAN_FX="$TMPDIR_FIXTURES/multi_tag_subtask_orphan_fx.json"
cat >"$MULTI_TAG_SUBTASK_ORPHAN_FX" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[]}]},"feature-x":{"tasks":[{"id":"2","dependencies":[],"subtasks":[{"id":"1","dependencies":["999"]}]}]}}
EOF

assert "multi-tag subtask orphan dep fails with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$MULTI_TAG_SUBTASK_ORPHAN_FX'"

assert "multi-tag subtask orphan dep error mentions 'feature-x' tag name" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$MULTI_TAG_SUBTASK_ORPHAN_FX' 2>&1 | grep -q 'feature-x'"

assert "multi-tag subtask orphan dep error mentions '999' or 'orphan'" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$MULTI_TAG_SUBTASK_ORPHAN_FX' 2>&1 | grep -qE '999|orphan'"

# -- Malformed top-level schema (not a dict) ----------------------------------
echo ""
echo "--- Test: malformed top-level schema (not a dict) ---"

# (a) Top-level is a JSON array.  The validator must exit nonzero with a clean
#     error message — NOT a raw AttributeError traceback.
NON_DICT_TOP_LEVEL="$TMPDIR_FIXTURES/non_dict_top_level.json"
cat >"$NON_DICT_TOP_LEVEL" <<'EOF'
[]
EOF

assert "non-dict top-level fails validator" \
    bash -c "! python3 '$VALIDATOR' '$NON_DICT_TOP_LEVEL'"

assert "non-dict top-level error mentions schema or object or dict" \
    bash -c "python3 '$VALIDATOR' '$NON_DICT_TOP_LEVEL' 2>&1 | grep -qE 'schema|object|dict'"

assert "non-dict top-level does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' '$NON_DICT_TOP_LEVEL' 2>&1 | grep -q 'Traceback'"

# (b) Top-level is JSON null.  Same expectations as array case.
NULL_TOP_LEVEL="$TMPDIR_FIXTURES/null_top_level.json"
cat >"$NULL_TOP_LEVEL" <<'EOF'
null
EOF

assert "null top-level fails validator" \
    bash -c "! python3 '$VALIDATOR' '$NULL_TOP_LEVEL'"

assert "null top-level error mentions schema or object or dict" \
    bash -c "python3 '$VALIDATOR' '$NULL_TOP_LEVEL' 2>&1 | grep -qE 'schema|object|dict'"

assert "null top-level does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' '$NULL_TOP_LEVEL' 2>&1 | grep -q 'Traceback'"

# -- Unhashable id (invariant-3 Counter hardening) ----------------------------
echo ""
echo "--- Test: unhashable id (invariant-3 Counter hardening) ---"

# A task whose id is a JSON list (['1','2']).  collections.Counter crashes on
# unhashable types; the validator must exit nonzero with a clean invariant-1
# error and NO traceback.
UNHASHABLE_ID_LIST="$TMPDIR_FIXTURES/unhashable_id_list.json"
cat >"$UNHASHABLE_ID_LIST" <<'EOF'
{"master":{"tasks":[{"id":["1","2"],"dependencies":[]}]}}
EOF

assert "unhashable list id fails validator" \
    bash -c "! python3 '$VALIDATOR' '$UNHASHABLE_ID_LIST'"

assert "unhashable list id error mentions invariant 1 or expected str" \
    bash -c "python3 '$VALIDATOR' '$UNHASHABLE_ID_LIST' 2>&1 | grep -qiE 'invariant 1|expected str'"

assert "unhashable list id does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' '$UNHASHABLE_ID_LIST' 2>&1 | grep -q 'Traceback'"

# A subtask whose id is a JSON object ({nested:true}).  Same crash path but in
# _validate_subtasks Counter (line 178).  Must be tested with --check-subtasks.
UNHASHABLE_SUBTASK_ID="$TMPDIR_FIXTURES/unhashable_subtask_id.json"
cat >"$UNHASHABLE_SUBTASK_ID" <<'EOF'
{"master":{"tasks":[{"id":"1","dependencies":[],"subtasks":[{"id":{"nested":true},"dependencies":[]}]}]}}
EOF

assert "unhashable dict subtask id fails validator with --check-subtasks" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$UNHASHABLE_SUBTASK_ID'"

assert "unhashable dict subtask id error mentions invariant 1 or expected str" \
    bash -c "python3 '$VALIDATOR' --check-subtasks '$UNHASHABLE_SUBTASK_ID' 2>&1 | grep -qiE 'invariant 1|expected str'"

assert "unhashable dict subtask id does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' --check-subtasks '$UNHASHABLE_SUBTASK_ID' 2>&1 | grep -q 'Traceback'"

# -- Sole-tag malformation is a schema error ----------------------------------
# When EVERY top-level key is WARN-skipped (all have malformed shapes) and no
# valid tag namespace was found, the validator must exit 1 — not silently exit 0.
# This is distinct from the multi-key fixtures above (lines 222-269) which all
# have a valid 'master' tag alongside the malformed secondary key.
echo ""
echo "--- Test: sole-tag malformation is a schema error ---"

SOLE_TAG_NON_DICT="$TMPDIR_FIXTURES/sole_tag_non_dict.json"
cat >"$SOLE_TAG_NON_DICT" <<'EOF'
{"master":"not-a-dict"}
EOF

assert "sole non-dict tag fails validator" \
    bash -c "! python3 '$VALIDATOR' '$SOLE_TAG_NON_DICT'"

assert "sole non-dict tag error mentions schema or tag or no valid" \
    bash -c "python3 '$VALIDATOR' '$SOLE_TAG_NON_DICT' 2>&1 | grep -qiE 'schema|no valid'"

assert "sole non-dict tag does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' '$SOLE_TAG_NON_DICT' 2>&1 | grep -q 'Traceback'"

SOLE_TAG_TASKS_NOT_LIST="$TMPDIR_FIXTURES/sole_tag_tasks_not_list.json"
cat >"$SOLE_TAG_TASKS_NOT_LIST" <<'EOF'
{"master":{"tasks":"not-a-list"}}
EOF

assert "sole tag-with-non-list-tasks fails validator" \
    bash -c "! python3 '$VALIDATOR' '$SOLE_TAG_TASKS_NOT_LIST'"

assert "sole tag-with-non-list-tasks error mentions schema or no valid" \
    bash -c "python3 '$VALIDATOR' '$SOLE_TAG_TASKS_NOT_LIST' 2>&1 | grep -qiE 'schema|no valid'"

assert "sole tag-with-non-list-tasks does NOT leak Traceback" \
    bash -c "! python3 '$VALIDATOR' '$SOLE_TAG_TASKS_NOT_LIST' 2>&1 | grep -q 'Traceback'"

# -- Summary ------------------------------------------------------------------
test_summary
