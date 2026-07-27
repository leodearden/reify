#!/usr/bin/env bash
# tests/infra/nextest_absent_lib.sh — the shared nextest-absent simulation harness
#
# Sourceable library consolidating the bespoke temp-HOME + PATH-shim harnesses
# that three tests/infra suites had each hand-rolled (task 5602):
#
#     tests/infra/test_verify_nextest_absent_suites.sh  (task 5599 — the
#         empirically-validated symlink-farm implementation this lib is lifted
#         from; it is the source of truth, not a fresh design)
#     tests/infra/test_verify_nextest_probe.sh          (task 4971)
#     tests/infra/test_verify_semaphore_wiring.sh       (task 4502)
#
# WHAT IT SIMULATES — exactly ONE variable: "cargo-nextest is not installed",
# and nothing else. scripts/verify.sh gracefully falls back to emitting
# `cargo test` instead of `cargo nextest run` when cargo-nextest is genuinely
# absent from PATH (plan header `nextest=0`); this harness reproduces that host
# state without touching the real toolchain.
#
# WHY NOT THE NAIVE `PATH="$STUB:/usr/bin:/bin"` RECIPE. The obvious harness
# (stub `cargo` + fresh HOME + PATH cut down to /usr/bin:/bin) does yield
# nextest=0, but it strips ~/.cargo/bin WHOLESALE — and the `tree-sitter` CLI
# lives there too, so suites gated behind a tree-sitter readiness check FAIL for
# reasons that have nothing to do with nextest. The confound is invisible unless
# you already know where tree-sitter lives.
#
# THE FIVE LOAD-BEARING ELEMENTS (all measured, see task 5599's acceptance):
#   (1) a symlink farm mirroring the cargo bin dir MINUS cargo-nextest;
#   (2) PATH = farm : (the real PATH with the cargo bin dir element filtered
#       out), so the rest of the toolchain — notably tree-sitter — still
#       resolves. The farm goes FIRST so an overlaid entry
#       (nextest_absent_farm_put) shadows any same-named binary later in PATH;
#   (3) HOME = a temp dir, so verify.sh's apply_env() finds no $HOME/.cargo/env
#       to source (sourcing it would re-prepend the real cargo bin dir and
#       un-hide cargo-nextest);
#   (4) CARGO_HOME deliberately NOT exported, and actively unset with `env -u`
#       — cargo resolves `cargo-<subcmd>` from $CARGO_HOME/bin in ADDITION to
#       PATH, so leaking the real CARGO_HOME makes cargo-nextest reappear and
#       flips the header back to nextest=1 (observed);
#   (5) RUSTUP_HOME, by contrast, IS carried across, resolved once while HOME is
#       still the real home. On a rustup host `cargo` is a symlink to `rustup`,
#       which derives its toolchain store from $RUSTUP_HOME and falls back to
#       $HOME/.rustup — so (3) alone strands the shim and it downloads a whole
#       fresh toolchain on the first cargo invocation (measured: 935 MB into the
#       temp HOME within 12 seconds, `cargo --version` not yet done). This does
#       NOT weaken the simulation: cargo-nextest is a standalone binary in the
#       cargo bin dir, not under ~/.rustup, so preserving the toolchain store
#       cannot un-hide it.
#
# NEST-SAFETY. test_verify_nextest_absent_suites.sh runs
# test_verify_semaphore_wiring.sh INSIDE its own nextest-absent env, so once
# both are migrated this lib runs inside ITSELF. Mirror-source resolution and
# the availability predicate are therefore written to survive a second
# nextest_absent_init from within an already-constructed env — see
# nextest_absent_init's resolution chain and the empty-farm degrade.
#
# Naming: this file does NOT match run_all.sh's `test_*.sh` discovery glob, so
# it is deliberately absent from tests/infra/run-all-classification.manifest —
# matching the established load_tolerance_lib.sh / plan_capture_lib.sh pairs.
# Its unit tests live in tests/infra/test_nextest_absent_lib.sh, which DOES
# match and DOES carry a manifest row.
#
# Usage:
#   [ -f "$SCRIPT_DIR/nextest_absent_lib.sh" ] || { echo "ERROR: nextest_absent_lib.sh not found"; exit 1; }
#   source "$SCRIPT_DIR/nextest_absent_lib.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_NEXTEST_ABSENT_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_NEXTEST_ABSENT_LIB_SH_SOURCED=1

# ---------------------------------------------------------------------------
# Lib state
#
# Deliberately NOT underscore-prefixed: migrated call sites read NX_FARM (to
# overlay an executable), NX_HOME (to assert on the throwaway HOME) and
# NX_WORKDIR (to park a counter file that the shared trap will clean up) by
# name. All are pre-initialised to the empty string so a caller running under
# `set -u` can reference them before nextest_absent_init.
# ---------------------------------------------------------------------------
NX_WORKDIR=""
NX_FARM=""
NX_HOME=""
NX_PATH=""
NX_RUSTUP_HOME=""

# Default path to the script under observation, resolved from THIS file's own
# location so a caller need not thread it through. Overridable per-call (first
# positional arg) and per-environment (NEXTEST_ABSENT_VERIFY).
_NEXTEST_ABSENT_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_NEXTEST_ABSENT_REPO_ROOT="$(cd "$_NEXTEST_ABSENT_LIB_DIR/../.." && pwd)"
NEXTEST_ABSENT_VERIFY="${NEXTEST_ABSENT_VERIFY:-$_NEXTEST_ABSENT_REPO_ROOT/scripts/verify.sh}"

# Why the constructed env is (or is not) a usable simulation — set by
# nextest_absent_init, read out by nextest_absent_reason.
_NEXTEST_ABSENT_REASON=""

# nextest_absent_init — build the nextest-absent environment.
#
# Idempotent-ish: calling it a second time in the SAME shell tears the previous
# workdir down first, so a re-init cannot leak a temp tree past the single
# trap registered below.
nextest_absent_init() {
    local mirror_src

    # Element (1)'s source directory. NOTE: this naive resolution is the one
    # task 5599 used; it is replaced by a nest-safe ordered chain in step-6,
    # because under an already-constructed env $HOME is the temp HOME and
    # $HOME/.cargo/bin does not exist.
    mirror_src="${CARGO_HOME:-$HOME/.cargo}/bin"

    # Element (5). Resolve RUSTUP_HOME ONCE, HERE — while $HOME is still the
    # REAL home. Capturing it into a variable rather than inlining the
    # expansion in nx_run's env line is deliberate: inline, the
    # ${RUSTUP_HOME:-$HOME/.rustup} default would be read against whichever
    # HOME is in scope at expansion time, and the whole point is that it must
    # be the real one, not the redirect.
    #
    # Set only when the resolved store actually exists, so a non-rustup host
    # (distro-packaged cargo, no ~/.rustup) is left completely unperturbed.
    # Either form is safe — a non-rustup cargo ignores the variable — but this
    # keeps the harness's footprint to exactly what the host needs.
    NX_RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
    [ -d "$NX_RUSTUP_HOME" ] || NX_RUSTUP_HOME=""

    # Tear down a previous env before replacing the variable the trap reads.
    [ -n "$NX_WORKDIR" ] && [ -d "$NX_WORKDIR" ] && rm -rf "$NX_WORKDIR"

    NX_WORKDIR="$(mktemp -d)"
    NX_FARM="$NX_WORKDIR/cargo-bin-farm"
    NX_HOME="$NX_WORKDIR/home"
    mkdir -p "$NX_FARM" "$NX_HOME"

    # INT/TERM/HUP as well as EXIT: verify.sh wraps each selected infra test in
    # `timeout --kill-after=60 <n>m` and run_all.sh applies a 30m cap, so an
    # outer timeout kill would otherwise leak the whole temp tree.
    trap 'rm -rf "$NX_WORKDIR"' EXIT INT TERM HUP

    # Element (1). Mirror every mirror_src entry into the farm EXCEPT
    # cargo-nextest — its absence from the farm IS the simulation.
    if [ -d "$mirror_src" ]; then
        local entry base
        for entry in "$mirror_src"/*; do
            [ -e "$entry" ] || continue       # unexpanded glob on an empty dir
            base="$(basename "$entry")"
            [ "$base" = "cargo-nextest" ] && continue
            ln -s "$entry" "$NX_FARM/$base"
        done
    fi

    # Element (2). PATH = farm : (real PATH minus the mirror_src element).
    # The farm goes FIRST so a nextest_absent_farm_put overlay shadows any
    # same-named binary later in PATH.
    local filtered="" p
    while IFS= read -r p; do
        [ -z "$p" ] && continue
        [ "$p" = "$mirror_src" ] && continue
        filtered="${filtered:+$filtered:}$p"
    done < <(printf '%s\n' "$PATH" | tr ':' '\n')
    NX_PATH="$NX_FARM:$filtered"

    _NEXTEST_ABSENT_REASON=""
    return 0
}

# nx_run <cmd...> — run a command under the nextest-absent environment.
#
# HOME is redirected (3), CARGO_HOME is deliberately unset (4), and RUSTUP_HOME
# is carried across (5) so the rustup shim is not stranded.
#
# Leading VAR=val assignments in "$@" are passed straight through to env(1),
# which applies them in order — so a caller can write
# `nx_run REIFY_SHIM_FAIL_COUNT=2 bash "$script"` and, because later
# assignments win, can even override one of the harness's own variables.
nx_run() {
    if [ -n "$NX_RUSTUP_HOME" ]; then
        env -u CARGO_HOME RUSTUP_HOME="$NX_RUSTUP_HOME" \
            HOME="$NX_HOME" PATH="$NX_PATH" "$@"
    else
        env -u CARGO_HOME HOME="$NX_HOME" PATH="$NX_PATH" "$@"
    fi
}

# nx_which <name> — resolve <name> on the harness PATH, printing the resolved
# path.
#
# `command` is a SHELL BUILTIN, so it must be run via `bash -c` under nx_run;
# `nx_run command -v foo` would hand "command" to env(1) as an executable name
# and fail unconditionally — which would make every absence check pass
# VACUOUSLY, exactly the failure mode this harness exists to rule out.
nx_which() { nx_run bash -c 'command -v "$1"' _ "$1"; }

# nextest_absent_available — did a genuine simulation get built?
# nextest_absent_reason   — if not, which conjunct failed?
#
# STUBBED always-available at step-2; step-6 keys them on the OBSERVABLE
# invariant (cargo-nextest unreachable AND cargo executable under the env)
# rather than on any directory's existence.
nextest_absent_available() { return 0; }
nextest_absent_reason() { printf '%s\n' "$_NEXTEST_ABSENT_REASON"; }

# nextest_absent_plan_header [verify-path]         — header UNDER the env
# nextest_absent_plan_header_ambient [verify-path] — header WITHOUT the env
#
# The plan is captured WHOLE and the header extracted afterwards, rather than
# piping verify.sh straight into `head -1`: head exits after the first line and
# the writer takes SIGPIPE, which under `set -o pipefail` surfaces as a
# spurious pipeline failure that has nothing to do with the header's content.
#
# The capture is guarded with `|| true` so a verify.sh hiccup yields an empty
# header — which every caller's `case` rejects — rather than aborting a `set -e`
# caller mid-suite with no Results line at all.
nextest_absent_plan_header() {
    local verify="${1:-$NEXTEST_ABSENT_VERIFY}"
    local full=""
    full="$(nx_run bash "$verify" test --scope all --print-plan 2>/dev/null)" || true
    printf '%s\n' "$full" | sed -n '1p'
}

nextest_absent_plan_header_ambient() {
    local verify="${1:-$NEXTEST_ABSENT_VERIFY}"
    local full=""
    full="$(bash "$verify" test --scope all --print-plan 2>/dev/null)" || true
    printf '%s\n' "$full" | sed -n '1p'
}
