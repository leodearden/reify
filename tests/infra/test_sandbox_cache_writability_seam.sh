#!/usr/bin/env bash
# Infrastructure test for task 5332 (OS-sandbox rollout γ6a).
#
# Landlock cache-writability seam guard: dark-factory-orchestrator.yaml's
# sandbox block (sandbox.enabled: true, backend: landlock; see PRD
# os-sandbox-worktree-containment, task γ6) makes the whole filesystem
# read-only for the three sandboxed roles (implementer, debugger,
# simple_task — roles.py sandboxed=True) except dark-factory's
# compute_write_set() (orchestrator/agents/write_set.py, DF-owned): the
# worktree, <base>/.task-meta/<name>/, main .git carve-outs, ~/.cache/uv/,
# ~/.claude/fleet/, /tmp/, and /dev/. That set has NO carve-out for
# $CARGO_HOME (~/.cargo, cargo's default) or the npm cache (~/.npm, npm's
# default).
#
# REPRODUCED on this host with the real helper
# (orchestrator/agents/landlock_exec.py):
#   (A) uncached dep, default CARGO_HOME: `cargo fetch` -> exit 101,
#       "error: failed to open `/home/leo/.cargo/registry/cache/
#        index.crates.io-*/hex-literal-0.4.1.crate`
#        Caused by: Permission denied (os error 13)"
#   (B) default ~/.npm: `npm install` -> exit 1,
#       "Log files were not written due to an error writing to the
#        directory: /home/leo/.npm/_logs"
# Warm paths still succeed, so this fails exactly on dependency-adding /
# cold-cache tasks — routine in a Rust workspace + Tauri GUI repo.
#
# FIX: dark-factory-orchestrator.yaml's `role_env_overrides` (DF
# config.py:3801, a dict[role -> dict[env]] merged LAST into EVERY role's
# env by workflow.py:_build_agent_env — not just the three infra roles)
# redirects CARGO_HOME and npm_config_cache to /tmp paths for each
# sandboxed role. /tmp is the one host-global writable root in the write
# set that a static yaml value can name. `verify_env` (elsewhere in this
# file) is the WRONG surface: it feeds VERIFY subprocesses, which are not
# sandboxed.
#
# This guard pins the reify-owned leaf of the contract only.
# compute_write_set itself is entirely DF-owned and not reify-observable
# from this repo; extending it (e.g. adding ~/.cargo/~/.npm carve-outs
# directly) is upstream dark-factory work, filed as reify task 5724
# ("dark-factory: add ~/.cargo and ~/.npm to compute_write_set...") --
# landing that would retire both this guard and the /tmp redirect below.
#
# SECOND, UNGUARDED COUPLING: SANDBOXED_ROLES below is a hardcoded reify-side
# copy of DF's roles.py sandboxed=True set (orchestrator/agents/roles.py --
# implementer, debugger, simple_task). If DF ever marks a fourth role
# sandboxed, THIS guard stays green while that role runs with an unwritable
# ~/.cargo -- the exact EACCES-mid-build failure this guard exists to
# prevent, just on a role this file doesn't know to check. There is no
# reify-observable way to close that gap from this repo (roles.py is
# DF-owned); a DF-side role addition needs a breadcrumb back to this file
# and to dark-factory-orchestrator.yaml's role_env_overrides comment.
#
# Pins, against dark-factory-orchestrator.yaml:
#   (A) STRUCTURE (python3+PyYAML, SKIP-guarded): IF sandbox.enabled is
#       true, THEN for EACH sandboxed role (implementer, debugger,
#       simple_task), role_env_overrides[<role>] defines CARGO_HOME,
#       npm_config_cache AND XDG_CACHE_HOME, each an absolute path lying
#       under /tmp (the landlock write set's host-global writable root)
#       and explicitly NOT under $HOME/.cargo, $HOME/.npm or
#       $HOME/.cache (respectively) — the exact regression this guard
#       exists to catch. Gated on sandbox.enabled so it stays correct if
#       sandbox is ever flipped back off (mirrors the precondition-gated
#       structure of test_merge_role_lint_first_seam.sh's
#       concurrent_verify guard).
#   (B) KNOB-NAME CROSS-CHECK (plain bash grep, always runs, no python
#       needed): `backend: landlock` and `enabled: true` are cited under
#       the sandbox block, and `role_env_overrides:`, `CARGO_HOME:`,
#       `npm_config_cache:` and `XDG_CACHE_HOME:` are all cited verbatim
#       in the yaml.
#
# WHY XDG_CACHE_HOME: compute_write_set() (orchestrator/agents/write_set.py)
# has no carve-out for ~/.cache/tree-sitter (verified: zero tree-sitter /
# tree_sitter hits in that file), so a sandboxed role's grammar probe dies
# writing ~/.cache/tree-sitter/lock — the same EACCES-mid-build shape as the
# CARGO_HOME/npm_config_cache gap above, just for tree-sitter's compile
# cache. tree-sitter 0.26.8 (the version on PATH at
# /home/leo/.cargo/bin/tree-sitter) exposes no tree-sitter-specific cache-dir
# env var; XDG_CACHE_HOME is the only lever, and it was MEASURED on this
# host to relocate tree-sitter's lib/ + lock/ dirs and to create the whole
# tree when the root is absent (no provisioning step needed). Upstream fix:
# cross-repo task 5896 (adds a ~/.cache/tree-sitter carve-out to
# compute_write_set directly, cancelled reify-side and refiled in
# dark-factory); landing it would retire this leg of the redirect the same
# way task 5724 would retire the CARGO_HOME/npm_config_cache leg.
#
# (A) is SKIPPED if python3 + PyYAML are unavailable (mirrors the SKIP
#     idiom used by test_merge_role_lint_first_seam.sh /
#     test_cpu_governance_config.sh).
# (B) always runs — plain bash grep, no python needed.
#
# RED at authoring time: role_env_overrides is absent from the yaml while
# sandbox.enabled is true.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== sandbox cache-writability seam contract tests (task 5332) ==="

ORCH_YAML="$REPO_ROOT/dark-factory-orchestrator.yaml"
# Hardcoded copy of DF's roles.py sandboxed=True set -- see the "SECOND,
# UNGUARDED COUPLING" note in the file header. Not reify-observable to keep
# in sync automatically; a DF-side role addition needs a manual update here.
SANDBOXED_ROLES="implementer debugger simple_task"

# ---------------------------------------------------------------------------
# (A) STRUCTURE -- parse YAML and assert key/value shape
# ---------------------------------------------------------------------------

if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping YAML structure assertions"
else
    echo "--- (A) structural assertions via PyYAML ---"

    _PARSE_PY="$(mktemp /tmp/sandbox_cache_seam_parse_XXXXXX.py)"
    trap 'rm -f "$_PARSE_PY"' EXIT

    cat > "$_PARSE_PY" << 'PYEOF'
"""Validate dark-factory-orchestrator.yaml's sandbox cache-writability
contract (landlock toolchain-cache redirect, task 5332 / os-sandbox rollout
gamma6a).

Usage:
  python3 <script> <orch_yaml> <check> [args...]

Checks:
  parse_ok                                 -- file parses as valid YAML
  sandbox_enabled_true                     -- top-level sandbox.enabled == true
  role_key_present <role> <key>            -- role_env_overrides[role][key]
                                               exists, is a string, and is
                                               non-empty
  role_key_absolute <role> <key>           -- ...and is an absolute path
  role_key_under_tmp <role> <key>          -- ...and lies STRICTLY under
                                               /tmp (a proper descendant --
                                               /tmp itself is rejected) --
                                               /tmp is the one host-global
                                               writable root in the landlock
                                               write set a static yaml value
                                               can name; see orchestrator/
                                               agents/write_set.py
  role_key_not_under <role> <key> <prefix> -- ...and does NOT lie under
                                               <prefix> (used to pin it is
                                               NOT under $HOME/.cargo or
                                               $HOME/.npm -- the exact
                                               regression this guard exists
                                               to catch)
  role_keys_distinct <role> <k1> <k2>      -- role_env_overrides[role][k1]
                                               != role_env_overrides[role][k2]
                                               (catches a degenerate same-dir
                                               target: cargo's and npm's
                                               cache-directory layouts would
                                               collide if CARGO_HOME and
                                               npm_config_cache pointed at
                                               the same path)

Exit 0 on pass, 1 on fail (or missing key/role), 2 on unknown check/bad args.
"""
import sys
import yaml
from pathlib import Path

orch_yaml_path = sys.argv[1]
check = sys.argv[2]
rest = sys.argv[3:]

with open(orch_yaml_path) as f:
    d = yaml.safe_load(f)


def _role_value(role, key):
    overrides = d.get("role_env_overrides")
    if not isinstance(overrides, dict):
        return None
    role_map = overrides.get(role)
    if not isinstance(role_map, dict):
        return None
    val = role_map.get(key)
    if not isinstance(val, str) or val.strip() == "":
        return None
    return val


if check == "parse_ok":
    sys.exit(0)

if check == "sandbox_enabled_true":
    sandbox = d.get("sandbox")
    ok = isinstance(sandbox, dict) and sandbox.get("enabled") is True
    sys.exit(0 if ok else 1)

if check == "role_key_present":
    role, key = rest
    sys.exit(0 if _role_value(role, key) is not None else 1)

if check == "role_key_absolute":
    role, key = rest
    val = _role_value(role, key)
    sys.exit(0 if val is not None and Path(val).is_absolute() else 1)

if check == "role_key_under_tmp":
    role, key = rest
    val = _role_value(role, key)
    if val is None:
        sys.exit(1)
    p = Path(val)
    # Strict descendant only -- /tmp itself is deliberately REJECTED here.
    # CARGO_HOME: /tmp (or npm_config_cache: /tmp) would pass every other
    # assertion in this suite yet make cargo/npm scatter registry/, git/,
    # bin/, .package-cache (or npm's cacache tree) directly into the
    # world-writable shared /tmp root of a many-concurrent-task host --
    # colliding with every other tenant of /tmp, inside the landlock write
    # set, so nothing else would catch it either.
    sys.exit(0 if Path("/tmp") in p.parents else 1)

if check == "role_keys_distinct":
    role, key1, key2 = rest
    val1 = _role_value(role, key1)
    val2 = _role_value(role, key2)
    if val1 is None or val2 is None:
        sys.exit(1)
    sys.exit(0 if val1 != val2 else 1)

if check == "role_key_not_under":
    role, key, prefix = rest
    val = _role_value(role, key)
    if val is None:
        sys.exit(1)
    # Plain string-prefix containment (deliberately not a resolve()-based
    # check -- symlink resolution could mask the exact literal-default
    # regression this guard exists to catch).
    norm_val = str(Path(val))
    norm_prefix = str(Path(prefix))
    sys.exit(1 if (norm_val == norm_prefix or norm_val.startswith(norm_prefix + "/")) else 0)

print(f"unknown check or bad args: {check} {rest}", file=sys.stderr)
sys.exit(2)
PYEOF

    assert "dark-factory-orchestrator.yaml parses as valid YAML" \
        python3 "$_PARSE_PY" "$ORCH_YAML" parse_ok

    if python3 "$_PARSE_PY" "$ORCH_YAML" sandbox_enabled_true; then
        echo "sandbox.enabled == true; asserting cache-writability contract for sandboxed roles ($SANDBOXED_ROLES)"
        for role in $SANDBOXED_ROLES; do
            for key in CARGO_HOME npm_config_cache XDG_CACHE_HOME; do
                assert "role_env_overrides.${role}.${key} is present and non-empty" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_present "$role" "$key"

                assert "role_env_overrides.${role}.${key} is an absolute path" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_absolute "$role" "$key"

                assert "role_env_overrides.${role}.${key} lies under /tmp (the landlock write set's host-global writable root)" \
                    python3 "$_PARSE_PY" "$ORCH_YAML" role_key_under_tmp "$role" "$key"
            done

            assert "role_env_overrides.${role}.CARGO_HOME is NOT under \$HOME/.cargo (the regression this guard exists to catch)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_key_not_under "$role" CARGO_HOME "$HOME/.cargo"

            assert "role_env_overrides.${role}.npm_config_cache is NOT under \$HOME/.npm (the regression this guard exists to catch)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_key_not_under "$role" npm_config_cache "$HOME/.npm"

            assert "role_env_overrides.${role}.XDG_CACHE_HOME is NOT under \$HOME/.cache (the ungranted ~/.cache/tree-sitter default this guard exists to catch)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_key_not_under "$role" XDG_CACHE_HOME "$HOME/.cache"

            assert "role_env_overrides.${role}.CARGO_HOME and .npm_config_cache are distinct paths (a same-dir target would pass every check above yet collide cargo's and npm's cache layouts)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_keys_distinct "$role" CARGO_HOME npm_config_cache

            assert "role_env_overrides.${role}.CARGO_HOME and .XDG_CACHE_HOME are distinct paths (a same-dir target would let tree-sitter's lib/ and lock/ dirs land inside cargo's cache layout and pass every other check)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_keys_distinct "$role" CARGO_HOME XDG_CACHE_HOME

            assert "role_env_overrides.${role}.npm_config_cache and .XDG_CACHE_HOME are distinct paths (a same-dir target would let tree-sitter's lib/ and lock/ dirs land inside npm's cache layout and pass every other check)" \
                python3 "$_PARSE_PY" "$ORCH_YAML" role_keys_distinct "$role" npm_config_cache XDG_CACHE_HOME
        done
    else
        echo "SKIP: sandbox.enabled is not true; cache-redirect contract does not apply (guard is conditional, see header)"
    fi
fi

# ---------------------------------------------------------------------------
# (B) KNOB-NAME / VALUE CROSS-CHECK -- always runs (bash grep, no python needed)
# ---------------------------------------------------------------------------
echo "--- (B) knob-name / value cross-check (bash grep) ---"

# Windowed grep (LINT_PLAN idiom from test_merge_role_lint_first_seam.sh (C) /
# test_release_mode_in_test_command.sh): isolate the sandbox: block's own
# lines so `enabled: true` / `backend: landlock` are pinned to THAT block,
# not any other `enabled:`-keyed block in the file (e.g. usage_cap.enabled).
# `|| true` guards the command substitution under `set -euo pipefail`: if
# `sandbox:` is ever renamed/moved/deleted, a bare failing grep here would
# abort the whole script before test_summary runs, losing the diagnostic
# for every assertion below (including the unrelated (B) knob-name checks)
# behind a bare non-zero exit. With `|| true`, SANDBOX_BLOCK is empty and
# the two windowed asserts below report a real FAIL against it instead.
SANDBOX_BLOCK="$(grep -A5 '^sandbox:' "$ORCH_YAML" || true)"
export SANDBOX_BLOCK

assert "'enabled: true' cited under the sandbox block" \
    bash -c 'printf "%s\n" "$SANDBOX_BLOCK" | grep -qE "^[[:space:]]*enabled:[[:space:]]*true[[:space:]]*$"'

assert "'backend: landlock' cited under the sandbox block" \
    bash -c 'printf "%s\n" "$SANDBOX_BLOCK" | grep -qE "^[[:space:]]*backend:[[:space:]]*landlock[[:space:]]*$"'

assert "'role_env_overrides:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^role_env_overrides:[[:space:]]*$' "$ORCH_YAML"

assert "'CARGO_HOME:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*CARGO_HOME:' "$ORCH_YAML"

assert "'npm_config_cache:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*npm_config_cache:' "$ORCH_YAML"

assert "'XDG_CACHE_HOME:' cited in dark-factory-orchestrator.yaml" \
    grep -qE '^[[:space:]]*XDG_CACHE_HOME:' "$ORCH_YAML"

test_summary
