#!/usr/bin/env bash
# scripts/check_event_inventory.sh
#
# Lint for Tauri event-channel drift (GR-016 §9 task μ).
# Policy: PRD §11 Q4 — start as warning; promote via --strict after
#         one release cycle of observed drift.
#
# Forward pass: extracts literal channel names from .emit("name", …) call sites
# under gui/src-tauri/ and warns if any are absent from docs/gui-event-channels.md.
#
# Reverse pass (--bidirectional, opt-in): for each §1-registered channel in the
# inventory, verifies that a quoted string literal "channel-name" appears somewhere
# in gui/src-tauri/**/*.rs. §1-only scoping: §2 (FICTION → WIRED) rows are
# pre-implementation and intentionally excluded to avoid phantom-warning noise.
# Permissive scan: searches for the channel name as a quoted literal anywhere in
# *.rs, not just in .emit(…) form — this naturally covers dynamic-emit patterns;
# see docs/gui-event-channels.md §1 producer columns for the source-of-truth list
# of which sites produce which channel literal. No hardcoded allowlist needed.
# Opt-in per esc-3552-52 reviewer note; default-on deferred pending §2 graduation.
#
# Dynamic emit-sites (app.emit(&name, …), app.emit(event_name, …)) are
# intentionally skipped by the forward pass: their channel names live in
# delta_to_events / MCP context emitters and are covered by the lockstep-commit
# convention (PRD §3.3), not by this regex lint. The reverse pass covers them
# via the permissive literal scan.
#
# Usage: scripts/check_event_inventory.sh [--strict] [--bidirectional] [--repo-root <dir>]
#
# Exit codes:
#   0  always (warning mode) unless --strict is given and orphans/phantoms are found
#   1  only when --strict and at least one orphan or phantom channel is detected

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/check_event_inventory.sh [options]

Options:
  --strict           Exit 1 when any orphan or phantom channels are detected.
                     Default: warning mode (always exits 0).
  --bidirectional    Run a second reverse pass — warn when a §1-registered
                     channel has no literal occurrence in gui/src-tauri/*.rs.
                     §1-scoped: §2 (FICTION → WIRED) rows are excluded pending
                     §2 graduation. Opt-in per esc-3552-52 reviewer note.
  --repo-root DIR    Repository root (default: git rev-parse --show-toplevel).
  -h, --help         Show this message.
USAGE
}

STRICT=0
BIDIRECTIONAL=0
REPO_ROOT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict) STRICT=1; shift ;;
        --bidirectional) BIDIRECTIONAL=1; shift ;;
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

# Build-artifact directories under SRC_DIR that must never be scanned.
# Rationale: the infra-check lane runs concurrently with a build lane (verify.sh
# "Overlap join" region). Tauri codegen writes transient .rs files to gen/ during
# any `cargo build -p reify-gui`; the Rust compiler writes to target/. A transient
# emit literal in those dirs would be flagged as a false-positive orphan/phantom.
#
# target/ and gen/ are the two real build-output dirs at the top of SRC_DIR
# (.gitignore'd: /target:5, gen/:36 in gui/src-tauri/.gitignore).
#
# dist and node_modules are defensive guards — they do NOT currently exist under
# gui/src-tauri/ (their .gitignore entries are for gui/dist/:49 and
# node_modules/:14 under the parent gui/ directory). They are included to guard
# against future Tauri or JS tooling additions to gui/src-tauri that could
# otherwise silently introduce false-positive scan hits.
#
# The find prune uses -name (basename match), NOT -path anchored to SRC_DIR.
# This is intentional: gen and target are project-reserved build-output names
# under gui/src-tauri; no tracked .rs source module should ever carry either
# name at any depth. If a future source module were named gen/ or target/ it
# would need renaming (both are conventional build-dir names that confuse tools).
# The grep --exclude-dir side is inherently basename-only (grep has no path-prune
# equivalent), so basename matching is also the only consistent option across
# both scan forms.
# No tracked .rs lives under any of these four names; pruning is safe.
# See esc-4357-20 (flake), task-4529 (fix).
PRUNE_DIRS=(target gen node_modules dist)

# Pre-build a find prune expression: \( -type d \( -name a -o -name b ... \) -prune \)
# and a grep --exclude-dir arg list, both derived from PRUNE_DIRS.
_find_prune_expr=()
_grep_exclude_args=()
for _d in "${PRUNE_DIRS[@]}"; do
    _grep_exclude_args+=("--exclude-dir=$_d")
done
# Build the find -prune clause: ( -type d ( -name a -o -name b -o ... ) -prune )
_find_prune_expr+=(\( -type d \()
_first=1
for _d in "${PRUNE_DIRS[@]}"; do
    if [[ $_first -eq 1 ]]; then
        _find_prune_expr+=(-name "$_d")
        _first=0
    else
        _find_prune_expr+=(-o -name "$_d")
    fi
done
_find_prune_expr+=(\) -prune \))

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
# Per the inventory format, §2a command rows use **bold** formatting (not
# backticks) and are mechanically outside this regex; no special-case handling
# needed here.
registered=$(grep -oP '\| `\K[a-z0-9-]+(?=` \|)' "$INVENTORY" | sort -u || true)

# Extract literal channel names from .emit("name", …) call sites in Rust.
# Uses perl slurp mode (-0777) so \s* can span the newline between .emit( and
# the quoted argument — e.g. the multi-line form:
#   app.emit(
#       "evaluation-status",
# Dynamic forms (.emit(&name, …) or .emit(event_name, …)) produce no match.
emit_channels=$(
    find "$SRC_DIR" \
        "${_find_prune_expr[@]}" \
        -o \( -type f -name "*.rs" -exec \
            perl -0777 -ne 'print "$1\n" while /\.emit\(\s*"([a-z0-9-]+)"/gm' {} + \
        \) \
    2>/dev/null | sort -u || true
)

# Compare: flag any emit-site literal not present in the registered set.
orphan_count=0
while IFS= read -r channel; do
    [[ -z "$channel" ]] && continue
    if ! printf '%s\n' "$registered" | grep -qx "$channel"; then
        orphan_count=$((orphan_count + 1))
        echo "WARNING: orphan channel '$channel' (not in docs/gui-event-channels.md):" >&2
        grep -rn --include="*.rs" "${_grep_exclude_args[@]}" "\"$channel\"" "$SRC_DIR" >&2 || true
    fi
done <<< "$emit_channels"

if [[ $orphan_count -gt 0 ]]; then
    echo "$orphan_count orphan channel(s) found — add to docs/gui-event-channels.md" >&2
fi

# Reverse pass (--bidirectional): for each §1-registered channel, verify it has
# at least one quoted string literal occurrence in gui/src-tauri/**/*.rs.
# §1-only: awk extracts between ^## §1 and the next ^## §[0-9] heading.
# Permissive scan: grep -F '"channel-name"' matches any literal occurrence,
# not just .emit("…") form, so dynamic-emit patterns are naturally covered.
phantom_count=0
if [[ $BIDIRECTIONAL -eq 1 ]]; then
    sec1_channels=$(
        awk '/^## §1 /{f=1;next} /^## §[0-9]+ /{f=0} f' "$INVENTORY" \
        | grep -oP '\| `\K[a-z0-9-]+(?=` \|)' | sort -u || true
    )
    while IFS= read -r ch; do
        [[ -z "$ch" ]] && continue
        if ! grep -rqF "\"$ch\"" --include="*.rs" "${_grep_exclude_args[@]}" "$SRC_DIR" 2>/dev/null; then
            phantom_count=$((phantom_count + 1))
            echo "WARNING: phantom channel '$ch' registered in inventory but no source occurrence in gui/src-tauri/" >&2
        fi
    done <<< "$sec1_channels"
    if [[ $phantom_count -gt 0 ]]; then
        echo "$phantom_count phantom channel(s) found — verify source wiring or remove from docs/gui-event-channels.md §1" >&2
    fi
fi

if [[ $((orphan_count + phantom_count)) -gt 0 ]]; then
    [[ $STRICT -eq 1 ]] && exit 1
fi

exit 0
