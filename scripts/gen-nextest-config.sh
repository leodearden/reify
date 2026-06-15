#!/usr/bin/env bash
# Generate a temp nextest config file with the occt max-threads cap set from
# the environment (REIFY_OCCT_NEXTEST_MAX_THREADS, default: host-relative min).
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
#   cap = min(HARD_CAP, nproc[, ram_bound])
#
#   HARD_CAP: REIFY_OCCT_NEXTEST_HARD_CAP (default 24, strict digits-only).
#   nproc:    REIFY_OCCT_NPROC (testability knob) if valid; else `nproc`;
#             else `getconf _NPROCESSORS_ONLN`; else CPU term is skipped
#             (cap = HARD_CAP), preserving today's behavior on hosts without
#             either tool.
#   ram_bound: added in a later step (REIFY_OCCT_MEMTOTAL_GIB / /proc/meminfo).
#
# Env knobs (all strictly digits-only validated):
#   REIFY_OCCT_NEXTEST_MAX_THREADS  — explicit override; wins verbatim.
#   REIFY_OCCT_NEXTEST_HARD_CAP     — upper ceiling (default 24).
#   REIFY_OCCT_NPROC                — inject CPU count (testability, CI injection).
#
# Workstation (32t): min(24,32)=24 — bit-identical to pre-4621 behavior.
# Laptop (16t):      min(24,16)=16 — avoids 24×2 GiB ≈ 48 GiB OOM risk.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

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

        # nproc: testability-injectable CPU count.
        _nproc=""
        case "${REIFY_OCCT_NPROC:-}" in
            (''|*[!0-9]*) ;;   # invalid or unset — try system commands below
            (*)           _nproc="${REIFY_OCCT_NPROC}" ;;
        esac
        if [ -z "$_nproc" ]; then
            if command -v nproc >/dev/null 2>&1; then
                _nproc="$(nproc 2>/dev/null || true)"
            fi
        fi
        if [ -z "$_nproc" ]; then
            if command -v getconf >/dev/null 2>&1; then
                _nproc="$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)"
            fi
        fi
        # Validate: must be a non-empty pure-digit string to be used.
        case "${_nproc:-}" in
            (''|*[!0-9]*) _nproc="" ;;
        esac

        # min(HARD_CAP, nproc) — skip nproc term if unavailable.
        cap="$hard_cap"
        if [ -n "$_nproc" ] && [ "$_nproc" -lt "$cap" ]; then
            cap="$_nproc"
        fi
        ;;
    (*)
        cap="${REIFY_OCCT_NEXTEST_MAX_THREADS}"
        ;;
esac

# Create a fresh temp file.  Template ends in X's (do NOT combine --suffix with
# a .toml-terminated template — that form errors on some systems; empirically
# confirmed during planning that the plain X-terminated template works).
tmp=$(mktemp "${TMPDIR:-/tmp}/reify-nextest-occt.XXXXXX")

# Write a full copy of .config/nextest.toml with ONLY the occt literal line
# rewritten to the resolved cap.  Anchored sed substitution preserves the
# [[profile.default.overrides]] filter verbatim.
sed "s/^occt = { max-threads = [0-9][0-9]* }$/occt = { max-threads = ${cap} }/" \
    "$REPO_ROOT/.config/nextest.toml" > "$tmp"

# Stdout contract: ONLY the path.
printf '%s\n' "$tmp"
