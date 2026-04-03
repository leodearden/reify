#!/usr/bin/env bash
# Shared shell utilities for reify build scripts.
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_SH_SOURCED=1

# Source portable helpers (portable_sha256, portable_timeout, etc.)
_LIB_SH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$_LIB_SH_DIR/lib_portable.sh"

# Backward-compatible wrapper: compute_sha256 delegates to portable_sha256.
compute_sha256() {
    portable_sha256 "$@"
}
