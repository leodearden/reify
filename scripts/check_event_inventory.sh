#!/usr/bin/env bash
# scripts/check_event_inventory.sh
#
# Lint for Tauri event-channel drift (GR-016 §9 task μ).
# Policy: PRD §11 Q4 — start as warning; promote via --strict after
#         one release cycle of observed drift.
#
# Extracts literal channel names from .emit("name", …) call sites under
# gui/src-tauri/ and warns if any are absent from docs/gui-event-channels.md.
#
# Dynamic emit-sites (app.emit(&name, …), app.emit(event_name, …)) are
# intentionally skipped: their channel names live in delta_to_events / MCP
# context emitters and are covered by the lockstep-commit convention (PRD §3.3),
# not by this regex lint.
#
# Usage: scripts/check_event_inventory.sh [--strict] [--repo-root <dir>]
#
# Exit codes:
#   0  always (warning mode) unless --strict is given and orphans are found
#   1  only when --strict and at least one orphan channel is detected

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/check_event_inventory.sh [options]

Options:
  --strict         Exit 1 when any orphan channels are detected.
                   Default: warning mode (always exits 0).
  --repo-root DIR  Repository root (default: git rev-parse --show-toplevel).
  -h, --help       Show this message.
USAGE
}

STRICT=0
REPO_ROOT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict) STRICT=1; shift ;;
        --repo-root) REPO_ROOT="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

if [[ -z "$REPO_ROOT" ]]; then
    REPO_ROOT="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
fi

INVENTORY="$REPO_ROOT/docs/gui-event-channels.md"
SRC_DIR="$REPO_ROOT/gui/src-tauri"

if [[ ! -f "$INVENTORY" ]]; then
    echo "ERROR: inventory file not found: $INVENTORY" >&2
    exit 1
fi

if [[ ! -d "$SRC_DIR" ]]; then
    echo "ERROR: source directory not found: $SRC_DIR" >&2
    exit 1
fi

# Build the set of registered channel names using the published grep contract:
# | `channel-name` | — matches every event-channel row in §1 / §2.
# §2a (Tauri commands) also matches but is a harmless superset: command names
# won't appear at emit-sites in correct code, so no false positives result.
registered=$(grep -oP '\| `\K[a-z0-9-]+(?=` \|)' "$INVENTORY" | sort -u || true)

# Extract literal channel names from .emit("name", …) call sites in Rust.
# Uses perl slurp mode (-0777) so \s* can span the newline between .emit( and
# the quoted argument — e.g. the multi-line form:
#   app.emit(
#       "evaluation-status",
# Dynamic forms (.emit(&name, …) or .emit(event_name, …)) produce no match.
emit_channels=$(
    find "$SRC_DIR" -name "*.rs" -exec \
        perl -0777 -ne 'print "$1\n" while /\.emit\(\s*"([a-z0-9-]+)"/gm' {} + \
    2>/dev/null | sort -u || true
)

# Compare: flag any emit-site literal not present in the registered set.
orphan_count=0
while IFS= read -r channel; do
    [[ -z "$channel" ]] && continue
    if ! printf '%s\n' "$registered" | grep -qx "$channel"; then
        orphan_count=$((orphan_count + 1))
        echo "WARNING: orphan channel '$channel' (not in docs/gui-event-channels.md):" >&2
        grep -rn --include="*.rs" "\"$channel\"" "$SRC_DIR" >&2 || true
    fi
done <<< "$emit_channels"

if [[ $orphan_count -gt 0 ]]; then
    echo "$orphan_count orphan channel(s) found — add to docs/gui-event-channels.md" >&2
    [[ $STRICT -eq 1 ]] && exit 1
fi

exit 0
