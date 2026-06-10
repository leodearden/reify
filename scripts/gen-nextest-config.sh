#!/usr/bin/env bash
# Generate a temp nextest config file with the occt max-threads cap set from
# the environment (REIFY_OCCT_NEXTEST_MAX_THREADS, default 24).
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
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Resolve the cap — strict digits-only check, same rule as parse_debug_port.
# Empty, non-digit, whitespace-padded, or otherwise non-numeric → default 24.
case "${REIFY_OCCT_NEXTEST_MAX_THREADS:-}" in
    (''|*[!0-9]*) cap=24 ;;
    (*)           cap="${REIFY_OCCT_NEXTEST_MAX_THREADS}" ;;
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
