#!/usr/bin/env bash
# tests/infra/test_no_prebuilt_reify_bin_spawn.sh
#
# Structural build-edge guard (task #5718).
#
# INVARIANT: no tracked Rust source OUTSIDE `crates/reify-cli/` may resolve the
# `reify` CLI binary by filesystem path, nor shell out to `cargo run -p
# reify-cli` / `--bin reify`, to obtain one.
#
# WHY. A test that resolves `target/<profile>/reify` by path has NO cargo build
# edge on `reify-cli`: `cargo test -p reify-eval` rebuilds the test binary and
# its lib deps but never rebuilds the CLI, so the test asserts against a binary
# that can arbitrarily predate the change under test — and PASSES. That is a
# measurement-validity bug, not a flake: it was observed on #5618, where two
# byte-identity goldens read GREEN across multiple iterations against a stale
# pre-change `target/debug/reify` and only turned RED after an explicit
# `cargo build --bin reify`. The `cargo run -p reify-cli` variant does have a
# real build edge, but pays a full-workspace re-fingerprint plus the global
# cargo build-lock per invocation — the documented cause of esc-4340-32
# (exit 124) — so it is barred here too.
#
# REMEDY (the only one that eliminates the class by construction): put the test
# in `crates/reify-cli/tests/harness_cli/` and spawn `env!("CARGO_BIN_EXE_reify")`.
# Cargo guarantees a package's `[[bin]]` targets are built before that package's
# own integration tests run, so the binary is fresh by construction — no env
# var, no sidecar, no mtime heuristic, no fail-open/fail-closed dilemma.
#
# SCOPE. This is a structural grep over source WIRING — the `.join("reify")`
# idiom and the `cargo`-spawn arg vectors that actually cause the bug. It
# deliberately does NOT grep comments, doc-strings, or markdown for wording:
# that prose-pin anti-pattern was introduced and then removed by task #4463
# (commit 2e1b474da5) precisely because it pins documentation rather than
# behaviour and goes stale on every rewording. A line that matches the idiom
# but provably is not binary resolution opts out with a trailing
# `// reify-bin-spawn:allow — <reason>` (see "Escape hatch" below).
#
# Checks:
#   1: `git ls-files` enumeration is non-empty (guard is not vacuously green)
#   2: (a) no filesystem-path resolution of a `reify` binary outside reify-cli
#   3: (b) no `cargo` subprocess spawn used to obtain/run the reify binary
#   4: positive control — the patterns DO match a synthetic violating file, and
#      the `reify-bin-spawn:allow` marker suppresses a marked line
#   5: `crates/reify-cli/` really is exempt (the sanctioned home has spawns)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== no-prebuilt-reify-bin-spawn structural guard (task #5718) ==="

TMPDIR_NOSPAWN=$(mktemp -d "${TMPDIR:-/tmp}/test-no-prebuilt-spawn-XXXXXX")
trap 'rm -rf "$TMPDIR_NOSPAWN"' EXIT

# The sanctioned home for CLI-spawning tests. Anything under this prefix is
# exempt: it is exactly the package that owns the `reify` [[bin]], so cargo
# builds the binary before running its tests.
EXEMPT_PREFIX='crates/reify-cli/'

# ── Pattern set ──────────────────────────────────────────────────────────────
#
# (a) Filesystem-path resolution of the reify binary. Covers both the
#     profile-local `<target>/<profile>/reify` form and the cross-profile
#     `<target>/debug/reify` fallback, in their `PathBuf::join` spelling.
PAT_PATH_JOIN='\.join\("reify"\)|\.join\("debug"\)\.join\("reify"\)'
#
# (b) A `cargo` subprocess spawned to build-and-run the CLI. Covers the
#     `env!("CARGO")` spelling (the cargo-provided path) and a literal
#     `Command::new("cargo")`.
PAT_CARGO_SPAWN='Command::new\(env!\("CARGO"\)\)|Command::new\("cargo"\)'
#
# ── Escape hatch ─────────────────────────────────────────────────────────────
#
# Pattern (a) matches the `.join("reify")` IDIOM; it cannot prove the joined path
# is the reify *binary*. The same spelling legitimately builds an XDG config or
# cache directory — `crates/reify-cli/src/cache.rs` does exactly that today, and
# is exempt only by accident of living under EXEMPT_PREFIX. The first XDG dir
# resolution added in reify-config, reify-lsp or gui/src-tauri would otherwise
# trip this hard gate for code that never spawns a binary.
#
# Narrowing the regex (requiring a target/profile-shaped receiver, or a
# downstream `Command::new`) would trade that false positive for false negatives,
# and a guard that has silently stopped firing is the exact failure mode this
# file exists to prevent. So the pattern stays broad and an individual line may
# opt out explicitly, mirroring the house PTODO convention (`// ptodo:allow —
# reason`, CLAUDE.md):
#
#     let cfg = xdg.join("reify"); // reify-bin-spawn:allow — XDG config dir
#
# The marker must sit on the matched line itself. The reason after it is
# conventional, not grep-enforced: review is what checks the justification.
PAT_ALLOW='//[[:space:]]*reify-bin-spawn:allow'

# _tracked_rs_outside_cli [root] — emit every tracked .rs path NOT under the
# exempt prefix, one per line, from the repo at [root] (default REPO_ROOT).
# Pure enumeration; no matching.
_tracked_rs_outside_cli() {
    local root="${1:-$REPO_ROOT}"
    git -C "$root" ls-files '*.rs' | grep -v "^$EXEMPT_PREFIX" || true
}

# _scan <regex> [root] — print `path:line:text` for every match of <regex> in
# the tracked .rs files outside reify-cli, rooted at [root] (default REPO_ROOT).
# Lines carrying the PAT_ALLOW marker are suppressed. Exits 0 regardless of
# whether anything matched; callers inspect the output.
_scan() {
    local regex="$1"
    local root="${2:-$REPO_ROOT}"
    local f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        [ -f "$root/$f" ] || continue
        grep -HnE "$regex" "$root/$f" 2>/dev/null \
            | grep -vE "$PAT_ALLOW" \
            | sed "s|^$root/||" || true
    done < <(_tracked_rs_outside_cli "$root")
}

# _report_and_fail <label> <hits> — print the violating path:line list plus the
# remedy, then return 1 so `assert` records a FAIL.
_report_and_fail() {
    local label="$1" hits="$2"
    echo "VIOLATION ($label): tracked .rs outside ${EXEMPT_PREFIX} resolves a prebuilt \`reify\`:"
    printf '%s\n' "$hits" | sed 's/^/  /'
    echo ""
    echo "REMEDY: relocate the test into crates/reify-cli/tests/harness_cli/ and use"
    echo "        env!(\"CARGO_BIN_EXE_reify\"), so cargo guarantees a freshly built binary."
    echo ""
    echo "NOT the reify binary (e.g. an XDG config/cache dir named \"reify\")? Opt the"
    echo "line out explicitly: append \`// reify-bin-spawn:allow — <reason>\` to it."
    return 1
}

# ==============================================================================
# Check 1: the enumeration is non-empty (guard is not vacuously green)
# ==============================================================================
echo ""
echo "--- Check 1: tracked .rs enumeration outside ${EXEMPT_PREFIX} is non-empty ---"

_enumeration_non_empty() {
    local n
    n="$(_tracked_rs_outside_cli | wc -l)"
    echo "enumerated $n tracked .rs files outside $EXEMPT_PREFIX"
    [ "$n" -gt 0 ]
}

assert "git ls-files '*.rs' outside ${EXEMPT_PREFIX} yields at least one file" \
    _enumeration_non_empty

# ==============================================================================
# Check 2 (a): no filesystem-path resolution of a reify binary
# ==============================================================================
echo ""
echo "--- Check 2: no .join(\"reify\") / .join(\"debug\").join(\"reify\") outside ${EXEMPT_PREFIX} ---"

_no_path_join_resolution() {
    local hits
    hits="$(_scan "$PAT_PATH_JOIN")"
    [ -z "$hits" ] || _report_and_fail "path-join resolution" "$hits"
}

assert "no tracked .rs outside ${EXEMPT_PREFIX} resolves the reify binary by filesystem path" \
    _no_path_join_resolution

# ==============================================================================
# Check 3 (b): no cargo subprocess spawn to obtain/run the reify binary
# ==============================================================================
echo ""
echo "--- Check 3: no Command::new(env!(\"CARGO\")) / Command::new(\"cargo\") outside ${EXEMPT_PREFIX} ---"

_no_cargo_spawn() {
    local hits
    hits="$(_scan "$PAT_CARGO_SPAWN")"
    [ -z "$hits" ] || _report_and_fail "cargo subprocess spawn" "$hits"
}

assert "no tracked .rs outside ${EXEMPT_PREFIX} shells out to cargo to run the reify binary" \
    _no_cargo_spawn

# ==============================================================================
# Check 4: positive control — the patterns DO fire on a synthetic violation
#
# Without this, a typo in either regex would make checks 2-3 vacuously green
# forever. The fixture is a real git repo scanned through `_scan` ITSELF (with
# its `root` argument pointed at the fixture), so this control exercises the
# whole path checks 2-3 depend on — the `git ls-files` enumeration, the
# EXEMPT_PREFIX exclusion, the file-existence filter, the while-read loop, the
# PAT_ALLOW suppression and the regexes — rather than a hand-rolled restatement
# of them. A restatement would leave the plumbing with no positive control at
# all: if enumeration or filtering broke, `_scan` would return empty, checks 2-3
# would report PASS on a real violation, and a private fixture scanner would
# stay green throughout.
# ==============================================================================
echo ""
echo "--- Check 4: positive control (synthetic violating file is detected) ---"

PC_REPO="$TMPDIR_NOSPAWN/positive-control"
mkdir -p "$PC_REPO/crates/synth/tests"
cat > "$PC_REPO/crates/synth/tests/violating.rs" <<'EOF'
#[test]
fn spawns_a_prebuilt_binary() {
    let profile_local = profile_dir.join("reify");
    let fallback = target_dir.join("debug").join("reify");
    let _ = std::process::Command::new(env!("CARGO")).arg("run");
    let _ = std::process::Command::new("cargo").arg("run");
    let cfg = xdg_dir.join("reify"); // reify-bin-spawn:allow — XDG config dir, not the binary
}
EOF
git -C "$PC_REPO" init -q
git -C "$PC_REPO" add -A
git -C "$PC_REPO" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "positive control" 2>/dev/null

_pc_path_join_fires() {
    _scan "$PAT_PATH_JOIN" "$PC_REPO" | grep -q 'violating\.rs:3'
}
_pc_debug_join_fires() {
    _scan "$PAT_PATH_JOIN" "$PC_REPO" | grep -q 'violating\.rs:4'
}
_pc_cargo_env_fires() {
    _scan "$PAT_CARGO_SPAWN" "$PC_REPO" | grep -q 'violating\.rs:5'
}
_pc_cargo_literal_fires() {
    _scan "$PAT_CARGO_SPAWN" "$PC_REPO" | grep -q 'violating\.rs:6'
}
# Negative leg of the same control: the escape hatch must actually suppress the
# marked line (7), or the documented opt-out is a dead letter.
_pc_allow_marker_suppresses() {
    ! _scan "$PAT_PATH_JOIN" "$PC_REPO" | grep -q 'violating\.rs:7'
}

assert "positive control: .join(\"reify\") is detected" _pc_path_join_fires
assert "positive control: .join(\"debug\").join(\"reify\") is detected" _pc_debug_join_fires
assert "positive control: Command::new(env!(\"CARGO\")) is detected" _pc_cargo_env_fires
assert "positive control: Command::new(\"cargo\") is detected" _pc_cargo_literal_fires
assert "positive control: a \`reify-bin-spawn:allow\` line is NOT reported" \
    _pc_allow_marker_suppresses

# ==============================================================================
# Check 5: the exemption is real, not vacuous — crates/reify-cli/ genuinely
# contains CARGO_BIN_EXE_reify spawns. If this ever goes empty, the exemption
# has stopped carrying any traffic and checks 2-3 would be guarding nothing.
# ==============================================================================
echo ""
echo "--- Check 5: ${EXEMPT_PREFIX} is the sanctioned home (CARGO_BIN_EXE_reify spawns live there) ---"

_exempt_prefix_carries_spawns() {
    local n
    n="$(git -C "$REPO_ROOT" ls-files "${EXEMPT_PREFIX}*.rs" \
        | while IFS= read -r f; do
              [ -f "$REPO_ROOT/$f" ] || continue
              grep -lE 'CARGO_BIN_EXE_reify' "$REPO_ROOT/$f" 2>/dev/null || true
          done | wc -l)"
    echo "found $n file(s) under $EXEMPT_PREFIX using CARGO_BIN_EXE_reify"
    [ "$n" -gt 0 ]
}

assert "crates/reify-cli/ contains at least one env!(\"CARGO_BIN_EXE_reify\") spawn" \
    _exempt_prefix_carries_spawns

test_summary
