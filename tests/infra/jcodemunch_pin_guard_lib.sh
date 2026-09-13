#!/usr/bin/env bash
# tests/infra/jcodemunch_pin_guard_lib.sh — the SUT-AGNOSTIC half of the
# jcodemunch invocation-triple guard (task 6929).
#
# scripts/lib_jcodemunch_pin.sh is the ONE definition site for the triple —
# wheel pin, interpreter, identity lever (#6454/#6548). Two suites guard it:
# tests/infra/test_with_jcodemunch_serve.sh (δ, plus α's two mirrored consts)
# and tests/infra/test_jcodemunch_index_reify.sh (β). Each keeps its OWN argv
# harness, because reading a CONSTRUCTED --dry-run argv means running that
# suite's own SUT and centralising it would make either `pool` suite execute a
# second one. Everything BELOW reads only the lib and one caller-supplied path,
# so it has no such tie — and a change whose whole subject is single-definition
# site has no business shipping its guard as a copy-paste pair.
#
# FUNCTIONS (defined when sourced):
#   jc_lib_pin_requirement            -> the FULL requirement string, jcodemunch-mcp==<n>.<n>.<n>
#   jc_lib_pin_version                -> the BARE version, <n>.<n>.<n>
#   jc_lib_python                     -> JC_PYTHON, <major>.<minor>
#   jc_guard_defines_no_triple <sut>  -> 0 iff <sut> declares none of the three itself
#   jc_guard_sources_the_lib   <sut>  -> 0 iff <sut> sources the lib
#   jc_guard_refuses_without_lib <sut> <scratch-dir>
#                                     -> 0 iff a lib-less copy of <sut> refuses correctly
#
# THE TWO PIN ACCESSORS ARE NAMED FOR WHAT THEY RETURN, and that is not
# cosmetic: this lib replaces a `jc_pin_lib` that returned the BARE version in
# one suite and the FULL requirement string in the other — one name, two
# meanings across sibling files, so a call copied between them silently
# compared the wrong halves.
#
# Sourced, never executed, and deliberately carries no `set -euo pipefail`:
# setting shell options here would impose them on the sourcing suite. Nothing is
# printed at load.

_JC_GUARD_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JC_PIN_LIB_FILE="$(cd "$_JC_GUARD_LIB_DIR/../.." && pwd)/scripts/lib_jcodemunch_pin.sh"

# Each extractor is ONE awk with an `exit` and no pipeline — a `… | head -n1`
# would be an early-closing consumer under `set -euo pipefail`. Each emits
# NOTHING when it does not match, so a renamed file or a reshaped literal fails
# loudly through the caller's own require_nonempty instead of comparing "" with
# "" and reporting that every site agrees.
jc_lib_pin_requirement() {
    [ -f "$JC_PIN_LIB_FILE" ] || return 0
    awk '/^JC_PIN=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }' "$JC_PIN_LIB_FILE"
}

# Derived from the requirement string rather than extracted a second time, so
# the two can never disagree about which line of the lib they read.
jc_lib_pin_version() {
    local req
    req="$(jc_lib_pin_requirement)"
    [ -n "$req" ] || return 0
    printf '%s\n' "${req##*==}"
}

jc_lib_python() {
    [ -f "$JC_PIN_LIB_FILE" ] || return 0
    awk '/^JC_PYTHON=/ { sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit }' "$JC_PIN_LIB_FILE"
}

# THE LOAD-BEARING HALF OF THE HOIST, and the reason an argv-agreement assertion
# is not sufficient on its own. Every copy of the triple AGREES today, so "the
# constructed argv matches the lib" was green the moment the lib was created and
# before either consumer was touched. What needs pinning is that the lib is the
# SOLE definition site AND that its value reaches the argv; each suite owns the
# second half against its own SUT.
jc_guard_defines_no_triple() {
    local sut="$1" v hit rc=0
    if [ ! -f "$sut" ]; then
        printf 'the SUT does not exist: %s\n' "$sut" >&2
        return 1
    fi
    for v in JC_PIN JC_PYTHON JC_IDENTITY_ENV; do
        hit="$(grep -n "^$v=" "$sut" || true)"
        if [ -n "$hit" ]; then
            printf '%s still defines %s itself:\n  %s\n' "$sut" "$v" "$hit" >&2
            rc=1
        fi
    done
    if [ "$rc" -ne 0 ]; then
        printf '  The triple has ONE definition site — scripts/lib_jcodemunch_pin.sh (#6454).\n  A consumer must SOURCE it, never re-declare it.\n' >&2
    fi
    return "$rc"
}

# The single-definition-site MARKER: the consumer reaches the lib by sourcing
# it. Deliberately the ONLY grep of a consumer's source left in this guard — the
# REFUSAL that source has to deliver when the lib is absent is asserted
# BEHAVIOURALLY by jc_guard_refuses_without_lib below, so every equally-correct
# spelling of an existence check (`[ ! -f … ]`, `[[ ! -r … ]]`, a `command -v`
# resolution) stays free to change without redding a working script.
jc_guard_sources_the_lib() {
    local sut="$1" src_line
    if [ ! -f "$sut" ]; then
        printf 'the SUT does not exist: %s\n' "$sut" >&2
        return 1
    fi
    src_line="$(grep -n 'source .*lib_jcodemunch_pin\.sh' "$sut" || true)"
    if [ -z "$src_line" ]; then
        printf '%s never sources scripts/lib_jcodemunch_pin.sh\n  The triple has ONE definition site (#6454); reach it by sourcing.\n' "$sut" >&2
        return 1
    fi
    return 0
}

# jc_guard_refuses_without_lib <sut> <scratch-dir> — the missing-lib REFUSAL,
# executed rather than grepped. Both consumers self-locate via BASH_SOURCE, so a
# copy of the script into a directory with no lib beside it reproduces the exact
# condition, hermetically and in milliseconds.
#
# All three legs together, because each alone is satisfiable by a broken script:
# a NON-ZERO exit (a consumer that carried on with an unset JC_PYTHON would spawn
# an unpinned interpreter), NOTHING on stdout (both consumers' --dry-run stdout
# IS their contract, and every argv assertion downstream parses it), and stderr
# NAMING the lib (without the name a missing lib reads as an unbound-variable bug
# in the consumer — which is the entire reason the existence check is there).
#
# IT PINS THE CONTRACT, NOT AN IMPLEMENTATION OF IT. MEASURED: delete δ's
# explicit `[ ! -f … ]` block and bash's own failed `source` under `set -e`
# still satisfies all three legs (exit 1, empty stdout, stderr naming the
# missing path), so this probe does NOT detect that block's removal — it is not
# a replacement for it, only for the grep that used to assert its spelling. What
# it does catch, and no grep can, is a consumer that stops refusing at all or
# that refuses while writing to stdout.
jc_guard_refuses_without_lib() {
    local sut="$1" scratch="$2" copy out err rc=0
    if [ ! -f "$sut" ]; then
        printf 'the SUT does not exist: %s\n' "$sut" >&2
        return 1
    fi
    if [ -z "$scratch" ] || [ ! -d "$scratch" ]; then
        printf 'jc_guard_refuses_without_lib needs an existing scratch dir, got [%s]\n' "$scratch" >&2
        return 1
    fi
    copy="$scratch/$(basename "$sut")"
    cp "$sut" "$copy" || return 1
    chmod +x "$copy" || return 1
    if [ -e "$scratch/lib_jcodemunch_pin.sh" ]; then
        printf 'the scratch dir already holds a lib_jcodemunch_pin.sh — the probe would be vacuous:\n  %s\n' "$scratch" >&2
        return 1
    fi
    out="$scratch/refuse.out"; err="$scratch/refuse.err"
    "$copy" --dry-run >"$out" 2>"$err" || rc=$?
    if [ "$rc" -eq 0 ]; then
        printf '%s --dry-run EXITED 0 with no lib_jcodemunch_pin.sh beside it.\n' "$(basename "$sut")" >&2
        printf '  It must refuse: an unset JC_PIN/JC_PYTHON spawns an unpinned wheel under an unpinned interpreter.\n' >&2
        return 1
    fi
    if [ -s "$out" ]; then
        printf '%s wrote to STDOUT while refusing a missing lib:\n' "$(basename "$sut")" >&2
        sed 's/^/  | /' "$out" >&2
        printf '  --dry-run stdout is the argv contract; a refusal must leave it empty.\n' >&2
        return 1
    fi
    if ! grep -q 'lib_jcodemunch_pin\.sh' "$err"; then
        printf '%s refused (exit %d) without NAMING lib_jcodemunch_pin.sh on stderr:\n' "$(basename "$sut")" "$rc" >&2
        sed 's/^/  | /' "$err" >&2
        printf '  Unnamed, a missing lib reads as a bug in the consumer rather than as a missing file.\n' >&2
        return 1
    fi
    return 0
}
