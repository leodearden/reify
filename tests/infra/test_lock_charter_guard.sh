#!/usr/bin/env bash
# tests/infra/test_lock_charter_guard.sh — TDD harness for scripts/lock-charter-guard.sh
#
# Drives the lock-charter guard in isolation.  Tests the syntactic
# directory-vs-file predicate (C-P1..C-P4) required by the task-lock-charter-
# lifecycle PRD (docs/prds/task-lock-charter-lifecycle.md §4.1).
#
# Skip-guard scope — read this before adding a block that touches the filesystem.
# Cycles 1-8 carry NO skip guard, because the predicate they exercise is
# host-independent (C-P3 — pure string, no filesystem stat, no model call), so
# they run identically on every host.  Cycles 9-10 are the exception and are
# deliberately different in kind: they are live-corpus drift alarms that sweep the
# ACTUAL tracked corpus via git, so they are host-dependent by construction.  Each
# confines that dependence to its own block with a `git rev-parse --git-dir` probe
# and SKIPs cleanly when $REPO_ROOT is not a git checkout.  A new block may touch
# the filesystem only if it carries the same probe; without one it belongs in the
# pure-string 1-8 family.
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
# This is the pinned shared α/γ test vector (PRD §11 Q2 — the allowlist-
# completeness question; §11 Q1 is the separate predicate-transport one).
# Pinned in BYTE order, for the same reason Cycle 8's CANONICAL_EXTLESS is: γ's
# Tier-2 comparison is against Python sorted() (code-point order), so the emitter
# pins LC_ALL=C and this vector must match that, not the ambient-locale order.
# Invisible on today's all-lowercase vector — the two orderings are md5-identical
# — but the list already carries hyphenated members and glibc collation ignores
# punctuation at the primary level, so it will not stay invisible forever.
#
# `csv` (the 59th entry, added #6067) mirrors dark_factory 43410b3418 and is
# dark-factory-evidenced only — reify tracks zero .csv files.  Its behavioural
# coverage is the coherence loop below, which classifies every emitted extension;
# the incident narrative lives with _EXTS in scripts/lock-charter-guard.sh, not
# here.  Per-entry test cycles are deliberately NOT the convention, because this
# block's equality assertion is already strictly stronger than any single-entry
# presence check.
#
# HOW TO WIDEN THE ALLOWLIST — canonical enumeration, owned HERE.  This is the one
# site that states it; _EXTS's header in scripts/lock-charter-guard.sh and Cycle 10
# below both POINT at this list rather than restate it, so the procedure has a
# single place to drift from.  Adding an extension is FOUR lockstep edits:
#   1. _EXTS in scripts/lock-charter-guard.sh          — the α source of truth
#   2. this CANONICAL_EXTS                              — the α pin
#   3. lcl_canonical_extensions() in tests/infra/lock_charter_harness_lib.sh
#   4. γ's copies (dark-factory shared/src/shared/locking.py and
#      fused-memory/.../lock_charter_guard.py, plus their own pins)
# _EXTS is a SHARED α/γ vector: a unilateral α widening re-opens exactly the seam
# divergence this subsystem exists to close, and γ's cross-source drift comparison
# goes RED on the γ side instead of here.
CANONICAL_EXTS="c
cc
cjs
conf
cpp
css
csv
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
tombstones
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
# 2026-07-31, mode-160000 gitlinks excluded.  Kept byte-identical to the copies
# in the _EXTLESS header (scripts/lock-charter-guard.sh) and in Cycle 9's
# executable form below; the -z + FS="\t" shape is tab-split-not-whitespace-split
# on purpose, so a tracked path containing a space is not truncated at it (see
# Cycle 9's comment for why that matters there):
#     git ls-files -s -z \
#       | awk 'BEGIN { RS = "\0"; FS = "\t" }
#              $1 !~ /^160000 / { n = split($2, s, "/"); print s[n] }' \
#       | grep -v '\.' | LC_ALL=C sort -u
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

# (d) ACCEPTED-LIMITATION pin (PRD §10) — a trailing directory slash does NOT
#     rescue the classification.  Both α and γ strip trailing slashes BEFORE
#     segmentation, so `hooks/project-checks/` — written with an explicit,
#     unambiguous directory marker — still ACCEPTs.  That is knowingly wrong and
#     knowingly kept: an α-only special case would manufacture a fresh α/γ
#     divergence, which is the exact failure class this whole seam exists to
#     prevent, so closing it is a JOINT α/γ decision (§10).
#
#     Pinned here because an accepted limitation with no test behind it is
#     indistinguishable from an untested accident: without these assertions a
#     future contributor could "fix" it α-only and nothing would fire, and
#     conversely a later joint flip would land with no signal that this file
#     records the old behaviour.  RED here is not "the guard is broken" — it is
#     "§10 changed, go read it and check γ changed too".
#
#     Distinct from Cycle 1's trailing-slash REJECTs (crates/, a/b/c/), which
#     cover segments outside BOTH allowlists; these are the allowlisted-basename
#     case, which is the one §10 flags.
for _slashed in \
    "hooks/project-checks/" \
    "LICENSE/" \
    "scripts/agent-bin/cargo/" \
    "Dockerfile//"
do
    run_classify "$_slashed"
    assert "accepted limitation (PRD §10): classify '$_slashed' exits 0 (trailing slash stripped)" \
        test "$GUARD_RC" -eq 0
    assert "accepted limitation (PRD §10): classify '$_slashed' stdout contains ACCEPT" \
        test "${GUARD_OUT#*ACCEPT}" != "$GUARD_OUT"
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
# Host-dependence is confined to this block by the rev-parse probe below — see
# the file header, which scopes the C-P3 pure-string/no-skip promise to Cycles 1-8
# and names this cycle as one of the two live-corpus exceptions.
#
# Green on arrival (7 reify names ⊂ the 8 pinned).  Per G6 it was shown to FIRE
# rather than assumed to: deleting `project-checks` from _EXTLESS in a scratch
# copy of the guard turns this block RED with
#   FAIL: live-corpus drift: 'project-checks' present in --list-extensionless
# Mutant not committed.
#
# The tab-split parse below was likewise shown to matter rather than argued to:
# in a scratch repo tracking `docs/my file.md` and `LICENSE`, the old
# whitespace-split form emits `LICENSE` AND the phantom `my`, the -z/FS="\t" form
# emits `LICENSE` only.  Scratch repo not committed.
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
    #
    # Split on the TAB, not on whitespace.  `git ls-files -s` emits
    # "<mode> SP <sha> SP <stage> TAB <path>", so the whitespace-splitting
    # `awk '{print $4}'` form truncates any tracked path containing a space at
    # that space: "docs/my file.md" -> "docs/my", whose final segment "my" has no
    # dot, survives the grep, and then FAILS the presence assertion below against
    # a basename that exists nowhere.  A standing alarm must not be able to RED
    # against a phantom name — the reader would go looking for a file that isn't
    # there.  Neither repo tracks such a path today (measured:
    # `git ls-files | grep -c ' '` -> 0 in both), so this is hardening against
    # corpus growth, which is precisely what this block exists for.  -z also makes
    # the git->awk hop newline-safe.  The `^160000 ` anchor still matches the mode
    # because $1 is now the whole "<mode> SP <sha> SP <stage>" prefix.
    _tracked_extless="$(
        git -C "$REPO_ROOT" ls-files -s -z \
            | awk 'BEGIN { RS = "\0"; FS = "\t" }
                   $1 !~ /^160000 / { n = split($2, s, "/"); print s[n] }' \
            | grep -v '\.' \
            | LC_ALL=C sort -u
    )"

    # Non-vacuity: the sweep must actually find something, or every assertion
    # below would pass by finding nothing to check.
    assert "live-corpus drift: sweep found at least one tracked extensionless basename" \
        test -n "$_tracked_extless"

    # `grep -cxF -- "$_name"` — the `--` is load-bearing for the same reason
    # Cycle 10's copy documents at length: without it a basename beginning with a
    # dash is parsed by grep as an option bundle rather than as a pattern, so the
    # assertion errors out instead of answering the question.  Zero such tracked
    # basenames today (measured -> 0), and extensionless basenames are if anything
    # MORE likely to acquire one than post-dot extension tokens are, so both
    # live-corpus alarms carry the hardening rather than one documenting the
    # hazard and its sibling standing exposed.
    while IFS= read -r _name; do
        [ -z "$_name" ] && continue
        assert "live-corpus drift: '$_name' present in --list-extensionless" \
            test "$(printf '%s\n' "$_emitted" | grep -cxF -- "$_name")" -eq 1
    done <<< "$_tracked_extless"
fi

# ---------------------------------------------------------------------------
# Cycle 10 — live-corpus extension drift alarm (standing guard against a stale
# _EXTS).  The extension-side mirror of Cycle 9.
#
# Cycle 4 above already compares --list-extensions against CANONICAL_EXTS and
# does so with a full equality assert, which is strictly stronger than any
# presence check — in ONE direction.  CANONICAL_EXTS is a pin living in this
# same repo, so it moves whenever _EXTS moves: it catches an accidental EDIT to
# the allowlist and can never catch a MISSING entry, because nobody edits the
# pin to mention an extension they did not know they needed.  #5726 was exactly
# that failure on exactly this vector — 22 tracked extensions the list
# misclassified as directories, found by a human running a manual sweep, not by
# a gate.  This block converts that sweep into a standing CI signal, the same
# conversion Cycle 9 performs for the extensionless vector.  The two are
# complementary, not redundant: Cycle 4 owns the accidental-edit direction,
# Cycle 10 owns the missing-entry direction.
#
# SUBSET, not equality, and measured to be binding here rather than merely
# inherited from Cycle 9: the sweep finds 37 extensions while _EXTS carries 60,
# and the 23-entry gap is legitimate because this is a SHARED α/γ vector —
#   cc cjs csv cts cxx diff example example-systemd-config gitattributes
#   gitmodules hh hpp jsonl jsx log mts python-version scss step stl svg
#   template typed
# have no reify file behind them today.  An equality assertion would be RED
# permanently and would fight the shared nature of the list.  Over-coverage is
# the safe direction — an entry with no reify file costs nothing, a missing
# entry costs a rejected charter.
#
# HONEST SCOPE LIMIT — this alarm would NOT have caught the `csv` drift that
# motivated #6067, and must not be read as closing that gap.  reify tracks zero
# .csv files (measured: `git ls-files '*.csv' | wc -l` -> 0); the evidence lived
# in dark-factory's corpus (plans/evidence/scheduler-scoring-2026-08-06/*.csv).
# Cycle 10 defends against reify-corpus-driven drift only — a NEW tracked reify
# extension landing with no allowlist entry, i.e. the #5726 shape.  At the time
# #6067 was filed, the cross-source half (γ's Tier-2 comparison) was also open:
# a path-resolution bug dropped that test at collection, so it skipped on every
# run (esc-6067-2).  MEASURED 2026-08-27 (#6856): that half is no longer open —
# dark-factory tasks 3843/4080 re-armed BOTH of γ's Tier-2 cross-source guards
# on a layout-independent resolver, and fresh -rs plus injected-drift runs
# confirm both the extension and extensionless comparisons now run and fire.
# Cycle 10 here still only covers the reify-corpus direction; the
# cross-source direction is covered from γ's side, not by this file — see the
# "Cross-repo seam: γ" header note in scripts/lock-charter-guard.sh for the
# current-state detail, deliberately not restated here.
#
# LAST-dot extraction is the filter rule.  The extension side, unlike the
# extensionless side, can surface non-extension tokens, and the answer is not a
# token blacklist — it is asking exactly the question the guard answers.
# _is_file_path()'s `local ext="${seg##*.}"` in scripts/lock-charter-guard.sh
# takes the post-LAST-dot token, so the sweep extracts the same way.  Cited by
# SYMBOL, not by line number, and deliberately: both files are comment-dense, so
# a line pin rots as soon as anything is inserted above it — an earlier draft of
# this very comment cited `:232` and was stale before it landed, because the
# _EXTS header hunk it shipped alongside pushed the line down to 239.  Measured on
# today's corpus: last-dot yields 36 clean real extensions and zero junk;
# first-dot yields 73 tokens, 38 of which still contain a dot, off the corpus's
# dotted-basename family (split.structure-Board.md, split.enum-Grade.md,
# foo.module.css, bar.e2e.test.ts, d.ts).  Faithfulness to the predicate is what
# makes the sweep junk-free — any other extraction would alarm on tokens
# _is_file_path() never inspects, producing REDs no allowlist edit can fix.
#
# THIS IS A DELIBERATE FOURTH VARIANT of the canonical sweep command, not a
# fourth copy.  The other three (the _EXTLESS header in
# scripts/lock-charter-guard.sh, Cycle 7's provenance comment, Cycle 9's
# executable form) are kept BYTE-IDENTICAL on purpose.  Cycle 10 differs in
# exactly one place: its terminal branch selects DOTTED final segments and
# reduces each to its post-last-dot token, where they select dotless ones via
# `grep -v '\.'`.  Everything through the `split($2, s, "/")` final-segment
# extraction is identical — including the mode-160000 gitlink filter, carried for
# the reason Cycle 9 gives.  Do not "unify the four": collapsing them silently
# breaks either this alarm or Cycle 9.
#
# Host-dependence is confined to this block by the rev-parse probe below — see the
# file header, which scopes the C-P3 pure-string/no-skip promise to Cycles 1-8.
#
# A RED HERE means a new tracked reify path has landed that the allowlists do not
# cover.  READ WHICH ASSERTION FIRED FIRST — this block's two halves have
# DIFFERENT repair procedures, because the backstop classifies both vectors:
#
#   * EXTENSION case — the token loop's "'<ext>' present in --list-extensions",
#     or a backstop REJECT of a path whose final segment is DOTTED.  The fix is
#     the FOUR-part lockstep widening enumerated in Cycle 4's comment above —
#     α's three pins PLUS γ's copies, never _EXTS alone.  That enumeration is
#     stated there once and pointed at from here on purpose, so the next widening
#     has one prose site to keep true instead of three.
#
#   * EXTENSIONLESS case — a backstop REJECT of a path whose final segment is
#     DOTLESS.  This is NOT an extension miss and Cycle 4's enumeration is the
#     wrong procedure for it: the fix is the _EXTLESS lockstep documented in
#     Cycle 9 above (α's _EXTLESS PLUS γ's EXTENSIONLESS_FILENAMES copies), and
#     Cycle 9's own assertion will normally RED beside this one.
#
# Green on arrival (37 swept ⊂ the 60 pinned) — by construction, since a RED on
# arrival would mean the allowlist was already broken.  Per G6 it was shown to
# FIRE rather than assumed to: deleting `ri` from _EXTS in a scratch copy of the
# guard (59 -> 58 entries) turns this block from 41 passed / 0 failed to
# 38 passed / 3 failed — the block's two halves reporting the same drift from
# opposite ends, the backstop in both of its shapes:
#   FAIL: live-corpus: guard check over the tracked corpus exits 0
#   FAIL: live-corpus: guard accepts every tracked path
#   FAIL: live-corpus ext drift: 'ri' present in --list-extensions
# The first two say a tracked path stopped classifying as a file — by exit code
# and by stdout, the two shapes that are asserted separately because a regression
# can produce either without the other; the block also prints the first five
# offending paths (`REJECT crates/reify-cli/tests/fixtures/affine_algebra.ri` …)
# so the reader sees WHICH paths broke.  The third names the allowlist entry
# whose absence caused it.  Targeted, not vacuous, not over-broad — the other 38
# assertions in the block stay green.  (Suite-wide the same mutant is
# 321 passed / 8 failed: it also REDs Cycle 1's `examples/foo.ri` classify pins,
# Cycle 3's two all-file list gates and Cycle 4's equality assert, as it should.
# The mutant total is 329 rather than the green 330 because Cycle 4's per-entry
# loop has one fewer entry to iterate over.)
# Mutant not committed; the guard was restored from a byte-copy and `git diff`
# re-confirmed empty.
#
# The gitlink filter both halves carry was likewise measured rather than argued:
# feeding a synthetic `ls-files -s -z` stream of
# `160000 … TAB graphiti`, `100644 … TAB src/a.rs`, `160000 … TAB vendor/mem0`
# through the awk above emits `src/a.rs` alone, and `classify graphiti` exits 1
# (REJECT) — i.e. without the filter a vendored submodule would RED the backstop
# with no legitimate fix.  Synthetic stream not committed.
#
# The tab-split parse was likewise shown to matter rather than argued to, and it
# matters MORE here than in Cycle 9 — the extension side has a failure mode the
# extensionless side does not.  In a scratch repo tracking `docs/notes.md`,
# `data/report v2.csv` and `docs/foo.bar baz.qux`:
#   -z + FS="\t" form  -> csv md qux   (correct)
#   whitespace $4 form -> bar md       (wrong, in TWO distinct ways)
# The first is Cycle 9's documented PHANTOM mode (`docs/foo.bar baz.qux` truncates
# at the space, and its post-dot token `bar` names an extension that exists
# nowhere).  The second is unique to here: `data/report v2.csv` truncates to
# `data/report`, now DOTLESS, so the dotted branch DROPS it and the real `csv` is
# never checked — the alarm quietly weakens instead of failing loudly.  On the
# extensionless side a truncated path stays dotless and is still swept, so that
# silent-miss mode cannot arise there.  reify tracks zero paths containing a space
# today (measured: `git ls-files | grep -c ' '` -> 0), so both are latent — but a
# standing alarm over a GROWING corpus is exactly where latent parse bugs surface.
# Scratch repo not committed.
# ---------------------------------------------------------------------------
echo ""
echo "--- Cycle 10: live-corpus drift alarm (_EXTS covers tracked corpus) ---"

if ! git -C "$REPO_ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    echo "  SKIP: $REPO_ROOT is not a git checkout — live-corpus sweep unavailable"
else
    run_list_extensions
    assert "live-corpus ext drift: --list-extensions exits 0" test "$GUARD_RC" -eq 0
    _emitted_exts="$GUARD_OUT"

    # PREDICATE-FAITHFUL BACKSTOP.  Hand the WHOLE tracked corpus to the guard's
    # own `check` and require every path to classify as a file.  This re-USES
    # _is_file_path() instead of re-implementing its extraction rule, so unlike
    # the token sweep below it cannot drift from the predicate, and it covers both
    # vectors plus any future rejection reason at once.  Concretely it closes the
    # one gap the token sweep is structurally blind to: a tracked final segment
    # ending in a dot (`docs/foo.`) reduces to an EMPTY extension token, which the
    # sweep's `seg != ""` filter drops, yet _is_file_path() REJECTs it — verified
    # by injecting `docs/foo.` into this stream (-> `REJECT docs/foo.`, exit 1).
    # Zero such paths today (measured: 0 tracked segments matching /\.$/).
    #
    # VECTOR-NEUTRAL BY CONSTRUCTION, and its label says so: `check` classifies
    # BOTH vectors, so a RED here is not necessarily an extension miss.  See the
    # two-branch fix guidance in the block header — a dotless-segment REJECT is
    # Cycle 9's _EXTLESS repair, not Cycle 4's four-part _EXTS widening.
    #
    # SAME GITLINK-FILTERED SOURCE as the token sweep below, and for the reason
    # Cycle 9 gives: a submodule mount point is extensionless, so _is_file_path()
    # REJECTs it and this assertion would go RED with no legitimate fix — the only
    # green would be adding the submodule's name to _EXTLESS, which that list's
    # enumerated-exception invariant forbids.  reify tracks zero gitlinks today
    # (measured -> 0), so like Cycle 9's copy this is hardening against corpus
    # growth.  The filter is spelled out again here rather than hoisted into one
    # shared stream because the sweep below is held BYTE-IDENTICAL with Cycle 9's
    # executable form and the two prose copies (see "DELIBERATE FOURTH VARIANT"
    # above); hoisting would silently break that invariant to save two lines.
    #
    # NON-VACUITY IS ASSERTED, NOT ARGUED.  `check` ACCEPTs an EMPTY path list
    # (the [] defer-to-architect value), so an empty fed list would pass silently.
    # The guard against that is the `test -n "$_tracked_paths"` assertion below,
    # deliberately in place of a prose path count: this block's whole premise is
    # that the corpus GROWS, so a hardcoded count is stale by the next landing
    # (an earlier draft claimed 3829 and was wrong at review time).  Injecting a
    # single directory-shaped path (`crates/reify-core/src`) turns the assertion
    # RED — measured, not assumed.
    #
    # THE EXIT CODE IS ASSERTED ALONGSIDE THE OUTPUT because they fail in
    # different shapes: a `check` that regressed to exiting non-zero while
    # printing NOTHING (a parse error, a `set -u` trip inside the check branch, a
    # usage-error exit 2 on stdin input) satisfies an output-only assertion and
    # would pass silently.  The `--list-extensions exits 0` assertion above does
    # not cover it — that catches a script broken for every subcommand, not one
    # broken for `check` alone.
    #
    # The token loop below is KEPT alongside all of this because it fails
    # differently and both diagnostics are worth having: `check` names the
    # offending PATH, the loop names the exact allowlist ENTRY to add, which is
    # what the G6 mutant evidence above is built on.
    #
    # -z keeps the git->awk half newline-safe (measured: 0 tracked paths contain a
    # newline); awk then emits newline-separated paths, which is what `check`
    # reads.  Fed through run_check_stdin rather than a pipe into `check` so the
    # EXIT CODE is capturable (GUARD_RC) — a pipeline inside `$( )` would hand
    # back only stdout.
    _tracked_paths="$(
        git -C "$REPO_ROOT" ls-files -s -z \
            | awk 'BEGIN { RS = "\0"; FS = "\t" }
                   $1 !~ /^160000 / { print $2 }'
    )"
    assert "live-corpus: sweep fed at least one tracked path" test -n "$_tracked_paths"

    run_check_stdin "$_tracked_paths"
    _corpus_rejects="$GUARD_OUT"
    if [ -n "$_corpus_rejects" ]; then
        printf '%s\n' "$_corpus_rejects" | sed -n '1,5s/^/    /p'
    fi
    assert "live-corpus: guard check over the tracked corpus exits 0" \
        test "$GUARD_RC" -eq 0
    assert "live-corpus: guard accepts every tracked path" \
        test -z "$_corpus_rejects"

    # Tracked, non-gitlink, DOTTED final segments, reduced to their post-LAST-dot
    # token.  Same sweep command as Cycle 9 through the gitlink filter and the
    # final-segment split; only the terminal branch differs.
    _tracked_exts="$(
        git -C "$REPO_ROOT" ls-files -s -z \
            | awk 'BEGIN { RS = "\0"; FS = "\t" }
                   $1 !~ /^160000 / {
                       n = split($2, s, "/"); seg = s[n]
                       if (seg ~ /\./) { sub(/^.*\./, "", seg); if (seg != "") print seg }
                   }' \
            | LC_ALL=C sort -u
    )"

    # Non-vacuity: the sweep must actually find something, or every assertion
    # below would pass by finding nothing to check.
    assert "live-corpus ext drift: sweep found at least one tracked extension" \
        test -n "$_tracked_exts"

    # `grep -cxF -- "$_e"` — the `--` is not decoration.  Without it an extension
    # token beginning with a dash would be parsed by grep as an option bundle
    # rather than as a pattern, so the assertion would error out instead of
    # answering the question.  Zero such tokens today (measured -> 0); this is the
    # same hardening-against-corpus-growth argument the tab-split carries.
    while IFS= read -r _e; do
        [ -z "$_e" ] && continue
        assert "live-corpus ext drift: '$_e' present in --list-extensions" \
            test "$(printf '%s\n' "$_emitted_exts" | grep -cxF -- "$_e")" -eq 1
    done <<< "$_tracked_exts"
fi

# ---------------------------------------------------------------------------
test_summary
