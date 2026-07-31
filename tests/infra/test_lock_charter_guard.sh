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

run_list_extensionless() {
    GUARD_OUT="$(bash "$SCRIPT" --list-extensionless 2>/dev/null)" && GUARD_RC=$? || GUARD_RC=$?
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
# Cycle 7 — extensionless tracked-file basenames ACCEPT (dark_factory:3248 mirror)
#
# Cycle 6 pins that a DOT does not make a segment a file.  This cycle pins the
# complementary hole it left open: the absence of a dot did make a segment a
# directory, unconditionally — so seven real, tracked reify files were
# undeclarable.  The originating symptom is the exact mirror of #5726's:
# declaring hooks/project-checks in a lock charter was REJECTed as a directory.
#
# PROVENANCE of the pinned vector — sweep of BOTH repos' tracked corpora, run
# 2026-07-31, mode-160000 gitlinks excluded:
#     git ls-files -s | awk '$1 != "160000" {print $4}' \
#       | awk -F/ '{print $NF}' | grep -v '\.' | sort -u
#   reify (@ab664afb9c): 7 names — LICENSE cargo cargo-audit-orphans pre-commit
#                        pre-merge-commit project-checks reference-transaction
#   dark-factory (main @8d276d3c5f): 5 names — Dockerfile LICENSE pre-commit
#                        pre-merge-commit project-checks
#   union: the 8 names pinned as CANONICAL_EXTLESS in Cycle 8.
# The gitlink exclusion is DARK-FACTORY-RELEVANT ONLY: reify tracks zero
# mode-160000 entries, while dark-factory tracks two (graphiti, mem0) whose
# submodule mount points are extensionless and must NEVER be admitted as files.
# The filter is applied on the reify side regardless — it is the documented
# invariant, and it future-proofs any later vendoring.
#
# Also measured in the same sweep: ZERO directory-name collisions for any of the
# 8 names across ALL path components (not just leaves) of either corpus — 177
# distinct reify directory names, 97 dark-factory.  So admitting these as
# final-segment ACCEPTs cannot make a real directory declarable.
#
# The 7 reify cases use REAL tracked paths (verified with git ls-files
# --error-unmatch); Dockerfile and deploy/Dockerfile are dark-factory-evidenced
# literals, exactly as Cycle 5 mixes the two.  C-P3 forbids any stat, so a
# literal is as valid an input as a real path — the same property case E1 relies
# on.  The bare-basename and deep-path cases pin that the predicate keys on the
# FINAL SEGMENT at any depth, not on the whole string and not on depth 1.
#
# step-1: verify RED (measured @ab664afb9c: every path below exits 1 / "REJECT
# <path>"); step-2 GREEN by adding _EXTLESS to the guard.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 7: extensionless tracked-file basenames ACCEPT (dark_factory:3248) ---"

for _extless_path in \
    "LICENSE" \
    "hooks/pre-commit" \
    "hooks/pre-merge-commit" \
    "hooks/project-checks" \
    "hooks/reference-transaction" \
    "scripts/agent-bin/cargo" \
    "scripts/cargo-audit-orphans" \
    "Dockerfile" \
    "deploy/Dockerfile" \
    "a/b/c/project-checks"
do
    run_classify "$_extless_path"
    assert "extensionless: classify '$_extless_path' exits 0 (ACCEPT)" test "$GUARD_RC" -eq 0
    assert "extensionless: classify '$_extless_path' stdout contains ACCEPT" \
        test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"
done

# ---------------------------------------------------------------------------
# Cycle 8 — --list-extensionless emitter + over-accept pins
#
# (a) EMITTER — the drift guard for _EXTLESS, mirroring Cycle 4's for _EXTS.
#     Its reason to exist is γ-side: dark_factory:3248 needs a machine-readable
#     α vector to compare against, exactly as --list-extensions gives it one for
#     the extension list.  Pinned in BYTE order (uppercase first), not the
#     ambient-locale order this host would otherwise produce — γ's counterpart
#     is Python sorted(), which is code-point order, so byte order is what the
#     two sides must agree on.  The emitter pins LC_ALL=C for that reason.
#
# (c) OVER-ACCEPT PINS — green on arrival, so per G6 they were SHOWN to fire
#     rather than assumed to.  Three scratch mutants of the guard, each a
#     plausible refactor that leaves all of Cycle 7 green (canonical-8 still
#     ACCEPT, 8/8, under every one):
#       A. relocate the _EXTLESS loop below the `ext` extraction and compare
#          "$ext" instead of "$seg"  → flips .cargo .pre-commit .LICENSE
#          x.cargo foo.LICENSE (5 assertions RED).  This is the trap the
#          _EXTLESS header warns about: "${seg##*.}" on .cargo yields cargo.
#       B. loosen [ "$seg" = "$x" ] to [[ "$seg" == "$x"* ]]  → flips
#          pre-commit-hooks cargo-lib project-checks-old (3 RED).
#       C. reverse it to [[ "$x" == "$seg"* ]]  → flips hooks/pre (1 RED).
#     Every pin below is covered by at least one mutant; none is vacuous.
#     Mutants not committed.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 8: --list-extensionless emitter + over-accept pins ---"

# Canonical extensionless-basename allowlist — sorted-unique BYTE order, one per
# line.  Shared α/γ test vector; γ's mirror is dark_factory:3248.
CANONICAL_EXTLESS="Dockerfile
LICENSE
cargo
cargo-audit-orphans
pre-commit
pre-merge-commit
project-checks
reference-transaction"

run_list_extensionless
assert "--list-extensionless exits 0" test "$GUARD_RC" -eq 0
assert "--list-extensionless stdout matches canonical extensionless allowlist" \
    test "$GUARD_OUT" = "$CANONICAL_EXTLESS"

# (b) Coherence: every emitted basename is ACCEPTed by classify.
while IFS= read -r _el; do
    [ -z "$_el" ] && continue
    run_classify "$_el"
    assert "--list-extensionless coherence: classify '$_el' exits 0" test "$GUARD_RC" -eq 0
done <<< "$GUARD_OUT"

# (c) Over-accept pins — the match is on the FULL segment and is EXACT.
for _near_miss in \
    ".cargo" \
    ".pre-commit" \
    ".LICENSE" \
    "x.cargo" \
    "foo.LICENSE" \
    "pre-commit-hooks" \
    "cargo-lib" \
    "project-checks-old" \
    "hooks/pre"
do
    run_classify "$_near_miss"
    assert "extensionless over-accept pin: classify '$_near_miss' exits 1 (REJECT)" \
        test "$GUARD_RC" -eq 1
    assert "extensionless over-accept pin: classify '$_near_miss' stdout contains REJECT" \
        test "${GUARD_OUT#*REJECT}" != "$GUARD_OUT"
done

# ---------------------------------------------------------------------------
# Cycle 9 — live-corpus drift alarm (standing guard against a stale _EXTLESS)
#
# Every other cycle in this file pins a fixed string vector.  This one sweeps the
# ACTUAL tracked corpus and asserts _EXTLESS still covers it, because the failure
# mode being defended against is not a wrong entry — it is a MISSING one, and a
# pinned vector cannot notice a file that landed after it was written.
#
# That failure mode has now produced the same incident twice: #5726 (22 tracked
# extensions the list misclassified as directories) and dark_factory:3248 (these
# 8 basenames).  Both were found by a human running a manual sweep.  This block
# converts that sweep into a standing CI signal.
#
# A RED here means a new extensionless tracked file has landed.  The fix is to
# add its basename to BOTH _EXTLESS in scripts/lock-charter-guard.sh AND γ's
# EXTENSIONLESS_FILENAMES (dark-factory shared/src/shared/locking.py +
# fused-memory/.../lock_charter_guard.py) — the two must stay byte-identical or
# the cross-source drift comparison goes RED on the γ side instead.
#
# SUBSET, not equality, and deliberately so: Dockerfile is dark-factory-evidenced
# only, so reify's tracked corpus is a strict subset of the shared vector.  An
# equality assertion would fail here permanently and would also fight the shared
# nature of the list.  Over-coverage is the safe direction — an entry with no
# reify file behind it costs nothing, a missing entry costs a rejected charter.
#
# The mode-160000 filter is carried even though reify tracks zero gitlinks: it is
# the documented invariant (dark-factory's graphiti/mem0 submodule mount points
# are extensionless and must never be admitted), and it keeps this sweep correct
# if reify ever vendors a submodule.
#
# Host-dependence is confined to this block by the rev-parse probe below, which
# SKIPs cleanly outside a git checkout — the file header's "no skip guard, the
# predicate is host-independent (C-P3)" promise holds for Cycles 1-8, which stay
# pure-string.
#
# Green on arrival (7 reify names ⊂ the 8 pinned).  Per G6 it was shown to FIRE
# rather than assumed to: deleting `project-checks` from _EXTLESS in a scratch
# copy of the guard turns this block RED with
#   FAIL: live-corpus drift: 'project-checks' present in --list-extensionless
# Mutant not committed.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 9: live-corpus drift alarm (_EXTLESS covers tracked corpus) ---"

if ! git -C "$REPO_ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    echo "  SKIP: $REPO_ROOT is not a git checkout — live-corpus sweep unavailable"
else
    run_list_extensionless
    assert "live-corpus drift: --list-extensionless exits 0" test "$GUARD_RC" -eq 0
    _emitted="$GUARD_OUT"

    # Tracked, non-gitlink, dotless final segments.
    _tracked_extless="$(
        git -C "$REPO_ROOT" ls-files -s \
            | awk '$1 != "160000" {print $4}' \
            | awk -F/ '{print $NF}' \
            | grep -v '\.' \
            | LC_ALL=C sort -u
    )"

    # Non-vacuity: the sweep must actually find something, or every assertion
    # below would pass by finding nothing to check.
    assert "live-corpus drift: sweep found at least one tracked extensionless basename" \
        test -n "$_tracked_extless"

    while IFS= read -r _name; do
        [ -z "$_name" ] && continue
        assert "live-corpus drift: '$_name' present in --list-extensionless" \
            test "$(printf '%s\n' "$_emitted" | grep -cxF "$_name")" -eq 1
    done <<< "$_tracked_extless"
fi

# ---------------------------------------------------------------------------
test_summary
