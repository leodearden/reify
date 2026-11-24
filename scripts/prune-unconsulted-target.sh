#!/usr/bin/env bash
# scripts/prune-unconsulted-target.sh — Delete the deps/ artefacts of cargo
# units that no build has consulted lately, judged by when cargo last read each
# unit's .fingerprint dir. The one home of the fingerprint-consult liveness rule
# (#8352; shared since task 9246).
#
# Usage:
#   scripts/prune-unconsulted-target.sh --window-days N [--atime-probe PATH]
#                                       [--lock FILE]...
#                                       [--free-floor-gib G --protect-hours H]
#                                       [--prune-dead-unit-records]
#                                       PROFILE_DIR...
#
# Callers:
#   - scripts/refresh-warm-base.sh Step 3b: the warm-lane base's staging copy,
#     with --prune-dead-unit-records.
#   - deploy/systemd/reify-merge-verify-trim.service: leo-laptop's persistent
#     _merge-verify target, without --prune-dead-unit-records.
#     dark_factory's install_laptop_merge_verify_trim.sh installs THIS FILE
#     ALONE into ~/.local/lib/reify-verify-host/, so it must never source a
#     sibling.
#
# The rule, applied to each PROFILE_DIR (e.g. target/debug) on its own:
#   A depth-1 file of <profile>/deps whose name, final dot-suffix stripped, ends
#   in `-<16 lowercase hex>` belongs to that cargo unit hash. It is deleted iff
#   the hash has a <profile>/.fingerprint/<pkg>-<hash> dir AND
#   last_use < anchor - window, where
#     last_use = max(atime, mtime) over that dir's files: cargo reads them on
#                every build whose graph holds the unit, and a rebuild rewrites
#                them, so a unit still being built is consulted continuously
#                whatever its artefact mtime;
#     anchor   = the newest last_use in THIS profile that is not in the future.
#                Anchoring on the tree, not on wall clock, means an idle target
#                ages nothing; anchoring per profile keeps a rarely built
#                profile from being wiped whenever another one is busy.
#   Never deleted: a hashed file with no fingerprint dir, a name outside the
#   grammar, anything below deps/ depth 1, build/, and (unless
#   --prune-dead-unit-records) .fingerprint itself.
#
# Eviction, under --free-floor-gib G --protect-hours H only, after the age pass
# above has run on every profile. For each PROFILE_DIR in argument order:
#   1. Measure the space available (df) on the filesystem of the FIRST
#      PROFILE_DIR, the one the floor guards. Stop evicting if df fails or
#      reports at least G GiB.
#   2. Rank this profile's units whose last_use < anchor - H hours (its own
#      anchor, as above) least recently consulted first, then larger summed
#      deps bytes first, then by hash.
#   3. Delete the deps files of the shortest prefix of that ranking whose summed
#      bytes cover the deficit (G GiB minus the measured space), or of every
#      ranked unit if none does. A unit's deps files always go together.
#   df is re-measured before each profile, never projected, so deps that free
#   less than their apparent size (hardlinks, sparse files) leave a deficit the
#   next profile or the next run sees.
#   H, not a day-granular window, bounds eviction: relatime updates atime at
#   most once a day, so a live unit's recorded consult can lag its real one by
#   up to 24 h, and H must clear that lag plus one build.
#
# Record victims, under --prune-dead-unit-records only: for each hash that had
# a deps file deleted in THIS pass (age or eviction) and has no
# <profile>/build/<pkg>-<hash> dir, also delete .fingerprint/<pkg>-<hash>/ and
# each depth-1 deps/*.dwo whose name, cut at its FIRST dot, ends in that
# -<hash> (the later 16-hex cgu token is not a unit hash). Order: deps files, then shards, then records, each stage
# only after the previous one succeeded, so a failed unlink leaves the record.
#   Safe: a unit whose deps are gone is rebuilt whether or not its record
#   survives. Build-script units own no deps files, so they keep their records
#   and build/ outputs, which a later build can still reuse as Fresh.
#   Opt-in: only a staging copy re-imports the records on every refresh. On a
#   persistent target the deletion is permanent and erases the evidence the
#   next run reads.
#
# Skips — a line containing "skip", nothing deleted in that profile, the other
# profiles still pruned:
#   - PROFILE_DIR is absent, or has no deps/ or no .fingerprint/;
#   - the atime probe's mount does not maintain atime (findmnt reports noatime,
#     fails, or prints nothing). Without atime last_use degrades to mtime, and
#     every unchanged registry unit would read as stale.
#
# Options:
#   --window-days N     Required. Integer >= 1.
#   --atime-probe PATH  The path whose mount must maintain atime: the tree
#                       cargo's fingerprint reads went through (default: the
#                       profile dir being pruned).
#   --lock FILE         Repeatable. Taken exclusively and non-blocking, in the
#                       order given, before any profile is read, and held until
#                       the script exits, so the trim never overlaps a build in
#                       the same target. These are dark_factory's flock(2) lane
#                       locks, created on first use and never unlinked, so an
#                       absent lock file is created here too. A lock another
#                       process holds is normal contention: the trim is skipped
#                       and the script exits 0, because the laptop service must
#                       end in Result=success. A lock that cannot be opened, or a
#                       flock fault, means the paths are misconfigured: exit 1,
#                       nothing read. flock's -E code tells contention apart
#                       from a fault, which a bare `flock -n` rc 1 cannot.
#   --free-floor-gib G  Both or neither; integers >= 1, H <= 24 x N. Turn on
#   --protect-hours H   eviction (above) to a floor of G GiB available. A df
#                       failure stops eviction, so a failed measurement never
#                       deletes more.
#   --prune-dead-unit-records
#                       Also delete record victims (above).
#   -h, --help          Print usage and exit 0.
#
# Output: nothing on stdout. stderr, per profile the age pass pruned:
#   prune deps=<deps dir> files=N victim_bytes=N records=N dwo=N window_days=N
# and per profile eviction reached:
#   prune deps=<deps dir> files=N victim_bytes=N records=N dwo=N free_floor_gib=G protect_hours=H
# files and victim_bytes (summed apparent size) count the deps/ victims only;
# records and dwo count the deleted fingerprint dirs and shards, and are 0
# without --prune-dead-unit-records.
#
# Exit: 0 pruned, skipped, or a --lock held elsewhere; 1 failure (a --lock
# that cannot be taken, victim selection failed, or a deps, shard or record
# removal stage failed); 2 usage error, before anything is read or deleted.
#
# Executable spec: tests/infra/test_prune_unconsulted_target.py, and through
# refresh-warm-base.sh, tests/infra/test_refresh_warm_base.sh Block LIVE.

set -euo pipefail

info() { printf '[info]  %s\n' "$*" >&2; }
err()  { printf '[error] %s\n' "$*" >&2; }

_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") --window-days N [--atime-probe PATH] [--lock FILE]...
           [--free-floor-gib G --protect-hours H] [--prune-dead-unit-records]
           PROFILE_DIR...

  Delete the deps/ files of cargo units whose .fingerprint was last consulted
  more than N days before the profile's newest consult; with a floor, then
  evict the least recently consulted units older than H hours until G GiB are
  available. Rule, skips and exit codes: this script's header.
EOF
}

_usage_error() {
    err "$*"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
}

_is_positive_int() {
    case "$1" in ''|*[!0-9]*|0*) return 1 ;; esac
}

# ── arg parsing ───────────────────────────────────────────────────────────────
WINDOW_DAYS=""
ATIME_PROBE=""
LOCKS=()
FREE_FLOOR_GIB=""
PROTECT_HOURS=""
PRUNE_RECORDS=0
PROFILE_DIRS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --window-days)
            [ $# -ge 2 ] || _usage_error "--window-days requires a value"
            WINDOW_DAYS="$2"; shift 2 ;;
        --atime-probe)
            [ $# -ge 2 ] && [ -n "$2" ] || _usage_error "--atime-probe requires a path"
            ATIME_PROBE="$2"; shift 2 ;;
        --lock)
            [ $# -ge 2 ] && [ -n "$2" ] || _usage_error "--lock requires a path"
            LOCKS+=("$2"); shift 2 ;;
        --free-floor-gib)
            [ $# -ge 2 ] && [ -n "$2" ] || _usage_error "--free-floor-gib requires a value"
            FREE_FLOOR_GIB="$2"; shift 2 ;;
        --protect-hours)
            [ $# -ge 2 ] && [ -n "$2" ] || _usage_error "--protect-hours requires a value"
            PROTECT_HOURS="$2"; shift 2 ;;
        --prune-dead-unit-records)
            PRUNE_RECORDS=1; shift ;;
        --)
            shift; PROFILE_DIRS+=("$@"); break ;;
        -*)
            _usage_error "Unknown flag: $1" ;;
        *)
            PROFILE_DIRS+=("$1"); shift ;;
    esac
done

_is_positive_int "$WINDOW_DAYS" \
    || _usage_error "--window-days must be an integer >= 1 (got '${WINDOW_DAYS}')"
[ "${#PROFILE_DIRS[@]}" -ge 1 ] || _usage_error "at least one PROFILE_DIR is required"
if [ -n "$FREE_FLOOR_GIB" ] || [ -n "$PROTECT_HOURS" ]; then
    [ -n "$FREE_FLOOR_GIB" ] && [ -n "$PROTECT_HOURS" ] \
        || _usage_error "--free-floor-gib and --protect-hours must be given together"
    _is_positive_int "$FREE_FLOOR_GIB" \
        || _usage_error "--free-floor-gib must be an integer >= 1 (got '${FREE_FLOOR_GIB}')"
    _is_positive_int "$PROTECT_HOURS" && [ "$PROTECT_HOURS" -le $(( WINDOW_DAYS * 24 )) ] \
        || _usage_error "--protect-hours must be an integer from 1 to 24 x --window-days (got '${PROTECT_HOURS}')"
fi

# ── lane locks: every one taken before the tree is read, held to exit ─────────
readonly _LOCK_HELD_RC=75
for _lock in "${LOCKS[@]}"; do
    if ! exec {_lock_fd}>>"$_lock"; then
        err "cannot open lock file $_lock — refusing to trim (nothing deleted)."
        exit 1
    fi
    _flock_rc=0
    flock -n -x -E "$_LOCK_HELD_RC" "$_lock_fd" || _flock_rc=$?
    case "$_flock_rc" in
        0) ;;
        "$_LOCK_HELD_RC")
            info "lane lock held: $_lock — skipping trim (files=0)."
            exit 0 ;;
        *)
            err "flock on $_lock failed (rc=$_flock_rc) — refusing to trim (nothing deleted)."
            exit 1 ;;
    esac
done

# ── the liveness rule ─────────────────────────────────────────────────────────

# _mount_tracks_atime <path>: does the mount holding <path> maintain atime? The
# option list is parsed once, here, on the exact token: nodiratime is not noatime,
# and findmnt failing or printing nothing is "unknown", which counts as no.
_mount_tracks_atime() {
    local opts
    opts="$(findmnt -n -o OPTIONS -T "$1" 2>/dev/null)" || return 1
    [ -n "$opts" ] || return 1
    case ",$opts," in *,noatime,*) return 1 ;; esac
    return 0
}

# A profile is read in three stages, each its own function:
#   _scan_units    find + the ONE copy of the unit-hash grammar -> a scan table;
#   a predicate    scan table -> the chosen unit hashes, one per line
#                  (_select_aged or _select_lru_prefix);
#   _emit_victims  scan table + chosen hashes -> the deps, dwo and records NUL
#                  lists and the summary.
# Scan table rows, tab-separated, one per line:
#   A <anchor>                        the profile's anchor (see the header)
#   U <hash> <last_use> <deps bytes>  a unit with deps files and a record dir
#   D <hash> <size> <deps file name>  one hashed depth-1 deps file
#   R <hash> <record dir name>        one .fingerprint dir   (prune_records only)
#   S <hash> <shard file name>        one depth-1 .dwo shard (prune_records only)
#   B <hash>                          one build/ dir         (prune_records only)

# _scan_units <profile_dir> <prune_records>: the scan table on stdout.
_scan_units() {
    local profile="$1" prune_records="$2"
    { find "$profile/.fingerprint" -mindepth 2 -maxdepth 2 -type f -printf 'F\t%A@\t%T@\t%P\n'
      find "$profile/deps" -maxdepth 1 -type f -printf 'D\t%s\t%f\n'
      if [ "$prune_records" = 1 ] && [ -d "$profile/build" ]; then
          find "$profile/build" -mindepth 1 -maxdepth 1 -type d -printf 'B\t%f\n'
      fi
    } | awk -F'\t' -v now="$(date +%s)" -v prune_records="$prune_records" '
        BEGIN {
            # The ONE copy of the unit-hash grammar: <stem>-<16 lowercase hex>.
            # Spelled out as repeated [0-9a-f] classes, not {16}: mawk < 1.3.4 and
            # busybox awk lack interval expressions, under which {16} matches
            # nothing and this whole stage would silently become a no-op.
            hash_len = 16
            hash_re = "^.+-"
            for (i = 0; i < hash_len; i++) hash_re = hash_re "[0-9a-f]"
            hash_re = hash_re "$"
            anchor = 0
        }
        function unit_hash(stem) {
            return (stem ~ hash_re) ? substr(stem, length(stem) - hash_len + 1) : ""
        }
        # F: one file of a unit dir. last_use[hash] is the newest atime-or-mtime seen.
        $1 == "F" {
            split($4, parts, "/")
            h = unit_hash(parts[1])
            if (h == "") next
            if (prune_records == 1 && !(parts[1] in record_seen)) {
                record_seen[parts[1]] = 1
                printf "R\t%s\t%s\n", h, parts[1]
            }
            atime = $2 + 0; mtime = $3 + 0
            t = (atime > mtime) ? atime : mtime
            if (!(h in last_use) || t > last_use[h]) last_use[h] = t
            # A future-dated consult still counts for its own unit but must not
            # become the anchor, or every live unit would age out against it.
            if (t <= now && t > anchor) anchor = t
            next
        }
        # D: one depth-1 deps file. Strip only the FINAL dot-suffix, so
        # reify_ast-<h> and reify_ast-<h>.d resolve to the same hash.
        $1 == "D" {
            stem = $3
            sub(/\.[^.]*$/, "", stem)
            h = unit_hash(stem)
            if (h != "") {
                unit_bytes[h] += $2
                printf "D\t%s\t%s\t%s\n", h, $2, $3
                next
            }
            # A split-debuginfo shard, <stem>-<h>.<crate>.<cgu>-cgu.N.rcgu.dwo, is
            # keyed on its name cut at the FIRST dot: its later 16-hex cgu token
            # is not a unit hash. Only prune_records ever reads a shard.
            if (prune_records != 1 || $3 !~ /\.dwo$/) next
            stem = $3
            sub(/\..*$/, "", stem)
            h = unit_hash(stem)
            if (h != "") printf "S\t%s\t%s\n", h, $3
            next
        }
        # B: one build/ dir, present only under prune_records.
        $1 == "B" {
            h = unit_hash($2)
            if (h != "") printf "B\t%s\n", h
            next
        }
        END {
            printf "A\t%.6f\n", anchor
            for (h in unit_bytes) {
                if (h in last_use) printf "U\t%s\t%.6f\t%.0f\n", h, last_use[h], unit_bytes[h]
            }
        }
    '
}

# _units_unconsulted_for <secs> <scan>: "hash<TAB>last_use<TAB>deps bytes" of
# every unit last consulted more than <secs> before the profile's anchor.
_units_unconsulted_for() {
    awk -F'\t' -v secs="$1" '
        $1 == "A" { cutoff = $2 - secs }
        $1 == "U" { n++; unit[n] = $2 "\t" $3 "\t" $4; last_use[n] = $3 + 0 }
        END { for (i = 1; i <= n; i++) if (last_use[i] < cutoff) print unit[i] }
    ' "$2"
}

# _select_aged <window_secs> <scan>: every unit outside the window.
_select_aged() {
    _units_unconsulted_for "$1" "$2" | cut -f1
}

# _select_lru_prefix <protect_secs> <budget_bytes> <scan>: of the units outside
# the protect window, least recently consulted first, larger first on a tie,
# the shortest prefix whose summed deps bytes reach <budget_bytes>.
_select_lru_prefix() {
    _units_unconsulted_for "$1" "$3" \
        | LC_ALL=C sort -t "$(printf '\t')" -k2,2n -k3,3nr -k1,1 \
        | awk -F'\t' -v budget="$2" '
            # Reads to the end rather than exiting early, so sort never sees
            # SIGPIPE under pipefail.
            !covered { print $1; taken += $3; if (taken >= budget + 0) covered = 1 }
        '
}

# _emit_victims <scan> <chosen> <prune_records> <work_dir>: writes three
# NUL-delimited name lists into <work_dir>: deps (files of <profile>/deps), dwo
# (shards of <profile>/deps) and records (dirs of <profile>/.fingerprint), plus
# summary: "files=N victim_bytes=N records=N dwo=N", where files and
# victim_bytes count the deps list only. With <prune_records> 0 the scan holds
# no R, S or B rows, so the dwo and records lists stay empty.
_emit_victims() {
    awk -F'\t' -v chosen="$2" -v prune_records="$3" -v work="$4" '
        BEGIN {
            while ((getline h < chosen) > 0) selected[h] = 1
            close(chosen)
            files = 0; bytes = 0; records = 0; dwo = 0; m = 0; r = 0
        }
        $1 == "D" {
            if (!($2 in selected)) next
            # NUL via %c: ORS="\0" is dropped by busybox awk, %c 0 is not.
            printf "%s%c", $4, 0 > (work "/deps")
            files++
            bytes += $3
            next
        }
        $1 == "S" { m++; shard_hash[m] = $2; shard_name[m] = $3; next }
        $1 == "R" { r++; record_hash[r] = $2; record_name[r] = $3; next }
        $1 == "B" { has_build[$2] = 1; next }
        END {
            # A record victim is a unit this pass pruned deps for, so the
            # predicate that chose it is its only test.
            if (prune_records == 1) {
                for (h in selected) if (!(h in has_build)) record_victim[h] = 1
                for (i = 1; i <= m; i++) {
                    if (!(shard_hash[i] in record_victim)) continue
                    printf "%s%c", shard_name[i], 0 > (work "/dwo")
                    dwo++
                }
                for (i = 1; i <= r; i++) {
                    if (!(record_hash[i] in record_victim)) continue
                    printf "%s%c", record_name[i], 0 > (work "/records")
                    records++
                }
            }
            printf "files=%d victim_bytes=%.0f records=%d dwo=%d\n", files, bytes, records, dwo \
                > (work "/summary")
        }
    ' "$1"
}

# The work dir of the profile being pruned; the EXIT trap reclaims it when a
# prune fails midway.
_WORK_DIR=""
_reclaim_work_dir() {
    [ -z "$_WORK_DIR" ] || rm -rf "$_WORK_DIR"
}
trap _reclaim_work_dir EXIT

# _unlink_listed <dir> <list> <what> <rm_opt>: rm <rm_opt> every name in the
# NUL-delimited <list>, from inside <dir>. Any failure exits 1, so no later
# stage runs.
_unlink_listed() {
    local dir="$1" list="$2" what="$3" rm_opt="$4"
    if ! (cd "$dir" && xargs -r -0 rm "$rm_opt" -- < "$list"); then
        err "removing the $what in $dir failed; they may be partly removed, and no later stage ran."
        exit 1
    fi
}

# _prune_profile <profile_dir> <label> <predicate> [predicate args...]: delete
# the victims of the units <predicate> chooses, called with its args and the
# scan table's path appended, and report them with <label>. The records any
# pass deletes are of units below the anchor, so never the anchor's own: a
# later pass over the same profile sees the same anchor.
_prune_profile() {
    local profile="$1" label="$2"
    shift 2
    local deps="$profile/deps" fp="$profile/.fingerprint" probe="${ATIME_PROBE:-$profile}"
    if [ ! -d "$profile" ]; then
        info "No profile dir $profile — skipping liveness prune (files=0)."
        return 0
    fi
    if [ ! -d "$deps" ]; then
        info "No deps dir under $profile — skipping liveness prune (files=0)."
        return 0
    fi
    if [ ! -d "$fp" ]; then
        info "No .fingerprint under $profile — no liveness evidence; skipping liveness prune (files=0)."
        return 0
    fi
    if ! _mount_tracks_atime "$probe"; then
        info "Mount of $probe does not maintain atime (noatime, or findmnt unavailable or empty) — no liveness evidence; skipping liveness prune of $profile (files=0)."
        return 0
    fi
    _WORK_DIR="$(mktemp -d)"
    : > "$_WORK_DIR/deps"; : > "$_WORK_DIR/dwo"; : > "$_WORK_DIR/records"
    if ! _scan_units "$profile" "$PRUNE_RECORDS" > "$_WORK_DIR/scan" \
            || ! "$@" "$_WORK_DIR/scan" > "$_WORK_DIR/chosen" \
            || ! _emit_victims "$_WORK_DIR/scan" "$_WORK_DIR/chosen" "$PRUNE_RECORDS" "$_WORK_DIR"; then
        err "victim selection for $profile failed; nothing deleted."
        exit 1
    fi
    _unlink_listed "$deps" "$_WORK_DIR/deps" "deps files" -f
    _unlink_listed "$deps" "$_WORK_DIR/dwo" ".dwo shards" -f
    _unlink_listed "$fp" "$_WORK_DIR/records" "fingerprint records" -rf
    info "prune deps=$deps $(cat "$_WORK_DIR/summary") $label"
    rm -rf "$_WORK_DIR"
    _WORK_DIR=""
}

# _free_bytes <path>: stdout the bytes available on <path>'s filesystem; fails
# when df fails or prints anything but a byte count on its last line.
_free_bytes() {
    local out
    out="$(df -B1 --output=avail -- "$1" 2>/dev/null)" || return 1
    out="${out##*$'\n'}"
    out="${out//[[:space:]]/}"
    case "$out" in ''|*[!0-9]*) return 1 ;; esac
    printf '%s\n' "$out"
}

for _profile in "${PROFILE_DIRS[@]}"; do
    _prune_profile "$_profile" "window_days=$WINDOW_DAYS" \
        _select_aged "$(( WINDOW_DAYS * 86400 ))"
done

if [ -n "$FREE_FLOOR_GIB" ]; then
    _floor_bytes=$(( FREE_FLOOR_GIB * 1024 * 1024 * 1024 ))
    for _profile in "${PROFILE_DIRS[@]}"; do
        if ! _free="$(_free_bytes "${PROFILE_DIRS[0]}")"; then
            info "cannot measure free space on ${PROFILE_DIRS[0]} — evicting nothing more."
            break
        fi
        [ "$_free" -lt "$_floor_bytes" ] || break
        _deficit=$(( _floor_bytes - _free ))
        info "free ${_free} bytes is below the ${FREE_FLOOR_GIB} GiB floor; evicting up to ${_deficit} bytes from $_profile."
        _prune_profile "$_profile" "free_floor_gib=$FREE_FLOOR_GIB protect_hours=$PROTECT_HOURS" \
            _select_lru_prefix "$(( PROTECT_HOURS * 3600 ))" "$_deficit"
    done
fi
