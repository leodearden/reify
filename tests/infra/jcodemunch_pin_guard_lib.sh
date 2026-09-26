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
#   jc_argv_python <argv>             -> the token FOLLOWING --python in <argv>
#   jc_guard_value_agrees <kind> <want-label> <want> <got-label> <got> [hint...]
#                                     -> 0 iff both sides are non-empty and equal
#   jc_guard_defines_no_triple <sut>  -> 0 iff <sut> declares none of the three itself
#   jc_guard_sources_the_lib   <sut>  -> 0 iff <sut> sources the lib
#   jc_guard_argv_uses_the_lib <sut>  -> 0 iff <sut>'s argv splices all three and
#                                        re-inlines no pin/interpreter literal
#   jc_guard_refuses_without_lib <sut> <scratch-dir>
#                                     -> 0 iff a lib-less copy of <sut> refuses correctly
#   jc_guard_is_registered <path> [hint...]
#                                     -> 0 iff <path> is a registered verify-pipeline artifact
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
_JC_REPO_ROOT="$(cd "$_JC_GUARD_LIB_DIR/../.." && pwd)"
JC_PIN_LIB_FILE="$_JC_REPO_ROOT/scripts/lib_jcodemunch_pin.sh"

# Each extractor is ONE awk with an `exit` and no pipeline — a `… | head -n1`
# would be an early-closing consumer under `set -euo pipefail`. Each emits
# NOTHING when it does not match, so a renamed file or a reshaped literal fails
# loudly through jc_guard_value_agrees below (or the caller's own
# require_nonempty) instead of comparing "" with "" and reporting that every
# site agrees.
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

# jc_argv_python <argv-string> — the token FOLLOWING `--python`, read
# POSITIONALLY out of an already-constructed argv. The argv PRODUCTION stays
# per-suite (it means running that suite's own SUT); the PARSING takes a plain
# string and has no such tie, so it lives here instead of as the byte-identical
# awk both suites used to carry.
#
# Read through a HERESTRING, never a pipeline: an awk with an `exit` on the read
# end of a pipe is an early-closing consumer, which is what SIGPIPEs the producer
# under `set -euo pipefail`. Emits NOTHING when `--python` is absent, so the
# comparator refuses the emptiness rather than reporting agreement.
jc_argv_python() {
    awk '{ for (i = 1; i <= NF; i++) if ($i == "--python") { print $(i + 1); exit } }' <<< "$1"
}

# jc_guard_value_agrees <kind> <want-label> <want> <got-label> <got> [hint...]
#
# The comparator behind every agreement assertion in both suites. It takes two
# already-extracted STRINGS, which is what makes it SUT-agnostic: δ's side comes
# out of δ's constructed argv, β's out of β's, α's out of Rust source, and the
# comparison is the same either way.
#
# EMPTY IS A FAILURE, NEVER AGREEMENT — refused HERE rather than at each call
# site so that no caller can forget. A renamed const or a reshaped literal must
# fail loudly instead of comparing "" against "" and reporting that every site
# agrees.
#
# Compared as WHOLE STRINGS, never as a substring: a substring test would report
# agreement between a lib value of "3.1" and an argv carrying "3.13".
jc_guard_value_agrees() {
    local kind="$1" want_label="$2" want="$3" got_label="$4" got="$5"
    shift 5
    if [ -z "$want" ]; then
        printf '%s is EMPTY — the %s comparison would be vacuous.\n' "$want_label" "$kind" >&2
        return 1
    fi
    if [ -z "$got" ]; then
        printf '%s is EMPTY — the %s comparison would be vacuous.\n' "$got_label" "$kind" >&2
        return 1
    fi
    if [ "$want" = "$got" ]; then
        return 0
    fi
    printf '%s DRIFT: %s is [%s] but %s is [%s].\n' "$kind" "$want_label" "$want" "$got_label" "$got" >&2
    if [ "$#" -gt 0 ]; then
        printf '  %s\n' "$@" >&2
    fi
    return 1
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

# -- THE SOURCED VALUES MUST REACH THE ARGV ---------------------------------
#
# jc_guard_argv_uses_the_lib <sut> — the SYMMETRIC COUNTERPART, for the two
# consumers that CAN source the lib, of the b2_alpha_consts_reach_the_argv leg
# δ's suite runs against α.
#
# WHY NOTHING ELSE COVERS IT. jc_guard_defines_no_triple proves the consumer
# does not DECLARE the three; jc_guard_sources_the_lib proves a `source` line
# exists. Neither can tell a script that sources the lib and then IGNORES it
# from one that uses it — and the argv-agreement assertions only fire when a
# re-inlined literal DIFFERS from today's lib value. MEASURED: rewriting β's
# INDEXER_CMD to `uvx --python 3.13 --from jcodemunch-mcp==1.108.54
# jcodemunch-mcp`, so β reads the lib nowhere at all, left
# tests/infra/test_jcodemunch_index_reify.sh at 54 passed, 0 failed; δ had the
# identical hole. The re-inlining was then caught only at the NEXT bump, and it
# surfaced there as a misleading "INTERPRETER DRIFT" rather than as "you
# re-inlined the literal".
#
# TWO LEGS, because a use site and a stray literal can coexist: each of the
# three must be SPLICED on a non-comment line, and no pin or interpreter LITERAL
# may appear on one. Whole-line comments only — the same filter α's check uses,
# and the same limitation: a literal in a trailing comment on a code line reads
# here as code.

# (the spelling that must appear in the consumer's argv, what it is).
JC_ARGV_USES=(
    '"$JC_PYTHON"'            'the --python argument'
    '"$JC_PIN"'               'the --from requirement string'
    '"${JC_IDENTITY_ENV[@]}"' 'the env identity prefix'
)

# (banned ERE, what a match is). Deliberately NOT a ban on a literal
# `JCODEMUNCH_GIT_ROOT_IDENTITY=0`: β names that var inside a runtime diagnostic
# string — a legitimate non-comment mention of a value it does not spawn — so
# the identity lever is covered by its use-site needle above and nothing more.
JC_ARGV_LITERAL_BANS=(
    'jcodemunch-mcp==[0-9]'         'a re-inlined wheel pin'
    '--python[[:space:]=]+"?[0-9]'  'a re-inlined interpreter'
)

jc_guard_argv_uses_the_lib() {
    local sut="$1" i needle what hit rc=0
    if [ ! -f "$sut" ]; then
        printf 'the SUT does not exist: %s\n' "$sut" >&2
        return 1
    fi
    for ((i = 0; i < ${#JC_ARGV_USES[@]}; i += 2)); do
        needle="${JC_ARGV_USES[i]}"
        what="${JC_ARGV_USES[i + 1]}"
        if ! awk -v needle="$needle" \
            '!/^[[:space:]]*#/ && index($0, needle) { found = 1 } END { exit !found }' "$sut"; then
            printf '%s never splices %s for %s.\n' "$sut" "$needle" "$what" >&2
            rc=1
        fi
    done
    for ((i = 0; i < ${#JC_ARGV_LITERAL_BANS[@]}; i += 2)); do
        needle="${JC_ARGV_LITERAL_BANS[i]}"
        what="${JC_ARGV_LITERAL_BANS[i + 1]}"
        hit="$(awk -v pat="$needle" '!/^[[:space:]]*#/ && $0 ~ pat { printf "  %d: %s\n", NR, $0 }' "$sut")"
        if [ -n "$hit" ]; then
            printf '%s carries %s on a non-comment line:\n%s\n' "$sut" "$what" "$hit" >&2
            rc=1
        fi
    done
    if [ "$rc" -ne 0 ]; then
        printf '  The triple has ONE definition site — scripts/lib_jcodemunch_pin.sh (#6454).\n' >&2
        printf '  Sourcing it is not enough: its values must REACH the constructed argv, and a\n' >&2
        printf '  re-inlined literal stays green in every value assertion until the next bump.\n' >&2
    fi
    return "$rc"
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

# -- MAP WIRING --------------------------------------------------------------
#
# jc_guard_is_registered <path> [hint...] — <path> is a registered
# verify-pipeline artifact, so an edit to IT selects the guard suite that checks
# it. Asked through scripts/verify-pipeline-guard.sh's own `is-registered`
# oracle rather than by re-implementing the map parse here: a second parser is a
# second thing to drift.
#
# WHY A REGISTRATION IS LOAD-BEARING: column 1 of
# scripts/verify-pipeline-infra-tests.txt is matched by EXACT STRING EQUALITY,
# never by glob or directory prefix, so a file with no row of its own selects
# NOTHING at task scope and is checked only once a diff reaches merge scope.
#
# NOT EVERY PATH IS WORTH ASSERTING THIS WAY, and the discriminator is which
# clause answers. A `tests/infra/*.{sh,py}` path — this guard lib included — is
# answered 0 by the guard's open-ended infra glob clause, which routes it to the
# FULL gate and owes nothing to its rows. MEASURED: tests/infra/test_helpers.sh
# has no row anywhere in the map and `is-registered` still exits 0 for it. So an
# assertion here over a tests/infra path is satisfied whether or not its rows
# exist — vacuous, and exactly the PASS-shaped evidence PRD §2.4 names as the
# disease. Call this only for paths whose sole registration IS a row.
#
# Exit 2 is distinguished from exit 1 deliberately: the former is the guard's
# arity refusal, so a CLI change would otherwise read here as "not registered".
jc_guard_is_registered() {
    local path="$1"; shift
    local guard="$_JC_REPO_ROOT/scripts/verify-pipeline-guard.sh" out rc=0
    if [ ! -f "$guard" ]; then
        printf 'scripts/verify-pipeline-guard.sh not found at %s\n' "$guard" >&2
        return 1
    fi
    out="$(bash "$guard" is-registered "$path" 2>&1)" || rc=$?
    case "$rc" in
        0) return 0 ;;
        1) printf '%s is NOT a registered verify-pipeline artifact:\n  %s\n' "$path" "$out" >&2
           if [ "$#" -gt 0 ]; then
               printf '  %s\n' "$@" >&2
           fi
           return 1 ;;
        *) printf 'is-registered exited %d for %s (expected 0 or 1) — its CLI contract may have changed:\n  %s\n' \
               "$rc" "$path" "$out" >&2
           return 1 ;;
    esac
}
