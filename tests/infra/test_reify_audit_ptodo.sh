#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo.sh
#
# Infra gate for the PTODO detector (task e / #4557):
#   (a) RATCHET  — TWO assertions, in this order: a RUN-EVIDENCE floor on the
#                  generator's stderr, then the subset check — live
#                  ptodo-baseline-gen fingerprints must be a subset of the
#                  committed crates/reify-audit/ptodo-baseline.txt
#                  (live - baseline = empty).  Subset-of alone is trivially
#                  satisfied by the empty set; see RATCHET VACUITY FLOOR below.
#                  Subset-of BY RULING, not by omission: the converse
#                  assertion (comm -13, baseline ⊆ live) was considered and
#                  DECLINED — PRD §18.
#   (b) SCENARIO 13 (hermetic) — a git-tracked code file carrying a fresh
#                  untracked marker produces fingerprints absent from an empty
#                  baseline, proving the ratchet fires red on new violations.
#   (c) EXIT-CODE HARD GATE (task η, #4559) — structural untracked High
#                  finding → reify-audit exits non-zero.  Runs whenever the
#                  binary is PRESENT, independent of staleness/RATCHET_SKIP.
#   (d) ORPHANED-CITE HARD GATE (task #4733) — a cite to a done task is
#                  classified orphaned→High → reify-audit exits non-zero.
#                  Hermetic (sqlite3 seeded tasks.db + --tasks-file []).
#                  Runs whenever binary is PRESENT and sqlite3 is available.
#   (e) G-ALLOW ORPHANED HARD GATE (task #4754) — a // G-allow: owner-cite
#                  pointing to a done task is classified g-allow-orphaned→High
#                  → reify-audit exits non-zero.  Hermetic (sqlite3 seeded
#                  tasks.db + --tasks-file []).  Mirrors scenario (d) for the
#                  G-allow advisory→hard-gate flip.
#   (f) LANE δ-A HARD GATE (task #6087) — an #[allow(...dead_code...)]
#                  attribute whose trailing rationale defers the work and cites
#                  a done task is classified orphaned→High → reify-audit exits
#                  non-zero.  Hermetic, mirrors (d); adds a BENIGN control repo
#                  so the lane's false-positive guards are proven at the binary
#                  level and not only in unit tests.
#   (g) LANE δ-B HARD GATE (task #6103) — an ORDINARY comment (no marker token,
#                  no attribute) that both defers the work and cites a terminal
#                  task is classified orphaned→High → reify-audit exits
#                  non-zero.  Hermetic, mirrors (f) including its BENIGN control
#                  repo — which matters more here than anywhere else: δ-B was
#                  rejected once (task #6087) at a 48% false-positive rate, so
#                  its guards are proven at the binary level, not only in unit
#                  tests.
#
# Design invariant (PRD 6.6): fingerprint derivation lives ONLY in the
# ptodo-baseline-gen binary (the same ptodo::fingerprint path the ratchet uses).
# No fingerprint re-derivation happens in this bash file.
#
# Budget-safe restructure (task #4733):
#   Scenarios (a)+(b) are precision-sensitive (require a FRESH gen binary to
#   emit correct fingerprints) and are wrapped in a RATCHET_SKIP guard.
#   Scenarios (c)+(d) are STABLE across the warm-lane staleness window — a
#   present-but-stale binary emits High findings correctly — so they run
#   whenever REIFY_AUDIT_BIN is executable, regardless of RATCHET_SKIP.
#   This prevents the whole-test-skip bug where a stale warm-lane binary
#   caused the PTODO hard gate to be silently bypassed (incident 2026-06-22/23).
#
# NO-SILENT-GREEN FLOOR (residual of #4733, esc-5405-7).  The rc-75 partition
# above is only sound while the binary is PRESENT-but-stale.  The same guard
# also returns 75 for an ABSENT binary, and returns 125 when a rebuild was
# attempted and the binary is still judged stale — whenever the binary is not
# executable the `[ -x "$REIFY_AUDIT_BIN" ]` guards below are false, EVERY
# scenario is skipped, and an unguarded `test_summary` would print "0 passed,
# 0 failed" and exit 0.  run_all.sh grades on exit code alone, so that is a
# hard gate reporting green having asserted nothing — the exact failure mode
# this file's partition was written to prevent.  Two floors close it:
#   * a guard rc outside {0,75} with the binary ABSENT aborts LOUD immediately
#     (a broken toolchain, not a budget-safe skip).  With the binary PRESENT
#     it degrades to RATCHET_SKIP=1 and falls through instead, so (c)+(d)+(e)
#     still run — see the rc-125 branch below for why the two cases differ
#     (#5962 review);
#   * the $RAN tracker refuses to exit 0 unless at least one scenario block
#     actually executed.
# Both are pinned by tests/infra/test_reify_audit_ptodo_budget_skip.sh.
#
# RATCHET VACUITY FLOOR (task #6127, rebased onto scan evidence by #6241).  The
# scenario-(a)-level analogue of the block above: that floor stops a run which
# executed no SCENARIOS from reporting green; this one stops a scenario whose
# DETECTOR NEVER RAN from doing the same, since subset-of is trivially
# satisfied by the empty set.  _ratchet_check_scan_evidence below is that floor,
# asserted BEFORE the subset check so the precondition is reported before the
# thing it conditions.
#
# It keys on the generator's own `@@PTODO_SCAN@@ files_scanned=<N> …` stderr
# line — evidence the sweep RAN — not on how many findings it produced, so a
# clean tree passes and an ordinary burn-down commit can never false-RED it.
# That is why no detector-kind knowledge lives in this file any more, and why
# the PRD 6.6 derivation invariant above holds in full: the helper reads a
# field off generator-emitted text, it derives nothing.  Rationale and known
# limits live in ONE place — PRD §6.6
# (docs/prds/reify-audit-ptodo-detector.md); do not restate them here.  Cited by
# SECTION NUMBER only, deliberately: a bolded paragraph title is not a stable
# anchor (#6241's retitle stranded six such cites at once), a section number is.  The
# in-file meta-test below pins its SEMANTICS; the WIRING into scenario (a),
# which that meta-test cannot observe, is pinned in BOTH directions (fires
# without scan evidence, silent with it) by
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh.
#
# ACCEPTED UNCOVERED SKIP: the required-tool loop below still exits 0 before
# any scenario runs when git/cargo/comm/sort/sqlite3 is off PATH, and the
# floors above do not cover it.  They are scoped to detector usability; that
# loop is an environment-capability check.  All five tools are present in the
# merge gate (verify.sh), and hard-failing there would break legitimate
# dev-box `run_all.sh` runs, so it stays a graceful skip by design.
#
# SELF-MATCH SAFETY: this file must not contain any literal marker tokens that
# the PTODO structural lane sweeps for.  Marker text in scenarios (b)/(c)/(d)/(f)/(g)
# is assembled from shell variables at runtime so the written fixture carries a
# real token while this .sh source stays clean.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Graceful skip when required tools are absent.
for _tool in git cargo comm sort sqlite3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== PTODO detector infra gate ==="

# -----------------------------------------------------------------------
# ORACLE DIRECTION — subset-of BY RULING (task #6859, PRD §18).  Two KNOWN
# LIMITATIONS are accepted here rather than overlooked: there is no drain
# forcing function (a grandfathered entry may sit in the baseline forever), and
# a grandfathered fingerprint is a re-entry permit for that text ANYWHERE in
# the same file (fingerprints erase line numbers).  Adding the converse
# `comm -13` assertion does not fix either and reds every DB-less context; the
# measurements, the alternatives and the revisit condition are in §18, not
# here.  Pinned in BOTH directions by
# tests/infra/test_reify_audit_ptodo_ratchet_superset.sh.
#
# Pure ratchet-regression checker (task 5260, ITEM 3). When the live-minus-
# baseline fingerprint set ($1) is non-empty, print the offending fingerprints
# (one per line, with a count header) to stderr and return 1; when empty, stay
# byte-for-byte silent and return 0. Passed directly to assert() so a real
# ratchet regression lands the fingerprints in assert()'s on-FAIL captured-
# output dump (test_helpers.sh:24, esc-4959-57) — co-located with the failing
# assertion, exactly where the 4636 RCA found zero actionable output.
# -----------------------------------------------------------------------
_ratchet_check_subset() {
    local _new="$1"
    if [ -n "$_new" ]; then
        printf 'RATCHET REGRESSION — %s live fingerprint(s) NOT in committed baseline:\n' \
            "$(printf '%s\n' "$_new" | grep -c .)" >&2
        printf '%s\n' "$_new" | sed 's/^/  + /' >&2
        return 1
    fi
    return 0
}

# -----------------------------------------------------------------------
# Vacuity floor, scan-evidence form (task #6241).  _ratchet_check_subset above
# can only ever report "no NEW fingerprints"; it says nothing about whether the
# generator RAN at all, and the empty set is a subset of everything.  This is
# the precondition that makes that subset assertion meaningful.
# WHY it keys on run evidence rather than on the live finding count, and what
# it does not cover: PRD §6.6 (section number, not a paragraph title — the
# latter is not a stable anchor) — the single home
# for that argument, not restated here.
#
# Local contract:
#   _ratchet_check_scan_evidence <generator-stderr>, ONE newline-joined STRING
#   (matching _ratchet_check_subset's argument convention) so it can be passed
#   straight to assert() and land its diagnostic in the on-FAIL captured-output
#   dump.  The committed baseline is NOT an input: the floor is independent of
#   ptodo-baseline.txt content by construction.
#
#   rc0 + BYTE-FOR-BYTE SILENT iff the stderr carries a
#   `@@PTODO_SCAN@@ files_scanned=<N> …` line whose N is a well-formed integer
#   >= 1 (silence keeps an all-green suite unchanged — test_helpers.sh:48-51).
#   Otherwise rc1 with a stderr diagnostic naming which of the three failure
#   shapes fired: line ABSENT, files_scanned=0, or a MALFORMED count.  A
#   matched-but-unparseable count falls to the firing branch DELIBERATELY —
#   loud over silent-disarm, the discipline the retired kind list applied to an
#   unrecognised kind.
#
#   MULTIPLICITY + EXTENSIBILITY (PRD §6.6 grammar).  The generator emits
#   EXACTLY ONE scan line per run; this helper reads only the FIRST (grep -m1)
#   rather than policing that count — the strict consumer is the Rust contract
#   test crates/reify-audit/tests/ptodo_baseline.rs, which asserts exactly one.
#   The field list is OPEN for additive extension, and this parse honours that
#   by construction: it reads the files_scanned= token and ignores every other
#   token on the line, so a future counter cannot turn the gate RED.
#
#   @@RATCHET_VACUITY_FIRED@@ leads the failing diagnostic and is a MACHINE
#   TOKEN — the deliberate contract with
#   test_reify_audit_ptodo_ratchet_vacuity.sh, in the same idiom as the
#   @@HARDGATE_*_PASSED@@ sentinels below.  Grep for the token, never for the
#   prose; the prose is free to change.
#
# No fingerprints are re-derived here: this reads two fields off a line the
# GENERATOR emitted, so the PRD 6.6 invariant in this file's header
# (derivation lives ONLY in ptodo-baseline-gen) is preserved in full.
# -----------------------------------------------------------------------
_ratchet_check_scan_evidence() {
    local _stderr="$1"
    local _line _count _shape

    _line="$(printf '%s\n' "$_stderr" | grep -m1 -F '@@PTODO_SCAN@@' || true)"
    if [ -z "$_line" ]; then
        _shape='the generator emitted NO @@PTODO_SCAN@@ line at all'
    else
        # Pure field read: take the files_scanned=<value> token verbatim, then
        # test it for well-formedness.  Anything unparseable stays visible in
        # the diagnostic rather than being coerced to a number.
        #
        # Split on whitespace FIRST, then anchor the key with `^`, mirroring the
        # Rust consumer's `split_whitespace()` + `strip_prefix("files_scanned=")`
        # (crates/reify-audit/tests/ptodo_baseline.rs::parse_scan_line) so the
        # two implementations of one grammar cannot drift.  An unanchored
        # `.*files_scanned=` would match any token whose name merely ENDS WITH
        # the key (`skipped_files_scanned=0`) and read its value instead —
        # turning an additive extension into a hard RED, the precise false-RED
        # the EXTENSIBILITY rule rules out.  Pinned by fixture (vi) above.
        _count="$(printf '%s\n' "$_line" | tr ' \t' '\n\n' \
            | sed -n 's/^files_scanned=\(.*\)$/\1/p' | head -n1)"
        if [ -z "$_count" ]; then
            _shape='the @@PTODO_SCAN@@ line carries no files_scanned field'
        elif ! printf '%s\n' "$_count" | grep -qE '^[0-9]+$'; then
            _shape="the @@PTODO_SCAN@@ line carries a MALFORMED files_scanned count: '$_count'"
        elif [ "$_count" -lt 1 ]; then
            _shape="the detector ran but walked NOTHING (files_scanned=$_count)"
        else
            return 0
        fi
    fi

    {
        printf '@@RATCHET_VACUITY_FIRED@@\n'
        printf 'RATCHET VACUITY — no usable scan evidence from ptodo-baseline-gen: %s.\n' "$_shape"
        printf '  NOT a pass: the oracle below is subset-of and the empty set is a subset\n'
        printf '  of everything, so without proof the detector RAN it would observe nothing.\n'
        printf '  A generator that ran over a clean tree still emits the line (with\n'
        printf '  files_scanned >= 1), so zero fingerprints alone is NOT this failure.\n'
        printf '  Most likely cause: target/release/ptodo-baseline-gen is STALE or reverted\n'
        printf '  — a binary predating the contract emits no scan line at all.  Rebuild with\n'
        printf '  `cargo build --release -p reify-audit`, then re-run.\n'
        printf '  Generator stderr as captured:\n'
        printf '%s\n' "$_stderr" | sed 's/^/    | /'
        printf '  Background and the emitted grammar: PRD §6.6\n'
        printf '  (docs/prds/reify-audit-ptodo-detector.md).\n'
    } >&2
    return 1
}

# -----------------------------------------------------------------------
# ITEM 3 meta-test (task 5260): _ratchet_check_subset must NAME the offending
# live fingerprints on stderr so a ratchet regression lands actionable output in
# assert()'s on-FAIL captured-output dump (the 4636 failure produced zero
# fingerprints — esc-4959-57). Unconditional + hermetic: runs before binary
# resolution and independent of RATCHET_SKIP, driven by a synthetic fingerprint
# set (a command-substitution subshell inherits the shell function).
# -----------------------------------------------------------------------
_FAKE_FP=$'crates/foo/src/a.rs:42:PTODO-untracked\ncrates/foo/src/b.rs:7:PTODO-orphaned'
_DIAG="$(_ratchet_check_subset "$_FAKE_FP" 2>&1 1>/dev/null || true)"
assert "ratchet regression diagnostic names the offending fingerprints (4636 actionability)" \
    bash -c 'case "$1" in *a.rs:42:PTODO-untracked*b.rs:7:PTODO-orphaned*) exit 0;; *) exit 1;; esac' -- "$_DIAG"
_EMPTY="$(_ratchet_check_subset "" 2>&1 || true)"
assert "ratchet check is silent + rc0 when live set is empty" \
    bash -c '[ -z "$1" ]' -- "$_EMPTY"

# -----------------------------------------------------------------------
# VACUITY-FLOOR meta-test (task #6241) — pins what
# _ratchet_check_scan_evidence DOES.  Why the floor exists at all, and why it
# keys on generator-emitted SCAN EVIDENCE rather than on the live fingerprint
# count: PRD §6.6 — not restated here.  Its
# WIRING into scenario (a), which this hermetic block cannot see, is pinned by
# tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh.
#
# Same properties as the block above — unconditional, hermetic, driven by
# synthetic strings, and placed BEFORE binary resolution so it runs
# independently of RATCHET_SKIP.  It deliberately mints no temp files: the
# single EXIT trap below (see "Single EXIT trap covers all temp paths") is
# registered later, and a second `trap` here would silently replace it.
#
# Fixtures are synthetic GENERATOR STDERR strings using the real emitted shape
# (`@@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M>`), so the parse is
# genuinely exercised.  The committed baseline is NOT an input to the helper —
# these asserts therefore also pin that the floor is independent of
# ptodo-baseline.txt content, which is the debt coupling #6241 removes.
#
# Asserts go through the machine token, the rc, and values DERIVED FROM THE
# INPUTS (the echoed count) rather than the diagnostic's wording — the prose is
# free to change, the contract is not.
# -----------------------------------------------------------------------

# (i) FIRES when there is NO scan line at all — a generator that never ran, or
# a stale/reverted binary predating the contract.  The fixture is exactly what
# such a pre-#6241 binary prints.
_SCAN_ERR_ABSENT='ptodo-baseline-gen: 0 fingerprint(s) emitted'
_SCAN_ABSENT_RC=0
_SCAN_ABSENT_DIAG="$(_ratchet_check_scan_evidence "$_SCAN_ERR_ABSENT" 2>&1 1>/dev/null)" \
    || _SCAN_ABSENT_RC=$?
assert "vacuity floor fires (rc1 + machine token) when the generator emitted no scan line" \
    bash -c '[ "$2" -eq 1 ] || exit 1
             [ "$(printf "%s\n" "$1" | head -n1)" = "@@RATCHET_VACUITY_FIRED@@" ]' \
    -- "$_SCAN_ABSENT_DIAG" "$_SCAN_ABSENT_RC"

# (ii) FIRES when the line is present but reports files_scanned=0 — the
# detector started and walked NOTHING (a broken `git ls-files` seam).
_SCAN_ERR_ZERO='@@PTODO_SCAN@@ files_scanned=0 markers_examined=0'
_SCAN_ZERO_RC=0
_SCAN_ZERO_DIAG="$(_ratchet_check_scan_evidence "$_SCAN_ERR_ZERO" 2>&1 1>/dev/null)" \
    || _SCAN_ZERO_RC=$?
assert "vacuity floor fires when the scan line reports files_scanned=0 (walked nothing)" \
    bash -c '[ "$2" -eq 1 ] || exit 1
             [ "$(printf "%s\n" "$1" | head -n1)" = "@@RATCHET_VACUITY_FIRED@@" ] || exit 1
             case "$1" in *"files_scanned=0"*) exit 0 ;; *) exit 1 ;; esac' \
    -- "$_SCAN_ZERO_DIAG" "$_SCAN_ZERO_RC"

# (iii) THE DECOUPLING ASSERT — a working detector over a CLEAN tree passes,
# byte-for-byte silent and rc0, even though it emitted zero fingerprints.  That
# partition is exactly what a floor on the live finding count got wrong, and
# what its bash-side kind list existed to paper over (PRD 6.6).  Silence keeps
# an all-green suite's output unchanged (test_helpers.sh:48-51).
_SCAN_ERR_CLEAN='@@PTODO_SCAN@@ files_scanned=1755 markers_examined=0'
_SCAN_CLEAN_RC=0
_SCAN_CLEAN_OUT="$(_ratchet_check_scan_evidence "$_SCAN_ERR_CLEAN" 2>&1)" || _SCAN_CLEAN_RC=$?
assert "vacuity floor is silent + rc0 on scan evidence from a clean tree (zero fingerprints)" \
    bash -c '[ -z "$1" ] && [ "$2" -eq 0 ]' -- "$_SCAN_CLEAN_OUT" "$_SCAN_CLEAN_RC"

# (iv) FIRES on a MALFORMED count — fail LOUD rather than silently disarm, the
# same discipline the retired kind list applied to an unrecognised kind.
_SCAN_ERR_MALFORMED='@@PTODO_SCAN@@ files_scanned=abc markers_examined=0'
_SCAN_MALFORMED_RC=0
_SCAN_MALFORMED_DIAG="$(_ratchet_check_scan_evidence "$_SCAN_ERR_MALFORMED" 2>&1 1>/dev/null)" \
    || _SCAN_MALFORMED_RC=$?
assert "vacuity floor fires on a malformed files_scanned count (loud, never silent-disarm)" \
    bash -c '[ "$2" -eq 1 ] || exit 1
             [ "$(printf "%s\n" "$1" | head -n1)" = "@@RATCHET_VACUITY_FIRED@@" ] || exit 1
             case "$1" in *"abc"*) exit 0 ;; *) exit 1 ;; esac' \
    -- "$_SCAN_MALFORMED_DIAG" "$_SCAN_MALFORMED_RC"

# (v) FIRES when the line is present but carries NO files_scanned field at all
# — the helper's fifth branch, distinct from (iv): the sed yields the EMPTY
# string rather than an unparseable one, and without its own guard that empty
# would fall through to the numeric `-lt` test and emit a raw bash error while
# the branch itself shipped untested.
_SCAN_ERR_NOFIELD='@@PTODO_SCAN@@ markers_examined=0'
_SCAN_NOFIELD_RC=0
_SCAN_NOFIELD_DIAG="$(_ratchet_check_scan_evidence "$_SCAN_ERR_NOFIELD" 2>&1 1>/dev/null)" \
    || _SCAN_NOFIELD_RC=$?
assert "vacuity floor fires when the scan line carries no files_scanned field" \
    bash -c '[ "$2" -eq 1 ] || exit 1
             [ "$(printf "%s\n" "$1" | head -n1)" = "@@RATCHET_VACUITY_FIRED@@" ] || exit 1
             case "$1" in *"no files_scanned field"*) exit 0 ;; *) exit 1 ;; esac' \
    -- "$_SCAN_NOFIELD_DIAG" "$_SCAN_NOFIELD_RC"

# (vi) THE EXTENSIBILITY ASSERT — an ADDITIVE third token must be ignored, even
# when its name ENDS WITH the required field's name.  The §6.6 grammar is open
# for extension and both consumers promise a future counter "cannot turn the
# gate RED"; a parse that matched `files_scanned=` as a SUBSTRING would read
# `skipped_files_scanned`'s 0 here and hard-RED the whole tests/infra PTODO
# gate — the exact false-RED the contract rules out.  Hence the fixture pins
# the adversarial name, not merely an unrelated extra token.  Its Rust
# counterpart (same grammar, other consumer) is
# `parse_scan_line_ignores_unrecognised_tokens` in
# crates/reify-audit/tests/ptodo_baseline.rs.
_SCAN_ERR_EXTRA='@@PTODO_SCAN@@ files_scanned=7 markers_examined=0 skipped_files_scanned=0'
_SCAN_EXTRA_RC=0
_SCAN_EXTRA_OUT="$(_ratchet_check_scan_evidence "$_SCAN_ERR_EXTRA" 2>&1)" || _SCAN_EXTRA_RC=$?
assert "vacuity floor ignores an additive third counter and stays silent + rc0" \
    bash -c '[ -z "$1" ] && [ "$2" -eq 0 ]' -- "$_SCAN_EXTRA_OUT" "$_SCAN_EXTRA_RC"

# -----------------------------------------------------------------------
# Resolve ptodo-baseline-gen binary (ride freshness guard).
# The freshness guard rebuilds target/release/reify-audit (and all crate
# bins, incl. ptodo-baseline-gen) when the binary predates the last
# crates/reify-audit commit.
#
# Testability seam (task #4624): REIFY_AUDIT_BIN and REIFY_PTODO_GEN_BIN can
# be overridden by environment variables for hermetic meta-tests that need to
# exercise the budget-safe skip path without a real binary on disk.
# -----------------------------------------------------------------------
REIFY_AUDIT_BIN="${REIFY_AUDIT_BIN:-$REPO_ROOT/target/release/reify-audit}"
GEN="${REIFY_PTODO_GEN_BIN:-$REPO_ROOT/target/release/ptodo-baseline-gen}"

source "$REPO_ROOT/scripts/reify-audit-freshness.sh"

# Use rebuild-budget-safe mode (task #4624): when REIFY_AUDIT_NO_COLD_BUILD=1
# and the binary is absent/stale, the guard returns 75 (EX_TEMPFAIL skip
# sentinel) instead of invoking `cargo build` inside the 20m run_all.sh wall.
#
# Task #4733 restructure: map 75 → RATCHET_SKIP=1 rather than exit 0.
# This lets scenarios (c)+(d) (the High-severity hard gate) still execute
# when the binary is PRESENT (only stale), while keeping the graceful-skip
# contract for scenarios (a)+(b) (the gen-driven ratchet, which is
# precision-sensitive and genuinely needs a fresh binary).
#
# A truly ABSENT binary cannot run anything; when REIFY_AUDIT_BIN is not
# executable, (c)+(d) skip gracefully with a one-line note.  That case is
# already defended upstream by verify.sh:1049's positive assertion that
# hard-aborts the plan if the pre-build produced no binary.
RATCHET_SKIP=0

# Did ANY scenario actually execute?  Consulted after test_summary; a run that
# asserted nothing must not exit 0.  See NO-SILENT-GREEN FLOOR in the header.
RAN=0

set +e
reify_audit_guard "$REIFY_AUDIT_BIN" rebuild-budget-safe "$REPO_ROOT" 2>&1
_guard_rc=$?
set -e

if [ "$_guard_rc" -eq 75 ]; then
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent/stale and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
    RATCHET_SKIP=1
elif [ "$_guard_rc" -ne 0 ]; then
    # Any other nonzero rc — 125 from reify_audit_guard means the binary is
    # STILL judged stale after the rebuild path ran.  That covers two very
    # different worlds: a failed `cargo build -p reify-audit` (no usable
    # detector at all), and a build that was a legitimate no-op — cargo's
    # fingerprint says up-to-date — while the on-disk mtime still predates the
    # last crates/reify-audit commit, e.g. a warm-lane seeded target/ with
    # stamped mtimes, where the binary is fully usable.
    #
    # So split on detector USABILITY, exactly as the rc-75 partition above
    # does (#5962 review).  Collapsing both worlds into one unconditional
    # `exit 1` would turn every stamped-mtime warm-lane run into a hard RED
    # while emitting a diagnostic ("could not be made usable") that is
    # factually wrong about an executable binary.
    if [ -x "$REIFY_AUDIT_BIN" ]; then
        # PRESENT: the detector runs, so the staleness-tolerant (c)+(d)+(e)
        # hard gate must still execute — that is exactly the #4733 partition
        # documented in the header ("a present-but-stale binary emits High
        # findings correctly").  Only the precision-sensitive ratchet
        # ((a)+(b)) is skipped.  This is not a silent green: those scenarios
        # set $RAN, and the floor after test_summary still refuses a run that
        # executed none of them.
        echo "test_reify_audit_ptodo.sh: reify-audit freshness guard failed (rc=$_guard_rc) but '$REIFY_AUDIT_BIN' is executable — skipping the precision-sensitive ratchet ((a)+(b)); the (c)+(d)+(e)+(f)+(g) hard gate still runs against the stale binary" >&2
        RATCHET_SKIP=1
    else
        # ABSENT: nothing can run.  Leaving RATCHET_SKIP=0 here would look
        # like "ratchet enabled" while the `-x` guards below silently skip
        # every scenario.  That is not a budget-safe skip; it is a broken
        # toolchain, and it must be loud.
        # Pinned by tests/infra/test_reify_audit_ptodo_budget_skip.sh,
        # second invocation (REIFY_AUDIT_NO_COLD_BUILD unset).
        echo "test_reify_audit_ptodo.sh: reify-audit freshness guard failed (rc=$_guard_rc) — the detector could not be made usable and no budget-safe skip was requested; refusing to report green" >&2
        exit 1
    fi
fi

# If ratchet not yet skipped, ensure GEN is available.
# GEN checks are inside RATCHET_SKIP==0 because they only affect the ratchet
# path; the (c)+(d) hard gate runs from REIFY_AUDIT_BIN, not GEN.
if [ "${RATCHET_SKIP}" = "0" ]; then
    if [ ! -x "$GEN" ]; then
        if [ "${REIFY_AUDIT_NO_COLD_BUILD:-0}" = "1" ]; then
            echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen absent and REIFY_AUDIT_NO_COLD_BUILD=1 — SKIP (budget-safe)" >&2
            RATCHET_SKIP=1
        else
            echo "ptodo-baseline-gen not found after freshness guard; building..." >&2
            cargo build --release -q -p reify-audit 2>/dev/null
        fi
    fi

    if [ ! -x "$GEN" ]; then
        echo "test_reify_audit_ptodo.sh: ptodo-baseline-gen unavailable — skipping ratchet" >&2
        RATCHET_SKIP=1
    fi
fi

BASELINE="$REPO_ROOT/crates/reify-audit/ptodo-baseline.txt"

# -----------------------------------------------------------------------
# Single EXIT trap covers all temp paths.  Registering two separate traps
# would silently replace the first with the second, leaking temps on exit.
# -----------------------------------------------------------------------
LIVE_TMP=""
FIX=""         # dirty fixture (scenario b/c): git repo with untracked marker
FIX_LIVE=""
FIX2=""        # scenario (c) clean-fixture temp dir
FIX2_RUNS=""   # scenario (c)/(d)/(e) empty runs-db file
FIX_D=""       # scenario (d) orphaned-cite fixture temp dir
FIX_C_TASKS="" # scenario (c) tasks-file bypass (empty JSON array, avoids MCP loading)
_err_tmp=""    # stderr capture file for run_audit (task #4800 defense-in-depth)
FIX_E=""       # scenario (e) G-allow orphaned-cite fixture temp dir
GEN_ERR_TMP="" # scenario (a) generator stderr capture (6.6 scan evidence, #6241)
FIX_F=""       # scenario (f) δ-A hard-gate fixture temp dir
FIX_F2=""      # scenario (f) δ-A benign control repo temp dir
FIX_G=""       # scenario (g) δ-B hard-gate fixture temp dir
FIX_G2=""      # scenario (g) δ-B benign control repo temp dir
cleanup_all() {
    # Use "|| true" to ensure each line exits 0 even when the variable is empty
    # ([ -n "" ] && rm exits 1 from the short-circuit, which would propagate as
    # the trap's exit code and override the script's exit status).
    [ -n "$LIVE_TMP"    ] && rm -f  "$LIVE_TMP"    || true
    [ -n "$FIX"         ] && rm -rf "$FIX"          || true
    [ -n "$FIX_LIVE"    ] && rm -f  "$FIX_LIVE"    || true
    [ -n "$FIX2"        ] && rm -rf "$FIX2"         || true
    [ -n "$FIX2_RUNS"   ] && rm -f  "$FIX2_RUNS"   || true
    [ -n "$FIX_C_TASKS" ] && rm -f  "$FIX_C_TASKS" || true
    [ -n "$FIX_D"       ] && rm -rf "$FIX_D"        || true
    [ -n "$_err_tmp"    ] && rm -f  "$_err_tmp"     || true
    [ -n "$FIX_E"       ] && rm -rf "$FIX_E"        || true
    [ -n "$GEN_ERR_TMP" ] && rm -f  "$GEN_ERR_TMP"  || true
    [ -n "$FIX_F"       ] && rm -rf "$FIX_F"        || true
    [ -n "$FIX_F2"      ] && rm -rf "$FIX_F2"       || true
    [ -n "$FIX_G"       ] && rm -rf "$FIX_G"        || true
    [ -n "$FIX_G2"      ] && rm -rf "$FIX_G2"       || true
}
trap cleanup_all EXIT

# -----------------------------------------------------------------------
# run_audit — defense-in-depth wrapper (task #4800)
#
# Routes all four reify-audit invocations through a retry+visibility helper:
#
#   (i)  Captures stderr to $_err_tmp so "git ls-files failed"/sqlite
#        breadcrumbs become VISIBLE in merge-gate logs when something goes
#        wrong — closing the 2>/dev/null blind spot that made transient
#        failures invisible.
#
#   (ii) Retries up to 3 times on specific transient-infra exit codes:
#        125 (IO-misconfig / sqlite EMFILE), 101 (Rust panic), 134/137/139
#        (SIGABRT/SIGKILL/SIGSEGV).  This absorbs the SECONDARY sqlite
#        rusqlite::Connection::open EMFILE→125 vector that the Rust
#        RealGitOps spawn-retry does NOT cover.  These codes are assumed
#        transient because the current fixtures only ever produce High-counts
#        of 0 or 1 — well below 101/125 — so there is no collision between
#        "infra failure" and "legitimate finding count".  All other exit
#        codes (not in the set above, and not in {0,1}) are treated as
#        AUTHORITATIVE and not retried.
#
#   Treats rc in {0,1} as AUTHORITATIVE — accepts immediately and never
#   retries — so a genuine wrong-finding-count (a real 0-vs-1 mismatch)
#   still goes RED.  No real assertion is weakened.
#
# Usage: run_audit [reify-audit-args...]
# Returns the final exit code; callers capture it with _exit_*=$?.
# -----------------------------------------------------------------------
_err_tmp="$(mktemp)"
run_audit() {
    local _attempt rc=0 _retried=0
    for _attempt in 1 2 3; do
        rc=0
        env -u REIFY_PTODO_TASKS_DB \
            "$REIFY_AUDIT_BIN" "$@" >/dev/null 2>"$_err_tmp" || rc=$?
        # rc in {0,1} is authoritative — accept immediately, never retry.
        if [ "$rc" -le 1 ]; then
            break
        fi
        # Retry only on the specific transient-infra codes enumerated above.
        # Any other rc (including ≥2 as a High-severity count) is authoritative.
        case "$rc" in
            125|101|134|137|139)
                _retried=1
                if [ "$_attempt" -lt 3 ]; then
                    sleep 2
                fi
                ;;
            *)
                # Non-infra exit code — treat as authoritative, do not retry.
                break
                ;;
        esac
    done
    # Surface captured stderr whenever a retry occurred — the retry itself is
    # the signal worth logging, regardless of the final rc.  This ensures the
    # "git ls-files failed"/sqlite breadcrumb is visible in merge-gate logs
    # even when the retry ultimately succeeds.
    if [ "$_retried" -eq 1 ]; then
        echo "run_audit: transient infra retry occurred (rc=$rc); stderr:" >&2
        cat "$_err_tmp" >&2
    fi
    return "$rc"
}

# -----------------------------------------------------------------------
# (a)+(b) RATCHET and HERMETIC FIXTURE — gen-driven, precision-sensitive.
# Wrapped in RATCHET_SKIP==0 guard (task #4733).
# -----------------------------------------------------------------------
if [ "${RATCHET_SKIP}" = "0" ] && [ -x "$GEN" ]; then
    RAN=1
    LIVE_TMP="$(mktemp)"
    GEN_ERR_TMP="$(mktemp)"

    # -----------------------------------------------------------------------
    # (a) RATCHET: live fingerprints (degraded-structural) must be a subset
    #     of the committed baseline.
    #     The committed baseline was generated WITH the task DB (a superset of
    #     structural + liveness findings), so a structural-only live set is
    #     guaranteed to be a subset of the baseline when the tree is clean.
    #     comm -23 <(sorted live) <(sorted baseline) = lines in live NOT in baseline.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (a) Ratchet: live fingerprints subset of committed baseline ---"

    # Run the generator in degraded-structural mode (no task DB).
    # Stderr is CAPTURED, not discarded: it carries the §6.6 scan-evidence line
    # the floor below reads (alongside an expected missing-DB breadcrumb).
    # Do NOT use || true here: a non-zero exit from the generator signals a broken
    # detector binary, not a missing DB.  The generator exits 0 regardless of
    # finding count; a non-zero exit is an infrastructure failure that must go red.
    env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$REPO_ROOT" \
        >"$LIVE_TMP" 2>"$GEN_ERR_TMP"

    # VACUITY FLOOR (task #6241) — the precondition that makes the subset
    # assertion below meaningful (PRD §6.6).
    # Reported FIRST, deliberately: a reader who saw only the subset assert fail
    # would draw the wrong conclusion about why.  The wiring here (not just the
    # helper) is pinned by tests/infra/test_reify_audit_ptodo_ratchet_vacuity.sh,
    # in BOTH directions.
    # $(cat ...) strips trailing newlines — correct and irrelevant, since the
    # helper reads one field off one line.
    assert "generator emitted scan evidence (subset oracle is not vacuous)" \
        _ratchet_check_scan_evidence "$(cat "$GEN_ERR_TMP")"

    # comm -23 requires both inputs sorted; the generator sorts internally but
    # sort -u here is defensive.
    NEW_IN_LIVE="$(comm -23 <(sort -u "$LIVE_TMP") <(sort -u "$BASELINE"))"

    assert "live fingerprints are a subset of committed baseline (no ratchet regression)" \
        _ratchet_check_subset "$NEW_IN_LIVE"

    # -----------------------------------------------------------------------
    # (b) SCENARIO 13 (hermetic): a fresh untracked marker in a temp git
    #     repo produces fingerprints absent from an empty baseline, proving
    #     the gate would go red on a new violation.
    #
    #     SELF-MATCH SAFETY: the marker token is assembled from a variable
    #     so this .sh source never contains a literal form.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (b) Scenario 13: hermetic fixture detects fresh untracked marker ---"

    FIX="$(mktemp -d)"
    FIX_LIVE="$(mktemp)"

    git -C "$FIX" init -q
    mkdir -p "$FIX/src"

    # Assemble the marker token at runtime so this source file never contains
    # a literal swept token (the written fixture file carries the real marker).
    M="TODO"
    printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
    git -C "$FIX" add -A

    # Run the generator on the hermetic fixture in degraded-structural mode.
    env -u REIFY_PTODO_TASKS_DB "$GEN" --project-root "$FIX" >"$FIX_LIVE" 2>/dev/null || true

    # The fixture live set must contain at least one untracked line for fresh.rs.
    UNTRACKED_LINE="$(grep 'src/fresh.rs' "$FIX_LIVE" | grep ':: untracked ::' || true)"
    assert "fixture live output contains an ':: untracked ::' line for src/fresh.rs" \
        bash -c '[ -n "$1" ]' -- "$UNTRACKED_LINE"

    # comm -23 against an empty baseline must be non-empty (ratchet goes red).
    NEW_IN_FIXTURE="$(comm -23 <(sort -u "$FIX_LIVE") /dev/null)"
    assert "fixture live fingerprints are NOT in empty baseline (gate fires red)" \
        bash -c '[ -n "$1" ]' -- "$NEW_IN_FIXTURE"
fi

# -----------------------------------------------------------------------
# (c) EXIT-CODE HARD GATE (task η, #4559): an untracked marker is
#     Severity::High → reify-audit exits NON-ZERO (exit code = High count).
#
#     Runs whenever REIFY_AUDIT_BIN is executable, independent of
#     staleness/RATCHET_SKIP (task #4733 fix).
#
#     Two hermetic cases using a dedicated dirty fixture (not $FIX from (b),
#     which may be absent when RATCHET_SKIP=1):
#       (c-dirty)  fresh.rs carries marker → non-zero exit
#       (c-clean)  repo with marker-free content → exit 0
#
#     VALIDATED DESIGN:
#       - An empty 0-byte file is an acceptable --runs-db for --pattern PTODO
#         (the CLI opens it but the PTODO lanes never read ctx.conn).
#       - env -u REIFY_PTODO_TASKS_DB prevents a stale env var from routing
#         liveness checks to an unexpected tasks DB.
#       - --tasks-file [] (FIX_C_TASKS) bypasses the MCP task loader entirely;
#         without it the binary tries to connect to fused-memory which may fail
#         with EMFILE or other infra errors, causing exit 125 (not 1).
#       - We test via EXIT CODE, not stream parsing (JSON goes to stderr;
#         the gate cares only about the process exit code = High-count).
#       - Uses structural High kind (untracked) which works without a
#         tasks.db; orphaned (liveness High) is exercised in scenario (d).
#
#     SELF-MATCH SAFETY: marker token assembled from $M at runtime.
# -----------------------------------------------------------------------
if [ -x "$REIFY_AUDIT_BIN" ]; then
    RAN=1
    echo ""
    echo "--- (c) Exit-code hard gate: untracked → High → non-zero exit ---"

    # Set up the dirty fixture for (c).  When scenario (b) ran (RATCHET_SKIP=0),
    # $FIX is already a git repo with src/fresh.rs carrying the marker, so we
    # reuse it.  When RATCHET_SKIP=1 (stale binary, b skipped), $FIX is empty
    # and we create a fresh fixture so (c) is self-contained.
    M="TODO"
    if [ -z "$FIX" ]; then
        FIX="$(mktemp -d)"
        git -C "$FIX" init -q
        mkdir -p "$FIX/src"
        printf '// %s: wire this into the real implementation\n' "$M" > "$FIX/src/fresh.rs"
        git -C "$FIX" add -A
    fi

    FIX2_RUNS="$(mktemp)"
    FIX_C_TASKS="$(mktemp)"
    printf '[]' > "$FIX_C_TASKS"

    # Snapshot FAIL before scenario (c) begins.  @@HARDGATE_C_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (c) asserts — i.e. every assert
    # passed.  A broken gate (any FAIL increment) suppresses the sentinel so the
    # meta-test stays RED (fixes silent_pass_on_failure).  The token contains no
    # TODO/FIXME/HACK substring and appears only in echo lines — SELF-MATCH SAFETY.
    _fail_before_c=$FAIL

    # Guard: assert the precondition — $FIX must be set and src/fresh.rs must
    # be git-tracked.  A failed precondition is an infra failure, not a product
    # regression — fail early with a clear message.
    assert "(c-dirty) precondition: \$FIX set and src/fresh.rs git-tracked" \
        bash -c 'git -C "$1" ls-files --error-unmatch src/fresh.rs >/dev/null 2>&1' -- "$FIX"

    # (c-dirty) marker present → exactly 1 High finding → exit 1.
    # Asserting the exact code (1) distinguishes "gate fired" from "binary errored"
    # (e.g. IO misconfig exits 125, Rust panic exits 101).
    # run_audit retries on transient rc>=2 and surfaces stderr on any retry.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_C_TASKS" \
        --no-jcodemunch
    _exit_dirty=$?
    set -e

    assert "(c-dirty) untracked marker → reify-audit exits 1 (exactly 1 High finding)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_dirty"

    # (c-clean) a clean repo has no High findings → exit 0.
    FIX2="$(mktemp -d)"
    git -C "$FIX2" init -q
    mkdir -p "$FIX2/src"
    printf '// no markers here — purely a comment\n' > "$FIX2/src/clean.rs"
    git -C "$FIX2" add -A

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX2" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_C_TASKS" \
        --no-jcodemunch
    _exit_clean=$?
    set -e

    assert "(c-clean) no markers → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_clean"

    # Emit passing-branch sentinel for scenario (c).  Gated on FAIL counter
    # unchanged — suppressed if any (c) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_c" ] && echo "@@HARDGATE_C_PASSED@@"

    # -----------------------------------------------------------------------
    # (d) ORPHANED-CITE HARD GATE (task #4733): a cite to a DONE task is
    #     classified orphaned→High → reify-audit exits NON-ZERO.
    #
    #     Hermetic recipe (mirrors crates/reify-audit/tests/cli.rs:1632-1716):
    #       - Temp git repo with a single cited marker in src/cited.rs
    #         (assembled from $M + $CITE_ID so this source never contains the
    #         literal swept form — SELF-MATCH SAFETY).
    #       - Seed <repo>/.taskmaster/tasks/tasks.db via sqlite3 with the
    #         production tasks schema + INSERT (master,4444,'done').
    #       - Write [] to a temp file for --tasks-file (bypasses MCP loader
    #         while the PTODO β liveness lane still reads the sqlite3 tasks.db).
    #       - env -u REIFY_PTODO_TASKS_DB prevents stale env from routing.
    #
    #     Two assertions:
    #       (d-orphan)  cited.rs + task done → orphaned High → exit 1
    #       (d-control) UPDATE task to pending → live cite → exit 0
    #
    #     VALIDATED DESIGN (from crates/reify-audit/tests/cli.rs §8.3):
    #       - src/cited.rs has ONLY the cited marker (no bare markers) so the
    #         structural untracked lane does NOT fire → exactly 1 High (orphaned).
    #       - The tasks.db is seeded AFTER git-add to mirror the untracked-in-
    #         worktree reality of a real merge verify.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (d) Orphaned-cite hard gate: done-task cite → orphaned → High → non-zero exit ---"

    FIX_D="$(mktemp -d)"
    git -C "$FIX_D" init -q
    mkdir -p "$FIX_D/src"

    # Assemble the cited marker token at runtime (SELF-MATCH SAFETY).
    M="TODO"
    CITE_ID="4444"
    printf '// %s(#%s): wire the orphaned-cite path\n' "$M" "$CITE_ID" > "$FIX_D/src/cited.rs"
    git -C "$FIX_D" add -A

    # Seed tasks.db AFTER the git commit (mirrors untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_D/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib, not /opt/reify-deps/lib's
    # newer libsqlite3 (set by verify.sh) which would crash with a header/source
    # version mismatch (esc-4581-87).
    #
    # STILL NEEDED, but no longer the primary mechanism (task 5730). verify.sh
    # now scrubs the loader path structurally: apply_env() captures the
    # pre-OCCT ambient into REIFY_AMBIENT_LD_LIBRARY_PATH and every non-cargo
    # plan line is emitted via add_tool(), which restores it — so under the gate
    # this suite no longer inherits the conda prefix at all. These six inline
    # scrubs are retained as DEFENCE IN DEPTH for the one case that fix cannot
    # reach: a bare `bash tests/infra/test_reify_audit_ptodo.sh` run from a
    # shell that already carries a hostile ambient loader path. They cost
    # nothing, so do not "clean them up" — but equally, do not copy this
    # per-call-site pattern into new code. A new verify.sh tool plan line
    # belongs on add_tool(); see docs/notes/verify-pipeline-knobs.md
    # ("Loader path") and tests/infra/test_verify_ld_library_path_scope.sh.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_D_TASKS_FILE="$FIX_D/tasks.json"
    printf '[]' > "$FIX_D_TASKS_FILE"

    # Snapshot FAIL before scenario (d) begins.  @@HARDGATE_D_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (d) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel — SELF-MATCH SAFETY as above.
    _fail_before_d=$FAIL

    # (d-orphan) done task → orphaned → High → exit 1.
    # run_audit retries on transient rc>=2 and surfaces stderr on any retry.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_D" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_D_TASKS_FILE" \
        --no-jcodemunch
    _exit_orphan=$?
    set -e

    assert "(d-orphan) orphaned cite (#${CITE_ID}) → done-task → reify-audit exits 1 (exactly 1 High)" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_orphan"

    # (d-control) UPDATE task status to pending → live cite → no High → exit 0.
    # LD_LIBRARY_PATH="" — see the (d) seed site above (defence-in-depth for a
    # standalone run; the gate itself is fixed structurally, task 5730).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_D/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID};"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_D" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_D_TASKS_FILE" \
        --no-jcodemunch
    _exit_live=$?
    set -e

    assert "(d-control) pending-task cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_live"

    # Emit passing-branch sentinel for scenario (d).  Gated on FAIL counter
    # unchanged — suppressed if any (d) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_d" ] && echo "@@HARDGATE_D_PASSED@@"

    # -----------------------------------------------------------------------
    # (e) G-ALLOW ORPHANED HARD GATE (task #4754): a `// G-allow:` OWNER-cite
    #     pointing to a DONE task is classified g-allow-orphaned→High →
    #     reify-audit exits NON-ZERO.
    #
    #     Mirrors scenario (d) exactly, differing only in the fixture line:
    #     a `// G-allow:` owner marker (assembled from $GA + $CITE_ID_E so
    #     this .sh source never contains a literal swept form — SELF-MATCH
    #     SAFETY) instead of a TODO cite.
    #
    #     check() only scans G-allow markers in .rs files, and the temp git
    #     repo is not under crates/reify-audit/ (the allowlisted prefix), so
    #     the marker in src/gallow.rs is swept and resolved.
    #
    #     Two assertions:
    #       (e-orphan)  gallow.rs + task done → g-allow-orphaned High → exit 1
    #       (e-control) UPDATE task to pending → live cite → exit 0
    #
    #     RED until step-3 removes the `f.severity = Severity::Medium` remap
    #     in check() — currently Medium findings are exit-neutral (exit 0).
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (e) G-allow orphaned hard gate: done-task owner-cite → High → non-zero exit ---"

    FIX_E="$(mktemp -d)"
    git -C "$FIX_E" init -q
    mkdir -p "$FIX_E/src"

    # Assemble the G-allow owner-cite marker at runtime (SELF-MATCH SAFETY:
    # check() only scans .rs files for `// G-allow:` markers, but we use the
    # $GA variable convention to keep this .sh source free of literal owner
    # markers, matching (d)'s $M/$CITE_ID discipline).
    GA="G-allow"
    CITE_ID_E="5555"
    printf '// %s: task #%s owner reason\n' "$GA" "$CITE_ID_E" > "$FIX_E/src/gallow.rs"
    git -C "$FIX_E" add -A

    # Seed tasks.db AFTER the git add (mirrors (d)'s untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_E/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib (esc-4581-87).
    # Defence-in-depth only since task 5730 — verify.sh now scrubs the loader
    # path structurally via add_tool(); see the (d) seed site above.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_E/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID_E}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_E_TASKS_FILE="$FIX_E/tasks.json"
    printf '[]' > "$FIX_E_TASKS_FILE"

    # Snapshot FAIL before scenario (e) begins.  @@HARDGATE_E_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (e) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel.
    _fail_before_e=$FAIL

    # (e-orphan) done task → g-allow-orphaned → High → exit 1.
    # RED until check() removes the Medium remap (step-3 of task #4754).
    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_E" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_E_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_gallow_orphan=$?
    set -e

    assert "(e-orphan) G-allow owner-cite (#${CITE_ID_E}) → done-task → g-allow-orphaned High → reify-audit exits 1" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_gallow_orphan"

    # (e-control) UPDATE task status to pending → live cite → no High → exit 0.
    # LD_LIBRARY_PATH="" — see the (d) seed site above (defence-in-depth for a
    # standalone run; the gate itself is fixed structurally, task 5730).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_E/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID_E};"

    set +e
    env -u REIFY_PTODO_TASKS_DB \
        "$REIFY_AUDIT_BIN" \
            --pattern PTODO \
            --project-root "$FIX_E" \
            --runs-db "$FIX2_RUNS" \
            --tasks-file "$FIX_E_TASKS_FILE" \
            --no-jcodemunch \
            >/dev/null 2>/dev/null
    _exit_gallow_live=$?
    set -e

    assert "(e-control) pending-task G-allow owner-cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_gallow_live"

    # Emit passing-branch sentinel for scenario (e).  Gated on FAIL counter
    # unchanged — suppressed if any (e) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_e" ] && echo "@@HARDGATE_E_PASSED@@"

    # -----------------------------------------------------------------------
    # (f) LANE δ-A HARD GATE (task #6087): an `#[allow(...dead_code...)]`
    #     attribute whose trailing rationale defers the work AND cites a DONE
    #     task is classified orphaned→High → reify-audit exits NON-ZERO.
    #
    #     A direct transposition of scenario (d), inheriting its three hard-won
    #     fixes for free: the LD_LIBRARY_PATH="" sqlite3 workaround
    #     (esc-4581-87), the gate-on-binary-not-on-RATCHET_SKIP structure
    #     (#4733), and the no-silent-green RAN floor.
    #
    #     Three assertions — the third is the one the ratified FP-control
    #     requirement buys, proving the guards at the BINARY level and not only
    #     in unit tests:
    #       (f-orphan)  allowed.rs + task done  → orphaned High → exit 1
    #       (f-control) UPDATE task to pending  → live cite     → exit 0
    #       (f-benign)  non-deferral rationale  → no finding    → exit 0
    #
    #     SELF-MATCH SAFETY: this file is swept and is NOT allowlisted.  Lane
    #     δ-A is .rs-gated so a literal attribute here could not fire it, but
    #     the file's discipline is followed regardless — the attribute text,
    #     the deferral needle and the cite id are assembled from shell
    #     variables at runtime, so no literal attribute+cite+prose triple ever
    #     appears in the committed source.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (f) Lane δ-A hard gate: allow(dead_code) + deferral + done-task cite → High → non-zero exit ---"

    FIX_F="$(mktemp -d)"
    git -C "$FIX_F" init -q
    mkdir -p "$FIX_F/src"

    # Assemble the δ-A anchor at runtime (SELF-MATCH SAFETY, as in (d)/(e)).
    ADC="allow(dead_code)"
    DEFER="pending"
    CITE_ID_F="6666"
    printf '#[%s] // production wiring %s task #%s (hermetic fixture)\n' \
        "$ADC" "$DEFER" "$CITE_ID_F" > "$FIX_F/src/allowed.rs"
    # The benign control lives in the SAME repo but must contribute nothing;
    # its rationale explains rather than defers (the dominant real-world class).
    printf '#[%s] // used by some, but not all, test binaries that include this\n' \
        "$ADC" > "$FIX_F/src/benign.rs"
    git -C "$FIX_F" add -A

    # Seed tasks.db AFTER the git add (mirrors (d)'s untracked-in-worktree reality).
    # Schema mirrors crates/reify-audit/tests/common/schema.rs TASKS_DB_SCHEMA.
    mkdir -p "$FIX_F/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib (esc-4581-87).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_F/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID_F}, 'done');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_F_TASKS_FILE="$FIX_F/tasks.json"
    printf '[]' > "$FIX_F_TASKS_FILE"

    # Snapshot FAIL before scenario (f) begins.  @@HARDGATE_F_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (f) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel.
    _fail_before_f=$FAIL

    # (f-orphan) done task → orphaned → High → exit 1.  Exactly one High: the
    # benign sibling contributes nothing, which is what makes 1 (not 2) the
    # assertion that also proves FP control.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_orphan=$?
    set -e

    assert "(f-orphan) allow(dead_code) deferral cite (#${CITE_ID_F}) → done-task → orphaned High → reify-audit exits 1" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_adc_orphan"

    # (f-control) UPDATE task status to pending → live cite → no High → exit 0.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_F/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID_F};"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_live=$?
    set -e

    assert "(f-control) pending-task allow(dead_code) deferral cite → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_adc_live"

    # (f-benign) a repo carrying ONLY non-deferral allow-rationales → no
    # finding at all → exit 0.  This is the FP-control guard proven at the
    # binary level: an over-broad lane fires `untracked`→High here and the
    # exit code becomes non-zero.
    FIX_F2="$(mktemp -d)"
    git -C "$FIX_F2" init -q
    mkdir -p "$FIX_F2/src"
    {
        printf '#[%s] // used by some, but not all, test binaries that include this\n' "$ADC"
        printf '#[%s] // superseded by mark_%s_with_cause at the wire site\n' "$ADC" "$DEFER"
        printf '#[%s] // constructs a Pending producer for the demand-prune tests\n' "$ADC"
        printf '#[%s] // serialises the "%s" wire tag for the GUI bridge\n' "$ADC" "$DEFER"
    } > "$FIX_F2/src/benign.rs"
    git -C "$FIX_F2" add -A

    FIX_F2_TASKS_FILE="$FIX_F2/tasks.json"
    printf '[]' > "$FIX_F2_TASKS_FILE"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_F2" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_F2_TASKS_FILE" \
        --no-jcodemunch
    _exit_adc_benign=$?
    set -e

    assert "(f-benign) non-deferral allow(dead_code) rationales (incl. identifier/enum-variant/quoted-state guards) → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_adc_benign"

    # Emit passing-branch sentinel for scenario (f).  Gated on FAIL counter
    # unchanged — suppressed if any (f) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_f" ] && echo "@@HARDGATE_F_PASSED@@"

    # -----------------------------------------------------------------------
    # (g) LANE δ-B HARD GATE (task #6103): an ORDINARY comment — no marker
    #     token, no attribute, nothing but comment text — that both DEFERS the
    #     work AND cites a TERMINAL task is classified orphaned→High →
    #     reify-audit exits NON-ZERO.
    #
    #     A direct transposition of scenario (f), inheriting the same hard-won
    #     fixes: the LD_LIBRARY_PATH="" sqlite3 workaround (esc-4581-87), the
    #     gate-on-binary-not-on-RATCHET_SKIP structure (#4733), and the
    #     no-silent-green RAN floor (block-level, shared with (c)-(f)).
    #
    #     The cite is seeded 'cancelled' rather than 'done' — the live
    #     deliverable this lane exists to surface
    #     (crates/reify-core/src/diagnostics.rs, "blocked on VolumeMesh
    #     realization (task #2947)") cites a CANCELLED task, and (d)/(e)/(f)
    #     already cover 'done'.  Both are terminal per §8.4.
    #
    #     Three assertions, matching (f)'s orphan/control/benign triple:
    #       (g-orphan)  deferred.rs + cancelled task → orphaned High → exit 1
    #       (g-control) UPDATE task to pending       → live cite     → exit 0
    #       (g-benign)  FP-guard control repo        → no finding    → exit 0
    #
    #     (g-benign) is the assertion that matters most, and it is deliberately
    #     built to be DISCRIMINATING rather than merely quiet.  Its repo carries
    #     a seeded tasks.db in which every cite is TERMINAL, so an over-firing
    #     lane produces orphaned→High and flips the exit code.  Without that
    #     seeding the liveness lane would degrade on a missing DB and the repo
    #     would exit 0 whether the guards held or not — a control that cannot
    #     fail.  Its four populations are exactly the ones task #6087 measured:
    #       class (a) the needle inside an IDENTIFIER (mark_pending_with_cause);
    #       class (b) a PRD-RELATIVE cite (deferred to PRD task #N) — §8.2;
    #       class (c) a deferral with NO cite (δ-B is cite-anchored);
    #       class (d) a G-allow marker, which belongs to its own lane.  Its
    #                 owner cite is written with "PRD " immediately left so the
    #                 G-allow lane's own rule (c) exempts it and stays silent —
    #                 which leaves δ-B's g_allow_marker_body guard as the only
    #                 thing preventing a finding, exactly as in
    #                 crates/reify-audit/tests/fixtures/ptodo/scenario20_delta_b_cited_deferral.rs.
    #
    #     SELF-MATCH SAFETY: this file is swept and is NOT allowlisted.  Lane
    #     δ-B is .rs-gated so a literal cited-deferral comment here could not
    #     fire it, but the file's discipline is followed regardless — every
    #     deferral needle and cite id is assembled from shell variables at
    #     runtime, so no literal deferral+cite pair ever appears in the
    #     committed source.
    # -----------------------------------------------------------------------
    echo ""
    echo "--- (g) Lane δ-B hard gate: cited deferral in an ordinary comment + terminal cite → High → non-zero exit ---"

    FIX_G="$(mktemp -d)"
    git -C "$FIX_G" init -q
    mkdir -p "$FIX_G/src"

    # Assemble the δ-B anchor at runtime (SELF-MATCH SAFETY, as in (d)/(e)/(f)).
    DEFER_G="blocked on"
    CITE_ID_G="7777"
    printf '/// VolumeMesh realization is %s task #%s (hermetic fixture)\nfn f() {}\n' \
        "$DEFER_G" "$CITE_ID_G" > "$FIX_G/src/deferred.rs"
    git -C "$FIX_G" add -A

    # Seed tasks.db AFTER the git add (mirrors (d)/(f)'s untracked-in-worktree
    # reality).  Schema mirrors crates/reify-audit/tests/common/schema.rs.
    mkdir -p "$FIX_G/.taskmaster/tasks"
    # LD_LIBRARY_PATH="" so sqlite3 uses the system lib (esc-4581-87).
    LD_LIBRARY_PATH="" sqlite3 "$FIX_G/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', ${CITE_ID_G}, 'cancelled');
"

    # Write an empty JSON array for --tasks-file (bypasses MCP; liveness lane
    # still reads the sqlite3 tasks.db at <project_root>/.taskmaster/tasks/tasks.db).
    FIX_G_TASKS_FILE="$FIX_G/tasks.json"
    printf '[]' > "$FIX_G_TASKS_FILE"

    # Snapshot FAIL before scenario (g) begins.  @@HARDGATE_G_PASSED@@ is emitted
    # ONLY when the counter is unchanged after all (g) asserts — i.e. every assert
    # passed.  A broken gate suppresses the sentinel.
    _fail_before_g=$FAIL

    # (g-orphan) cancelled task → orphaned → High → exit 1.
    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_G" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_G_TASKS_FILE" \
        --no-jcodemunch
    _exit_db_orphan=$?
    set -e

    assert "(g-orphan) cited deferral comment (#${CITE_ID_G}) → cancelled task → orphaned High → reify-audit exits 1" \
        bash -c '[ "$1" -eq 1 ]' -- "$_exit_db_orphan"

    # (g-control) UPDATE task status to pending → live cite → no High → exit 0.
    LD_LIBRARY_PATH="" sqlite3 "$FIX_G/.taskmaster/tasks/tasks.db" \
        "UPDATE tasks SET status='pending' WHERE id=${CITE_ID_G};"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_G" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_G_TASKS_FILE" \
        --no-jcodemunch
    _exit_db_live=$?
    set -e

    assert "(g-control) pending-task cited deferral comment → live cite → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_db_live"

    # (g-benign) a repo carrying ONLY lines that must be saved by a guard → no
    # finding at all → exit 0.  Every cite below is seeded TERMINAL, so an
    # over-broad lane fires orphaned→High here and the exit code becomes
    # non-zero: the control can actually fail.
    FIX_G2="$(mktemp -d)"
    git -C "$FIX_G2" init -q
    mkdir -p "$FIX_G2/src"

    DEFER_A="pending"
    DEFER_B="deferred to"
    DEFER_C="not yet"
    {
        # class (a) — the needle is inside an identifier (has_deferral_prose guard 3).
        printf '// cause via mark_%s_with_cause (task #2330 §9.2 invariant).\n' "$DEFER_A"
        printf '// --- mark_pruned_%s producer tests (task #4739) ---\n' "$DEFER_A"
        # class (b) — the cite is PRD-relative, not a task id (§8.2).
        printf '/// Diagnostic emission is %s PRD task #10 (Diagnostic mapping for\n' "$DEFER_B"
        printf '//   is %s a hydrated Value::GeometryHandle (PRD invariant #2:\n' "$DEFER_C"
        # class (c) — a deferral with no cite at all (δ-B is cite-anchored).
        printf '/// wiring is %s the morph rewrite\n' "$DEFER_A"
        # class (d) — a G-allow marker; "PRD " left of the cite keeps the
        # G-allow lane itself silent (its rule (c)), isolating δ-B's guard.
        printf '// G-allow: shared envelope assembler; the four surfaces are %s PRD #8888 — no non-test caller until then\n' "$DEFER_G"
    } > "$FIX_G2/src/benign.rs"
    git -C "$FIX_G2" add -A

    mkdir -p "$FIX_G2/.taskmaster/tasks"
    LD_LIBRARY_PATH="" sqlite3 "$FIX_G2/.taskmaster/tasks/tasks.db" "
CREATE TABLE tasks (
    tag TEXT NOT NULL DEFAULT 'master',
    id INTEGER NOT NULL,
    title TEXT,
    status TEXT NOT NULL,
    metadata TEXT,
    PRIMARY KEY (tag, id)
);
INSERT INTO tasks (tag, id, status) VALUES ('master', 2330, 'done');
INSERT INTO tasks (tag, id, status) VALUES ('master', 4739, 'done');
INSERT INTO tasks (tag, id, status) VALUES ('master', 10, 'done');
INSERT INTO tasks (tag, id, status) VALUES ('master', 2, 'done');
INSERT INTO tasks (tag, id, status) VALUES ('master', 8888, 'done');
"

    FIX_G2_TASKS_FILE="$FIX_G2/tasks.json"
    printf '[]' > "$FIX_G2_TASKS_FILE"

    set +e
    run_audit \
        --pattern PTODO \
        --project-root "$FIX_G2" \
        --runs-db "$FIX2_RUNS" \
        --tasks-file "$FIX_G2_TASKS_FILE" \
        --no-jcodemunch
    _exit_db_benign=$?
    set -e

    assert "(g-benign) identifier / PRD-relative / uncited / G-allow lines, all cites seeded terminal → reify-audit exits 0" \
        bash -c '[ "$1" -eq 0 ]' -- "$_exit_db_benign"

    # Emit passing-branch sentinel for scenario (g).  Gated on FAIL counter
    # unchanged — suppressed if any (g) assert failed (fixes silent_pass_on_failure).
    [ "$FAIL" -eq "$_fail_before_g" ] && echo "@@HARDGATE_G_PASSED@@"
else
    echo ""
    echo "test_reify_audit_ptodo.sh: reify-audit binary absent at '$REIFY_AUDIT_BIN' — (c)+(d)+(e)+(f)+(g) hard gate could not run" >&2
fi

# -----------------------------------------------------------------------
# Summary
#
# test_summary exits 1 when FAIL > 0, so control only reaches the $RAN floor
# on the otherwise-all-green path — which is exactly where a zero-assertion
# run would have been laundered into a passing hard gate.
# -----------------------------------------------------------------------
test_summary

if [ "$RAN" -eq 0 ]; then
    echo "test_reify_audit_ptodo.sh: NO scenario executed (REIFY_AUDIT_BIN='$REIFY_AUDIT_BIN' not executable; RATCHET_SKIP=$RATCHET_SKIP, guard rc=$_guard_rc) — refusing to report green for a hard gate that asserted nothing" >&2
    exit 1
fi
