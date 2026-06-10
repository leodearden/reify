#!/usr/bin/env bash
# scripts/ensure-gui-sidecar-placeholder.sh — create a minimal executable
# reify-sidecar stub if one does not already exist.
#
# Usage:
#   ./scripts/ensure-gui-sidecar-placeholder.sh [repo_root]
#
#   repo_root  Optional path to the repository root.
#              Default: two levels above this script's own location
#              (i.e. the checkout root when the script lives at scripts/).
#
# Why this script exists
# ───────────────────────
# gui/src-tauri/build.rs runs tauri_build::build() whenever --features gui is
# active (including during `cargo check`). tauri_build validates every entry in
# bundle.externalBin — concretely, it panics with "resource path … doesn't
# exist" when gui/src-tauri/sidecar/reify-sidecar-<triple> is absent from disk.
# That file is always gitignored and absent on a clean checkout, so without this
# stub the `cargo check -p reify-gui --features gui --tests` gate added by
# verify.sh would immediately panic in build.rs instead of type-checking code.
#
# This script creates the stub iff the file is absent, so:
#   - A real built sidecar (from build-sidecar.sh / run-gui.sh) is NEVER clobbered.
#   - Running the script multiple times is safe (idempotent).
#   - Diagnostics go to stderr only; the script is silent on stdout.
#
# The stub content is a minimal #!/usr/bin/env node shebang so it looks like a
# valid Node script to any tool that inspects it, matching what build-sidecar.sh
# produces for real builds.

set -euo pipefail

# ── resolve repository root ───────────────────────────────────────────────────

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ $# -gt 1 ]; then
    echo "Usage: $(basename "$0") [repo_root]" >&2
    echo "  repo_root  Optional path to the repository root (default: auto-detected)" >&2
    exit 1
fi

if [ $# -eq 1 ]; then
    ROOT="$(cd "$1" && pwd)"
else
    # Default: parent of the scripts/ directory.
    ROOT="$(cd "$_SCRIPT_DIR/.." && pwd)"
fi

# ── detect target triple ──────────────────────────────────────────────────────

TRIPLE=""
if command -v rustc >/dev/null 2>&1; then
    TRIPLE="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' || true)"
fi
if [ -z "$TRIPLE" ]; then
    # Fallback: derive from uname (covers the common CI host).
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64)  TRIPLE="x86_64-unknown-linux-gnu" ;;
        Linux-aarch64) TRIPLE="aarch64-unknown-linux-gnu" ;;
        Darwin-x86_64) TRIPLE="x86_64-apple-darwin" ;;
        Darwin-arm64)  TRIPLE="aarch64-apple-darwin" ;;
        *)             TRIPLE="x86_64-unknown-linux-gnu" ;;
    esac
    echo "ensure-gui-sidecar-placeholder: rustc not found; using fallback triple '${TRIPLE}'" >&2
fi

# ── create stub if absent ─────────────────────────────────────────────────────

SIDECAR_DIR="$ROOT/gui/src-tauri/sidecar"
SIDECAR="$SIDECAR_DIR/reify-sidecar-$TRIPLE"

if [ -e "$SIDECAR" ]; then
    echo "ensure-gui-sidecar-placeholder: sidecar already exists at '$SIDECAR', skipping" >&2
    exit 0
fi

mkdir -p "$SIDECAR_DIR"
printf '#!/usr/bin/env node\n// placeholder — real sidecar built by gui/sidecar/build-sidecar.sh\n' > "$SIDECAR"
chmod +x "$SIDECAR"

echo "ensure-gui-sidecar-placeholder: created stub at '$SIDECAR'" >&2
exit 0
