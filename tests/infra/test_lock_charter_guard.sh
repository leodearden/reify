#!/usr/bin/env bash
# tests/infra/test_lock_charter_guard.sh — TDD harness for scripts/lock-charter-guard.sh
#
# Drives the lock-charter guard in isolation.  Tests the syntactic
# directory-vs-file predicate (C-P1..C-P4) required by the task-lock-charter-
# lifecycle PRD (docs/prds/task-lock-charter-lifecycle.md §4.1).
#
# No skip guard: the predicate is host-independent (C-P3 — pure string, no
# filesystem stat, no model call), so the test runs on every host.
#
# Auto-discovered by tests/infra/run_all.sh (glob test_*.sh).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/lock-charter-guard.sh"

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || {
    echo "ERROR: tests/infra/test_helpers.sh not found at $REPO_ROOT/tests/infra/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$REPO_ROOT/tests/infra/test_helpers.sh"

# ---------------------------------------------------------------------------
# Harness helper — run classify, capture exit code + stdout
# ---------------------------------------------------------------------------
GUARD_RC=0
GUARD_OUT=""

run_classify() {
    local path="$1"
    GUARD_OUT="$(bash "$SCRIPT" classify "$path" 2>/dev/null)" && GUARD_RC=$? || GUARD_RC=$?
}

run_check() {
    GUARD_OUT="$(bash "$SCRIPT" check "$@" 2>/dev/null)" && GUARD_RC=$? || GUARD_RC=$?
}

run_check_stdin() {
    GUARD_OUT="$(bash "$SCRIPT" check 2>/dev/null <<STDIN_EOF
$1
STDIN_EOF
)" && GUARD_RC=$? || GUARD_RC=$?
}

run_list_extensions() {
    GUARD_OUT="$(bash "$SCRIPT" --list-extensions 2>/dev/null)" && GUARD_RC=$? || GUARD_RC=$?
}

# ---------------------------------------------------------------------------
# Set up a temp dir for C-P3 on-disk probes; cleaned up on exit.
# ---------------------------------------------------------------------------
TMPWORK="$(mktemp -d)"
trap 'rm -rf "$TMPWORK"' EXIT

# ---------------------------------------------------------------------------
# Cycle 1 — SCRIPT exists & is executable; C-P1 REJECT corpus; .rs ACCEPT;
#            C-P4 deep file path; C-P3 no-stat/determinism; usage exit 2
# ---------------------------------------------------------------------------
echo "--- Cycle 1: SCRIPT exists, REJECT corpus, .rs ACCEPT, C-P3/C-P4 ---"

# (A) SCRIPT exists and is executable
assert "SCRIPT exists" test -f "$SCRIPT"
assert "SCRIPT is executable" test -x "$SCRIPT"

# (B) C-P1 REJECT corpus — directory-shaped paths (G6: observe rejection FIRING)
for _dir_path in \
    "crates/" \
    "crates/reify-eval/src" \
    "crates/reify-eval/tests" \
    "examples" \
    "compute_targets" \
    "modal" \
    "crates/reify-eval/src/" \
    "a/b/c/"
do
    run_classify "$_dir_path"
    assert "classify '$_dir_path' exits 1 (REJECT)" test "$GUARD_RC" -eq 1
    assert "classify '$_dir_path' stdout contains REJECT" test "${GUARD_OUT#*REJECT}" != "$GUARD_OUT"
done

# (C) ACCEPT sanity — .rs file path
run_classify "crates/foo/src/bar.rs"
assert "classify 'crates/foo/src/bar.rs' exits 0 (ACCEPT)" test "$GUARD_RC" -eq 0
assert "classify 'crates/foo/src/bar.rs' stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"

# (D) C-P4 — deep file path accepted despite lock_depth-related segment names
run_classify "a/b/compute_targets/foo.rs"
assert "C-P4: deep file a/b/compute_targets/foo.rs exits 0 (ACCEPT)" test "$GUARD_RC" -eq 0
assert "C-P4: deep file a/b/compute_targets/foo.rs stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"

# (E) C-P3 no-stat / determinism
# (E1) Non-existent .rs path → exit 0 ACCEPT (no test -f/-e)
_ghost_path="no/such/path/ghost.rs"
run_classify "$_ghost_path"
assert "C-P3 E1: non-existent .rs path exits 0 (no test -f)" test "$GUARD_RC" -eq 0
assert "C-P3 E1: non-existent .rs path stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"

# (E2) Real on-disk directory named x.rs → exit 0 ACCEPT (no test -d)
mkdir -p "$TMPWORK/x.rs"
run_classify "$TMPWORK/x.rs"
assert "C-P3 E2: real dir named x.rs exits 0 (no test -d)" test "$GUARD_RC" -eq 0
assert "C-P3 E2: real dir named x.rs stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"

# (E3) Two successive classify runs produce byte-identical stdout + exit
_out1="$(bash "$SCRIPT" classify "crates/" 2>/dev/null)" && _rc1=$? || _rc1=$?
_out2="$(bash "$SCRIPT" classify "crates/" 2>/dev/null)" && _rc2=$? || _rc2=$?
assert "C-P3 E3: successive runs same exit code" test "$_rc1" -eq "$_rc2"
assert "C-P3 E3: successive runs same stdout" test "$_out1" = "$_out2"

# (F) Unknown subcommand → exit 2
bash "$SCRIPT" bogus >/dev/null 2>&1 && _bogus_rc=$? || _bogus_rc=$?
assert "unknown subcommand 'bogus' exits 2" test "$_bogus_rc" -eq 2

# (G) classify with missing/empty path → exit 2 (argument-validation contract)
bash "$SCRIPT" classify >/dev/null 2>&1 && _rc_nop=$? || _rc_nop=$?
assert "classify with no path exits 2" test "$_rc_nop" -eq 2

bash "$SCRIPT" classify "" >/dev/null 2>&1 && _rc_empty=$? || _rc_empty=$?
assert "classify with empty path exits 2" test "$_rc_empty" -eq 2

# ---------------------------------------------------------------------------
# Cycle 2 — Full OQ#2 extension allowlist (C-P2 accept side)
# step-3: verify RED with seed impl; step-4 GREEN by expanding _EXTS.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 2: Full OQ#2 extension allowlist ACCEPT ---"

for _ext_path in \
    "examples/foo.ri" \
    "crates/x/Cargo.toml" \
    "k/x.cpp" \
    "k/y.c" \
    "k/z.h" \
    "k/w.hpp" \
    "notes.md" \
    "data.json" \
    "conf.yaml" \
    "conf.yml" \
    "Cargo.lock" \
    "mod.py" \
    "run.sh" \
    "a.ts" \
    "b.tsx" \
    "c.js" \
    "d.txt" \
    "part.step" \
    "mesh.stl" \
    "gui/src/styles/main.css" \
    "scripts/tool.mjs" \
    "page.html" \
    "cfg.jsonc" \
    "out/part.gcode" \
    "units/orchestrator.service" \
    "k/a.cc" \
    "k/b.cxx" \
    "k/c.hh" \
    "m.mts" \
    "n.cts" \
    "o.cjs" \
    "p.jsx" \
    "s.scss" \
    "icon.svg" \
    "logo.png"
do
    run_classify "$_ext_path"
    assert "classify '$_ext_path' exits 0 (ACCEPT)" test "$GUARD_RC" -eq 0
    assert "classify '$_ext_path' stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"
done

# ---------------------------------------------------------------------------
# Cycle 3 — check list-gate (C-P2 list + [] empty-accept)
# step-5: verify RED with seed impl; step-6 GREEN (check already scaffolded).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 3: check list-gate (all-file / mixed / empty / stdin) ---"

# (A) All-file list → exit 0
run_check "crates/x/src/a.rs" "examples/b.ri"
assert "check all-file list exits 0" test "$GUARD_RC" -eq 0

# (B) Mixed list → exit 1; both rejected dirs appear in stdout (G6)
run_check "crates/x/src/a.rs" "crates/" "compute_targets"
assert "check mixed list exits 1" test "$GUARD_RC" -eq 1
assert "check mixed list stdout contains 'REJECT crates/'" test "${GUARD_OUT#*REJECT crates/}" != "$GUARD_OUT"
assert "check mixed list stdout contains 'REJECT compute_targets'" test "${GUARD_OUT#*REJECT compute_targets}" != "$GUARD_OUT"

# (C) Empty input ([] defer-to-architect) → exit 0
run_check </dev/null
assert "check zero args + empty stdin exits 0" test "$GUARD_RC" -eq 0

GUARD_OUT="$(printf '' | bash "$SCRIPT" check 2>/dev/null)" && GUARD_RC=$? || GUARD_RC=$?
assert "check empty pipe exits 0" test "$GUARD_RC" -eq 0

# (D) Stdin parity
GUARD_OUT="$(printf 'crates/x/src/a.rs\nexamples/b.ri\n' | bash "$SCRIPT" check 2>/dev/null)" \
    && GUARD_RC=$? || GUARD_RC=$?
assert "check stdin all-file exits 0" test "$GUARD_RC" -eq 0

GUARD_OUT="$(printf 'crates/x/src/a.rs\ncrates/\n' | bash "$SCRIPT" check 2>/dev/null)" \
    && GUARD_RC=$? || GUARD_RC=$?
assert "check stdin mixed exits 1" test "$GUARD_RC" -eq 1

# ---------------------------------------------------------------------------
# Cycle 4 — --list-extensions drift guard + coherence
# step-7: verify RED with seed impl; step-8 GREEN (--list-extensions scaffolded in step-2,
# output matches canonical after step-4 full allowlist expansion).
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 4: --list-extensions drift guard + coherence ---"

# Canonical OQ#2 allowlist — sorted-unique, one extension per line.
# This is the pinned shared α/γ test vector (PRD §11 Q1).
CANONICAL_EXTS="c
cc
cjs
conf
cpp
css
cts
cxx
diff
envrc
example
example-systemd-config
gcode
gitattributes
gitignore
gitkeep
gitmodules
golden
grammar
h
hh
hpp
html
icns
ico
jq
js
json
jsonc
jsonl
jsx
lock
log
manifest
md
mjs
mts
npmrc
png
py
python-version
ri
rs
scss
service
sh
step
stl
svg
template
timer
toml
ts
tsx
txt
typed
yaml
yml"

run_list_extensions
assert "--list-extensions exits 0" test "$GUARD_RC" -eq 0
assert "--list-extensions stdout matches canonical allowlist" test "$GUARD_OUT" = "$CANONICAL_EXTS"

# Coherence: every listed extension is ACCEPTed by classify
while IFS= read -r _ext; do
    [ -z "$_ext" ] && continue
    run_classify "f.$_ext"
    assert "--list-extensions coherence: classify 'f.$_ext' exits 0" test "$GUARD_RC" -eq 0
done <<< "$GUARD_OUT"

# ---------------------------------------------------------------------------
# Cycle 5 — newly-allowlisted extensions ACCEPT (#5726)
#
# 22 extensions that a git-ls-files sweep found on real tracked files across
# reify + dark-factory, but which _EXTS misclassified as directories.  The
# originating symptom: declaring tests/infra/run-all-classification.manifest in
# a lock charter was REJECTed as a directory.
#
# The 12 reify-evidenced extensions below use REAL tracked paths (verified with
# git ls-files --error-unmatch).  The other 10 are dark-factory-evidenced and
# use literal path strings — valid inputs because C-P3 forbids any stat, the
# same property Cycle 1 case E1 relies on.
#
# step-1: verify RED; step-2 GREEN by expanding _EXTS.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 5: newly-allowlisted extensions ACCEPT (#5726) ---"

for _ext_path in \
    "deploy/systemd/orchestrator-reify.service.d/warm-lane.conf" \
    ".envrc" \
    ".gitignore" \
    "crates/reify-doc/tests/snapshots/.gitkeep" \
    "crates/reify-fdm/tests/fixtures/toolpath_bracket.golden" \
    "gui/src/editor/reify.grammar" \
    "gui/src-tauri/icons/icon.icns" \
    "gui/src-tauri/icons/icon.ico" \
    "scripts/reify-audit-snapshot-filter.jq" \
    "tests/infra/run-all-classification.manifest" \
    "tree-sitter-reify/.npmrc" \
    "deploy/systemd/reify-warm-lane-gc.timer" \
    "orchestrator/tests/fixtures/expected.diff" \
    ".env.example" \
    "deploy/orchestrator.example-systemd-config" \
    ".gitattributes" \
    ".gitmodules" \
    "logs/agent-events.jsonl" \
    "logs/orchestrator.log" \
    ".python-version" \
    "orchestrator/templates/task-brief.template" \
    "fused_memory/py.typed"
do
    run_classify "$_ext_path"
    assert "classify '$_ext_path' exits 0 (ACCEPT)" test "$GUARD_RC" -eq 0
    assert "classify '$_ext_path' stdout contains ACCEPT" test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"
done

# ---------------------------------------------------------------------------
# Cycle 6 — dotted directory segments still REJECT (anti-dotfile-rule pin, #5726)
#
# Cycle 5 widened _EXTS with entries that read like dotfile names (gitignore,
# envrc, npmrc, python-version).  The tempting generalisation — "a final segment
# starting with a dot is a FILE" — is REJECTED, and this block pins that.
#
# All five paths below are real, untracked DIRECTORIES in the main checkout
# (.worktrees is the orchestrator's entire worktree pool).  A blanket dotfile
# rule would flip every one of them to ACCEPT and admit an over-wide charter:
# exactly the failure the guard exists to prevent.  The allowlist stays
# enumerated for this reason.
#
# Being untracked, two of them do not exist in a freshly-seeded warm lane — so
# these assertions deliberately do not depend on the filesystem, and C-P3
# guarantees they cannot: the verdict is identical either way.
#
# Green on arrival (these REJECT both before and after the Cycle 5 expansion),
# so per G6 the pin was shown to FIRE rather than assumed to: inserting
# `case "$seg" in .*) return 0 ;; esac` at the top of _is_file_path() in a
# scratch copy of the guard fails all 10 assertions below.  Mutant not committed.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 6: dotted directory segments still REJECT (#5726) ---"

for _dot_dir in \
    ".worktrees" \
    ".task" \
    ".claude" \
    ".cargo" \
    ".taskmaster"
do
    run_classify "$_dot_dir"
    assert "anti-dotfile pin: classify '$_dot_dir' exits 1 (REJECT)" test "$GUARD_RC" -eq 1
    assert "anti-dotfile pin: classify '$_dot_dir' stdout contains REJECT" test "${GUARD_OUT#*REJECT}" != "$GUARD_OUT"
done

# ---------------------------------------------------------------------------
test_summary
