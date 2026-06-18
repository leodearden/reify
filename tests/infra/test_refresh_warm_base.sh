#!/usr/bin/env bash
# tests/infra/test_refresh_warm_base.sh
# Hermetic tests for scripts/refresh-warm-base.sh.
#
# PATH stubs:
#   cp       — records argv to CALLS_FILE; when REIFY_TEST_REFLINK_OK=1 performs
#              a real recursive copy via the absolute cp (stripping --reflink=always);
#              else prints an error + exits 1.
#   mv       — NOT stubbed; real mv so filesystem postconditions are observable.
#   xfs_bmap — records argv + emits REIFY_TEST_FRAG_EXTENTS extent rows per file.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag, missing positional args
#   B — basic refresh happy path: cp --reflink=always, atomic rename, content OK
#   C — fail-closed reflink: probe failure -> non-zero, no partial base, pre-existing untouched
#   D — in-flight clone independence: clone dir untouched after refresh (B6)
#   E — base self-description stamps: .rustflags and .invocation written after swap
#   F — --check-frag defrag signal: verdict token + extent count, read-only
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/refresh-warm-base.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/refresh-warm-base.sh hermetic tests (task 4661) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

STUB_DIR="$(mktemp -d /tmp/test-refresh-warm-base-stub-XXXXXX)"
_TMPDIRS+=("$STUB_DIR")

CALLS_FILE="$(mktemp /tmp/test-refresh-warm-base-calls-XXXXXX)"
_TMPDIRS+=("$CALLS_FILE")

ERR_FILE="$(mktemp /tmp/test-refresh-warm-base-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── PATH stubs ─────────────────────────────────────────────────────────────────

# cp stub: record argv; if REIFY_TEST_REFLINK_OK=1 perform a real recursive copy
# (absolute cp with --reflink=always stripped); else simulate a partial copy
# (create the destination directory as a real cp would) then error + exit 1.
# This simulates the real-world failure mode where cp creates a partial <base>.new
# directory before encountering a non-reflink filesystem, so the EXIT trap test
# (Block C) can distinguish "partial .new cleaned up" from "never created".
# The real cp path is embedded at stub-creation time.
_REAL_CP="$(command -v cp)"
cat > "$STUB_DIR/cp" << STUB_EOF
#!/usr/bin/env bash
echo "cp \$*" >> "\${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "\${REIFY_TEST_REFLINK_OK:-}" = "1" ]; then
    args=()
    for a in "\$@"; do
        [ "\$a" = "--reflink=always" ] && continue
        args+=("\$a")
    done
    exec "${_REAL_CP}" "\${args[@]}"
fi
# Simulate partial failure: create destination dir (as real cp would) before failing
# The destination is always the last argument; ${!#} gives the last positional.
_dst="\${!#}"
if [ -n "\$_dst" ]; then
    mkdir -p "\$_dst" 2>/dev/null || true
fi
echo "cp: failed to clone: Operation not supported" >&2
exit 1
STUB_EOF
chmod +x "$STUB_DIR/cp"

# xfs_bmap stub: record argv; emit REIFY_TEST_FRAG_EXTENTS extent rows.
# REIFY_TEST_XFSBMAP_OK=0 simulates xfs_bmap being unavailable/failing (exits 1).
cat > "$STUB_DIR/xfs_bmap" << 'STUB_EOF'
#!/usr/bin/env bash
echo "xfs_bmap $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_XFSBMAP_OK:-1}" = "0" ]; then
    echo "xfs_bmap: failed to get extents" >&2
    exit 1
fi
count="${REIFY_TEST_FRAG_EXTENTS:-1}"
for i in $(seq 1 "$count"); do
    printf "    %d: [0..511]: 1234..%d 512\n" "$((i-1))" "$((1234 + i*512))"
done
exit 0
STUB_EOF
chmod +x "$STUB_DIR/xfs_bmap"

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes the script under the stub PATH.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(
        REIFY_TEST_CALLS_FILE="$CALLS_FILE" \
        PATH="$STUB_DIR:$PATH" \
            bash "$SCRIPT" "$@" 2>"$ERR_FILE"
    )" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

reset_calls() {
    > "$CALLS_FILE"
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard: --help, unknown flag, missing positional args
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0
reset_calls
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
reset_calls
run_helper --unknown-flag-xyz
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# A3: missing all positional args exits non-zero
reset_calls
run_helper
assert "A3: missing all positional args exits non-zero" test "$RC" -ne 0

# A4: only one positional arg (missing base_dir) exits non-zero
reset_calls
run_helper /some/nonexistent/dir
assert "A4: missing second positional arg exits non-zero" test "$RC" -ne 0

# ──────────────────────────────────────────────────────────────────────────────
# Block B — basic refresh happy path
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: basic refresh happy path ---"

B_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b-XXXXXX)"
_TMPDIRS+=("$B_TMP")

# Create an advancing target dir with content
B_ADV="$B_TMP/advancing"
mkdir -p "$B_ADV"
echo "file1 content" > "$B_ADV/file1.txt"
echo "file2 content" > "$B_ADV/file2.txt"
mkdir -p "$B_ADV/subdir"
echo "nested" > "$B_ADV/subdir/nested.txt"

B_BASE="$B_TMP/base"

# B1: basic refresh (no pre-existing base) exits 0
reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B_ADV" "$B_BASE"
assert "B1: basic refresh exits 0" test "$RC" -eq 0

# B2: cp was invoked with --reflink=always
assert "B2: cp invoked with --reflink=always" \
    bash -c 'grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"

# B3: cp targeted <base_dir>.new
assert "B3: cp targeted <base_dir>.new" \
    bash -c 'grep "^cp " "$1" | grep -qF "'"$B_BASE"'.new"' _ "$CALLS_FILE"

# B4: <base_dir> exists and contains the advancing content
assert "B4: <base_dir> exists after refresh" test -d "$B_BASE"
assert "B4: file1.txt has advancing content" \
    bash -c '[ "$(cat "$1/file1.txt")" = "file1 content" ]' _ "$B_BASE"
assert "B4: file2.txt has advancing content" \
    bash -c '[ "$(cat "$1/file2.txt")" = "file2 content" ]' _ "$B_BASE"
assert "B4: subdir/nested.txt exists" test -f "$B_BASE/subdir/nested.txt"

# B5: <base_dir>.new does NOT exist (cleaned up by the successful rename)
assert "B5: <base_dir>.new absent after successful refresh" \
    test ! -e "$B_BASE.new"

# B6: diagnostics on stderr (ERR_OUT non-empty)
assert "B6: diagnostics on stderr (non-empty)" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# B7: stdout is empty (no stdout output from the script — diagnostics only on stderr)
assert "B7: stdout is empty" \
    bash -c '[ -z "$1" ]' _ "$OUT"

# B8: refresh when base already exists (two-rename atomic swap path)
B2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-b2-XXXXXX)"
_TMPDIRS+=("$B2_TMP")
B2_ADV="$B2_TMP/advancing"
mkdir -p "$B2_ADV"
echo "new content" > "$B2_ADV/newfile.txt"
B2_BASE="$B2_TMP/base"
mkdir -p "$B2_BASE"
echo "old content" > "$B2_BASE/oldfile.txt"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$B2_ADV" "$B2_BASE"
assert "B8: refresh with existing base exits 0" test "$RC" -eq 0
assert "B8: new base has advancing content" \
    bash -c '[ "$(cat "$1/newfile.txt")" = "new content" ]' _ "$B2_BASE"
assert "B8: old content gone after swap" \
    test ! -f "$B2_BASE/oldfile.txt"
assert "B8: <base_dir>.new cleaned up" test ! -e "$B2_BASE.new"
assert "B8: <base_dir>.old cleaned up" test ! -e "$B2_BASE.old"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — fail-closed reflink: probe failure → non-zero, no partial, pre-existing untouched
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: fail-closed reflink ---"

C_TMP="$(mktemp -d /tmp/test-refresh-warm-base-c-XXXXXX)"
_TMPDIRS+=("$C_TMP")
C_ADV="$C_TMP/advancing"
mkdir -p "$C_ADV"
echo "adv content" > "$C_ADV/file.txt"

C_BASE="$C_TMP/base"

# C1: reflink failure exits non-zero (no pre-existing base)
reset_calls
REIFY_TEST_REFLINK_OK=0 run_helper "$C_ADV" "$C_BASE"
assert "C1: reflink failure exits non-zero" test "$RC" -ne 0

# C2: stderr names the reflink failure (actionable)
assert "C2: stderr names reflink failure" \
    bash -c 'printf "%s\n" "$1" | grep -qiE "reflink|Operation not supported"' _ "$ERR_OUT"

# C3: <base_dir>.new does NOT exist after failure (no partial left behind)
assert "C3: <base_dir>.new absent after reflink failure" \
    test ! -e "$C_BASE.new"

# C4: <base_dir>.old does NOT exist after failure
assert "C4: <base_dir>.old absent after reflink failure" \
    test ! -e "$C_BASE.old"

# C5: a pre-existing <base_dir> is left unchanged after a failed refresh
C2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-c2-XXXXXX)"
_TMPDIRS+=("$C2_TMP")
C2_ADV="$C2_TMP/advancing"
mkdir -p "$C2_ADV"
echo "new adv" > "$C2_ADV/new.txt"
C2_BASE="$C2_TMP/base"
mkdir -p "$C2_BASE"
echo "original" > "$C2_BASE/orig.txt"

reset_calls
REIFY_TEST_REFLINK_OK=0 run_helper "$C2_ADV" "$C2_BASE"
assert "C5: reflink failure with existing base exits non-zero" test "$RC" -ne 0
assert "C5: pre-existing base still exists" test -d "$C2_BASE"
assert "C5: pre-existing base content unchanged (orig.txt present)" \
    test -f "$C2_BASE/orig.txt"
assert "C5: <base_dir>.new absent (no partial)" test ! -e "$C2_BASE.new"
assert "C5: <base_dir>.old absent (base not disturbed)" test ! -e "$C2_BASE.old"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — in-flight clone independence (B6): clone dir untouched after refresh
# The cp stub performs a real recursive copy of the advancing dir.
# A pre-existing sibling clone dir (simulating an in-flight lane) must remain
# byte-identical and must never appear in the CALLS_FILE (no drain protocol).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: in-flight clone independence (B6) ---"

D_TMP="$(mktemp -d /tmp/test-refresh-warm-base-d-XXXXXX)"
_TMPDIRS+=("$D_TMP")
D_ADV="$D_TMP/advancing"
mkdir -p "$D_ADV"
echo "new adv content" > "$D_ADV/newfile.txt"
D_BASE="$D_TMP/base"
mkdir -p "$D_BASE"
echo "old base content" > "$D_BASE/oldfile.txt"

# Create a sibling in-flight clone (simulating a lane that grabbed the OLD base)
D_CLONE="$D_TMP/clone-lane-42"
mkdir -p "$D_CLONE"
echo "old base content" > "$D_CLONE/oldfile.txt"
_CLONE_MTIME="$(stat -c '%Y' "$D_CLONE/oldfile.txt")"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$D_ADV" "$D_BASE"
assert "D1: refresh with in-flight clone exits 0" test "$RC" -eq 0

# D2: the clone dir still has its original content
assert "D2: clone dir still has original file" test -f "$D_CLONE/oldfile.txt"
assert "D2: clone dir original content unchanged" \
    bash -c '[ "$(cat "$1/oldfile.txt")" = "old base content" ]' _ "$D_CLONE"

# D3: clone mtime is unchanged (no touch/write to clone)
assert "D3: clone file mtime unchanged after refresh" \
    bash -c '[ "$(stat -c "%Y" "$1/oldfile.txt")" = "$2" ]' _ "$D_CLONE" "$_CLONE_MTIME"

# D4: CALLS_FILE never references the clone path (no drain: script never touches clone)
assert "D4: CALLS_FILE has no reference to clone path (no drain protocol)" \
    bash -c '! grep -qF "'"$D_CLONE"'" "$1"' _ "$CALLS_FILE"

# D5: the new advancing content is in the base (correct refresh happened)
assert "D5: base has advancing content after refresh" \
    bash -c '[ "$(cat "$1/newfile.txt")" = "new adv content" ]' _ "$D_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — base self-description stamps: .rustflags and .invocation
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: self-description stamps ---"

E_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e-XXXXXX)"
_TMPDIRS+=("$E_TMP")
E_ADV="$E_TMP/advancing"
mkdir -p "$E_ADV"
echo "content" > "$E_ADV/f.txt"
E_BASE="$E_TMP/base"

# E1: .rustflags stamp written with RUSTFLAGS env value
reset_calls
RUSTFLAGS="-C foo" REIFY_TEST_REFLINK_OK=1 run_helper "$E_ADV" "$E_BASE"
assert "E1: refresh with RUSTFLAGS exits 0" test "$RC" -eq 0
assert "E1: <base_dir>.rustflags exists after refresh" test -f "$E_BASE.rustflags"
assert "E1: <base_dir>.rustflags contains RUSTFLAGS value" \
    bash -c '[ "$(cat "$1.rustflags")" = "-C foo" ]' _ "$E_BASE"

# E2: .invocation stamp written with --invocation value
assert "E2: <base_dir>.invocation exists after refresh" test -f "$E_BASE.invocation"
assert "E2: <base_dir>.invocation is empty when --invocation not passed" \
    bash -c '[ -z "$(cat "$1.invocation")" ]' _ "$E_BASE"

# E3: stamps present after the dir swap (consistent with the new base)
assert "E3: stamps are siblings of <base_dir> (not inside it)" \
    bash -c 'test -f "$1.rustflags" && ! test -f "$1/base.rustflags"' _ "$E_BASE"

# E4: --rustflags flag overrides the RUSTFLAGS env
E2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e2-XXXXXX)"
_TMPDIRS+=("$E2_TMP")
E2_ADV="$E2_TMP/advancing"
mkdir -p "$E2_ADV"
echo "c" > "$E2_ADV/f.txt"
E2_BASE="$E2_TMP/base"

reset_calls
RUSTFLAGS="-C env-value" REIFY_TEST_REFLINK_OK=1 \
    run_helper "$E2_ADV" "$E2_BASE" --rustflags "-C override"
assert "E4: --rustflags override exits 0" test "$RC" -eq 0
assert "E4: .rustflags contains --rustflags value (not RUSTFLAGS env)" \
    bash -c '[ "$(cat "$1.rustflags")" = "-C override" ]' _ "$E2_BASE"

# E5: RUSTFLAGS unset -> .rustflags file exists but is empty
E3_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e3-XXXXXX)"
_TMPDIRS+=("$E3_TMP")
E3_ADV="$E3_TMP/advancing"
mkdir -p "$E3_ADV"
echo "c" > "$E3_ADV/f.txt"
E3_BASE="$E3_TMP/base"

reset_calls
unset RUSTFLAGS 2>/dev/null || true
REIFY_TEST_REFLINK_OK=1 run_helper "$E3_ADV" "$E3_BASE"
assert "E5: unset RUSTFLAGS refresh exits 0" test "$RC" -eq 0
assert "E5: .rustflags exists even when RUSTFLAGS unset" test -f "$E3_BASE.rustflags"
assert "E5: .rustflags is empty when RUSTFLAGS unset" \
    bash -c '[ -z "$(cat "$1.rustflags")" ]' _ "$E3_BASE"

# E6: --invocation value written to .invocation stamp
E4_TMP="$(mktemp -d /tmp/test-refresh-warm-base-e4-XXXXXX)"
_TMPDIRS+=("$E4_TMP")
E4_ADV="$E4_TMP/advancing"
mkdir -p "$E4_ADV"
echo "c" > "$E4_ADV/f.txt"
E4_BASE="$E4_TMP/base"

reset_calls
REIFY_TEST_REFLINK_OK=1 run_helper "$E4_ADV" "$E4_BASE" --invocation "sha256:abc123"
assert "E6: --invocation refresh exits 0" test "$RC" -eq 0
assert "E6: .invocation contains --invocation value" \
    bash -c '[ "$(cat "$1.invocation")" = "sha256:abc123" ]' _ "$E4_BASE"

# ──────────────────────────────────────────────────────────────────────────────
# Block F — --check-frag defrag signal: verdict token + extent count, read-only
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: --check-frag defrag signal ---"

F_TMP="$(mktemp -d /tmp/test-refresh-warm-base-f-XXXXXX)"
_TMPDIRS+=("$F_TMP")
F_BASE="$F_TMP/base"
mkdir -p "$F_BASE"
echo "binary" > "$F_BASE/rustc"
echo "other" > "$F_BASE/libstd.rlib"

# F1: extents below threshold -> stdout "ok N", exits 0
reset_calls
REIFY_TEST_FRAG_EXTENTS=2 run_helper --check-frag "$F_BASE" --frag-threshold 64
assert "F1: --check-frag below threshold exits 0" test "$RC" -eq 0
assert "F1: stdout starts with 'ok'" \
    bash -c 'printf "%s\n" "$1" | grep -q "^ok "' _ "$OUT"
assert "F1: stdout contains extent count" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^ok [0-9]+"' _ "$OUT"

# F2: extents at/above threshold -> stdout "reseed-due N", exits 0
reset_calls
REIFY_TEST_FRAG_EXTENTS=64 run_helper --check-frag "$F_BASE" --frag-threshold 64
assert "F2: --check-frag at threshold exits 0" test "$RC" -eq 0
assert "F2: stdout starts with 'reseed-due'" \
    bash -c 'printf "%s\n" "$1" | grep -q "^reseed-due "' _ "$OUT"
assert "F2: stdout contains extent count" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^reseed-due [0-9]+"' _ "$OUT"

# F3: --check-frag performs NO refresh (no cp --reflink or mv recorded)
reset_calls
REIFY_TEST_FRAG_EXTENTS=1 run_helper --check-frag "$F_BASE"
assert "F3: --check-frag: no cp --reflink recorded (read-only)" \
    bash -c '! grep "^cp " "$1" | grep -q -- "--reflink=always"' _ "$CALLS_FILE"
assert "F3: --check-frag: no mv recorded (no rename)" \
    bash -c '! grep -q "^mv " "$1"' _ "$CALLS_FILE"

# F4: xfs_bmap was invoked per file under base
reset_calls
REIFY_TEST_FRAG_EXTENTS=1 run_helper --check-frag "$F_BASE"
assert "F4: xfs_bmap invoked at least once (per-file extent scan)" \
    bash -c 'grep -q "^xfs_bmap " "$1"' _ "$CALLS_FILE"

# F5: xfs_bmap unavailable/failing -> non-zero exit + actionable stderr
# REIFY_TEST_XFSBMAP_OK=0 makes the stub exit 1 (simulates xfs_bmap failure).
# The script must propagate this failure rather than swallowing it with || true.
reset_calls
REIFY_TEST_XFSBMAP_OK=0 run_helper --check-frag "$F_BASE"
assert "F5: xfs_bmap failure exits non-zero" test "$RC" -ne 0
assert "F5: actionable stderr when xfs_bmap fails" \
    bash -c 'printf "%s\n" "$1" | grep -qi "xfs_bmap"' _ "$ERR_OUT"

# F6: base_dir missing -> non-zero exit + actionable stderr
reset_calls
run_helper --check-frag "$F_TMP/nonexistent"
assert "F6: missing base_dir exits non-zero" test "$RC" -ne 0
assert "F6: actionable stderr when base_dir missing" \
    bash -c '[ -n "$1" ]' _ "$ERR_OUT"

# F7: --check-frag with higher-extent file triggers reseed-due correctly
F2_TMP="$(mktemp -d /tmp/test-refresh-warm-base-f2-XXXXXX)"
_TMPDIRS+=("$F2_TMP")
F2_BASE="$F2_TMP/base"
mkdir -p "$F2_BASE"
echo "bin" > "$F2_BASE/binary"

reset_calls
REIFY_TEST_FRAG_EXTENTS=65 run_helper --check-frag "$F2_BASE" --frag-threshold 64
assert "F7: extents 65 >= threshold 64 -> reseed-due" \
    bash -c 'printf "%s\n" "$1" | grep -q "^reseed-due "' _ "$OUT"

reset_calls
REIFY_TEST_FRAG_EXTENTS=63 run_helper --check-frag "$F2_BASE" --frag-threshold 64
assert "F7: extents 63 < threshold 64 -> ok" \
    bash -c 'printf "%s\n" "$1" | grep -q "^ok "' _ "$OUT"

test_summary
