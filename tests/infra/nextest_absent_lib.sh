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
# ADDING A NEW nextest-absent SUITE? Source this file and call
# nextest_absent_init / nx_run — do not hand-roll a fourth temp-HOME + PATH-shim
# harness. (See WHY NOT THE NAIVE RECIPE below for what hand-rolling gets wrong.)
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
# TRAPS. nextest_absent_init COMPOSES with a handler the caller registered
# BEFORE it (both fire, on EXIT/INT/TERM/HUP); a handler registered AFTER it is
# caller-owned and must call nextest_absent_cleanup itself. Full contract at
# "TRAP OWNERSHIP" below.
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

# Handler that was registered for each signal BEFORE nextest_absent_init armed
# its own — replayed by _nextest_absent_trap_dispatch. See "TRAP OWNERSHIP".
_NEXTEST_ABSENT_PREV_EXIT=""
_NEXTEST_ABSENT_PREV_INT=""
_NEXTEST_ABSENT_PREV_TERM=""
_NEXTEST_ABSENT_PREV_HUP=""

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
# _nextest_absent_mirror_source — print the directory the farm mirrors, or
# nothing if there is none. First EXISTING candidate wins:
#
#   (1) $CARGO_HOME/bin, when CARGO_HOME is set
#   (2) $HOME/.cargo/bin
#   (3) the directory containing cargo-nextest itself, when it resolves
#
# (3) is a FALLBACK, not the primary rule, and it keys on cargo-nextest rather
# than on cargo. Measured on this host: ambient `command -v cargo` resolves to
# /home/leo/.local/bin/cargo, NOT ~/.cargo/bin/cargo — so keying the farm's
# source on cargo's location would mirror the wrong directory, leaving the real
# ~/.cargo/bin/cargo-nextest unhidden while also failing to carry tree-sitter
# across. cargo-nextest's own location is BY DEFINITION the directory that must
# be mirrored-minus-itself, which makes it the only sound fallback. It is a
# fallback because on an already-nextest-absent host it resolves to nothing —
# which is exactly when the empty-farm degrade below applies.
_nextest_absent_mirror_source() {
    local c
    if [ -n "${CARGO_HOME:-}" ] && [ -d "$CARGO_HOME/bin" ]; then
        printf '%s\n' "$CARGO_HOME/bin"; return 0
    fi
    if [ -d "$HOME/.cargo/bin" ]; then
        printf '%s\n' "$HOME/.cargo/bin"; return 0
    fi
    if c="$(command -v cargo-nextest 2>/dev/null)" && [ -n "$c" ]; then
        printf '%s\n' "$(dirname "$c")"; return 0
    fi
    return 0
}

# ---------------------------------------------------------------------------
# TRAP OWNERSHIP — the lib composes, it does not clobber.
#
# bash traps REPLACE rather than compose, so a bare
# `trap 'rm -rf "$NX_WORKDIR"' EXIT` inside nextest_absent_init would silently
# disarm whatever the sourcing suite had already registered. That is not
# hypothetical: test_verify_semaphore_wiring.sh registers `trap cleanup EXIT` at
# suite start for its throwaway git repos, and before this composition existed it
# had to hand-patch around the lib (push NX_WORKDIR onto its own list, then
# re-arm its own trap — which in turn disarmed the LIB's, so the arrangement only
# worked because the workaround was written just so).
#
# The contract, in two halves:
#
#   (a) A handler registered BEFORE nextest_absent_init is COMPOSED — init
#       stashes it and arms a dispatcher that runs nextest_absent_cleanup first,
#       then replays the stashed handler. Both fire, on all four signals (the
#       caller's handler is thereby upgraded to EXIT/INT/TERM/HUP even if it only
#       asked for EXIT — see the fallback in _nextest_absent_arm_traps).
#
#       ONE CONSEQUENCE, and it is not new: because the dispatcher does not
#       re-raise, a handler replayed on INT/TERM/HUP may run AGAIN at EXIT if the
#       shell resumes and then exits normally. A COMPOSED HANDLER MUST THEREFORE
#       BE IDEMPOTENT. That is exactly the semantics of the plain
#       `trap cleanup EXIT INT TERM HUP` this composition replaces (and which
#       test_verify_semaphore_wiring.sh itself used before it moved onto the
#       lib); both live consumers' handlers are `rm -rf` loops, which are.
#       MEASURED at task 5602 against semaphore_wiring's exact handler shape
#       (:22-24) driven through this dispatcher — TERM dispatch then normal
#       exit: cleanup ran exactly twice, rc=0, empty stderr, no residue.
#
#   (b) A handler registered AFTER nextest_absent_init is CALLER-OWNED: bash
#       gives the lib no hook to compose retroactively, so that handler replaces
#       the dispatcher and MUST call nextest_absent_cleanup itself (or the temp
#       tree leaks). nextest_absent_cleanup is exported for exactly this, and is
#       idempotent so calling it from several handlers is safe.
# ---------------------------------------------------------------------------

# nextest_absent_cleanup — tear the constructed env down. Idempotent, and safe
# to call when no env was ever built.
nextest_absent_cleanup() {
    if [ -n "$NX_WORKDIR" ] && [ -d "$NX_WORKDIR" ]; then
        rm -rf "$NX_WORKDIR"
    fi
    return 0
}

# _nextest_absent_trap_body <sig> — print the handler currently registered for
# <sig>, unquoted, or nothing.
#
# `eval "parts=($spec)"` rather than string-surgery on `trap -p`'s output:
# `trap -p` emits a shell-QUOTED form (`trap -- 'rm -rf "$d"' EXIT`), so letting
# bash re-parse its own quoting is the only extraction that survives a handler
# containing spaces, quotes or semicolons. Verified that `trap -p` inside a
# command substitution reports the PARENT shell's handlers rather than the
# subshell's reset ones.
_nextest_absent_trap_body() {
    local spec
    spec="$(trap -p "$1")"
    [ -n "$spec" ] || return 0
    local -a parts=()
    eval "parts=($spec)" 2>/dev/null || return 0
    printf '%s' "${parts[2]:-}"
}

# _nextest_absent_trap_dispatch <sig> — the composed handler: lib teardown
# first, then the caller's stashed handler.
#
# Signals are deliberately NOT re-raised after the handler runs, matching the
# behaviour of the plain `trap 'rm -rf ...' EXIT INT TERM HUP` this replaces.
_nextest_absent_trap_dispatch() {
    local sig="$1" prev=""
    nextest_absent_cleanup
    eval "prev=\"\${_NEXTEST_ABSENT_PREV_${sig}:-}\""
    if [ -n "$prev" ]; then
        eval "$prev" || true
    fi
    return 0
}

# _nextest_absent_arm_traps — stash-and-compose on all four signals.
#
# INT/TERM/HUP as well as EXIT: verify.sh wraps each selected infra test in
# `timeout --kill-after=60 <n>m` and run_all.sh applies a 30m cap, so an outer
# timeout kill would otherwise leak the whole temp tree.
_nextest_absent_arm_traps() {
    local sig prev
    for sig in EXIT INT TERM HUP; do
        prev="$(_nextest_absent_trap_body "$sig")"
        case "$prev" in
            # Already ours — a re-init in the same shell. Leave the ORIGINAL
            # caller handler stashed rather than nesting the dispatcher inside
            # itself (which would run the caller's handler once per init).
            _nextest_absent_trap_dispatch*) continue ;;
        esac
        # THE UPGRADE, i.e. contract half (a)'s "even if it only asked for EXIT".
        # A caller that registered `trap cleanup EXIT` alone has NOTHING of its
        # own on INT/TERM/HUP, so without this the dispatcher would run lib
        # teardown and stop — and a `timeout --kill-after` of that suite would
        # strand every temp dir the CALLER made while dutifully removing the
        # lib's own. Strictly AFTER the re-init guard above, which must keep
        # testing the signal's OWN body.
        #
        # Loop order makes it sound: EXIT is resolved and stashed on the first
        # iteration, so INT/TERM/HUP always read a populated stash. On a re-init
        # the EXIT iteration `continue`s without re-stashing, but
        # _NEXTEST_ABSENT_PREV_EXIT still holds the ORIGINAL caller handler from
        # the first init, so the fallback stays correct there too (the nested
        # semaphore_wiring under this suite's S2 is exactly that path). A caller
        # with a genuinely distinct TERM handler keeps it — the fallback fires
        # only where the signal is unhandled. A caller with no handler at all
        # stashes empty, falls back to empty, and the dispatcher just runs lib
        # teardown.
        [ -n "$prev" ] || prev="${_NEXTEST_ABSENT_PREV_EXIT:-}"
        printf -v "_NEXTEST_ABSENT_PREV_${sig}" '%s' "$prev"
        # shellcheck disable=SC2064  # $sig must expand NOW, not at trap time
        trap "_nextest_absent_trap_dispatch $sig" "$sig"
    done
}

nextest_absent_init() {
    local mirror_src

    # NEST-SAFETY. Resolved through the ordered chain above, and when NOTHING
    # resolves the farm DEGRADES TO EMPTY rather than the caller skipping.
    #
    # This is the difference between the lib and task 5599's inline harness.
    # test_verify_nextest_absent_suites.sh runs test_verify_semaphore_wiring.sh
    # inside its own nextest-absent env, so after migration this lib runs
    # inside ITSELF — and measured under that env, 5599's
    # ${CARGO_HOME:-$HOME/.cargo}/bin resolves to $NX_HOME/.cargo/bin, which
    # does not exist. Copying 5599's host-precondition SKIP verbatim would make
    # the nested semaphore_wiring emit "Results: 0 passed, 0 failed", blow its
    # S2 floor of 22, and turn the outer suite RED.
    #
    # "Nothing to mirror" does NOT mean the simulation is impossible — in a
    # nested context it means the environment is ALREADY nextest-absent, and an
    # empty farm layered over a cargo-nextest-free PATH is still a correct
    # simulation (the outer farm keeps supplying a real cargo). Availability is
    # therefore keyed on the OBSERVABLE invariant below, never on any
    # directory's existence.
    mirror_src="$(_nextest_absent_mirror_source)"

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
    nextest_absent_cleanup

    NX_WORKDIR="$(mktemp -d)"
    NX_FARM="$NX_WORKDIR/cargo-bin-farm"
    NX_HOME="$NX_WORKDIR/home"
    mkdir -p "$NX_FARM" "$NX_HOME"

    # Compose rather than clobber — see "TRAP OWNERSHIP" above.
    _nextest_absent_arm_traps

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

# nextest_absent_available — is the constructed env a genuine simulation?
# nextest_absent_reason   — if not, which conjunct failed?
#
# Keyed on the OBSERVABLE INVARIANT, deliberately not on any directory's
# existence: cargo-nextest must be UNREACHABLE under the env, and cargo must
# still EXECUTE under it. That pair is what "a nextest-absent host" means
# operationally, and it is the only formulation that survives nesting — where
# there is no directory to mirror yet the env is perfectly correct.
#
# Both conjuncts are necessary and neither is sufficient:
#   - cargo-nextest reachable  -> the simulation is vacuous; verify.sh would
#     emit nextest=1 and every assert downstream would be testing the ambient
#     host, not the intended variable.
#   - cargo not executable     -> we are simulating "the toolchain is broken"
#     rather than "cargo-nextest is not installed", which is a different (and
#     much noisier) thing to assert against.
#
# `cargo --version` is executed, not merely resolved, for the reason in the
# header: a resolvable-but-unrunnable shim is precisely the broken-toolchain
# case this rules out.
nextest_absent_available() {
    _NEXTEST_ABSENT_REASON=""

    if [ -z "$NX_WORKDIR" ] || [ ! -d "$NX_WORKDIR" ]; then
        _NEXTEST_ABSENT_REASON="nextest_absent_init has not been called (no env has been constructed)"
        return 1
    fi

    if nx_which cargo-nextest >/dev/null 2>&1; then
        _NEXTEST_ABSENT_REASON="cargo-nextest is still REACHABLE under the constructed env ($(nx_which cargo-nextest 2>/dev/null)) — the simulation would be vacuous"
        return 1
    fi

    if ! nx_run cargo --version >/dev/null 2>&1; then
        _NEXTEST_ABSENT_REASON="cargo does not EXECUTE under the constructed env — this would simulate a broken toolchain rather than an absent cargo-nextest"
        return 1
    fi

    return 0
}

nextest_absent_reason() { printf '%s\n' "$_NEXTEST_ABSENT_REASON"; }

# ---------------------------------------------------------------------------
# Caller-facing convenience helpers
# ---------------------------------------------------------------------------

# nextest_absent_assert_real — emit the realness checks as assert() calls
# against the SOURCING suite's PASS/FAIL counters.
#
# Deliberately NOT a boolean-returning predicate: a migrated suite's pass count
# must rise exactly as its hand-rolled H-section did, because
# test_verify_nextest_absent_suites.sh's S1-S4 pass floors are calibrated
# against those counts. Requires test_helpers.sh to have been sourced first.
#
# ORDER IS PART OF THE CONTRACT — it reproduces that suite's H1-H7 one-for-one:
#   H1  cargo-nextest unreachable       (absence, as non-resolvability)
#   H2  cargo EXECUTES                  (presence, as executability)
#   H3  tree-sitter EXECUTES
#   H4  plan header nextest=0 under the env
#   H5  plan header nextest=1 ambiently — ONLY where cargo-nextest is installed
#       ambiently; on a genuinely nextest-less host there is nothing for the
#       farm to hide and asserting it would go red for an irrelevant reason
#   H6  no .rustup in the throwaway HOME
#   H7  throwaway HOME under the ceiling
#
# H6/H7 come LAST so that every check which actually exercises the env (H2, H3,
# H4 — H5 is deliberately ambient) has already had its chance to provoke a
# toolchain sync before hygiene is measured.

# H1-H5 as file-scope, SELF-CONTAINED predicates.
#
# Defined here rather than nested inside nextest_absent_assert_real for two
# reasons. (a) bash installs a nested function GLOBALLY once its enclosing
# function runs, so nesting bought no privacy — it only delayed the definition
# and invited a name collision under a generic prefix. (b) H4/H5 need the
# verify.sh path: taking it as an explicit "$1" keeps them callable from
# anywhere, whereas reading a `local` of the enclosing function relies on
# dynamic scoping and would abort a `set -u` caller with an unbound-variable
# error — a crash rather than a test failure — the moment either is invoked on
# its own.
_nextest_absent_h1() { ! nx_which cargo-nextest; }
_nextest_absent_h2() { nx_run cargo --version; }
_nextest_absent_h3() { nx_run tree-sitter --version; }

_nextest_absent_h4() {
    local hdr; hdr="$(nextest_absent_plan_header "${1:-$NEXTEST_ABSENT_VERIFY}")"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=0"*) return 0 ;; *) return 1 ;; esac
}

_nextest_absent_h5() {
    local hdr; hdr="$(nextest_absent_plan_header_ambient "${1:-$NEXTEST_ABSENT_VERIFY}")"
    printf '%s\n' "$hdr"
    case "$hdr" in *"nextest=1"*) return 0 ;; *) return 1 ;; esac
}

nextest_absent_assert_real() {
    local verify="${1:-$NEXTEST_ABSENT_VERIFY}"

    assert "H1: cargo-nextest is NOT resolvable under the nextest-absent harness env" _nextest_absent_h1
    assert "H2: cargo still RUNS under the harness env (farm keeps the toolchain intact, not merely on PATH)" _nextest_absent_h2
    assert "H3: tree-sitter still RUNS under the harness env (not stripped with ~/.cargo/bin)" _nextest_absent_h3
    assert "H4: verify.sh plan header reads nextest=0 UNDER the harness" _nextest_absent_h4 "$verify"

    if command -v cargo-nextest >/dev/null 2>&1; then
        assert "H5: verify.sh plan header reads nextest=1 WITHOUT the harness (this host has cargo-nextest, so the simulation is meaningful)" _nextest_absent_h5 "$verify"
    else
        echo "  SKIP: H5 (harness unavailable on this host: cargo-nextest is not installed"
        echo "        ambiently, so there is nothing for the farm to hide and 'nextest=1"
        echo "        without the harness' cannot hold.)"
    fi

    assert "H6: the harness did NOT provoke a rustup toolchain sync (no .rustup in its temp HOME)" \
        nextest_absent_no_rustup_sync
    assert "H7: the harness's temp HOME is still small (it perturbs only cargo-nextest's visibility)" \
        nextest_absent_home_is_small
}

# nextest_available_ambient — does the AMBIENT host have cargo-nextest, as
# verify.sh sees it? Returns 0 on a `nextest=1` plan header, non-zero otherwise.
#
# This DETECTS host state; it does not simulate absence. It is the idiom
# test_verify_retry_subset.sh open-coded — that suite deliberately tests against
# the host's real nextest state, and forcing absence on it would silently drop
# every nextest-shaped assert it guards.
#
# The capture is guarded inside nextest_absent_plan_header_ambient, so a
# verify.sh hiccup yields an empty header (reported as "not available") rather
# than aborting a `set -e` caller.
nextest_available_ambient() {
    local hdr
    hdr="$(nextest_absent_plan_header_ambient "${1:-$NEXTEST_ABSENT_VERIFY}")"
    case "$hdr" in *"nextest=1"*) return 0 ;; *) return 1 ;; esac
}

# ---------------------------------------------------------------------------
# Farm overlay — the presence marker and the fake toolchain as PARAMETERS of
# the shared harness rather than a reason to fork it.
#
# test_verify_nextest_probe.sh needs a counter-driven fake `cargo` AND needs to
# toggle cargo-nextest's presence between cycles; test_verify_semaphore_wiring.sh
# needs neither. Exposing the farm as an overlayable directory lets both share
# one harness.
# ---------------------------------------------------------------------------

# nextest_absent_farm_put <name> <file> — install <file> as the farm's <name>,
# shadowing any same-named binary later in PATH (the farm is first in NX_PATH).
#
# rm -f FIRST: the farm entry for a mirrored tool is a SYMLINK, and copying
# onto an existing symlink follows it and would clobber the real binary in the
# mirror source. Removing first makes overlaying a mirrored entry safe and
# repeatable.
nextest_absent_farm_put() {
    local name="$1" src="$2"
    [ -n "$NX_FARM" ] || { echo "nextest_absent_init has not been called"; return 1; }
    [ -f "$src" ] || { echo "nextest_absent_farm_put: no such file: $src"; return 1; }
    rm -f "$NX_FARM/$name"
    cp "$src" "$NX_FARM/$name" || return 1
    chmod +x "$NX_FARM/$name"
}

# nextest_absent_farm_add_nextest_stub / _rm_nextest_stub — flip the simulated
# host between "cargo-nextest installed" and "cargo-nextest absent".
#
# The stub is a PRESENCE MARKER, not a working cargo-nextest: verify.sh's probe
# never execs it directly — the genuine-absence disambiguation at
# scripts/verify.sh:1412 checks only that `command -v cargo-nextest` resolves.
# It is deliberately kept OUT of the farm by default, so the plain env is
# nextest-absent with no caller action.
nextest_absent_farm_add_nextest_stub() {
    [ -n "$NX_FARM" ] || { echo "nextest_absent_init has not been called"; return 1; }
    printf '#!/usr/bin/env bash\n# presence marker only — see nextest_absent_lib.sh\nexit 0\n' \
        > "$NX_FARM/cargo-nextest" || return 1
    chmod +x "$NX_FARM/cargo-nextest"
}

nextest_absent_farm_rm_nextest_stub() {
    [ -n "$NX_FARM" ] || { echo "nextest_absent_init has not been called"; return 1; }
    rm -f "$NX_FARM/cargo-nextest"
}

# ---------------------------------------------------------------------------
# Toolchain hygiene — the harness must not provoke a rustup TOOLCHAIN SYNC.
#
# These are not paranoia. Task 5599 measured it: a bounded 12-second probe of
# `cargo --version` under a RUSTUP_HOME-less harness wrote 935 MB into the temp
# HOME and had still not printed a version when it was killed. Element (5) of
# the harness (RUSTUP_HOME carry-across, in nextest_absent_init) is what
# prevents it; these two predicates are what NOTICE if it is ever dropped again.
#
# Two predicates because they fail differently — nextest_absent_no_rustup_sync
# names the mechanism exactly, nextest_absent_home_is_small is the blunt
# backstop for any other way the harness starts writing into a directory it
# advertises as a throwaway.
#
# Both print the offending entries on failure, so assert()'s tail-50 dump names
# the cause rather than reporting a bare non-zero rc.
# ---------------------------------------------------------------------------

# Ceiling for the throwaway HOME. On a correctly-isolated env it holds 4 KB
# after the env has been exercised (measured, both by task 5599 and again at
# task 5602 HEAD=27f8b62eaa) — this is ~12000x that, high enough never to flap
# on incidental dotfile writes, low enough to trip within the first second of a
# toolchain sync.
NEXTEST_ABSENT_HOME_MAX_KB="${NEXTEST_ABSENT_HOME_MAX_KB:-51200}"

nextest_absent_no_rustup_sync() {
    if [ -z "$NX_HOME" ]; then
        echo "nextest_absent_init has not been called — no throwaway HOME to check"
        return 1
    fi
    if [ -e "$NX_HOME/.rustup" ]; then
        echo "$NX_HOME/.rustup EXISTS — the harness stranded rustup and provoked"
        echo "a toolchain sync. cargo is a rustup shim and rustup resolves its"
        echo "store from \$RUSTUP_HOME, defaulting to \$HOME/.rustup; the harness"
        echo "redirects HOME, so RUSTUP_HOME must be carried across explicitly"
        echo "(element 5 in nextest_absent_init)."
        echo "Contents:"
        ls -la "$NX_HOME/.rustup" 2>&1 | head -20
        return 1
    fi
    echo "no $NX_HOME/.rustup — the harness did not provoke a toolchain sync"
    return 0
}

nextest_absent_home_is_small() {
    local kb ceiling
    ceiling="${NEXTEST_ABSENT_HOME_MAX_KB:-51200}"

    if [ -z "$NX_HOME" ]; then
        echo "nextest_absent_init has not been called — no throwaway HOME to measure"
        return 1
    fi

    kb="$(du -sk "$NX_HOME" 2>/dev/null | awk 'NR==1 {print $1}')"
    if [ -z "$kb" ]; then
        echo "could not measure the throwaway HOME size at $NX_HOME"
        return 1
    fi

    echo "throwaway HOME $NX_HOME holds ${kb} KB (ceiling ${ceiling} KB)"
    if [ "$kb" -gt "$ceiling" ]; then
        echo "-> the harness is writing a large amount into the throwaway HOME it"
        echo "   redirects to. It is perturbing more than the single intended"
        echo "   variable (cargo-nextest absent). Largest entries:"
        du -sk "$NX_HOME"/* "$NX_HOME"/.[!.]* 2>/dev/null | sort -rn | head -10
        return 1
    fi
    return 0
}

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

# _nextest_absent_header_of <plan> — pick the plan header out of a captured plan.
#
# CONTENT-KEYED, NOT POSITIONAL. `sed -n '1p'` would be silently wrong the moment
# verify.sh emits anything at all before the header on stdout — a warning, a
# deprecation notice, an apply_env diagnostic. The failure would not be loud:
# nextest_available_ambient would start returning non-zero, test_verify_retry_
# subset.sh would set NEXTEST_AVAILABLE=0, and every assert behind its
# `[ "$NEXTEST_AVAILABLE" -eq 1 ]` guards would be silently DROPPED — a vacuous
# green, which is the exact failure class this task family exists to prevent.
# Measured at task 5602: the header IS line 1 today, so this is latent rather
# than live — which is precisely when it is cheap to close.
_NEXTEST_ABSENT_HEADER_RE='^# verify\.sh plan'

_nextest_absent_header_of() {
    printf '%s\n' "$1" | grep -m1 -E "$_NEXTEST_ABSENT_HEADER_RE" || true
}

nextest_absent_plan_header() {
    local verify="${1:-$NEXTEST_ABSENT_VERIFY}"
    local full=""
    full="$(nx_run bash "$verify" test --scope all --print-plan 2>/dev/null)" || true
    _nextest_absent_header_of "$full"
}

nextest_absent_plan_header_ambient() {
    local verify="${1:-$NEXTEST_ABSENT_VERIFY}"
    local full=""
    full="$(bash "$verify" test --scope all --print-plan 2>/dev/null)" || true
    _nextest_absent_header_of "$full"
}
