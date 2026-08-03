#!/usr/bin/env bash
# Generate a temp nextest config file with BOTH concurrency caps set from the
# environment:
#   * the occt test-group cap  (REIFY_OCCT_NEXTEST_MAX_THREADS, task 4503/4621)
#   * the global [profile.default] test-threads pool cap
#                              (REIFY_NEXTEST_TEST_THREADS,     task 5984)
#
# Stdout contract: prints ONLY the resolved temp file path (bare path).
# All diagnostics go to stderr.  Mirrors scripts/setup-worktree-debug-port.sh.
#
# Usage:
#   cfg=$(scripts/gen-nextest-config.sh)
#   cargo nextest run ... --config-file "$cfg"
#   rm -f "$cfg"
#
# The caller is responsible for cleanup; scripts/verify.sh registers a top-level
# _verify_cleanup EXIT trap that removes $_NEXTEST_CONFIG_FILE on all exit paths.
#
# NOTE: cargo nextest --config overrides CARGO configuration (its --help says
# "Override a Cargo configuration value").  It is a NO-OP for nextest's own
# test-groups — those are read ONLY from a config file.  --config-file <PATH>
# (highest nextest config precedence) is the correct mechanism for the cap
# (verified on cargo-nextest 0.9.136 via `cargo nextest show-config test-groups`).
#
# Cap derivation (when REIFY_OCCT_NEXTEST_MAX_THREADS is not set explicitly):
#   cap = min(HARD_CAP, nproc, ram_bound)   [each term skipped if unavailable]
#
#   HARD_CAP:  REIFY_OCCT_NEXTEST_HARD_CAP (default 24, strict digits-only).
#   nproc:     REIFY_OCCT_NPROC (testability knob) if valid; else `nproc`;
#              else `getconf _NPROCESSORS_ONLN`; else CPU term is skipped
#              (cap = HARD_CAP), preserving today's behavior on hosts without
#              either tool.
#   ram_bound: floor(MemTotalGiB / GIB_PER_THREAD).
#              GIB_PER_THREAD from REIFY_OCCT_GIB_PER_THREAD (default 2).
#              MemTotalGiB from REIFY_OCCT_MEMTOTAL_GIB (testability knob);
#              else parsed from /proc/meminfo (MemTotal kB → GiB); else RAM
#              term is skipped so non-Linux/unreadable hosts keep CPU-only.
#
# Global pool derivation (task 5984; when REIFY_NEXTEST_TEST_THREADS is unset):
#   tt = min(HARD_CAP, nproc)                [nproc term skipped if unavailable]
#
#   HARD_CAP:  REIFY_NEXTEST_TEST_THREADS_HARD_CAP (default 16, strict
#              digits-only).  16 matches the dark-factory half
#              (dark_factory:3589, pytest `-n auto` -> `-n 16`) so neither
#              fleet gains CPU share over the other.
#   nproc:     the SAME resolved host CPU count as above.
#
#   The result is clamped to >= 1: HARD_CAP=0 is digits-only-VALID and would
#   otherwise emit `test-threads = 0`, which nextest reads as UNBOUNDED.
#
#   NO RAM TERM — deliberate, and asymmetric with the occt cap ON PURPOSE.
#   This cap's justification is CPU-share symmetry and runqueue depth, not RSS:
#   orchestrator-reify.service holds 25.3 GiB memory.current against only
#   3.6 GiB anon (rest = reclaimable cargo page cache), and a live
#   df-verify-reify-*.scope measured 25.4 GiB current / 0.1 GiB anon.  OCCT
#   threads really do carry ~2 GiB anon each, which is why that term belongs on
#   the occt cap and not here.  Full rationale: .config/nextest.toml.
#
#   Host-relative rather than a bare literal, for the task 4621 reason: a fixed
#   16 equals nproc on a 16t laptop (no reduction at all) and oversubscribes an
#   8-core host or CPU-quota'd container 2x.
#
# Env knobs (all strictly digits-only validated):
#   REIFY_OCCT_NEXTEST_MAX_THREADS  — explicit occt-group override; wins verbatim.
#   REIFY_OCCT_NEXTEST_HARD_CAP     — occt-group upper ceiling (default 24).
#   REIFY_OCCT_NPROC                — inject CPU count (testability, CI injection).
#                                       NOTE: the OCCT_ prefix is HISTORICAL
#                                       (it landed with task 4621's occt-only
#                                       derivation).  The value is the host's
#                                       logical CPU count — nothing about it is
#                                       OCCT-specific — and it is now the single
#                                       injection point for BOTH derivations.
#                                       Deliberately NOT aliased to a second
#                                       name: two knobs would need a precedence
#                                       rule and duplicated tests for zero added
#                                       expressiveness.
#   REIFY_OCCT_MEMTOTAL_GIB         — inject MemTotal in GiB (testability).
#   REIFY_OCCT_GIB_PER_THREAD       — GiB per OCCT thread (default 2; see
#                                       docs/notes/multi-process-occt-bench.md).
#   REIFY_NEXTEST_TEST_THREADS      — explicit global-pool override; wins verbatim.
#   REIFY_NEXTEST_TEST_THREADS_HARD_CAP
#                                   — global-pool upper ceiling (default 16).
#                                       Ignored when REIFY_NEXTEST_TEST_THREADS
#                                       is set (the explicit override wins).
#
# occt cap:
#   Workstation (32t, ~125 GiB): min(24,32,62)=24 — bit-identical to pre-4621.
#   Laptop (16t, 32 GiB):        min(24,16,16)=16 — avoids 24×2 GiB ≈ 48 GiB OOM.
#   Laptop (16t, 16 GiB):        min(24,16,8)=8.
# global pool:
#   Workstation (32t): min(16,32)=16 — the reduction this task exists for.
#   Laptop (16t):      min(16,16)=16.
#   8-core host:       min(16,8)=8   — no oversubscription.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---------------------------------------------------------------------------
# Host logical-CPU count — resolved ONCE here (task 5984 hoist) and shared by
# BOTH derivations below (the occt test-group cap and the global test-threads
# pool cap).  Precedence and validation are preserved verbatim from the
# occt-only form this replaces: REIFY_OCCT_NPROC → `nproc` →
# `getconf _NPROCESSORS_ONLN` → empty (the CPU term is then skipped by each
# caller, preserving today's behavior on hosts without either tool).
#
# Behaviour delta from the hoist: nproc is now resolved even when
# REIFY_OCCT_NEXTEST_MAX_THREADS is set explicitly — one extra subprocess, no
# change to any resolved value.
# ---------------------------------------------------------------------------
_resolve_host_nproc() {
    local n=""
    case "${REIFY_OCCT_NPROC:-}" in
        (''|*[!0-9]*) ;;   # invalid or unset — try system commands below
        (*)           n="${REIFY_OCCT_NPROC}" ;;
    esac
    if [ -z "$n" ]; then
        if command -v nproc >/dev/null 2>&1; then
            n="$(nproc 2>/dev/null || true)"
        fi
    fi
    if [ -z "$n" ]; then
        if command -v getconf >/dev/null 2>&1; then
            n="$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)"
        fi
    fi
    # Validate: must be a non-empty pure-digit string to be used.
    case "${n:-}" in
        (''|*[!0-9]*) n="" ;;
    esac
    printf '%s' "$n"
}

_nproc="$(_resolve_host_nproc)"

# Resolve the cap — strict digits-only check, same rule as parse_debug_port.
# Empty, non-digit, whitespace-padded, or otherwise non-numeric → default 24.
case "${REIFY_OCCT_NEXTEST_MAX_THREADS:-}" in
    (''|*[!0-9]*)
        # Explicit override not set (or invalid) — derive host-relative cap.

        # HARD_CAP: upper ceiling, default 24.
        case "${REIFY_OCCT_NEXTEST_HARD_CAP:-}" in
            (''|*[!0-9]*) hard_cap=24 ;;
            (*)           hard_cap="${REIFY_OCCT_NEXTEST_HARD_CAP}" ;;
        esac

        # nproc: resolved once at top level by _resolve_host_nproc ($_nproc).

        # GIB_PER_THREAD: GiB consumed per concurrent OCCT thread (default 2).
        case "${REIFY_OCCT_GIB_PER_THREAD:-}" in
            (''|*[!0-9]*|0) gib_per_thread=2 ;;   # default; 0 is nonsensical
            (*)              gib_per_thread="${REIFY_OCCT_GIB_PER_THREAD}" ;;
        esac

        # MemTotal in GiB: testability-injectable, then /proc/meminfo, else skip.
        _memtotal_gib=""
        case "${REIFY_OCCT_MEMTOTAL_GIB:-}" in
            (''|*[!0-9]*) ;;   # invalid or unset — try /proc/meminfo below
            (*)           _memtotal_gib="${REIFY_OCCT_MEMTOTAL_GIB}" ;;
        esac
        if [ -z "$_memtotal_gib" ] && [ -r /proc/meminfo ]; then
            # MemTotal is in kB; convert to GiB (floor).
            _memtotal_kb="$(grep '^MemTotal:' /proc/meminfo | awk '{print $2}' || true)"
            case "${_memtotal_kb:-}" in
                (''|*[!0-9]*) ;;
                (*)           _memtotal_gib=$(( 10#${_memtotal_kb} / 1048576 )) ;;
            esac
        fi

        # min(HARD_CAP, nproc, ram_bound) — skip any term that is unavailable.
        cap="$hard_cap"
        if [ -n "$_nproc" ] && [ "$_nproc" -lt "$cap" ]; then
            cap="$_nproc"
        fi
        if [ -n "$_memtotal_gib" ] && [ "$_memtotal_gib" -gt 0 ]; then
            # Force base-10 arithmetic: leading-zero env values (e.g. 08) would be
            # interpreted as invalid octal by $(( )) and abort under set -e.
            _ram_bound=$(( 10#${_memtotal_gib} / 10#${gib_per_thread} ))
            # Clamp to minimum 1: floor(MemTotal / GIB_PER_THREAD) = 0 when the host
            # has less RAM than a single OCCT thread's budget (e.g. a small container).
            # nextest rejects max-threads = 0 as an invalid test-group config.
            if [ "$_ram_bound" -lt 1 ]; then _ram_bound=1; fi
            if [ "$_ram_bound" -lt "$cap" ]; then
                cap="$_ram_bound"
            fi
        fi
        ;;
    (*)
        cap="${REIFY_OCCT_NEXTEST_MAX_THREADS}"
        ;;
esac

# ---------------------------------------------------------------------------
# Global [profile.default] test-threads pool cap (task 5984).
#
# tt = min(HARD_CAP=16, nproc)  — the nproc term is skipped when unavailable,
# mirroring the occt min()'s skip-unavailable-term structure above, so a host
# without nproc/getconf falls back to HARD_CAP rather than erroring.
#
# Same strict digits-only parse idiom as REIFY_OCCT_NEXTEST_MAX_THREADS: empty,
# non-digit or whitespace-padded input falls through to the derivation.
# ---------------------------------------------------------------------------
case "${REIFY_NEXTEST_TEST_THREADS:-}" in
    (''|*[!0-9]*)
        # Explicit override not set (or invalid) — derive host-relative cap.

        # HARD_CAP: upper ceiling, default 16 (matches dark_factory:3589's
        # pytest -n 16 so neither fleet gains CPU share over the other).
        case "${REIFY_NEXTEST_TEST_THREADS_HARD_CAP:-}" in
            (''|*[!0-9]*) tt_hard_cap=16 ;;
            (*)           tt_hard_cap="${REIFY_NEXTEST_TEST_THREADS_HARD_CAP}" ;;
        esac

        # Force base-10: a leading-zero env value (e.g. 08) would otherwise be
        # read as invalid octal by $(( )) and abort under set -euo pipefail.
        tt=$(( 10#${tt_hard_cap} ))
        if [ -n "$_nproc" ] && [ "$(( 10#${_nproc} ))" -lt "$tt" ]; then
            tt=$(( 10#${_nproc} ))
        fi
        ;;
    (*)
        tt=$(( 10#${REIFY_NEXTEST_TEST_THREADS} ))
        ;;
esac

# Clamp to minimum 1.  HARD_CAP=0 is digits-only-VALID, so it passes the parse
# guard above and would otherwise emit `test-threads = 0` — which nextest reads
# as UNBOUNDED, i.e. exactly the uncapped pool this cap exists to prevent,
# reached via the cap knob itself.  Same defence the occt ram_bound carries.
if [ "$tt" -lt 1 ]; then tt=1; fi

# Create a fresh temp file.  Template ends in X's (do NOT combine --suffix with
# a .toml-terminated template — that form errors on some systems; empirically
# confirmed during planning that the plain X-terminated template works).
tmp=$(mktemp "${TMPDIR:-/tmp}/reify-nextest-occt.XXXXXX")

# Write a full copy of .config/nextest.toml with ONLY the two cap literal lines
# rewritten to their resolved values.  Both substitutions are ANCHORED, so the
# [[profile.default.overrides]] blocks are preserved verbatim (including the
# per-test slow-timeout/terminate-after ceilings, task 5141) and every other
# [profile.default] key is untouched.
#
# `^test-threads = [0-9][0-9]*$` (task 5984) is unique in the file — the
# overrides blocks carry no such key.
#
# Both forms fail SAFE: if a future edit breaks an anchor the substitution
# silently no-ops and the generated config keeps that line's in-file literal —
# still a BOUNDED cap, never an uncapped pool.  Because a no-op is invisible at
# runtime, Assertion I in tests/infra/test_nextest_slow_priority.sh rewrites
# BOTH anchors in ONE generate and checks both landed, converting the silent
# no-op into a loud CI failure.
sed -e "s/^occt = { max-threads = [0-9][0-9]* }$/occt = { max-threads = ${cap} }/" \
    -e "s/^test-threads = [0-9][0-9]*$/test-threads = ${tt}/" \
    "$REPO_ROOT/.config/nextest.toml" > "$tmp"

# Stdout contract: ONLY the path.
printf '%s\n' "$tmp"
