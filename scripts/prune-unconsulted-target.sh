#!/usr/bin/env bash
# scripts/prune-unconsulted-target.sh — Delete the deps/ artefacts of cargo
# units that no build has consulted lately, judged by when cargo last read each
# unit's .fingerprint dir. The one home of the fingerprint-consult liveness rule
# (#8352; shared since task 9246).
#
# Usage:
#   scripts/prune-unconsulted-target.sh --window-days N [--atime-probe PATH]
#                                       PROFILE_DIR...
#
# Callers:
#   - scripts/refresh-warm-base.sh Step 3b: the warm-lane base's staging copy.
#   - deploy/systemd/reify-merge-verify-trim.service: leo-laptop's persistent
#     _merge-verify target. dark_factory's install_laptop_merge_verify_trim.sh
#     installs THIS FILE ALONE into ~/.local/lib/reify-verify-host/, so it must
#     never source a sibling.
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
#   grammar, anything below deps/ depth 1, and .fingerprint itself.
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
#   -h, --help          Print usage and exit 0.
#
# Output: nothing on stdout. stderr, per pruned profile:
#   prune deps=<deps dir> files=N victim_bytes=N window_days=N
# victim_bytes is the victims' summed apparent size.
#
# Exit: 0 pruned or skipped; 1 failure (the unlink pipeline failed); 2 usage
# error, before anything is read or deleted.
#
# Executable spec: tests/infra/test_prune_unconsulted_target.py, and through
# refresh-warm-base.sh, tests/infra/test_refresh_warm_base.sh Block LIVE.

set -euo pipefail

info() { printf '[info]  %s\n' "$*" >&2; }
err()  { printf '[error] %s\n' "$*" >&2; }

_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") --window-days N [--atime-probe PATH] PROFILE_DIR...

  Delete the deps/ files of cargo units whose .fingerprint was last consulted
  more than N days before the profile's newest consult. Rule, skips and exit
  codes: this script's header.
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

# _select_unconsulted_victims <fingerprint_dir> <deps_dir> <window_secs> <summary_file>
# stdout: the NUL-delimited names of the <deps_dir> files to delete.
# <summary_file>: "files=N victim_bytes=N" for exactly those files. stdout
# carries the list, so the summary crosses back through the file.
_select_unconsulted_victims() {
    local fp="$1" deps="$2" window="$3" summary="$4"
    { find "$fp" -mindepth 2 -maxdepth 2 -type f -printf 'F\t%A@\t%T@\t%P\n'
      find "$deps" -maxdepth 1 -type f -printf 'D\t%s\t%f\n'
    } | awk -F'\t' -v window="$window" -v now="$(date +%s)" -v summary_file="$summary" '
        BEGIN {
            # The ONE copy of the unit-hash grammar: <stem>-<16 lowercase hex>.
            # Spelled out as repeated [0-9a-f] classes, not {16}: mawk < 1.3.4 and
            # busybox awk lack interval expressions, under which {16} matches
            # nothing and this whole stage would silently become a no-op.
            hash_len = 16
            hash_re = "^.+-"
            for (i = 0; i < hash_len; i++) hash_re = hash_re "[0-9a-f]"
            hash_re = hash_re "$"
            anchor = 0; n = 0; files = 0; bytes = 0
        }
        function unit_hash(stem) {
            return (stem ~ hash_re) ? substr(stem, length(stem) - hash_len + 1) : ""
        }
        # F: one file of a unit dir. last_use[hash] is the newest atime-or-mtime seen.
        $1 == "F" {
            split($4, parts, "/")
            h = unit_hash(parts[1])
            if (h == "") next
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
            if (h == "") next
            n++
            cand_name[n] = $3; cand_size[n] = $2; cand_hash[n] = h
        }
        END {
            cutoff = anchor - window
            for (i = 1; i <= n; i++) {
                h = cand_hash[i]
                if (!(h in last_use) || last_use[h] >= cutoff) continue
                # NUL via %c: ORS="\0" is dropped by busybox awk, %c 0 is not.
                printf "%s%c", cand_name[i], 0
                files++
                bytes += cand_size[i]
            }
            printf "files=%d victim_bytes=%.0f\n", files, bytes > summary_file
        }
    '
}

# The summary file of the profile being pruned; the EXIT trap reclaims it when
# a prune fails midway.
_SUMMARY_FILE=""
_reclaim_summary() {
    [ -z "$_SUMMARY_FILE" ] || rm -f "$_SUMMARY_FILE"
}
trap _reclaim_summary EXIT

# _prune_profile <profile_dir> <window_days>
_prune_profile() {
    local profile="$1" window_days="$2"
    local deps="$1/deps" fp="$1/.fingerprint" probe="${ATIME_PROBE:-$1}"
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
    _SUMMARY_FILE="$(mktemp)"
    if ! _select_unconsulted_victims "$fp" "$deps" "$(( window_days * 86400 ))" "$_SUMMARY_FILE" \
            | (cd "$deps" && xargs -r -0 rm -f --); then
        err "prune of $deps failed (victim selection or unlink); its files may be partly pruned."
        exit 1
    fi
    info "prune deps=$deps $(cat "$_SUMMARY_FILE") window_days=$window_days"
    rm -f "$_SUMMARY_FILE"
    _SUMMARY_FILE=""
}

for _profile in "${PROFILE_DIRS[@]}"; do
    _prune_profile "$_profile" "$WINDOW_DAYS"
done
