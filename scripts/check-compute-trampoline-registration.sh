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
# WHAT IS MATCHED — two independent passes:
#
#   POSITIVE (delegation).  A literal table of the known engine-construction
#   sites, each pinned to the MorphRegistration variant it must pass. Each site
#   must be tracked, must call `register_production_compute_fns(`, and must
#   carry its required variant token. Matching is on CONTENT, never on line
#   numbers: engine.rs's call moved :1057 -> :1085 between two revisions purely
#   from the file growing.
#
#   NEGATIVE (no fourth bundler).  Over production sources only, any DIRECT call
#   to a bundle HALF — `register_compute_fns(` or
#   `register_shell_extract_compute_fns(` — is a hand-rolled bundle and is
#   flagged as file:line. The halves are the precise signal: a hand-rolled
#   bundler is definitionally a site that calls them itself instead of
#   delegating. `Engine::new` / `Engine::with_registered_kernel` are deliberately
#   NOT matched — they have hundreds of legitimate call sites — and neither is
#   `register_morph_producer(`, which is a legitimate public API rather than a
#   bundler.
#
# COVERED SCOPE (negative pass): tracked `crates/*/src/*.rs` and
# `gui/src-tauri/src/*.rs`.
#
# EXCLUDED (negative pass):
#   - the two DEFINITION files, declared explicitly below: compute_targets/mod.rs
#     defines one half and legitimately calls both from the bundler body, and
#     shell_extract_compute.rs defines the other;
#   - comments: the `//…` tail is stripped before matching, so a rustdoc mention
#     of a half is not a call. This is load-bearing, not decorative —
#     crates/reify-mesh-morph/src/lib.rs carries such a mention OUTSIDE any
#     `#[cfg(test)]` module, so the cfg(test) skipper alone would not cover it;
#   - test code: `tests/` dirs (by path) and `#[cfg(test)]` modules (brace-depth
#     tracked, best-effort). Five real in-src callers live in `#[cfg(test)]`
#     modules and are legitimate: compute_persist.rs:529,672;
#     compute_targets/as_printed_material.rs:542; compute_targets/mod.rs:541,583;
#   - escaped sites: any line carrying the inline escape
#         // trampoline-registration:allow — <reason>
#     mirroring reify-audit's `// ptodo:allow` convention (§6.8) and
#     check-nan-safe-ordering.sh's `// nan-safe:allow`.
#
# HERMETIC SOURCE SET: `git ls-files` lists only tracked files, so untracked
# build artifacts never enter the scan (mirrors check_event_inventory.sh).
#
# Usage: scripts/check-compute-trampoline-registration.sh [--repo-root <dir>]
# Exit codes:
#   0  clean — every known site delegates with its required variant, and no
#      production source hand-rolls the bundle
#   1  at least one violation (each printed to stderr)
#   2  usage / not-a-git-work-tree error

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
# src/.
SCOPE_PATHSPECS=(
    'crates/*/src/*.rs'
    'gui/src-tauri/src/*.rs'
)

# ── The ONLY files allowed to call a bundle half at top level: the two that
# DEFINE the halves. compute_targets/mod.rs additionally calls both from inside
# `register_production_compute_fns`'s own body — that IS the bundler.
EXEMPT_DEFINITION_FILES=(
    'crates/reify-eval/src/compute_targets/mod.rs'
    'crates/reify-eval/src/shell_extract_compute.rs'
)

violations=""
note() { violations+="$1"$'\n'; }

# _code_has <file> <awk-regex> — is the pattern present on a comment-stripped
# line? Stripping matters in the positive pass too: a rustdoc mention of
# `register_production_compute_fns(` must not stand in for a real call.
_code_has() {
    awk -v pat="$2" '
        { code = $0; sub(/\/\/.*/, "", code)
          if (code ~ pat) { found = 1; exit } }
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
_files=()
while IFS= read -r -d '' _f; do
    case "$_f" in
        */tests/*) continue ;;
    esac
    for _x in "${EXEMPT_DEFINITION_FILES[@]}"; do
        [[ "$_f" == "$_x" ]] && continue 2
    done
    _files+=("$_f")
done < <(git -C "$REPO_ROOT" ls-files -z -- "${SCOPE_PATHSPECS[@]}" 2>/dev/null)

# Per-file scan. The awk pass, for each line, in order:
#   1. tracks brace depth and skips lines inside a #[cfg(test)] module;
#   2. honors the same-line `trampoline-registration:allow` escape;
#   3. strips the //… comment tail, then flags a direct call to a bundle half.
# (Steps 1-3 are check-nan-safe-ordering.sh:106-127 reused verbatim; only the
# escape token and the match patterns differ.)
for f in "${_files[@]}"; do
    out="$(awk -v rel="$f" '
        {
            # Brace counts on this line (throwaway copies so $0 stays intact).
            c = $0; n_open  = gsub(/[{]/, "x", c)
            c = $0; n_close = gsub(/[}]/, "x", c)

            # --- #[cfg(test)] module skipping (best-effort brace tracking) ---
            if (in_test) {
                depth += n_open - n_close
                if (depth <= test_base) in_test = 0
                next
            }
            if ($0 ~ /#\[cfg\(test\)\]/) pending_test = 1
            if (pending_test && n_open > 0) {
                in_test = 1
                test_base = depth        # depth BEFORE this block opened
                depth += n_open - n_close
                pending_test = 0
                next
            }
            depth += n_open - n_close

            # --- inline escape (same-line), mirrors ptodo:allow §6.8 ---
            if ($0 ~ /trampoline-registration:allow/) next

            # --- strip //… comment tail, then match a direct bundle-half call ---
            code = $0
            sub(/\/\/.*/, "", code)
            if (code ~ /register_compute_fns[(]/ || code ~ /register_shell_extract_compute_fns[(]/) {
                printf "%s:%d: %s\n", rel, FNR, $0
            }
        }
    ' "$REPO_ROOT/$f")"
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
