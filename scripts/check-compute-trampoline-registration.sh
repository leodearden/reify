#!/usr/bin/env bash
# scripts/check-compute-trampoline-registration.sh
#
# INV-FEA-1 regression guard (PRD docs/prds/compute-fea-hardening.md task A5;
# task 5076).
#
# INV-FEA-1: every engine-construction site registers the compute trampolines
# through the ONE bundler, `Engine::register_production_compute_fns`, so no site
# can drift into registering a partial bundle. The hazards this catches are both
# silent — each compiles clean and passes every type check:
#   (1) a known construction site stops delegating to the bundler;
#   (2) a site keeps delegating but passes the WRONG MorphRegistration variant —
#       specifically, flipping gui/src-tauri/src/engine.rs's
#       `#[cfg(feature = "gui")]` arm from `Enabled(..)` to `Unavailable{..}`
#       silently un-registers the mesh-morph producer (the esc-2962-66 class);
#   (3) a FOURTH site hand-rolls the bundle from its halves instead of calling
#       the bundler, so a later leg added to the bundler never reaches it.
#
# WHAT IS MATCHED — two independent passes, both reading the SAME
# production-code view (see PRODUCTION-CODE VIEW below):
#
#   POSITIVE (delegation).  A literal table of the known engine-construction
#   sites, each pinned to the MorphRegistration variant it must pass. Each site
#   must be tracked, must call `register_production_compute_fns(`, and must
#   carry its required variant token. Matching is on CONTENT, never on line
#   numbers: engine.rs's call moved :1057 -> :1085 between two revisions purely
#   from the file growing.
#
#   NEGATIVE (no fourth bundler).  Any DIRECT call to a bundle HALF —
#   `register_compute_fns(` or `register_shell_extract_compute_fns(` — is a
#   hand-rolled bundle and is flagged as file:line. The halves are the precise
#   signal: a hand-rolled bundler is definitionally a site that calls them
#   itself instead of delegating. `Engine::new` /
#   `Engine::with_registered_kernel` are deliberately NOT matched — they have
#   hundreds of legitimate call sites — and neither is `register_morph_producer(`,
#   which is a legitimate public API rather than a bundler.
#
# PRODUCTION-CODE VIEW (shared by BOTH passes — RUST_PRODUCTION_VIEW_AWK from
# scripts/lib_rust_production_view.sh, whose header documents the lexer
# mechanism and its best-effort limits). Comment text is DROPPED; string,
# char-literal and raw-string CONTENTS are BLANKED with their delimiters kept (a
# blanked string reads exactly `""`). A line is invisible to both passes when it
# is inside a test-gated `mod` body, or wholly inside a block comment or a
# carried-over multi-line string.
#
# The brace counts that drive `depth` — and so BOTH depth-driven skippers, the
# test-module one and the definition-file `in_bundler` one — are taken
# from that LEXED view, never from the raw line. A brace that exists only inside
# a comment, a string, a char literal or a raw string must not move `depth`: a
# stray `{` over-extends the test-module skipper until the fourth-bundler pass
# goes blind (hazard 3), and a stray `}` releases it early until the per-site
# variant pin goes vacuous (hazard 2). Both are live shapes, not hypotheticals —
# four unbalanced brace char literals sit in the scan set today
# (crates/reify-kernel-occt/src/lib.rs:4930,4948,9167 and
# crates/reify-test-support/src/orphan_audit.rs:778) and 277 of its files carry
# raw strings.
#
# A desync is surfaced rather than trusted: the shared view's END block WARNs —
# never fails — when a file's lexer state is unbalanced at EOF (the negative
# pass only; `_code_has` passes `-v quiet_eof=1`, see there). It is the only
# signal for the failure mode that is otherwise INVISIBLE, because a line
# matching `carried_in && code == ""` is `next`ed and so never scanned, and an
# unscanned line cannot be flagged.
# MEASURED on this gate's own scan set at the time of writing: 562 files, of
# which 562 end balanced, so the warning is silent on the live tree (verified
# under gawk 5.2.1 and mawk 1.3.4; check-nan-safe-ordering.sh records the same
# for its 121-file set). That measurement is what makes it safe as a warning
# and premature as a failure.
#
# The two passes are therefore symmetric by construction. That symmetry is
# load-bearing on BOTH sides:
#   - negative: crates/reify-mesh-morph/src/lib.rs carries a rustdoc MENTION of
#     a bundle half OUTSIDE any `#[cfg(test)]` module, so only the //-strip
#     keeps it green;
#   - positive: engine.rs already carries three inline `#[cfg(test)]` modules,
#     and crates/reify-cli/src/main.rs:4390 carries the near-miss STRING
#     "…must pass MorphRegistration::Enabled, ". Without test/comment/string
#     removal, one unit test or assertion message naming
#     `MorphRegistration::Enabled(` would make the variant pin VACUOUS — the
#     production `#[cfg(feature = "gui")]` arm could then be flipped to
#     `Unavailable` with the gate still exiting 0, i.e. hazard (2) unguarded.
# Deliberately NOT required: that the variant token and the delegation call sit
# in the same enclosing item. All three sites satisfy that today, but a routine
# refactor that hoists the variant into a helper fn would then be a false RED,
# and a false RED on a clean tree is worse than the residual it closes (a
# SECOND *production* mention of the variant in the SAME file — none exists:
# engine.rs has exactly one `MorphRegistration::Enabled(` in production code).
#
# COVERED SCOPE (negative pass) — tracked Rust production inputs:
#   crates/*/src/*.rs, crates/*/benches/*.rs, crates/*/examples/*.rs,
#   crates/*/build.rs, gui/src-tauri/src/*.rs, gui/src-tauri/build.rs,
#   gui/src-tauri/benches/*.rs.
# NOT COVERED, stated so the gate is not mistaken for exhaustive: `tests/` dirs
# (integration tests are test code by construction — skipped by path), and any
# Rust outside those pathspecs (e.g. a new top-level crate root outside
# `crates/`). Widening is a one-line edit to SCOPE_PATHSPECS.
#
# EXCLUDED (negative pass):
#   - comments and string literals, per the PRODUCTION-CODE VIEW above;
#   - test code: `tests/` dirs (by path) and test-gated `mod` bodies —
#     `#[cfg(test)]`, `#[cfg(all(test, …))]`,
#     `#[cfg(any(test, feature = "test…"))]`; the
#     arming rule and its limits live in scripts/lib_rust_production_view.sh
#     (brace-depth tracked, best-effort). Five real in-src callers live in
#     `#[cfg(test)]` modules and are legitimate: compute_persist.rs:529,672;
#     compute_targets/as_printed_material.rs:542; compute_targets/mod.rs:541,583;
#   - inside the DEFINITION files (EXEMPT_DEFINITION_FILES) — and ONLY there —
#     the two `fn register_*_compute_fns(` definition lines and the body of
#     `fn register_production_compute_fns(` (that body IS the bundler). The
#     exemption is scoped to those two constructs rather than granted
#     file-wholesale, so a fourth hand-rolled bundle added ELSEWHERE in
#     compute_targets/mod.rs — the single most likely place for someone to add
#     one — is still flagged;
#   - escaped sites: any line whose `//` COMMENT carries the inline escape
#         // trampoline-registration:allow — <reason>
#     mirroring reify-audit's `// ptodo:allow` convention (§6.8) and
#     check-nan-safe-ordering.sh's `// nan-safe:allow`. It must be a real
#     comment: the token is matched against the comment text the lexer drops,
#     so merely NAMING it inside a string literal on the same line does not
#     suppress anything. The escape is a NEGATIVE-pass concept only (it
#     declares an intentional direct half-call); it never suppresses a
#     positive-pass delegation requirement.
#
# HERMETIC SOURCE SET: `git ls-files` lists only tracked files, so untracked
# build artifacts never enter the scan (mirrors check_event_inventory.sh).
#
# Usage: scripts/check-compute-trampoline-registration.sh [--repo-root <dir>]
# Exit codes:
#   0  clean — every known site delegates with its required variant, and no
#      production source hand-rolls the bundle
#   1  at least one violation (each printed to stderr)
#   2  the gate could not scan: usage / not-a-git-work-tree error, the shared
#      lexer lib could not be loaded, an EMPTY SCAN SET (SCOPE_PATHSPECS matched
#      nothing, or matched only paths the */tests/* filter removes), or an AWK
#      FAILURE while scanning. Exit 2 OUTRANKS exit 1 — a gate that could not
#      scan has established neither that the tree is clean nor that it is
#      dirty, so a violation already queued by the positive pass does not
#      downgrade it.
# A `WARN: … lexer state unbalanced at EOF` line on stderr is verdict-neutral
# and never changes any of the above; see PRODUCTION-CODE VIEW below.

set -euo pipefail

REPO_ROOT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo-root) REPO_ROOT="${2:-}"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--repo-root <dir>]"
            exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$REPO_ROOT" ]]; then
    REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
fi
if [[ -z "$REPO_ROOT" ]] || ! git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ERROR: not a git work tree: ${REPO_ROOT:-<cwd>}" >&2
    exit 2
fi

# The shared lexer, resolved beside THIS script (never the CWD or --repo-root:
# the gate scans fixture repos that have no scripts/). A failed load is "could
# not scan", exit 2; a bare `source` failing under set -e would exit 1, which
# reads as "violation found".
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_rust_production_view.sh
if ! source "$SCRIPT_DIR/lib_rust_production_view.sh" || [[ -z "${RUST_PRODUCTION_VIEW_AWK:-}" ]]; then
    echo "ERROR: cannot load the shared Rust lexer lib: $SCRIPT_DIR/lib_rust_production_view.sh" >&2
    exit 2
fi

# ── Known engine-construction sites: <path>|<required MorphRegistration variant>
# The variant pin is what makes hazard (2) statically RED. reify-eval's test
# runner requires Unavailable rather than Enabled because reify-mesh-morph is
# not one of its dependencies.
KNOWN_SITES=(
    'crates/reify-cli/src/main.rs|MorphRegistration::Enabled[(]'
    'gui/src-tauri/src/engine.rs|MorphRegistration::Enabled[(]'
    'crates/reify-eval/src/test_runner.rs|MorphRegistration::Unavailable'
)

# ── Negative-pass scope. Single-star git pathspecs are NOT path-boundary-aware,
# so 'crates/*/src/*.rs' matches every tracked .rs at any depth beneath a crate's
# src/. benches/examples/build.rs are included because they are real build
# inputs a hand-rolled bundle could hide in (see COVERED SCOPE above).
SCOPE_PATHSPECS=(
    'crates/*/src/*.rs'
    'crates/*/benches/*.rs'
    'crates/*/examples/*.rs'
    'crates/*/build.rs'
    'gui/src-tauri/src/*.rs'
    'gui/src-tauri/build.rs'
    'gui/src-tauri/benches/*.rs'
)

# ── The two files that DEFINE the bundle halves. They are NOT skipped
# wholesale: they are scanned with `allow_defs=1`, which permits exactly two
# constructs (the `fn register_*_compute_fns(` definition lines, and the body of
# `fn register_production_compute_fns(` — the bundler itself) and flags every
# other direct half-call in them like anywhere else.
EXEMPT_DEFINITION_FILES=(
    'crates/reify-eval/src/compute_targets/mod.rs'
    'crates/reify-eval/src/shell_extract_compute.rs'
)

violations=""
note() { violations+="$1"$'\n'; }

# ── Both passes append their rules to RUST_PRODUCTION_VIEW_AWK (loaded above),
# so neither can see what the other cannot. Its test-module skipper requires
# the opening line to declare a `mod`: a bare `#[cfg(test)] use …;` —
# gui/src-tauri/src/engine.rs:21, :102, :104 all have one — must not arm it,
# because that misfire is a silent under-flag on the negative pass but a FALSE
# RED on the positive pass.

# _code_has <file> <awk-regex> — is the pattern present in PRODUCTION code?
# quiet_eof=1 because the `exit` below stops at the FIRST match by design,
# which leaves the lexer legitimately unbalanced mid-item, so the shared END
# block would otherwise WARN on every SUCCESSFUL site lookup. No coverage is
# lost: all three KNOWN_SITES are also in the negative pass's scan set and are
# scanned there in full, where the WARN stays live.
_code_has() {
    awk -v pat="$2" -v quiet_eof=1 "$RUST_PRODUCTION_VIEW_AWK"'
        code ~ pat { found = 1; exit }
        END { exit(found ? 0 : 1) }
    ' "$1"
}

# ── POSITIVE PASS ─────────────────────────────────────────────────────────────
for entry in "${KNOWN_SITES[@]}"; do
    site="${entry%%|*}"
    variant="${entry##*|}"

    if [[ -z "$(git -C "$REPO_ROOT" ls-files -- "$site")" ]] || [[ ! -f "$REPO_ROOT/$site" ]]; then
        note "$site: known engine-construction site is missing (expected a tracked file)"
        continue
    fi
    if ! _code_has "$REPO_ROOT/$site" 'register_production_compute_fns[(]'; then
        note "$site: no longer calls Engine::register_production_compute_fns(...)"
    fi
    if ! _code_has "$REPO_ROOT/$site" "$variant"; then
        note "$site: does not pass the required ${variant//\[(\]/(} to register_production_compute_fns"
    fi
done

# ── NEGATIVE PASS ─────────────────────────────────────────────────────────────
# Scan set: tracked production Rust minus `tests/` dirs. The two definition
# files stay IN the set and are scanned with allow_defs=1 (scoped exemption).
_files=()
while IFS= read -r -d '' _f; do
    case "$_f" in
        */tests/*) continue ;;
    esac
    _files+=("$_f")
done < <(git -C "$REPO_ROOT" ls-files -z -- "${SCOPE_PATHSPECS[@]}" 2>/dev/null)

# An empty scan set (a crate rename, a module move, a repo reorg that no longer
# matches SCOPE_PATHSPECS) must fail loudly, not exit 0 vacuously — a gate that
# scans nothing looks identical, from the caller's side, to a gate that scanned
# everything and found it clean. Checked AFTER the */tests/* filter, because
# that filter can empty a non-empty match set on its own: single-star pathspecs
# are not path-boundary-aware, so 'crates/*/src/*.rs' does match
# crates/reify-foo/src/tests/helper.rs.
#
# Deliberately placed BEFORE the violations check below, so it outranks any
# violation the positive pass has already queued: a stale scope is an
# infrastructure fault, not a code finding, and a gate that could not scan has
# established neither that the tree is clean nor that it is dirty.
if [[ ${#_files[@]} -eq 0 ]]; then
    echo "ERROR: no tracked .rs files matched SCOPE_PATHSPECS — scope is stale?" >&2
    exit 2
fi

# Per-file action block, appended to the shared production view. In order:
#   1. honor the same-line `trampoline-registration:allow` escape;
#   2. in a DEFINITION file only, skip the bundler body and the two definition
#      lines (everything else in those files is still matched);
#   3. flag a direct call to a bundle half.
for f in "${_files[@]}"; do
    _allow_defs=0
    for _x in "${EXEMPT_DEFINITION_FILES[@]}"; do
        [[ "$f" == "$_x" ]] && { _allow_defs=1; break; }
    done
    # Checked explicitly (rather than left to `set -e` propagation) so a failing
    # awk gets a diagnostic and the documented exit 2. Under plain `set -e` a
    # failing `out="$(awk …)"` aborts with awk's OWN status, which for some awk
    # failure modes is 1 — indistinguishable from "found a violation" (verified:
    # PATH-shadowing awk to a stub that always exits 1 makes the pre-fix gate
    # exit 1 on a CLEAN fixture, silently, with nothing printed).
    if ! out="$(awk -v rel="$f" -v allow_defs="$_allow_defs" "$RUST_PRODUCTION_VIEW_AWK"'
        {
            # --- inline escape (same-line), mirrors ptodo:allow §6.8 ---
            # Matched against comment_tail (the dropped `//…` text that
            # _strip_line stashed), NOT `code` and NOT raw $0. `code` is wrong
            # because the escape lives in a `//` comment, which the lexer
            # drops — matching `code` would silently kill every escape. Raw $0
            # is wrong the OTHER way: it also carries any string/char-literal
            # content on the line, so a token that merely *appears inside a
            # string* — e.g. `let _doc = "trampoline-registration:allow";`
            # sharing a line with a real half-call — would wrongly suppress a
            # real violation.
            if (comment_tail ~ /trampoline-registration:allow/) next

            if (allow_defs) {
                # Body of fn register_production_compute_fns — THE bundler.
                # depth here is the depth AFTER this line, so subtracting this
                # line net brace delta recovers the depth before it.
                if (in_bundler) {
                    if (depth <= bundler_base) in_bundler = 0
                    else next
                }
                if (!in_bundler) {
                    # A DECLARATION rather than a definition — `fn
                    # register_production_compute_fns(…);` as a trait-method
                    # signature or an extern entry — opens no body. Arming on it
                    # would leave `pending_bundler` set until some unrelated
                    # LATER brace-opening line, whose whole block is then
                    # `next`ed out of this pass: a silent under-flag (fails
                    # toward GREEN) inside exactly the two files the header
                    # calls the single most likely place for someone to add a
                    # fourth hand-rolled bundle. Mirrors the `pending_mod` guard
                    # in the shared view (`n_open == 0 && t !~ /;/`); the
                    # `n_open > 0 ||` arm keeps the ordinary same-line `) {`
                    # spelling armed.
                    # NOTE: this block is single-quoted bash — no apostrophes.
                    if (code ~ /fn[ \t]+register_production_compute_fns[(]/) {
                        if (n_open > 0 || code !~ /;/) pending_bundler = 1
                    } else if (pending_bundler && n_open == 0 && code ~ /;/) {
                        # The multi-line spelling of that same declaration: a
                        # `;` reached with no body opened ends the item.
                        # Signature continuation lines (params, `->`, a
                        # where-clause) carry no `;`, so a genuine multi-line
                        # DEFINITION stays armed until its `{`.
                        pending_bundler = 0
                    }
                    if (pending_bundler && n_open > 0) {
                        in_bundler = 1
                        bundler_base = depth - (n_open - n_close)
                        pending_bundler = 0
                        next
                    }
                }
                # The definition lines themselves are declarations, not calls.
                if (code ~ /fn[ \t]+register_compute_fns[(]/) next
                if (code ~ /fn[ \t]+register_shell_extract_compute_fns[(]/) next
            }

            if (code ~ /register_compute_fns[(]/ || code ~ /register_shell_extract_compute_fns[(]/) {
                # RAW $0 on purpose: this is human-readable violation output,
                # so it must show the source line as written, not the lexed view.
                printf "%s:%d: %s\n", rel, FNR, $0
            }
        }
    ' "$REPO_ROOT/$f")"; then
        echo "ERROR: awk failed while scanning $f" >&2
        exit 2
    fi
    [[ -n "$out" ]] && note "$out"
done

if [[ -n "${violations//$'\n'/}" ]]; then
    printf '%s' "$violations" | grep -v '^$' >&2
    n="$(printf '%s' "$violations" | grep -c '.')"
    {
        echo ""
        echo "ERROR: $n INV-FEA-1 violation(s) found (task 5076)."
        echo "Every engine-construction site must register the compute trampolines"
        echo "through the single bundler:"
        echo "    engine.register_production_compute_fns(<MorphRegistration variant>);"
        echo "rather than calling register_compute_fns / register_shell_extract_compute_fns"
        echo "itself — see docs/prds/compute-fea-hardening.md task A5 (INV-FEA-1)."
        echo "If a site genuinely must call a half directly, annotate it with:"
        echo "    // trampoline-registration:allow — <why the bundle is wrong here>"
    } >&2
    exit 1
fi

exit 0
