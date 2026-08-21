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

# lib.rs — mod declarations and private drivers.  All drivers are private
# (fn, not pub fn) so they do not become candidates themselves.
cat > "$FIXTURE/crates/reify-fixture/src/lib.rs" <<'RUST'
pub mod collide_mod;
pub mod wired;
pub mod collide_path;
pub mod turbo;
pub mod dangling_str;
pub mod dangling_raw;
pub mod dangling_comment;
pub mod dangling_char;
pub mod lifetime_wired;
pub mod stmt_trailing_comment;

// Private driver — provides a genuine bare-call token for `wired`.
fn drive_wired() -> i32 { wired() }
// Private driver — references collide_path only via a NAME::Item path-qualifier.
fn refer_path() -> u32 { collide_path::HELPER }
// Private driver — calls turbo only via turbofish NAME::<T>().
fn drive_turbo() { turbo::<i32>(); }
// Private driver — genuine bare-call token for `lifetime_wired`, itself
// lifetime-bearing.  Negative guard: if the literal-aware stripper is ever
// over-eager and blanks real code between two lifetime ticks, this call
// site is lost and `lifetime_wired` would wrongly show up as an orphan.
fn drive_lifetime_wired<'a>(s: &'a str) -> &'a str { lifetime_wired(s) }
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

# collide_path.rs — fn name collides with its module name.
# The only reference to the fn (outside cfg(test)) is the path-qualifier
# `collide_path::HELPER` in refer_path() — never a direct bare call.
cat > "$FIXTURE/crates/reify-fixture/src/collide_path.rs" <<'RUST'
pub const HELPER: u32 = 7;
pub fn collide_path() -> i32 { 2 }
RUST

# turbo.rs — generic fn called only via turbofish `turbo::<i32>()`.
# The `::` is followed by `<`, so it must be preserved as a real call.
cat > "$FIXTURE/crates/reify-fixture/src/turbo.rs" <<'RUST'
pub fn turbo<T>() {}
RUST

# dangling_str.rs — a `#[cfg(test)]` mod near the top of the file whose test
# body holds an unbalanced `{` inside a STRING LITERAL (the exact #6096
# shape). A raw-text brace counter reads that `{` as a real open brace, so
# the mask never closes and swallows the guarded pub fn below it.
cat > "$FIXTURE/crates/reify-fixture/src/dangling_str.rs" <<'RUST'
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let s = "structure def ZVShaper : Shaper {";
        assert!(!s.is_empty());
    }
}

// G-allow: hermetic fixture for the literal-aware brace counter
pub fn after_str_guard() -> i32 { 1 }
RUST

# dangling_raw.rs — same defect, but the unbalanced `{` sits inside a RAW
# string literal (`r#"..."#`).
cat > "$FIXTURE/crates/reify-fixture/src/dangling_raw.rs" <<'RUST'
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let s = r#"structure def X : Y {"#;
        assert!(!s.is_empty());
    }
}

// G-allow: hermetic fixture for the literal-aware brace counter
pub fn after_raw_guard() -> i32 { 1 }
RUST

# dangling_comment.rs — same defect, but the unbalanced `{` sits inside a
# BLOCK COMMENT.
cat > "$FIXTURE/crates/reify-fixture/src/dangling_comment.rs" <<'RUST'
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        /* sample decl: struct Foo { */
        assert!(true);
    }
}

// G-allow: hermetic fixture for the literal-aware brace counter
pub fn after_comment_guard() -> i32 { 1 }
RUST

# dangling_char.rs — same defect, but the unbalanced `{` sits inside a CHAR
# LITERAL on a line that also carries lifetime syntax (`<'a>`, `&'a`),
# pinning the lifetime-vs-char-literal disambiguation: the ticks in `<'a>`
# and `&'a` have no closing `'` at the grammar-implied offset and must NOT be
# treated as opening a char literal, while `'{'` (closing `'` at offset 2)
# must.
cat > "$FIXTURE/crates/reify-fixture/src/dangling_char.rs" <<'RUST'
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        fn f<'a>(s: &'a str) -> char { let _ = '{'; s.chars().next().unwrap() }
        assert_eq!(f("z"), 'z');
    }
}

// G-allow: hermetic fixture for the literal-aware brace counter
pub fn after_char_guard() -> i32 { 1 }
RUST

# lifetime_wired.rs — negative guard.  A genuinely-called pub fn whose own
# signature carries lifetime syntax and no `#[cfg(test)]` at all.  Paired
# with the drive_lifetime_wired() caller in lib.rs; see assert_not_orphan
# below.
cat > "$FIXTURE/crates/reify-fixture/src/lifetime_wired.rs" <<'RUST'
pub fn lifetime_wired<'a>(s: &'a str) -> &'a str { s }
RUST

# stmt_trailing_comment.rs — `#[cfg(test)]` above a SINGLE-STATEMENT item
# whose `;` is followed by a comment.  mask_cfg_test judges the item's
# shape (BLOCK_KW_RE + the `;`/`,` suffix test) from the raw line, so the
# trailing comment defeats `endswith(";")`, `mod` makes it look like a
# block, and the mask misfires.
#
# inner_probe.rs is deliberately NOT created: the audit is a pure text
# scanner and never resolves module paths, so the dangling `mod
# inner_probe;` declaration below is intentional fixture content, not a
# bug for a future reader to "fix".
cat > "$FIXTURE/crates/reify-fixture/src/stmt_trailing_comment.rs" <<'RUST'
#[cfg(test)]
mod inner_probe; // sibling test module — declaration only

// G-allow: hermetic fixture for cfg(test) single-statement header shape
pub fn after_stmt_guard() -> i32 { 4 }
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

# assert_allowed NAME — succeeds iff NAME appears EXACTLY ONCE in allowed[]
# with callers==0.  Distinguishes "correctly allow-listed" from "invisible
# because a cfg(test) mask swallowed it", which assert_not_orphan cannot.
assert_allowed() {
    local name="$1"
    local json
    json="$(audit_json)"
    python3 - "$json" "$name" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
name = sys.argv[2]
count = sum(1 for r in data.get("allowed", []) if r["name"] == name and r["callers"] == 0)
sys.exit(0 if count == 1 else 1)
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
# step-3 / step-4: path-qualifier collision + turbofish preservation
# ---------------------------------------------------------------------------
echo ""
echo "--- step-3/step-4: path-qualifier collision + turbofish preservation ---"

assert "collide_path (referenced only via NAME::Item path qualifier) is flagged orphan" \
    assert_orphan collide_path

assert "turbo (called only via turbofish NAME::<T>()) is not orphan" \
    assert_not_orphan turbo

# ---------------------------------------------------------------------------
# cfg(test) mask: literal/comment-aware brace counting
# ---------------------------------------------------------------------------
echo ""
echo "--- cfg(test) mask: literal/comment-aware brace counting ---"

assert "after_str_guard (dangling { inside a string literal) is allow-listed, not swallowed" \
    assert_allowed after_str_guard

assert "after_raw_guard (dangling { inside a raw string literal) is allow-listed, not swallowed" \
    assert_allowed after_raw_guard

assert "after_comment_guard (dangling { inside a block comment) is allow-listed, not swallowed" \
    assert_allowed after_comment_guard

assert "after_char_guard (dangling { inside a char literal beside a lifetime) is allow-listed, not swallowed" \
    assert_allowed after_char_guard

assert "lifetime_wired (genuine caller, lifetime-bearing signature) is not orphan" \
    assert_not_orphan lifetime_wired

assert "after_stmt_guard (single-statement cfg(test) item, trailing comment defeats the ';' suffix test) is allow-listed, not swallowed" \
    assert_allowed after_stmt_guard

# ---------------------------------------------------------------------------
test_summary
