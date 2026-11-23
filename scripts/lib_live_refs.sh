#!/usr/bin/env bash
# scripts/lib_live_refs.sh — Shared live-reference /proc scanner for the warm-lane pool.
#
# Sourced by (both consumers, one definition — G7 no-lockstep-duplication):
#   scripts/warm-lane-gc.sh        — the ε reclaim PRIMITIVE. Calls
#       live_ref_present per lane (Pass 1) and per orphan (Pass 2), immediately
#       before the reset/remove, while that entry's flock is already held. Every
#       caller of gc.sh therefore inherits the guard — including dark-factory's
#       ε path (git_ops.py _run_warm_lane_gc_reclaim), which invokes
#       `warm-lane-gc.sh reclaim --mount <base>` and never passes
#       --extra-protect-glob (task 5572).
#   scripts/warm-lane-gc-sweep.sh  — the δ periodic backstop. Calls
#       live_referenced_paths ONCE per sweep for the whole batch of dead-PID
#       .reseed-trash candidates (_reap_stale_trash, task 5326).
#
# FUNCTIONS (defined when sourced):
#   live_referenced_paths RP...
#       Batch predicate: print the subset of the given resolved realpaths that
#       ANY live process references (cwd / open fd / mmap). ONE /proc walk for
#       the WHOLE set.
#   live_ref_present DIR
#       Single-candidate predicate: return 0 iff DIR (or a descendant) is
#       referenced by a live process; 1 otherwise. Short-circuits: the cheaper
#       verdict wins, see below.
#   Both also see a same-user process in ANOTHER mount namespace that reaches a
#   candidate through that namespace's own mount of it (dark-factory
#   plans/canonical-build-path-shared-cache-prd.md decision 16 / C11): a mount
#   whose root, stat'd through /proc/<pid>/root, has the candidate's dev:ino
#   gives an alias, and that namespace's cwd/fd/mmap paths are translated back
#   through it before the same match. Each namespace is resolved once per scan.
#   A namespace whose mountinfo or root is unreadable is skipped, never counted
#   (browser sandboxes are same-user foreign namespaces). An fd on a file moved
#   OUT of the bound tree reads as "/" there and stays unattributable.
#
# MEASURED COST (this host, 2026-07-28: 1159 live processes, 11527 fd/cwd
# symlinks, 93 entries under the warm-lane worktrees dir):
#   cwd/fd pass (find+awk over /proc) ...... ~1.5s   — unavoidable for a
#                                                     candidate that turns out
#                                                     NOT to be referenced
#   mmap pass  (grep over /proc/*/maps) .... ~0.38s
#   one live_ref_present call .............. ~1.9s (both passes) /
#                                            ~1.5s (cwd-or-fd hit, short-circuit)
#   namespace pass (2026-11-19) ............ ~0.12s CPU; 0.6-1.6s wall at load
#                                            150-190 on 32 cores (fork latency),
#                                            4 foreign namespaces present
# A free candidate pays all three passes.
# The awk early-exit only fires once EVERY candidate is accounted for, so a
# free (about-to-be-reclaimed) candidate always pays the full walk — and that
# is precisely the verdict that must be fresh. See the "cost reversal" note in
# scripts/warm-lane-gc.sh's header for what this means for a whole reclaim pass
# and why the per-entry placement is still the right trade.
#
# Neither function can distinguish "scanned, found nothing" from "scan failed":
# every stage is `|| true`-guarded because find/grep over /proc legitimately
# return non-zero on other users' permission-denied entries and on SIGPIPE from
# awk's early exit, so under `set -o pipefail` the exit status is unreliable
# (see the per-function note below). Callers get the fail-OPEN behaviour the
# sweep has always shipped: a failed scan reads as "no reference". That property
# is heavier now that the same verdict gates `rm -rf <lane>/target` and
# `git worktree remove --force`, so _live_refs_env_probe warns ONCE per process
# when the scan environment looks degraded — see its own note for what it does
# and does not catch, and for why it warns rather than failing CLOSED.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_LIVE_REFS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_LIVE_REFS_SH_SOURCED=1

# _live_refs_env_probe — one-time, O(1) sanity check that the scan environment
# can actually SEE other processes; warns once on stderr when it cannot.
#
# Why it exists: every scan pass is `|| true`-guarded and the verdict is read
# from OUTPUT, so a wholesale scan failure (procfs not mounted in a container,
# a hidepid= mount option that hides other uids' entries) is indistinguishable
# from "nothing is live" — and that silently-empty verdict now sits in front of
# `rm -rf <lane>/target` and `git worktree remove --force`. The probe cannot fix
# that, but it makes a fail-open scan ATTRIBUTABLE in dark-factory's logs
# instead of invisible.
#
# Warn-only, deliberately NOT fail-closed: treating a degraded scan as "every
# entry is live" would freeze reclaim entirely, and reclaim is the response to
# the ENOSPC the pool is trying to avoid. A visible warning plus an unchanged
# fail-open verdict is the lesser failure.
#
# O(1) by construction — ONE readlink plus one readdir of /proc. The stronger
# probe (run the fd/cwd walk and confirm it saw this process's own cwd) would
# cost a full ~1.5s walk per process, i.e. it would pay the very cost the
# short-circuit in live_ref_present exists to cut. What this catches: /proc
# absent/unmounted, and a hidepid= mount that leaves us seeing only ourselves.
# What it does NOT catch: a partially-unreadable /proc (some entries denied),
# which still degrades the verdict silently.
_REIFY_LIVE_REFS_ENV_PROBED=0
_live_refs_env_probe() {
    [ "${_REIFY_LIVE_REFS_ENV_PROBED:-0}" = "1" ] && return 0
    _REIFY_LIVE_REFS_ENV_PROBED=1

    local self_cwd=""
    self_cwd="$(readlink /proc/self/cwd 2>/dev/null)" || self_cwd=""

    # Glob-only headcount: no per-entry stat, no symlink resolution. Under a
    # hidepid=2 mount this collapses to this process's own entry; on any host
    # healthy enough to be running a reclaim it is in the hundreds.
    local -a proc_dirs=(/proc/[0-9]*)
    local nproc=${#proc_dirs[@]}
    [ -d "${proc_dirs[0]}" ] || nproc=0   # glob-nomatch guard (literal pattern)

    if [ -n "$self_cwd" ] && [ "$nproc" -ge 5 ]; then
        return 0
    fi
    printf '[warn]  live-ref scan DEGRADED: /proc exposes %d process entries and self-cwd %s — the liveness scan fails OPEN, so every candidate reads as "not live" and reclaim will proceed unguarded. Check that /proc is mounted and not hidepid-restricted for this uid.\n' \
        "$nproc" "${self_cwd:+resolves}${self_cwd:-does NOT resolve}" >&2
    return 0
}

# _live_refs_scan_fdcwd RP... — the cwd + open-fd pass of the scan. Split out of
# live_referenced_paths (task 5572 review) with its logic byte-identical, purely
# so live_ref_present can take a verdict from this pass alone and skip the mmap
# pass on a hit. Not a public entry point: callers want live_referenced_paths or
# live_ref_present, which run every pass.
_live_refs_scan_fdcwd() {
    [ $# -gt 0 ] || return 0

    # cwd + open-fd references: one find pass prints every process's cwd and
    # open-fd symlink TARGETS (%l = the kernel-canonical target — no per-fd
    # fork). /proc/<pid>/task is pruned to skip the per-thread symlink
    # duplicates.
    find /proc -maxdepth 3 -path '*/task' -prune -o -type l \
            \( -name cwd -o -path '*/fd/*' \) -printf '%l\n' 2>/dev/null \
        | _live_refs_match_targets "$@" || true
}

# _live_refs_match_targets RP... — reads target paths on stdin. awk loads the
# candidate realpaths as input records (the NR==FNR two-file trick — avoids
# `-v`'s backslash-escape processing on data), then for each target prints any
# candidate it equals or is a "$cand/" descendant of (string compare, so
# arbitrary path characters are safe), each candidate at most once, exiting
# early once every candidate is accounted for.
_live_refs_match_targets() {
    [ $# -gt 0 ] || return 0
    awk '
        NR==FNR { if ($0 != "") cand[++n] = $0; next }
        {
            for (i = 1; i <= n; i++)
                if (!(i in seen) && ($0 == cand[i] || index($0, cand[i] "/") == 1)) {
                    print cand[i]; seen[i] = 1; nfound++
                }
            if (n > 0 && nfound >= n) exit
        }
    ' <(printf '%s\n' "$@") - 2>/dev/null
}

# _live_refs_scan_maps RP... — the mmap pass of the scan. Same split rationale
# as _live_refs_scan_fdcwd; logic byte-identical to the inline version it
# replaces. Measurably the CHEAPER of the two passes (~0.38s vs ~1.5s here), but
# it is run SECOND because the reference shape this gate exists to catch — a
# live cargo/rustc build inside <lane>/target — always holds a cwd and open fds
# there, so the fd/cwd pass is the one far likelier to short-circuit first.
_live_refs_scan_maps() {
    [ $# -gt 0 ] || return 0

    # mmap'd regions: a single grep over every process's maps for a path mapped
    # from INSIDE any candidate dir. Each pattern carries a trailing "/" so the
    # fixed-string match is path-BOUNDARY-aware: ".../_lane-1/" matches a maps
    # line ".../_lane-1/target/x" but NOT the longer sibling ".../_lane-10/..."
    # whose basename merely has "_lane-1" as a name-prefix. A bare-substring
    # match (the original) spuriously mapped an "_lane-10" build's maps line back
    # to a free "_lane-1" whenever the longer path was not itself a live
    # candidate — e.g. a lane torn down mid-build, whose "(deleted)" mapping
    # lingers — perpetually shielding low-numbered lanes' divergent target/ from
    # reclaim (esc-5378 review). This mirrors the cwd/fd pass's cand"/"
    # descendant test above; a mmap'd file always lives UNDER the dir, so
    # requiring "/" drops no real reference. The trailing "/" is then stripped so
    # each emitted line equals the input candidate realpath EXACTLY — callers
    # key on it (the reaper's `referenced` set). (The reaper's trash inputs
    # already carried a ".<pid>" delimiter, so this only tightens — never
    # loosens — its matching.)
    grep -hoFf <(printf '%s/\n' "$@") /proc/[0-9]*/maps 2>/dev/null \
        | sed 's:/$::' || true
}

# _live_refs_foreign_mntns_pids — prints "<ns> <pid>" for every readable process
# whose mount namespace differs from the scanner's. The device filter is
# load-bearing: stat -L on a process we may not ptrace exits 0 with a procfs
# directory's dev:ino, so only entries on the nsfs device are namespaces.
_live_refs_foreign_mntns_pids() {
    local self
    self="$(stat -L -c '%d:%i' /proc/self/ns/mnt 2>/dev/null)" || return 0
    stat -L -c '%d:%i %n' /proc/[0-9]*/ns/mnt 2>/dev/null \
        | awk -v self="$self" '
            BEGIN { split(self, s, ":") }
            {
                split($1, d, ":")
                if (d[1] != s[1] || $1 == self) next
                split($2, p, "/"); print $1, p[3]
            }
        ' || true
}

# _live_refs_ns_representatives FOREIGN — prints the pids each namespace in
# FOREIGN (_live_refs_foreign_mntns_pids' output) is resolved through: its
# _LIVE_REFS_NS_REPRESENTATIVES oldest processes by start time (/proc/<pid>/stat
# field 22, read after the last ")"). The oldest process of a build or agent
# namespace is the long-lived root of its tree, unlike pid or glob order, which
# a pid wrap or a digit-count crossing turns into "newest". Resolving through
# several guards against one of them exiting mid-scan or being chrooted away.
_LIVE_REFS_NS_REPRESENTATIVES=3
_live_refs_ns_representatives() {
    local -a stats
    mapfile -t stats < <(awk '{ print "/proc/" $2 "/stat" }' <<<"$1")
    cat "${stats[@]}" 2>/dev/null | awk '
        FILENAME == ARGV[1] { ns_of[$2] = $1; next }
        match($0, /\) [^)]*$/) {
            split(substr($0, RSTART + 2), f, " ")
            if ($1 in ns_of) print ns_of[$1], f[20], $1
        }
    ' <(printf '%s\n' "$1") - 2>/dev/null \
        | sort -k1,1 -k2,2n \
        | awk -v k="$_LIVE_REFS_NS_REPRESENTATIVES" '++taken[$1] <= k { print $3 }' || true
}

# _live_refs_ns_mount_roots DEVS MOUNTINFO... — prints `/proc/<pid>/root<M>`,
# one per line, for each mount M listed in the given /proc/<pid>/mountinfo files
# whose device (field 3, major:minor) is one of the space-separated DEVS. Only
# those roots can hold a candidate, so no FUSE server or unrelated filesystem is
# ever stat'd. A btrfs mount is kept whatever its device: btrfs gives each
# subvolume an st_dev that mountinfo never shows. Mount points holding a
# newline are dropped, which keeps the output line-safe.
_live_refs_ns_mount_roots() {
    local devs="$1"; shift
    grep -H '' "$@" 2>/dev/null | LC_ALL=C awk -v devs="$devs" '
        BEGIN { n = split(devs, d, " "); for (i = 1; i <= n; i++) wanted[d[i]] = 1 }
        {
            colon = index($0, ":")
            split(substr($0, 1, colon - 1), f, "/")
            line = substr($0, colon + 1)
            split(line, fld, " ")
            split(substr(line, index(line, " - ") + 3), fstype, " ")
            if (!(fld[3] in wanted) && fstype[1] != "btrfs") next
            m = fld[5]
            if (index(m, "\\012")) next
            gsub(/\\040/, " ", m); gsub(/\\011/, "\t", m); gsub(/\\134/, "\\", m)
            print "/proc/" f[3] "/root" m
        }
    ' || true
}

# _live_refs_ns_aliases FOREIGN RP... — FOREIGN is _live_refs_foreign_mntns_pids'
# output. Prints three lines per alias, `<ns>`, `<mount point M>` and
# `<candidate>`, for every foreign-namespace mount whose root has a candidate's
# dev:ino at an M other than that candidate's own path. Each namespace is read
# through its _live_refs_ns_representatives, the union of their aliases taken,
# and the whole set is resolved in a fixed number of processes; xargs batches
# the root stat under ARG_MAX rather than letting it fail. A namespace whose
# mountinfo or root cannot be read through any representative yields no alias:
# it is skipped, never counted as referencing. Remaining limit: a namespace
# whose every representative exits mid-scan or is chrooted away from the
# lane's mount is skipped too.
_live_refs_ns_aliases() {
    local foreign="$1"; shift
    [ -n "$foreign" ] && [ $# -gt 0 ] || return 0

    local devs
    devs="$(stat -L --printf '%Hd:%Ld ' "$@" 2>/dev/null)" || true
    [ -n "$devs" ] || return 0

    local -a mountinfos
    mapfile -t mountinfos < <(_live_refs_ns_representatives "$foreign" \
        | awk '{ print "/proc/" $1 "/mountinfo" }')
    [ ${#mountinfos[@]} -gt 0 ] || return 0

    awk '
        FILENAME == ARGV[1] {
            sp = index($0, " "); key = substr($0, 1, sp - 1)
            cand[key, ++ncand[key]] = substr($0, sp + 1); next
        }
        FILENAME == ARGV[2] { ns_of[$2] = $1; next }
        {
            sp = index($0, " "); key = substr($0, 1, sp - 1)
            if (!(key in ncand)) next
            rest = substr($0, sp + 1 + length("/proc/"))
            slash = index(rest, "/")
            pid = substr(rest, 1, slash - 1)
            m = substr(rest, slash + length("/root"))
            if (m == "") m = "/"
            for (i = 1; i <= ncand[key]; i++)
                if (cand[key, i] != m && !((ns_of[pid], m, cand[key, i]) in done)) {
                    done[ns_of[pid], m, cand[key, i]] = 1
                    print ns_of[pid] "\n" m "\n" cand[key, i]
                }
        }
    ' <(stat -L -c '%d:%i %n' "$@" 2>/dev/null) \
      <(printf '%s\n' "$foreign") \
      <(_live_refs_ns_mount_roots "$devs" "${mountinfos[@]}" \
            | xargs -d '\n' -r stat -L -c '%d:%i %n' 2>/dev/null) 2>/dev/null || true
}

# _live_refs_ns_targets PID... — the cwd and open-fd link targets and the mapped
# file pathnames of exactly the given pids, as those processes see them. cat,
# not awk, reads the maps files: mawk aborts on an unreadable input file.
_live_refs_ns_targets() {
    [ $# -gt 0 ] || return 0
    find "${@/#//proc/}" -maxdepth 2 -path '*/task' -prune -o -type l \
            \( -name cwd -o -path '*/fd/*' \) -printf '%l\n' 2>/dev/null || true

    local -a maps=("${@/#//proc/}")
    maps=("${maps[@]/%//maps}")
    cat "${maps[@]}" 2>/dev/null \
        | awk '{ sub(/^[^ ]+ +[^ ]+ +[^ ]+ +[^ ]+ +[^ ]+ */, ""); if (substr($0, 1, 1) == "/") print }' \
        || true
}

# _live_refs_translate PAIRS_FILE — PAIRS_FILE holds interleaved `M` and
# `candidate` lines. Reads target paths on stdin and prints, for each target at
# or under some M, the candidate followed by the target's remainder after M.
# Other targets are dropped: the host-namespace passes already saw them as-is.
_live_refs_translate() {
    awk '
        FILENAME == ARGV[1] { if (FNR % 2) m[++n] = $0; else c[n] = $0; next }
        {
            for (i = 1; i <= n; i++) {
                if (m[i] == "/") print ($0 == "/" ? c[i] : c[i] $0)
                else if ($0 == m[i]) print c[i]
                else if (index($0, m[i] "/") == 1) print c[i] substr($0, length(m[i]) + 1)
            }
        }
    ' "$1" - 2>/dev/null
}

# _live_refs_scan_namespaces RP... — the namespace pass of the scan: references
# held by same-user processes in another mount namespace, which reach a
# candidate through that namespace's own mount of it (canonical-build-path PRD
# decision 16). Translation is per namespace, because two namespaces may mount
# different candidates at the same path.
_live_refs_scan_namespaces() {
    [ $# -gt 0 ] || return 0

    local foreign aliases ns
    foreign="$(_live_refs_foreign_mntns_pids)"
    [ -n "$foreign" ] || return 0
    aliases="$(_live_refs_ns_aliases "$foreign" "$@")"
    [ -n "$aliases" ] || return 0

    local -a pids
    while IFS= read -r ns; do
        mapfile -t pids < <(awk -v ns="$ns" '$1 == ns { print $2 }' <<<"$foreign")
        _live_refs_ns_targets "${pids[@]}" \
            | _live_refs_translate <(awk -v ns="$ns" 'NR % 3 == 1 { keep = ($0 == ns); next } keep' <<<"$aliases") \
            | _live_refs_match_targets "$@" || true
    done < <(awk 'NR % 3 == 1 && !seen[$0]++' <<<"$aliases")
}

# live_referenced_paths RP... — given one or more resolved realpaths (trash
# dirs for the reaper, OR pool-lane / orphan-worktree dirs for the reclaim
# live-consumer gate — task 5378, task 5572), print (one per line,
# deduplicated) the subset that ANY live process references via its cwd
# symlink, an open fd symlink, or a mmap'd region in /proc/<pid>/maps.
# Performs exactly ONE /proc symlink walk, ONE /proc/*/maps grep and ONE
# namespace pass for the WHOLE candidate set — O(/proc) once, NOT once per
# entry: with N stranded entries the older per-entry scan was O(N × /proc) (a
# single /proc walk already cost ~84s on a ~1300-process host, and the ENOSPC
# strand that motivates the reaper can leave many entries at once).
#
# The verdict is read from captured OUTPUT, never a pipeline exit status:
# find/grep over /proc return non-zero on the unavoidable permission-denied
# entries of other users' processes (and SIGPIPE on an early awk exit), so
# under `set -o pipefail` the exit status is an unreliable signal. A vanishing
# /proc entry only produces suppressed stderr; every scan is `|| true`-guarded
# so it never aborts under set -e.
live_referenced_paths() {
    [ $# -gt 0 ] || return 0
    _live_refs_env_probe || true

    # Every pass, unconditionally: a BATCH caller needs the full matched subset,
    # so there is nothing to short-circuit on (unlike live_ref_present, which
    # needs only a yes/no). Duplicates across the passes are possible and
    # always were — every caller keys the output into a set.
    _live_refs_scan_fdcwd "$@"
    _live_refs_scan_maps "$@"
    _live_refs_scan_namespaces "$@"
}

# live_ref_present DIR — single-candidate predicate over the three scan passes.
# Returns 0 iff DIR itself, or anything under it, is referenced by a live
# process (cwd / open fd / mmap); 1 otherwise.
#
# SHORT-CIRCUITS on the cwd/fd pass (task 5572 review): a single yes/no verdict
# does not need the mmap pass once the cheaper-to-hit one has already answered
# "yes", which cuts a live candidate's cost from ~1.9s to ~1.5s here; the
# namespace pass runs last on the same rule. A NEGATIVE verdict still pays every
# pass — unavoidably, since "not referenced" is only established by exhausting
# every reference kind, and that negative is exactly the verdict that
# authorises the reset.
#
# DIR is resolved with `readlink -f` FIRST, and that is mandatory rather than
# defensive: /proc cwd/fd symlink targets and /proc/*/maps paths are
# kernel-canonical, so matching a literal "$WORKTREES_DIR/<name>" would
# silently never match under a symlinked mount — a guard that quietly never
# fires, i.e. the same class of failure the per-lane gate exists to remove.
#
# An unresolvable DIR returns 1 (not live): the path cannot be matched against
# kernel-canonical /proc targets at all, so there is nothing to compare. This
# mirrors the batch callers' per-entry `[ -n "$rp" ] || continue` skip.
live_ref_present() {
    local dir="${1:-}"
    [ -n "$dir" ] || return 1

    # Probed BEFORE the realpath resolution below: a /proc that cannot be read
    # also breaks `readlink -f`, and that early `return 1` is itself a fail-open
    # verdict, so the warning has to precede it or the worst case stays silent.
    _live_refs_env_probe || true

    local rp
    rp="$(readlink -f "$dir" 2>/dev/null)" || rp=""
    [ -n "$rp" ] || return 1

    local hits
    hits="$(_live_refs_scan_fdcwd "$rp")" || hits=""
    [ -n "$hits" ] && return 0

    hits="$(_live_refs_scan_maps "$rp")" || hits=""
    [ -n "$hits" ] && return 0

    hits="$(_live_refs_scan_namespaces "$rp")" || hits=""
    [ -n "$hits" ]
}
