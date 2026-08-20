#!/usr/bin/env bash
# Canonical, self-provisioning runner for the reify-gui frontend (vitest) test
# suites — the command an agent should use at a mid-task FRONTEND CHECKPOINT.
#
# Usage:
#   scripts/gui-test.sh                    # provision + typecheck + full vitest run
#   scripts/gui-test.sh --no-typecheck     # provision + vitest only (fast iteration)
#   scripts/gui-test.sh -- <vitest args>   # forward args to `vitest run`
#       e.g.  scripts/gui-test.sh -- src/__tests__/unitLadder.test.ts
#
# WHY THIS EXISTS (esc-5232-3): warm-lane / task worktrees are provisioned by a
# `git worktree add` (tracked files only) plus a CoW Cargo `target/`; gui/node_modules
# is gitignored (.gitignore:23) and dark-factory's acquire runs
# `git clean -xfd -e target`, so a freshly-seeded lane reliably has NO
# gui/node_modules. Running `npm test` (or a bare `vitest`) there fails at the
# `pretest`->`build:grammar` hook because lezer-generator (in node_modules/.bin)
# is absent — the exact step-9 checkpoint failure in esc-5232-3. The full
# merge-verify gate only works because it prefixes `npm ci` (verify.sh gui
# block). This script gives development checkpoints the same self-provisioning:
# it runs `npm ci` FIRST, so the vitest suites run in any lane regardless of
# node_modules state. The npm cache (~/.npm) is shared across worktrees and
# warm, so `npm ci --prefer-offline` completes in ~1s offline.
#
# This is intentionally a STANDALONE checkpoint helper and is NOT wired into
# verify.sh: the merge gate keeps its own inlined `npm ci && npm run typecheck
# && npm test` in the gui block. Keeping them separate avoids making this file a
# load-bearing verify-pipeline artifact (scripts/verify-pipeline-guard.sh). The
# commands are kept equivalent by convention; if the gui block's command shape
# changes materially, update this script to match.

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage: scripts/gui-test.sh [--no-typecheck] [-- <vitest args>]

  Runs the reify-gui frontend (vitest) suites, self-provisioning gui/node_modules
  first via `npm ci --prefer-offline`, so it works in any warm-lane / task
  worktree where node_modules is absent.

  --no-typecheck   Skip `npm run typecheck` (tsc --noEmit); run vitest only.
  -- <vitest args> Forward everything after `--` to `vitest run`
                   (e.g. -- src/__tests__/unitLadder.test.ts to run one file).
  -h, --help       Show this help and exit.
EOF
}

# Resolve repo root from this script's path so it works from ANY cwd (including
# a warm-lane worktree whose cwd is not the repo root).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
GUI_DIR="$REPO_ROOT/gui"

DO_TYPECHECK=1
VITEST_ARGS=()
while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-typecheck) DO_TYPECHECK=0; shift ;;
        --) shift; VITEST_ARGS=("$@"); break ;;
        -h|--help) usage; exit 0 ;;
        *)
            echo "gui-test.sh: unknown argument '$1' (forward vitest args after '--', e.g. '-- $1')" >&2
            usage
            exit 64
            ;;
    esac
done

[ -d "$GUI_DIR" ] || { echo "gui-test.sh: no gui/ directory at $GUI_DIR" >&2; exit 1; }
cd "$GUI_DIR"

# 1. Provision node_modules deterministically from package-lock.json.
#    --prefer-offline uses the shared warm ~/.npm cache first (network only on a
#    cache miss), so this is ~1s in a warm lane and still correct on a cold
#    cache. `npm ci` installs EXACTLY the lockfile versions — identical to the
#    verify gate's gui block.
echo "==> gui-test: npm ci (provisioning gui/node_modules)..."
npm ci --prefer-offline --no-audit --no-fund

# 2. Typecheck (tsc --noEmit) — parity with the verify gate's gui block, which
#    catches type-only breakage that renders fine at runtime. Skippable for fast
#    vitest-only iteration.
if [ "$DO_TYPECHECK" -eq 1 ]; then
    echo "==> gui-test: npm run typecheck (tsc --noEmit)..."
    npm run typecheck
fi

# 3. Vitest. `npm test` = `vitest run`, whose `pretest` hook regenerates the
#    lezer parser (build:grammar) — which is why node_modules must exist first.
#    Forward any args after `--` to vitest via `npm test -- <args>`.
echo "==> gui-test: npm test (vitest run)..."
if [ "${#VITEST_ARGS[@]}" -gt 0 ]; then
    npm test -- "${VITEST_ARGS[@]}"
else
    npm test
fi
