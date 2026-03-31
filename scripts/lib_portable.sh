#!/usr/bin/env bash
# Portable shell helpers for reify build scripts and infrastructure tests.
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_portable.sh"
#   or:   source "$REPO_ROOT/scripts/lib_portable.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_PORTABLE_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_PORTABLE_SH_SOURCED=1

# Portable SHA-256: prefer sha256sum (GNU coreutils), fall back to shasum (macOS).
portable_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    else
        echo "ERROR: neither sha256sum nor shasum found on PATH." >&2
        return 1
    fi
}
