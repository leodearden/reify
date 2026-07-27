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
