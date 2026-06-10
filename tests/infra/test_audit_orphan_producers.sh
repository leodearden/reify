#!/usr/bin/env bash
# Meta-test for scripts/audit-orphan-producers.sh: name==module collision
# detection.
#
# Hermetic fixture with crates/reify-fixture/src modules whose pub-fn names
# collide with their module names.  Drives the REAL audit script via its
# public CLI (--format json --quiet --scope 'crates/reify-*/src') and asserts
# orphan/allowed membership via python3.
#
# step-1/step-2: mod-declaration collision (pub mod NAME; inflates callers)
# step-3/step-4: path-qualifier collision (NAME::Item inflates callers)
#                + turbofish preservation (NAME::<T>() is a real call)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
AUDIT="$REPO_ROOT/scripts/audit-orphan-producers.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Graceful skip when required tools are absent (mirrors orphan_audit.rs pattern).
for _tool in python3 git; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_audit_orphan_producers.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

echo "=== audit-orphan-producers.sh collision-detection tests ==="

# ---------------------------------------------------------------------------
# Build hermetic fixture
# ---------------------------------------------------------------------------
FIXTURE="$(mktemp -d)"
cleanup() { rm -rf "$FIXTURE"; }
trap cleanup EXIT

git -C "$FIXTURE" init -q
mkdir -p "$FIXTURE/crates/reify-fixture/src"

# lib.rs — mod declarations and a private driver for wired() so the bare
# `wired()` token is counted as an external caller.  The driver is private
# (fn, not pub fn) so it does not become a candidate itself.
cat > "$FIXTURE/crates/reify-fixture/src/lib.rs" <<'RUST'
pub mod collide_mod;
pub mod wired;

// Private driver — provides a genuine bare-call token for `wired`.
fn drive_wired() -> i32 { wired() }
RUST

# collide_mod.rs — fn name collides with its own module name.
# Only caller outside cfg(test) is the `pub mod collide_mod;` declaration.
cat > "$FIXTURE/crates/reify-fixture/src/collide_mod.rs" <<'RUST'
pub fn collide_mod() -> i32 { 1 }

#[cfg(test)]
mod tests {
    use super::collide_mod;
    #[test]
    fn t() { assert_eq!(collide_mod(), 1); }
}
RUST

# wired.rs — fn name does NOT collide with the module name on its own.
# Genuinely called from drive_wired() in lib.rs (bare token `wired()`).
cat > "$FIXTURE/crates/reify-fixture/src/wired.rs" <<'RUST'
pub fn wired() -> i32 { 3 }
RUST

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Run the audit against the fixture and emit JSON to stdout.
audit_json() {
    ( cd "$FIXTURE" && bash "$AUDIT" --format json --quiet --scope 'crates/reify-*/src' )
}

# assert_orphan NAME — succeeds iff NAME appears in orphans[] with callers==0.
assert_orphan() {
    local name="$1"
    local json
    json="$(audit_json)"
    python3 - "$json" "$name" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
name = sys.argv[2]
for r in data.get("orphans", []):
    if r["name"] == name and r["callers"] == 0:
        sys.exit(0)
sys.exit(1)
PY
}

# assert_not_orphan NAME — succeeds iff NAME is absent from BOTH orphans[] and
# allowed[].
assert_not_orphan() {
    local name="$1"
    local json
    json="$(audit_json)"
    python3 - "$json" "$name" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
name = sys.argv[2]
for key in ("orphans", "allowed"):
    for r in data.get(key, []):
        if r["name"] == name:
            sys.exit(1)
sys.exit(0)
PY
}

# ---------------------------------------------------------------------------
# step-1 / step-2: mod-declaration collision
# ---------------------------------------------------------------------------
echo ""
echo "--- step-1/step-2: mod-declaration collision ---"

assert "collide_mod (name==module, mod-decl-only ref) is flagged orphan" \
    assert_orphan collide_mod

assert "wired (genuine bare caller) is not orphan" \
    assert_not_orphan wired

# ---------------------------------------------------------------------------
test_summary
