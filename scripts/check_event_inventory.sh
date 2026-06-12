#!/usr/bin/env bash
# scripts/check_event_inventory.sh
#
# Lint for Tauri event-channel drift (GR-016 §9 task μ).
# Policy: PRD §11 Q4 — start as warning; promote via --strict after
#         one release cycle of observed drift.
#
# Source enumeration (hermetic, task-4572): both passes operate on the shared
# _tracked_rs array — `git ls-files 'gui/src-tauri/*.rs'` lists ONLY indexed
# (tracked) files, so untracked/transient build artifacts are excluded by
# construction regardless of which directory they land in. This supersedes the
# directory-name PRUNE_DIRS mechanism from task-4529 (esc-4357-20) and closes
# the gap where transients land outside the named dirs (esc-3798-78). Single-
# star pathspec: git * is not path-boundary-aware, so it matches recursively at
# every depth AND the top-level build.rs (same set as the prior find -name).
# Staged-only coverage: a new .emit("new-channel", …) call in an unstaged file
# is not detected until the file is `git add`ed. This is intentional — the
# verify/CI gate always runs on staged/committed trees.
#
# Forward pass: extracts literal channel names from .emit("name", …) call sites
# in tracked sources and warns if any are absent from docs/gui-event-channels.md.
#
# Reverse pass (--bidirectional, opt-in): for each §1-registered channel in the
# inventory, verifies that a quoted string literal "channel-name" appears somewhere
# in a tracked .rs source. §1-only scoping: §2 (FICTION → WIRED) rows are
# pre-implementation and intentionally excluded to avoid phantom-warning noise.
# Permissive scan: searches for the channel name as a quoted literal anywhere in
# tracked *.rs, not just in .emit(…) form — naturally covers dynamic-emit patterns;
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
                     Must be a git work tree; exits 1 with an error otherwise.
  --print-registered Print the extracted registered-channel set (one per line)
                     and exit 0.  Field-debug tool: loop this to detect a
                     truncation drop without running the full check (task-4586).
  -h, --help         Show this message.

Note: only git-tracked (staged) .rs files are scanned. A .emit("new-channel")
call in an unstaged file is not detected until the file is added to the index.
USAGE
}

STRICT=0
BIDIRECTIONAL=0
PRINT_REGISTERED=0
REPO_ROOT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict) STRICT=1; shift ;;
        --bidirectional) BIDIRECTIONAL=1; shift ;;
        --print-registered) PRINT_REGISTERED=1; shift ;;
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

if ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ERROR: repo-root is not a git work tree: $REPO_ROOT" >&2
    exit 1
fi

# Build the absolute-path array of tracked .rs sources under gui/src-tauri/.
# git ls-files -z lists only indexed (tracked) files — untracked/transient
# build artifacts are excluded by construction regardless of which directory
# they land in (esc-4357-20, esc-3798-78). Supersedes task-4529 PRUNE_DIRS.
# Single-star pathspec: git * is NOT path-boundary-aware, so
# 'gui/src-tauri/*.rs' recursively matches at every depth AND the top-level
# build.rs (verified 37 files), reproducing the prior find -name '*.rs' set.
_tracked_rs=()
while IFS= read -r -d '' _f; do
    _tracked_rs+=("$REPO_ROOT/$_f")
done < <(git -C "$REPO_ROOT" ls-files -z -- 'gui/src-tauri/*.rs' 2>/dev/null)

# Build the set of registered channel names using the published grep contract:
# | `channel-name` | — matches every event-channel row in §1 / §2.
# Per the inventory format, §2a command rows use **bold** formatting (not
# backticks) and are mechanically outside this regex; no special-case handling
# needed here.
#
# extract_registered_channels: single awk pass over the inventory file.
# Matches | `name` | rows (literal pipes + backticks), deduplicates in-process,
# and prints without a downstream pipe — removing the grep -oP | sort -u two-
# process pipeline that the task-4586 hypothesis names as the truncation point.
# Membership tests are order-independent so output order does not matter.
extract_registered_channels() {
    local inv="$1"
    awk '
    {
        line = $0
        while (match(line, /\| `[a-z0-9-]+` \|/)) {
            name = substr(line, RSTART + 3, RLENGTH - 6)
            seen[name] = 1
            line = substr(line, RSTART + RLENGTH - 1)
        }
    }
    END { for (ch in seen) print ch }
    ' "$inv"
}
registered=$(extract_registered_channels "$INVENTORY")

# task-4586 fault-injection seam: drop one channel from registered to simulate
# under-load single-line truncation (inert unless REIFY_EVENT_INVENTORY_DROP_REGISTERED is set).
if [[ -n "${REIFY_EVENT_INVENTORY_DROP_REGISTERED:-}" ]]; then
    registered=$(printf '%s\n' "$registered" | grep -vx -- "$REIFY_EVENT_INVENTORY_DROP_REGISTERED" || true)
fi

# --print-registered: emit the extracted set and exit 0 (field-debug + test seam).
if [[ $PRINT_REGISTERED -eq 1 ]]; then
    printf '%s\n' "$registered"
    exit 0
fi

# Extract literal channel names from .emit("name", …) call sites in Rust.
# Uses perl slurp mode (-0777) so \s* can span the newline between .emit( and
# the quoted argument — e.g. the multi-line form:
#   app.emit(
#       "evaluation-status",
# Dynamic forms (.emit(&name, …) or .emit(event_name, …)) produce no match.
emit_channels=""
if [[ ${#_tracked_rs[@]} -gt 0 ]]; then
    emit_channels=$(perl -0777 -ne 'print "$1\n" while /\.emit\(\s*"([a-z0-9-]+)"/gm' "${_tracked_rs[@]}" 2>/dev/null | sort -u || true)
fi

# Compare: flag any emit-site literal not present in the registered set.
# task-4586/4529/4572 defense-in-depth: before flagging an orphan, re-confirm
# absence via a targeted fixed-string grep of the inventory file.  The in-memory
# `registered` set may have transiently dropped a line under subprocess load
# (esc-4578-61); the per-channel grep is far cheaper and far less prone to that
# truncation.  Only genuine orphans (absent from the file itself) are flagged.
orphan_count=0
while IFS= read -r channel; do
    [[ -z "$channel" ]] && continue
    if ! printf '%s\n' "$registered" | grep -qx "$channel"; then
        # Re-confirm absence against the source file before flagging.
        grep -qF -- "| \`$channel\` |" "$INVENTORY" && continue
        orphan_count=$((orphan_count + 1))
        echo "WARNING: orphan channel '$channel' (not in docs/gui-event-channels.md):" >&2
        grep -n -- "\"$channel\"" "${_tracked_rs[@]}" >&2 || true
    fi
done <<< "$emit_channels"

if [[ $orphan_count -gt 0 ]]; then
    echo "$orphan_count orphan channel(s) found — add to docs/gui-event-channels.md" >&2
fi

# Reverse pass (--bidirectional): for each §1-registered channel, verify it has
# at least one quoted string literal occurrence in the tracked _tracked_rs set.
# §1-only: awk extracts between ^## §1 and the next ^## §[0-9]+ heading.
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
        if [[ ${#_tracked_rs[@]} -eq 0 ]] || ! grep -qF -- "\"$ch\"" "${_tracked_rs[@]}" 2>/dev/null; then
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
