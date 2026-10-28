#!/usr/bin/env bash
# scripts/lib_event_log.sh — encode verify events as JSON lines and append them
# to the event sinks (sourced-only).
#
# SINKS — every record goes to each one that is set, channel first:
#   REIFY_VERIFY_EVENT_LOG  the exported verify-tree channel.
#   EVENT_LOG_COPY_TO       a caller's own copy path. A shell variable the
#                           caller never exports, so only its shell and its
#                           subshells write it, never a child process.
#
# CONTRACT:
#   Inert when both sinks are unset or empty: every emitter's FIRST statement
#   returns, so a call costs no byte, no fork and no open.
#   One record per physical line, appended to each sink by ONE O_APPEND
#   printf, at most EVENT_LOG_LINE_MAX bytes including its newline. List
#   fields are fitted to that cap and what they drop is counted in the record.
#   Never fatal and silent: every emitter returns 0 and an unwritable sink
#   prints nothing, so a caller's control flow under set -e never changes.
#   ts_ns and every *_NS argument a caller passes read event_log_clock_ns.
#
# EMITTERS (positional, PRD D3):
#   event_log_emit_run_start     RUN_ID SCOPE BRANCH DISCOVERED
#   event_log_emit_member_start  MEMBER ATTEMPT MODE
#   event_log_emit_member_finish MEMBER ATTEMPT RC VERDICT DURATION_MS QUEUED_NS STARTED_NS ENDED_NS PRE_OMITTED [DETAIL_LINE...]
#   event_log_emit_phase_finish  PHASE STARTED_NS ENDED_NS
#   event_log_emit_slot_timeout  REASON SLOTS WAITED DISPOSITION LOCK EMITTER
#   event_log_emit_run_finish    DISCOVERED FAILURES INTERRUPTED N_FAILED [name...] N_FLAKY [name...] N_SKIPPED [name...]
# An int argument that is not an integer, and a bool that is not true/false,
# encodes JSON null.
#
# Readers: scripts/verify_events_lib.py (the parser) and
# tests/infra/verify_events.py (the CLI over it). Schema and writers:
# tests/infra/README.md "Verify event channel". Contract and consumer rules:
# PRD docs/prds/v0_6/run-all-runner-infra-harness.md §4. This lib lives in
# scripts/ because scripts/lib_slot_acquire.sh sources it and must not depend
# on tests/infra/.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_EVENT_LOG_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_EVENT_LOG_SH_SOURCED=1

# Bytes in one appended line, newline included. Below PIPE_BUF (4096), so one
# printf is one write(2) and concurrent appenders never interleave.
EVENT_LOG_LINE_MAX=4000
# Characters kept of each member_finish fail_detail entry; bytes under a
# C/POSIX caller locale, less any multibyte character the cut splits.
EVENT_LOG_DETAIL_ENTRY_MAX=400
# Bytes reserved for a count written after its list is fitted.
_EVENT_LOG_COUNT_RESERVE=20

# ---------------------------------------------------------------------------
# event_json_string OUT_VAR VALUE
#   The one escaper: assigns VALUE as a quoted JSON string literal to OUT_VAR.
#   `\` first, then `"`, \n, \r and \t; any other C0 byte becomes \u00XX.
#   DEL and non-ASCII bytes pass through raw. Fork-free.
# ---------------------------------------------------------------------------
event_json_string() {
    local _ejs_v="$2" _ejs_bs='\' _ejs_q='"' _ejs_i _ejs_hex _ejs_c
    _ejs_v="${_ejs_v//"$_ejs_bs"/"$_ejs_bs$_ejs_bs"}"
    _ejs_v="${_ejs_v//"$_ejs_q"/"$_ejs_bs$_ejs_q"}"
    _ejs_v="${_ejs_v//$'\n'/"${_ejs_bs}n"}"
    _ejs_v="${_ejs_v//$'\r'/"${_ejs_bs}r"}"
    _ejs_v="${_ejs_v//$'\t'/"${_ejs_bs}t"}"
    if [[ $_ejs_v == *[[:cntrl:]]* ]]; then
        for (( _ejs_i = 1; _ejs_i < 32; _ejs_i++ )); do
            printf -v _ejs_hex '%02x' "$_ejs_i"
            printf -v _ejs_c "\\x$_ejs_hex"
            _ejs_v="${_ejs_v//"$_ejs_c"/"${_ejs_bs}u00$_ejs_hex"}"
        done
    fi
    printf -v "$1" '"%s"' "$_ejs_v"
}

# _event_json_int OUT_VAR VALUE — VALUE as a JSON integer (an optional '-'
# then digits, leading zeros dropped), else null.
_event_json_int() {
    local _eji_v="$2" _eji_sign=""
    if [[ $_eji_v == -* ]]; then
        _eji_sign="-"
        _eji_v="${_eji_v#-}"
    fi
    if [[ -z $_eji_v || $_eji_v == *[!0123456789]* ]]; then
        printf -v "$1" '%s' null
        return 0
    fi
    _eji_v="${_eji_v#"${_eji_v%%[!0]*}"}"
    printf -v "$1" '%s' "${_eji_v:+$_eji_sign}${_eji_v:-0}"
}

# _event_json_bool OUT_VAR VALUE — true or false, else null.
_event_json_bool() {
    case "$2" in
        true | false) printf -v "$1" '%s' "$2" ;;
        *) printf -v "$1" '%s' null ;;
    esac
}

# _event_log_add LINE_VAR str|int|bool KEY VALUE — append ,"KEY":<VALUE> to
# the record text in LINE_VAR.
_event_log_add() {
    local _ela_v
    case "$2" in
        str) event_json_string _ela_v "$4" ;;
        int) _event_json_int _ela_v "$4" ;;
        bool) _event_json_bool _ela_v "$4" ;;
    esac
    printf -v "$1" '%s,"%s":%s' "${!1}" "$3" "$_ela_v"
}

# event_log_clock_ns OUT_VAR — the one event clock: epoch nanoseconds (at
# microsecond resolution under bash 5's EPOCHREALTIME), else empty.
event_log_clock_ns() {
    local _elc_ts="${EPOCHREALTIME:-}"
    if [ -n "$_elc_ts" ]; then
        _elc_ts="${_elc_ts//[.,]/}000"
    else
        _elc_ts="$(date +%s%N 2>/dev/null)" || _elc_ts=""
    fi
    printf -v "$1" '%s' "$_elc_ts"
}

# _event_log_no_sink — true when neither sink is set: the inert guard.
_event_log_no_sink() {
    [ -z "${REIFY_VERIFY_EVENT_LOG:-}" ] && [ -z "${EVENT_LOG_COPY_TO:-}" ]
}

# _event_log_head OUT_VAR EVENT — the record's opening: v, ts_ns, pid, event.
_event_log_head() {
    local _elh_ts
    event_log_clock_ns _elh_ts
    _event_json_int _elh_ts "$_elh_ts"
    printf -v "$1" '{"v":1,"ts_ns":%s,"pid":%s,"event":"%s"' "$_elh_ts" "$BASHPID" "$2"
}

# _event_log_drop_split_char OUT_VAR VALUE — VALUE without a trailing
# incomplete UTF-8 sequence (a lead byte followed by fewer continuation bytes
# than it announces), so a byte-wise cut never leaves invalid UTF-8.
_event_log_drop_split_char() {
    local LC_ALL=C
    local _eds_v="$2" _eds_cont=$'[\x80-\xbf]'
    local _eds_lead=$'[\xc0-\xff]' _eds_lead3=$'[\xe0-\xff]' _eds_lead4=$'[\xf0-\xff]'
    case $_eds_v in
        *$_eds_lead) _eds_v="${_eds_v%?}" ;;
        *$_eds_lead3$_eds_cont) _eds_v="${_eds_v%??}" ;;
        *$_eds_lead4$_eds_cont$_eds_cont) _eds_v="${_eds_v%???}" ;;
    esac
    printf -v "$1" '%s' "$_eds_v"
}

# _event_log_budget OUT_VAR RECORD_SO_FAR FIXED_REST — the bytes a record's
# list items may still use, after RECORD_SO_FAR, FIXED_REST (every remaining
# byte that is not a list item), the reserved count and the newline.
_event_log_budget() {
    local LC_ALL=C
    printf -v "$1" '%d' \
        $(( EVENT_LOG_LINE_MAX - 1 - ${#2} - ${#3} - _EVENT_LOG_COUNT_RESERVE ))
}

# _event_log_fit_list OUT_JSON OUT_DROPPED OUT_USED BUDGET head|tail [ITEM...]
#   Escapes ITEMs into the comma-joined body of a JSON array of at most BUDGET
#   bytes. `head` keeps the longest prefix that fits, `tail` the longest
#   suffix (in its original order). OUT_DROPPED gets the count left out and
#   OUT_USED the bytes the body takes.
_event_log_fit_list() {
    local _efl_budget="$4" _efl_order="$5"
    local LC_ALL=C
    local -a _efl_items=("${@:6}")
    local _efl_n=${#_efl_items[@]} _efl_i _efl_item _efl_cost
    local _efl_json="" _efl_used=0 _efl_kept=0
    for (( _efl_i = 0; _efl_i < _efl_n; _efl_i++ )); do
        if [ "$_efl_order" = tail ]; then
            _efl_item="${_efl_items[_efl_n - 1 - _efl_i]}"
        else
            _efl_item="${_efl_items[_efl_i]}"
        fi
        event_json_string _efl_item "$_efl_item"
        _efl_cost=${#_efl_item}
        [ "$_efl_kept" -eq 0 ] || _efl_cost=$(( _efl_cost + 1 ))
        [ $(( _efl_used + _efl_cost )) -le "$_efl_budget" ] || break
        if [ "$_efl_order" = tail ]; then
            _efl_json="$_efl_item${_efl_json:+,$_efl_json}"
        else
            _efl_json="${_efl_json:+$_efl_json,}$_efl_item"
        fi
        _efl_used=$(( _efl_used + _efl_cost ))
        _efl_kept=$(( _efl_kept + 1 ))
    done
    printf -v "$1" '%s' "$_efl_json"
    printf -v "$2" '%d' $(( _efl_n - _efl_kept ))
    printf -v "$3" '%d' "$_efl_used"
}

# _event_log_append LINE EVENT — the one writer, to each set sink. A line
# over the cap is dropped with one stderr diagnostic; only an oversized scalar
# (say a multi-KB lock path) can reach that, since list fields are fitted first.
_event_log_append() {
    local LC_ALL=C
    local _elap_bytes=$(( ${#1} + 1 )) _elap_sink
    if [ "$_elap_bytes" -gt "$EVENT_LOG_LINE_MAX" ]; then
        printf 'lib_event_log.sh: %s record of %d bytes exceeds the %d-byte line cap; not written\n' \
            "$2" "$_elap_bytes" "$EVENT_LOG_LINE_MAX" >&2 || true
        return 0
    fi
    for _elap_sink in "${REIFY_VERIFY_EVENT_LOG:-}" "${EVENT_LOG_COPY_TO:-}"; do
        [ -n "$_elap_sink" ] || continue
        { printf '%s\n' "$1" >> "$_elap_sink"; } 2>/dev/null || true
    done
    return 0
}

# ---------------------------------------------------------------------------
# Emitters. Each sets LC_ALL=C once, after the guard, so byte measurement in
# the helpers never restores the caller's locale more than once per call.
# ---------------------------------------------------------------------------
event_log_emit_run_start() {
    _event_log_no_sink && return 0
    local LC_ALL=C
    local _line
    _event_log_head _line run_start
    _event_log_add _line str run_id "${1-}"
    _event_log_add _line str scope "${2-}"
    _event_log_add _line str branch "${3-}"
    _event_log_add _line int discovered "${4-}"
    _event_log_append "$_line}" run_start
    return 0
}

event_log_emit_member_start() {
    _event_log_no_sink && return 0
    local LC_ALL=C
    local _line
    _event_log_head _line member_start
    _event_log_add _line str member "${1-}"
    _event_log_add _line int attempt "${2-}"
    _event_log_add _line str mode "${3-}"
    _event_log_append "$_line}" member_start
    return 0
}

event_log_emit_member_finish() {
    _event_log_no_sink && return 0
    # Cut in the caller's locale: at a character under UTF-8, at a byte under
    # C, where a character the cut splits is then dropped whole.
    local -a _entries=()
    local _entry
    for _entry in "${@:10}"; do
        _entries+=("${_entry:0:EVENT_LOG_DETAIL_ENTRY_MAX}")
    done
    local LC_ALL=C
    local _i
    for _i in "${!_entries[@]}"; do
        _event_log_drop_split_char "_entries[$_i]" "${_entries[_i]}"
    done
    local _line _pre _budget _detail _dropped _used _omitted=null
    _event_log_head _line member_finish
    _event_log_add _line str member "${1-}"
    _event_log_add _line int attempt "${2-}"
    _event_log_add _line int rc "${3-}"
    _event_log_add _line str verdict "${4-}"
    _event_log_add _line int duration_ms "${5-}"
    _event_log_add _line int queued_ns "${6-}"
    _event_log_add _line int started_ns "${7-}"
    _event_log_add _line int ended_ns "${8-}"
    _event_json_int _pre "${9-}"
    _event_log_budget _budget "$_line" ',"fail_detail":[],"fail_detail_omitted":}'
    _event_log_fit_list _detail _dropped _used "$_budget" tail \
        "${_entries[@]+"${_entries[@]}"}"
    [ "$_pre" = null ] || _omitted=$(( _pre + _dropped ))
    _event_log_append \
        "$_line,\"fail_detail\":[$_detail],\"fail_detail_omitted\":$_omitted}" member_finish
    return 0
}

event_log_emit_phase_finish() {
    _event_log_no_sink && return 0
    local LC_ALL=C
    local _line
    _event_log_head _line phase_finish
    _event_log_add _line str phase "${1-}"
    _event_log_add _line int started_ns "${2-}"
    _event_log_add _line int ended_ns "${3-}"
    _event_log_append "$_line}" phase_finish
    return 0
}

event_log_emit_slot_timeout() {
    _event_log_no_sink && return 0
    local LC_ALL=C
    local _line
    _event_log_head _line slot_timeout
    _event_log_add _line str reason "${1-}"
    _event_log_add _line int slots "${2-}"
    _event_log_add _line int waited "${3-}"
    _event_log_add _line str disposition "${4-}"
    _event_log_add _line str lock "${5-}"
    _event_log_add _line str emitter "${6-}"
    _event_log_append "$_line}" slot_timeout
    return 0
}

# run_finish fills failed, then flaky, then skipped from ONE budget; once a
# list drops a name every later name is dropped too, so the kept names are a
# prefix of failed‖flaky‖skipped and names_omitted counts the rest. A count is
# read as every int argument is, so a leading zero is decimal, never octal. A
# count that is not a non-negative integer, or exceeds the arguments left,
# encodes that list and every later one as [] rather than mis-slicing.
event_log_emit_run_finish() {
    _event_log_no_sink && return 0
    local LC_ALL=C
    local _line _interrupted _budget _list _count _json _dropped _used
    local _omitted=0 _malformed=0
    local -a _names
    _event_log_head _line run_finish
    _event_log_add _line int discovered "${1-}"
    _event_log_add _line int failures "${2-}"
    _event_json_bool _interrupted "${3-}"
    if [ $# -ge 3 ]; then shift 3; else set --; fi
    _event_log_budget _budget "$_line" \
        ",\"failed\":[],\"flaky\":[],\"skipped\":[],\"interrupted\":$_interrupted,\"names_omitted\":}"
    for _list in failed flaky skipped; do
        _names=()
        _event_json_int _count "${1-}"
        if [ "$_malformed" -eq 0 ] && [[ $_count == [0-9]* ]] \
            && [ "${#_count}" -le 9 ] && [ "$_count" -lt $# ]; then
            _names=("${@:2:_count}")
            shift $(( _count + 1 ))
        else
            _malformed=1
        fi
        _event_log_fit_list _json _dropped _used "$_budget" head \
            "${_names[@]+"${_names[@]}"}"
        _budget=$(( _budget - _used ))
        [ "$_dropped" -eq 0 ] || _budget=0
        _omitted=$(( _omitted + _dropped ))
        _line="$_line,\"$_list\":[$_json]"
    done
    _event_log_append \
        "$_line,\"interrupted\":$_interrupted,\"names_omitted\":$_omitted}" run_finish
    return 0
}
