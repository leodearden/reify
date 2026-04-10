#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Detect target triple
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)    TARGET="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64)   TARGET="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64)   TARGET="x86_64-apple-darwin" ;;
    Darwin-arm64)    TARGET="aarch64-apple-darwin" ;;
    *) echo "Unsupported platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

SIDECAR_NAME="reify-sidecar-${TARGET}"
DEST_DIR="../src-tauri/sidecar"
DEST="${DEST_DIR}/${SIDECAR_NAME}"

# Bundle cli.ts as single ESM file (workspace deps inlined, node builtins external)
npx tsup --config tsup.sidecar.ts

# Create executable: shebang + bundled JS
mkdir -p "$DEST_DIR"
printf '#!/usr/bin/env node\n' > "$DEST"
cat dist/cli.js >> "$DEST"
chmod +x "$DEST"

echo "Built ${SIDECAR_NAME} ($(wc -c < "$DEST") bytes)"
