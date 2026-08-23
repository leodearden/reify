#!/usr/bin/env bash
# scripts/tree-sitter-freshness.sh
#
# Prove that the compiled libtree_sitter_reify.a matches the tree-sitter sources
# currently on disk — and, before a build, force a rebuild when it does not.
#
# WHY THIS GUARD EXISTS  (task #5629, esc-5392-1)
# ----------------------------------------------
# Cargo re-runs a build script only for paths that script declared via
# `cargo:rerun-if-changed`.  tree-sitter-reify/build.rs declared exactly ONE:
# `grammar.js`.  But it compiles TWO C translation units —
#
#     c_config.file(&parser_path);      // src/parser.c   (generated, gitignored)
#     c_config.file("src/scanner.c");   // hand-written, TRACKED
#
# — plus the headers under src/tree_sitter/.  The `cc` crate emits no
# rerun-if-changed directives of its own (verified against cc 1.2.62: zero hits
# in its source).  So before this guard, `src/scanner.c` and every
# `src/tree_sitter/*.h` were watched by NOTHING.
#
# The consequence is a false GREEN, and it does not depend on warm lanes or on
# any mtime accident: an edit confined to src/scanner.c gave cargo no reason
# whatsoever to re-run the build script, in ANY checkout.  cc::Build::compile
# was never re-invoked, the previously-built libtree_sitter_reify.a stayed
# linked, and the external-scanner change was simply never under test — while
# the merge gate reported success.
#
# `scripts/tree-sitter-generate.sh` cannot repair this.  It refreshes on-disk
# sources (src/parser.c, grammar.json, node-types.json); it never writes
# scanner.c, and nothing it does makes cargo recompile anything.
#
# WHY NOT MTIME
# -------------
# `cargo:rerun-if-changed` is an mtime comparison, so declaring the missing
# inputs (build.rs now does) is NECESSARY but not SUFFICIENT here.
# scripts/seed-warm-lane.sh bulk-stamps every non-target/, non-.git file to
# 2020-01-01T00:00:00 while the build-script `output` files inside the
# CoW-cloned target/ carry seed-time (measured by task #5630).  Those two clocks
# disagree per-fingerprint, so "newer than" says nothing useful in a warm lane.
#
# Only CONTENT identity distinguishes "this archive was built from these bytes"
# from "this archive is merely newer".  build.rs therefore writes a per-file
# sha256 manifest to $OUT_DIR/tree_sitter_inputs.stamp immediately after
# cc::Build::compile() returns — and compile() panics on failure, so a stamp
# sitting next to a libtree_sitter_reify.a ATTESTS that archive's inputs.  This
# script recomputes the same manifest from the worktree and compares.
#
# The repair must also come from OUTSIDE the build script: a build script that
# cargo has declined to run cannot repair itself.  Hence a standalone plan leaf
# that verify.sh runs after tree-sitter-generate.sh and before every cargo leaf.
#
# MODES
# -----
#   --list-inputs   Print the C-compilation input set — one TS_DIR-relative path
#                   per line, LC_ALL=C sorted, de-duplicated.  This is the single
#                   source of truth for what the fingerprint covers; build.rs
#                   enumerates the same set so the two manifests agree byte-for-byte.
#   --print-fingerprint
#                   Print the per-file sha256 manifest for that input set —
#                   '<hash>  <relpath>' lines, sorted by relpath.  Exactly what
#                   build.rs writes to $OUT_DIR/tree_sitter_inputs.stamp.
#   check           Hard assertion, scoped to the LIVE SET (below): every dir
#                   rebuilt inside this run's window, plus the newest-marker dir.
#                   Exit 1 if any of their sibling stamps is missing or disagrees
#                   with the current fingerprint, naming the offending dir and the
#                   first differing input.  Stale DORMANT dirs are reported as
#                   informational `note:` lines and do NOT fail.  verify.sh runs
#                   this after the LAST cargo leaf that can compile the parser, to
#                   prove every archive cargo just linked is fresh; also the mode
#                   to run at a human/agent checkpoint.
#   ensure          REPAIR, scoped to the WHOLE tree.  On any stale archive — live
#                   or dormant — bump the mtime of the watched inputs (grammar.js,
#                   src/scanner.c, src/tree_sitter/*.h) so cargo is guaranteed to
#                   re-run the build script, and thus recompile, on the next
#                   invocation.  This is what verify.sh runs after
#                   tree-sitter-generate.sh and before every cargo leaf.
#   --help, -h      This message.
#
# LIVE vs DORMANT — WHY `check` AND `ensure` HAVE DIFFERENT SCOPES
# ---------------------------------------------------------------
# A checkout accumulates one build RUN dir per distinct build fingerprint
# (profile x features x RUSTFLAGS x target): 7 in this lane, 9 in the main
# checkout.  Cargo will only ever rebuild the one matching the config it is
# invoked with; all the others are DORMANT — never rebuilt, so once their
# sources move on they are stale forever, with no path back to fresh.
#
# That is why `check` cannot simply assert over every dir: it would be
# guaranteed RED in any long-lived checkout, which makes it useless as an
# assertion — a permanently-red gate is one nobody can act on.  So `check`
# hard-asserts on the LIVE SET and demotes the rest to `note:` lines.
#
# The LIVE SET is the union of two things:
#
#   1. The RUN WINDOW — every run dir whose marker advanced during this verify
#      run.  `ensure` stamps an epoch file ($TARGET_DIR/.tree-sitter-freshness.epoch)
#      before the cargo wave starts; any dir whose marker is >= that epoch is one
#      cargo actually (re-)ran the build script for during this run, so it is an
#      archive this run's binaries can have linked, and it must be attested.
#
#      This is what makes the multi-archive case safe (task #5629, review round
#      3).  A single `--profile both --scope all` plan compiles the parser into
#      SEVERAL fingerprint dirs — clippy's, the debug nextest build's, the release
#      nextest build's.  Asserting on one dir attests an archive that no test
#      binary necessarily linked, while the archives that WERE linked pass
#      unexamined: exactly the false GREEN this script exists to close, one level
#      up.  The window covers all of them.
#
#   2. The NEWEST-MARKER dir, always, as a floor.  It keeps a standalone `check`
#      (no epoch on disk — a human/agent checkpoint, or a mirrored fixture in the
#      tests) behaving exactly as it did before the window existed, and it means a
#      missing/unwritable epoch degrades to the old scope rather than to no scope.
#
# A window lasts exactly ONE ensure/check pair: `check` CONSUMES the epoch,
# unlinking it once ts_scan has read it.  Without that the file — created by
# `ensure`, previously never removed — would make "no epoch on disk" a state a
# real checkout reaches only before its first verify run.  Every later standalone
# `check` would silently inherit the LAST verify run's window, hours or days old,
# and hard-assert on dirs that run rebuilt but which have since gone dormant
# (someone flipped profile or RUSTFLAGS) — reproducing exactly the un-actionable
# permanent RED the live/dormant split exists to avoid.  Consuming it means a
# standalone `check` really is newest-marker-only, as documented, and the gate's
# own `ensure` re-opens a fresh window on every run.
#
# A dir's marker is read from cargo's OWN per-run files inside the run dir —
# `output` (its verbatim capture of build-script stdout), falling back to
# `invoked.timestamp`.  Measured on this host rather than assumed: a `cargo check`
# that re-ran the build script bumped both markers together, and the next one —
# which cargo resolved as fresh and did not re-run — bumped neither.  So a marker
# at or after the epoch means "cargo ran this build script inside the window", and
# "newest marker" names the dir cargo last executed the build script for.
#
# Dirs cargo did NOT touch this run keep their marker below the epoch and stay
# dormant, so the 7-9 forever-stale dirs a real checkout carries still cannot turn
# the gate red.
#
# `ensure` deliberately keeps the WHOLE-tree scope.  A dormant dir is only
# dormant until someone flips RUSTFLAGS or profile back, at which point cargo
# resurrects it — and it would resurrect it WITHOUT rebuilding, because the mtime
# comparison that drives rerun-if-changed can perfectly well say "fresh" for an
# archive built from different bytes (that is the whole defect this script
# exists to close).  Forcing on dormant staleness too is what keeps a resurrected
# dir honest; the ledger below is what stops it costing a recompile every run —
# and the ledger's mtime witness is what stops the ledger itself from silently
# cancelling that force after a warm-lane reseed rewinds the source mtimes.
#
# The two scopes compose into the actual gate guarantee.  `ensure` runs BEFORE
# the cargo wave and repairs anything stale; `check` runs AFTER it and asserts
# the archive cargo actually built is fresh.  Without the second leaf the gate
# only ever ATTEMPTED the repair — if the mtime force failed to trigger a
# rebuild, the run went green with no signal at all, which is the same
# false-GREEN class as the original defect, one level up (#5629 review round 2).
#
# When NO dir carries either marker, liveness is undeterminable and EVERY
# archive is treated as live.  Fail-closed: an unknown-liveness tree asserts
# over everything rather than silently asserting over nothing.
#
# WHY `touch`, AND WHY A LEDGER
# -----------------------------
# `touch` mutates mtime only, never content, so `git status` stays clean in the
# shared merge lane.  Deleting target/*/build/tree-sitter-reify-*/out/ was
# rejected: cargo keys its rebuild decision on its own fingerprint files, not on
# the archive's existence, so removing the .a yields a missing-symbol link error
# rather than a rebuild.
#
# `check` must scan EVERY fingerprint dir, because nothing in the shell can tell
# which one cargo will link.  But a checkout accumulates dead fingerprint dirs
# (7 in this lane, 9 in the main checkout) that are never rebuilt and are
# therefore stale forever.  Without a ledger, `ensure` would see a non-empty
# stale set on every single verify run and pay a full ~5 MB parser.c recompile
# each time.  target/.tree-sitter-freshness.ledger records the fingerprint the
# last force was applied for, bounding the cost to exactly one extra build-script
# run per source change, and zero when the sources are unchanged.  It is written
# only AFTER the touch, so a build that then fails still has newer mtimes.
#
# The ledger carries a WITNESS beside it (`.tree-sitter-freshness.ledger.mtime`):
# the highest watched-input mtime at the moment it was written.  A fingerprint
# alone says "a force was applied for these bytes"; it does NOT say the mtime bump
# that force consists of is still on disk — and in the exact environment this
# guard targets, it routinely is not.  scripts/seed-warm-lane.sh bulk-stamps every
# non-target/ file back to 2020-01-01 while target/ — ledger included — is
# CoW-cloned intact, so a freshly-seeded lane can inherit ledger==CURRENT_FP
# beside 2020 source mtimes and a stale DORMANT fingerprint dir.  Honouring the
# ledger there skips a force that no longer exists; if the lane's build config
# then resolves to that dir, cargo compares the 2020 mtimes against the dir's
# later recorded reference, concludes "unchanged", and links the stale archive.
# That is a residual instance of the very false GREEN this script closes, so the
# witness is checked too: a current max watched mtime BELOW the recorded one is
# the warm-lane rewind signature, and it bypasses the ledger.  Missing or
# unreadable witness also bypasses — fail closed, at a cost of one force.
#
# FAIL POLICY
# -----------
# A missing stamp beside an archive means that archive's provenance is UNPROVEN,
# which is treated as STALE — never as fresh.  Exactly two conditions fail OPEN
# for the WHOLE run, matching the reify-bin-freshness.sh / reify-audit-freshness.sh
# precedent: a host with neither sha256sum nor shasum (nothing can be attested,
# so a hard failure would be a permanent spurious RED), and a target tree with
# nothing built yet (nothing built means nothing can be stale).
#
# A third degradation exists and is deliberately SCOPED TO ONE ARCHIVE: a stamp
# whose content is the literal UNAVAILABLE — written by build.rs when it could
# not hash its own inputs — marks that ONE fingerprint dir unattestable.  It is
# reported as a labelled per-dir SKIP and excluded from the verdict; it never
# suppresses a sibling's stale verdict and never converts the run into a global
# skip.  Widening it to the run would be a false GREEN of exactly the class this
# script exists to close, and a permanent, silent one: dead fingerprint dirs are
# never rebuilt, so such a stamp is never rewritten, and it would disable the
# guard for that checkout indefinitely — then propagate, since target/ is
# CoW-cloned into every lane seeded from that base.  Nor is it an exotic case:
# build.rs's sha256_of returns None on ANY subprocess failure (fork pressure,
# EMFILE), not merely a missing binary, so one momentary resource spike during a
# single build is enough to mint the sentinel on the ordinary Linux host (#5629).
#
# `ensure` never fails the build for staleness it can REPAIR — failing the merge
# gate on a repairable condition would just convert a false GREEN into a spurious
# RED that every subsequent run reproduces.  It does exit 1 when a `touch`
# actually fails (read-only tree): detected-but-unrepairable must be loud, never
# silently green.
#
# TEST-ONLY ENVIRONMENT OVERRIDES
# -------------------------------
#   REIFY_TS_FRESHNESS_TS_DIR      Point at a copy of tree-sitter-reify/ instead
#                                  of the real one.  Exists solely so the
#                                  hermetic fixtures in
#                                  tests/infra/test_tree_sitter_pipeline.sh can
#                                  exercise the repair path without mutating the
#                                  worktree.  Never set in production.
#   REIFY_TS_FRESHNESS_TARGET_DIR  Point at a fake cargo build tree instead of
#                                  ./target.  Same rationale.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# compute_sha256 (-> portable_sha256): the sha256sum-then-shasum fallback already
# used by scripts/tree-sitter-generate.sh for its grammar.js stamp.
# shellcheck source=scripts/lib.sh
source "$SCRIPT_DIR/lib.sh"

TS_DIR="${REIFY_TS_FRESHNESS_TS_DIR:-$REPO_ROOT/tree-sitter-reify}"
TARGET_DIR="${REIFY_TS_FRESHNESS_TARGET_DIR:-$REPO_ROOT/target}"

# The manifest build.rs writes next to each libtree_sitter_reify.a it produces.
STAMP_NAME="tree_sitter_inputs.stamp"

# Records the fingerprint the last force was applied for.  Lives under target/ so
# it is gitignored and travels with the per-lane build tree (CoW-cloned from the
# warm base alongside the archives it describes).
LEDGER_NAME=".tree-sitter-freshness.ledger"

# Suffix of the ledger's companion WITNESS file, which records the highest
# watched-input mtime at the moment the ledger was written. The ledger says WHICH
# bytes a force was applied for; the witness says the bump is still on disk. See
# "WHY `touch`, AND WHY A LEDGER" in the header for why the second half is
# load-bearing in a CoW-seeded warm lane.
LEDGER_MTIME_SUFFIX=".mtime"

# Stamped by `ensure` at the START of a verify run, before any cargo leaf.  Its
# MTIME (the file's content is irrelevant) opens the RUN WINDOW: any build-script
# run dir whose marker is >= this is one cargo re-ran during this run, so `check`
# must attest it.  See "LIVE vs DORMANT" in the header.  Lives under target/
# beside the ledger, for the same gitignored/per-lane reasons.
EPOCH_NAME=".tree-sitter-freshness.epoch"

# Distinct return code meaning "no hasher on this host" — callers map it to a
# labelled SKIP rather than to a failure or to a repair attempt.
TS_RC_UNAVAILABLE=3

usage() {
    cat <<'EOF'
Usage: scripts/tree-sitter-freshness.sh <mode>

Modes:
  --list-inputs        Print the C-compilation input set (one TS_DIR-relative
                       path per line, LC_ALL=C sorted, de-duplicated).
  --print-fingerprint  Print the per-file sha256 manifest for that input set
                       ('<hash>  <relpath>' lines, sorted by relpath) — exactly
                       what build.rs writes to $OUT_DIR/tree_sitter_inputs.stamp.
  check                Assert the LIVE libtree_sitter_reify.a archives — every
                       one cargo re-ran the build script for since 'ensure'
                       stamped this run's epoch, plus the newest-marker dir —
                       were compiled from the sources currently on disk. Exit 1
                       on a mismatch or missing attestation. Stale dormant
                       fingerprint dirs are reported as 'note:' lines and do not
                       fail.
  ensure               REPAIR, over the WHOLE tree: on any stale archive, live or
                       dormant, bump the mtime of the watched inputs so cargo
                       re-runs the build script (and thus recompiles) next
                       invocation. Exit 1 only if the repair itself fails.
  --help, -h           Show this message.
EOF
}

# ts_inputs
#
# THE single source of truth for the C-compilation input set: every file whose
# bytes end up inside libtree_sitter_reify.a.  Emits TS_DIR-relative paths so
# the manifest is location-independent and reproducible by build.rs.
#
# Headers come from a SORTED GLOB rather than a hardcoded alloc.h/array.h/parser.h
# list, and build.rs enumerates them the same way, so a future header is covered
# automatically by both sides.  A hardcoded list is exactly how this defect class
# recurs.
ts_inputs() {
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob

    local f
    local -a headers=()
    for f in "$TS_DIR"/src/tree_sitter/*.h; do
        headers+=("src/tree_sitter/${f##*/}")
    done

    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob

    {
        # The two translation units build.rs hands to cc::Build.  src/parser.c is
        # generated and gitignored, but its bytes are compiled in, so it belongs
        # in the fingerprint — even though it is deliberately NOT watched (see
        # build.rs: watching an output this script writes causes double execution).
        printf 'src/parser.c\n'
        printf 'src/scanner.c\n'
        if [ "${#headers[@]}" -gt 0 ]; then
            printf '%s\n' "${headers[@]}" | LC_ALL=C sort
        fi
    } | awk 'NF && !seen[$0]++'
}

# ts_hash_file
#
# Print the bare sha256 of ONE existing file, or return 1 having printed nothing.
#
# This is the shell counterpart of build.rs's `sha256_of`, and it exists for the
# same reason: the hasher can be present on PATH and still fail on a given call —
# an unreadable mode, or a transient fork/EMFILE spike spawning the subprocess
# under the parallel-cargo load this host routinely runs at (the same trigger
# already documented for `sha256_of` in FAIL POLICY above).  A bare
# `hash=$(compute_sha256 "$f" | awk '{print $1}')` cannot see that: command
# substitution reports only awk's status, awk happily succeeds on empty input,
# and `set -e` is disabled inside this function anyway because BOTH call sites
# invoke it in a `||` list.  The result was an EMPTY hash field in the manifest
# and a 0 exit — which `check` renders as a spurious STALE naming a file that is
# in fact unchanged, and which `ensure` writes verbatim into the ledger, so every
# later run mismatches it and pays another full parser.c force.
#
# So: retry a present-but-failing hasher on the same 3-attempt / 100ms-linear-
# backoff ladder build.rs uses, then concede.  Conceding is `return 1`, never an
# empty hash: an empty field is indistinguishable from a real mismatch, and this
# script's whole job is to keep "changed" and "could not tell" apart.
ts_hash_file() {
    local abs="$1"
    local attempt raw hash

    for attempt in 1 2 3; do
        # stderr is suppressed per-attempt so a retry that later SUCCEEDS stays
        # quiet; the caller emits the single authoritative diagnostic on failure.
        if raw=$(compute_sha256 "$abs" 2>/dev/null); then
            hash=$(printf '%s\n' "$raw" | awk '{print $1}')
            if [ -n "$hash" ]; then
                printf '%s\n' "$hash"
                return 0
            fi
        fi
        # Plain `if`, not `[ ... ] && sleep`: as the LAST statement in the loop
        # body the short-circuit form evaluates to 1 on the final attempt, which
        # would make the function's exit status depend on arithmetic rather than
        # on the explicit `return 1` below.
        if [ "$attempt" -lt 3 ]; then
            sleep "0.$attempt"
        fi
    done

    return 1
}

# ts_fingerprint
#
# Print the per-file sha256 manifest for the input set: '<hash>  <relpath>'
# lines in ts_inputs() order (already LC_ALL=C sorted by relpath).  The relpath
# column is normalised to TS_DIR-relative, so the manifest is location-
# independent and byte-reproducible by build.rs from a different cwd.
#
# Returns $TS_RC_UNAVAILABLE (printing the single literal line UNAVAILABLE) when
# the host has no hasher; returns 1 naming the path when an input is missing, or
# when an input exists but could not be hashed (see ts_hash_file).
ts_fingerprint() {
    if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
        printf 'UNAVAILABLE\n'
        return "$TS_RC_UNAVAILABLE"
    fi

    local rel abs hash
    while IFS= read -r rel; do
        [ -n "$rel" ] || continue
        abs="$TS_DIR/$rel"
        if [ ! -f "$abs" ]; then
            echo "ERROR: tree-sitter freshness input is missing: $abs" >&2
            echo "       Run scripts/tree-sitter-generate.sh first — every compiled input," >&2
            echo "       including the generated src/parser.c, must exist on disk before it" >&2
            echo "       can be fingerprinted." >&2
            return 1
        fi
        # ts_hash_file -> bare '<hash>'; pair it with the normalised relpath.
        # A failure here is NOT degradable: unlike a missing hasher (which is
        # global, and fails OPEN as a whole-run skip), one unhashable file leaves
        # the rest of the manifest perfectly valid-LOOKING, so emitting it would
        # assert a comparison this script cannot actually make.
        if ! hash=$(ts_hash_file "$abs"); then
            echo "ERROR: tree-sitter freshness input could not be hashed: $abs" >&2
            echo "       sha256sum/shasum is on PATH but failed on this file after 3" >&2
            echo "       attempts — check its permissions, or retry if the host was" >&2
            echo "       under fork/EMFILE pressure.  Refusing to emit a partial" >&2
            echo "       fingerprint: an empty hash field is indistinguishable from a" >&2
            echo "       real content change and would red the gate (or poison the" >&2
            echo "       ledger) for a file that has not changed." >&2
            return 1
        fi
        printf '%s  %s\n' "$hash" "$rel"
    done < <(ts_inputs)
}

# ts_archives
#
# Print every built libtree_sitter_reify.a under TARGET_DIR, de-duplicated.
# Both the plain (target/<profile>/build/...) and the --target-qualified
# (target/<triple>/<profile>/build/...) layouts are covered: there is no cheap
# way to tell from the shell WHICH fingerprint dir cargo will actually link, so
# all of them must be attested.
ts_archives() {
    local had_nullglob=0
    shopt -q nullglob && had_nullglob=1
    shopt -s nullglob

    local a
    local -a found=()
    for a in "$TARGET_DIR"/*/build/tree-sitter-reify-*/out/libtree_sitter_reify.a \
             "$TARGET_DIR"/*/*/build/tree-sitter-reify-*/out/libtree_sitter_reify.a; do
        found+=("$a")
    done

    [ "$had_nullglob" -eq 1 ] || shopt -u nullglob

    if [ "${#found[@]}" -gt 0 ]; then
        printf '%s\n' "${found[@]}" | LC_ALL=C sort -u
    fi
}

# ts_run_dir_marker <run-dir>
#
# Epoch mtime of cargo's own per-run marker for a build-script RUN dir:
# `output` if present, else `invoked.timestamp`.  Prints 0 when neither exists
# (a dir cargo has never executed the build script in, or a test fixture).
#
# `stat -c %Y` is GNU; `stat -f %m` is BSD/macOS.  Both are tried before giving
# up, matching the portability posture of compute_sha256 in lib.sh.
ts_run_dir_marker() {
    local run_dir="$1" f m
    for f in "$run_dir/output" "$run_dir/invoked.timestamp"; do
        [ -f "$f" ] || continue
        m=$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f" 2>/dev/null || echo 0)
        case "$m" in
            '' | *[!0-9]*) continue ;;
            *) printf '%s\n' "$m"; return 0 ;;
        esac
    done
    printf '0\n'
}

# ts_live_run_dir
#
# Print the ABSOLUTE path of the live build-script run dir — the one carrying the
# newest marker among the dirs that actually hold an archive.  Prints nothing
# when no candidate carries a marker at all, which callers read as "liveness
# undeterminable, treat every archive as live" (see the header).
#
# Ties are broken by LC_ALL=C path order so the answer is deterministic: two run
# dirs written inside the same filesystem-timestamp granularity must not make the
# verdict depend on readdir order.
ts_live_run_dir() {
    local a run_dir m best="" best_m=0
    while IFS= read -r a; do
        [ -n "$a" ] || continue
        run_dir="$(dirname "$(dirname "$a")")"
        m=$(ts_run_dir_marker "$run_dir")
        [ "$m" -gt 0 ] || continue
        if [ "$m" -gt "$best_m" ] || { [ "$m" -eq "$best_m" ] && [ "$run_dir" \> "$best" ]; }; then
            best_m="$m"
            best="$run_dir"
        fi
    done < <(ts_archives)
    printf '%s' "$best"
}

# ts_epoch_mtime
#
# Print the RUN WINDOW epoch as an integer mtime, or nothing when no epoch file
# exists (or its mtime cannot be read).  Empty means "no window": callers fall
# back to newest-marker-only scope, which is the pre-window behaviour.
#
# A window is opened by `ensure` and CONSUMED by `check` (ts_consume_epoch), so
# "no epoch on disk" is the normal state between verify runs — which is what makes
# a standalone `check` genuinely newest-marker-only rather than a re-run of the
# last gate's window.  A mirrored test fixture and an unwritable target tree reach
# the same state by never having had one.
ts_epoch_mtime() {
    local f="$TARGET_DIR/$EPOCH_NAME" m
    [ -f "$f" ] || return 0
    m=$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f" 2>/dev/null || echo '')
    case "$m" in
        '' | *[!0-9]*) return 0 ;;
        *) printf '%s' "$m" ;;
    esac
}

# ts_write_epoch
#
# Open a fresh RUN WINDOW.  Best-effort by design: on a read-only tree this fails
# and `check` degrades to newest-marker-only scope — the pre-window behaviour,
# i.e. a NARROWER assertion, never a wider or a silently-skipped one.  It is not
# worth failing `ensure` (and so the whole gate) over, but it is worth saying out
# loud, because it silently shrinks what the later `check` will attest.
ts_write_epoch() {
    local f="$TARGET_DIR/$EPOCH_NAME"
    mkdir -p "$TARGET_DIR" 2>/dev/null || true
    if ! : > "$f" 2>/dev/null; then
        echo "tree-sitter-freshness: note: could not stamp the run-window epoch ($f);"
        echo "    a later 'check' will attest only the newest fingerprint dir, not every"
        echo "    dir rebuilt during this run."
        return 0
    fi
}

# ts_consume_epoch
#
# Close the RUN WINDOW: unlink the epoch once `check` has read it, so it lasts
# exactly one ensure/check pair.
#
# Why this is not just tidiness: the epoch was created by `ensure` and, before
# this, never removed.  A real checkout therefore reached "no epoch on disk" only
# before its FIRST verify run, and every later standalone `check` silently reused
# the last run's window — hours or days old — hard-asserting on dirs that run
# rebuilt but which have since gone dormant.  That is the permanent, un-actionable
# RED the live/dormant split exists to prevent, arrived at from the other side.
#
# The narrowing is disclosed rather than silent: a reader who re-runs `check`
# after a failure must know the second run asserts on less, and that `ensure` is
# what re-opens the window.  An unlink that fails is only a note — the fallback is
# the pre-existing (over-wide, fail-closed) behaviour, not a missed assertion.
ts_consume_epoch() {
    local f="$TARGET_DIR/$EPOCH_NAME"
    [ -f "$f" ] || return 0
    if rm -f "$f" 2>/dev/null; then
        echo "tree-sitter-freshness: run window consumed; a repeat 'check' with no new"
        echo "    'ensure' asserts at newest-fingerprint-dir scope only."
    else
        echo "tree-sitter-freshness: note: could not remove the run-window epoch ($f);"
        echo "    a later standalone 'check' will inherit THIS run's window."
    fi
}

# ts_in_run_window <run-dir> <epoch>
#
# True when cargo (re-)ran this dir's build script at or after <epoch> — i.e. the
# archive in it is one THIS run produced and could have linked.  False when there
# is no epoch, or the dir carries no readable marker at all (absence of evidence
# never promotes a dir into the failing set here; the newest-marker floor in
# ts_scan is what covers the no-marker-anywhere case).
ts_in_run_window() {
    local run_dir="$1" epoch="$2" m
    [ -n "$epoch" ] || return 1
    m=$(ts_run_dir_marker "$run_dir")
    [ "$m" -gt 0 ] || return 1
    [ "$m" -ge "$epoch" ]
}

# ts_first_differing_input <stamp-file>
#
# Name the first input whose attested hash differs from the current one, so a
# stale verdict points at the actual culprit file rather than just at a hex
# fingerprint dir. Compares against $CURRENT_FP.
ts_first_differing_input() {
    local stamp_file="$1"
    local -A attested=()
    local line h p

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        h="${line%% *}"
        p="${line##*  }"
        attested["$p"]="$h"
    done < "$stamp_file"

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        h="${line%% *}"
        p="${line##*  }"
        if [ -z "${attested[$p]:-}" ]; then
            printf '%s (not present in the attested manifest)' "$p"
            return 0
        fi
        if [ "${attested[$p]}" != "$h" ]; then
            printf '%s' "$p"
            return 0
        fi
    done <<< "$CURRENT_FP"

    printf '(input set shrank: the attested manifest lists paths that are no longer compiled)'
}

# ts_scan
#
# The shared freshness computation behind both `check` and `ensure`.
# Sets, in the caller's shell:
#   CURRENT_FP              the current fingerprint manifest
#   TS_SCAN_STATUS          fresh | stale | dormant | skip | none
#                           `stale`   the LIVE archive was built from other bytes
#                           `dormant` only non-live archives are stale — `check`
#                                     reports these and exits 0; `ensure` still
#                                     repairs them (see the header on scopes)
#   TS_SCAN_SKIP_REASON     set when status=skip
#   TS_LIVE_DIR             basename of the live run dir, or the empty string when
#                           liveness is undeterminable (then EVERY archive is
#                           treated as live — fail-closed)
#   TS_STALE_REPORT         newline-joined, one actionable line per stale LIVE
#                           archive
#   TS_DORMANT_REPORT       newline-joined, one line per stale DORMANT archive
#   TS_DORMANT_COUNT        how many, so `check` can say so without failing
#   TS_UNATTESTABLE_REPORT  newline-joined, one line per archive whose stamp says
#                           UNAVAILABLE; empty when there are none.  A CAVEAT,
#                           never a verdict — see FAIL POLICY.  `status` is
#                           computed from TS_STALE_REPORT alone, so an
#                           unattestable dir can neither mask a stale sibling nor
#                           suppress the repair.
#   TS_UNATTESTABLE_COUNT   how many, so a `fresh` verdict can say PARTIAL rather
#                           than claim every archive was checked.
#
# `skip` means only what the header says it means: the CURRENT host has no
# hasher, so nothing at all can be attested.  Every archive is examined
# regardless of what its neighbours' stamps say.
ts_scan() {
    TS_SCAN_STATUS=""
    TS_SCAN_SKIP_REASON=""
    TS_LIVE_DIR=""
    TS_STALE_REPORT=""
    TS_DORMANT_REPORT=""
    TS_DORMANT_COUNT=0
    TS_UNATTESTABLE_REPORT=""
    TS_UNATTESTABLE_COUNT=0
    # How many archives this scan ASSERTED on (the live set: in-window dirs plus
    # the newest-marker dir). Reset here with the rest, and initialized before any
    # use — the script runs under `set -u`, so an unset counter would abort the
    # scan on the very first archive.
    TS_RUN_WINDOW_COUNT=0

    local rc=0
    CURRENT_FP=$(ts_fingerprint) || rc=$?
    if [ "$rc" -eq "$TS_RC_UNAVAILABLE" ]; then
        TS_SCAN_STATUS="skip"
        TS_SCAN_SKIP_REASON="neither sha256sum nor shasum on PATH — archive inputs cannot be attested"
        return 0
    elif [ "$rc" -ne 0 ]; then
        return "$rc"
    fi

    local archives
    archives=$(ts_archives)
    if [ -z "$archives" ]; then
        TS_SCAN_STATUS="none"
        return 0
    fi

    # Empty when no run dir carries a cargo marker: liveness is undeterminable,
    # so the `is_live` test below is true for every archive (fail-closed).
    local live_dir
    live_dir=$(ts_live_run_dir)
    [ -n "$live_dir" ] && TS_LIVE_DIR="$(basename "$live_dir")"

    # The RUN WINDOW opened by `ensure` earlier in this verify run. Empty when no
    # epoch is on disk, in which case the live set collapses to the newest-marker
    # dir alone — the pre-window scope. That is the normal state OUTSIDE a verify
    # run, because `check` consumes the epoch it reads (ts_consume_epoch): a
    # standalone `check` at a human/agent checkpoint, a mirrored test fixture and
    # an unwritable target tree all land here.
    local epoch
    epoch=$(ts_epoch_mtime)

    local a run_dir dir stamp attested detail is_live
    local -a stale=()
    local -a dormant=()
    local -a unattestable=()
    while IFS= read -r a; do
        [ -n "$a" ] || continue
        # .../build/tree-sitter-reify-<fingerprint>/out/libtree_sitter_reify.a
        run_dir="$(dirname "$(dirname "$a")")"
        dir="$(basename "$run_dir")"
        stamp="$(dirname "$a")/$STAMP_NAME"

        # Live when cargo re-ran this dir's build script inside THIS run's window,
        # or when it IS the newest-marker dir, or when liveness could not be
        # determined at all (then nothing may be demoted). The window arm is what
        # makes a multi-archive plan — clippy's dir, the debug nextest dir, the
        # release nextest dir — assert over every archive this run actually built,
        # rather than over whichever one happens to be newest when `check` runs.
        #
        # Decided BEFORE freshness, and counted here rather than at the
        # stale/dormant split below, so the reported count is "dirs this run
        # asserted on" — not "dirs that happened to fail".
        is_live=0
        if [ -z "$live_dir" ] || [ "$run_dir" = "$live_dir" ] \
            || ts_in_run_window "$run_dir" "$epoch"; then
            is_live=1
            TS_RUN_WINDOW_COUNT=$(( TS_RUN_WINDOW_COUNT + 1 ))
        fi

        detail=""
        if [ ! -f "$stamp" ]; then
            detail="$dir — no $STAMP_NAME beside the archive; its provenance is UNPROVEN"
        else
            attested=$(cat "$stamp")
            if [ "$attested" = "UNAVAILABLE" ]; then
                # Scoped to THIS dir: record it and keep going.  Aborting the scan
                # here would discard every stale archive already found AND skip
                # every one not yet reached — see FAIL POLICY (#5629).
                unattestable+=("SKIP $dir — built where sha256sum/shasum was unavailable or failed; its inputs cannot be attested")
                continue
            fi
            if [ "$attested" != "$CURRENT_FP" ]; then
                detail="$dir — built from different bytes; first differing input: $(ts_first_differing_input "$stamp")"
            fi
        fi
        [ -n "$detail" ] || continue

        # Reuses the is_live decision made above rather than re-deriving it: the
        # membership test and the count must never be able to disagree, and
        # ts_in_run_window stats the run dir, so recomputing also re-pays that.
        if [ "$is_live" -eq 1 ]; then
            stale+=("$detail")
        else
            dormant+=("$detail")
        fi
    done <<< "$archives"

    if [ "${#unattestable[@]}" -gt 0 ]; then
        TS_UNATTESTABLE_REPORT=$(printf '%s\n' "${unattestable[@]}")
        TS_UNATTESTABLE_COUNT="${#unattestable[@]}"
    fi
    if [ "${#dormant[@]}" -gt 0 ]; then
        TS_DORMANT_REPORT=$(printf '%s\n' "${dormant[@]}")
        TS_DORMANT_COUNT="${#dormant[@]}"
    fi

    # The FAILING verdict rides on stale[] — the live archive — alone.
    # dormant[] downgrades to its own non-failing status so `ensure` can still
    # see it and repair; unattestable[] is reported alongside but never decides.
    if [ "${#stale[@]}" -gt 0 ]; then
        TS_SCAN_STATUS="stale"
        TS_STALE_REPORT=$(printf '%s\n' "${stale[@]}")
    elif [ "${#dormant[@]}" -gt 0 ]; then
        TS_SCAN_STATUS="dormant"
    else
        TS_SCAN_STATUS="fresh"
    fi
}

# ts_stale_lines — the indented per-archive detail, on stdout. Callers redirect.
ts_stale_lines() {
    local line
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        echo "    $line"
    done <<< "$TS_STALE_REPORT"
}

# ts_dormant_lines — the stale-but-not-live set, on stdout, as `note:` lines.
#
# Never on stderr and never fatal: these dirs are genuinely stale, but cargo will
# not link them and will never rebuild them, so there is no action a reader can
# take. Reported rather than swallowed so a green `check` still discloses exactly
# what it declined to assert on.
ts_dormant_lines() {
    [ -n "$TS_DORMANT_REPORT" ] || return 0
    echo "tree-sitter-freshness: note: $TS_DORMANT_COUNT dormant fingerprint dir(s) are stale."
    echo "    Cargo neither links nor rebuilds these, so they cannot affect this build;"
    echo "    'ensure' still forces for them once (ledger-bounded). Not asserted on:"
    local line
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        echo "    $line"
    done <<< "$TS_DORMANT_REPORT"
}

# ts_live_label — how the live dir is named in output, for both modes.
ts_live_label() {
    if [ -z "$TS_LIVE_DIR" ]; then
        printf 'liveness undeterminable (no cargo run marker) — asserting on EVERY archive'
        return 0
    fi
    # Name the run window when one is open, not just the newest dir. Without this
    # a verdict that fails on an IN-WINDOW dir still printed only "live
    # fingerprint dir: <newest>" — pointing the reader at a dir that is not the
    # one that failed, and hiding why a non-newest dir was asserted on at all.
    if [ "$TS_RUN_WINDOW_COUNT" -gt 1 ]; then
        printf 'newest fingerprint dir: %s; asserting on %s dir(s) rebuilt during this run' \
            "$TS_LIVE_DIR" "$TS_RUN_WINDOW_COUNT"
    else
        printf 'live fingerprint dir: %s' "$TS_LIVE_DIR"
    fi
}

# ts_unattestable_lines — mirrors ts_stale_lines for the unattestable set.
#
# Always on stdout, in EVERY branch that has one: an unattestable dir is a
# caveat, not a failure, so it must not land on stderr next to a stale block —
# but it must not be swallowed either, or a fresh verdict would silently claim
# to have checked an archive it could not.  Each line carries the literal token
# SKIP and names its fingerprint dir.
ts_unattestable_lines() {
    [ -n "$TS_UNATTESTABLE_REPORT" ] || return 0
    echo "tree-sitter-freshness: these archives could not be attested and were skipped:"
    local line
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        echo "    $line"
    done <<< "$TS_UNATTESTABLE_REPORT"
}

# ts_watched_inputs
#
# The subset of the compilation inputs that build.rs declares via
# cargo:rerun-if-changed — i.e. exactly what `ensure` is permitted to touch.
# Derived FROM ts_inputs() (rather than restated) so it cannot drift, plus
# grammar.js, which is an input to generation rather than to compilation.
#
# src/parser.c is excluded on purpose: build.rs WRITES it, so watching it would
# cause double execution, and touching an unwatched file repairs nothing.
ts_watched_inputs() {
    printf 'grammar.js\n'
    local rel
    while IFS= read -r rel; do
        if [ "$rel" != "src/parser.c" ]; then
            printf '%s\n' "$rel"
        fi
    done < <(ts_inputs)
}

# ts_max_watched_mtime
#
# Highest mtime, in epoch seconds, across the inputs `ensure` is allowed to touch.
# Prints 0 when none of them can be stat'ed.  Derived from ts_watched_inputs() so
# it measures exactly the set a force bumps — nothing else can drift into it.
ts_max_watched_mtime() {
    local rel abs m best=0
    while IFS= read -r rel; do
        [ -n "$rel" ] || continue
        abs="$TS_DIR/$rel"
        [ -f "$abs" ] || continue
        m=$(stat -c %Y "$abs" 2>/dev/null || stat -f %m "$abs" 2>/dev/null || echo 0)
        case "$m" in
            '' | *[!0-9]*) continue ;;
        esac
        if [ "$m" -gt "$best" ]; then
            best="$m"
        fi
    done < <(ts_watched_inputs)
    printf '%s' "$best"
}

# ts_ledger_force_in_effect <witness-path>
#
# True when the mtime bump the ledger stands for is STILL ON DISK.
#
# The ledger records which bytes a force was applied for. That is only half the
# fact: a force IS an mtime bump, and a bump can be undone underneath the ledger.
# scripts/seed-warm-lane.sh does exactly that — it bulk-stamps every non-target/
# file back to 2020-01-01 while target/ (ledger included) is CoW-cloned intact —
# so a seeded lane can inherit ledger==CURRENT_FP beside 2020 source mtimes and a
# stale dormant fingerprint dir. Skipping the force there leaves cargo free to
# resurrect that dir, compare the 2020 mtimes against its later recorded
# reference, call it unchanged, and link the stale archive.
#
# The witness is the max watched-input mtime at ledger-write time, so a current
# max BELOW it is precisely the rewind signature. Fail closed: a missing or
# unreadable witness (an older ledger, a partially-cloned tree) also returns
# false, costing one force rather than one silent false GREEN.
ts_ledger_force_in_effect() {
    local witness="$1" recorded now
    [ -f "$witness" ] || return 1
    recorded=$(cat "$witness" 2>/dev/null) || return 1
    case "$recorded" in
        '' | *[!0-9]*) return 1 ;;
    esac
    now=$(ts_max_watched_mtime)
    [ "$now" -ge "$recorded" ]
}

# ts_write_ledger <ledger-path> — record CURRENT_FP, plus the mtime witness that
# says the force it stands for is on disk. Never fatal.
#
# Both are written at the same instant, from the same call site, and the witness
# is sampled AFTER ts_force_touch has run — so it records the state the force
# created, not the state it replaced.
ts_write_ledger() {
    local ledger="$1"
    local witness="$ledger$LEDGER_MTIME_SUFFIX"
    mkdir -p "$(dirname "$ledger")" 2>/dev/null || true
    if ! printf '%s\n' "$CURRENT_FP" > "$ledger" 2>/dev/null; then
        echo "warning: could not write the freshness ledger at $ledger" >&2
        return 0
    fi
    if ! printf '%s\n' "$(ts_max_watched_mtime)" > "$witness" 2>/dev/null; then
        # A ledger with no witness is bypassed on the next run (fail-closed), so
        # this costs one extra force, never a missed one.
        echo "warning: could not write the freshness ledger mtime witness at $witness" >&2
    fi
}

# ts_force_touch — bump the watched inputs' mtimes. Returns 1 if any touch failed.
ts_force_touch() {
    local rel abs failed=0
    while IFS= read -r rel; do
        [ -n "$rel" ] || continue
        abs="$TS_DIR/$rel"
        if [ ! -f "$abs" ]; then
            echo "ERROR: cannot force a rebuild — watched input is missing: $abs" >&2
            failed=1
            continue
        fi
        if touch "$abs" 2>/dev/null; then
            echo "    touched $rel"
        else
            echo "ERROR: cannot force a rebuild — 'touch' failed on $abs (read-only tree?)" >&2
            failed=1
        fi
    done < <(ts_watched_inputs)
    return "$failed"
}

ts_mode_check() {
    echo "tree-sitter-freshness: check — attesting libtree_sitter_reify.a against $TS_DIR"
    ts_scan
    # Close the window as soon as ts_scan has read it, in EVERY branch below: a
    # window describes what one verify run rebuilt, and leaving it on disk turns
    # every later standalone check into a replay of that run at ever-staler scope.
    ts_consume_epoch
    case "$TS_SCAN_STATUS" in
        skip)
            echo "tree-sitter-freshness: SKIP — $TS_SCAN_SKIP_REASON"
            return 0
            ;;
        none)
            echo "tree-sitter-freshness: no built archive under $TARGET_DIR; nothing to check"
            return 0
            ;;
        fresh | dormant)
            ts_unattestable_lines
            ts_dormant_lines
            if [ "$TS_UNATTESTABLE_COUNT" -gt 0 ]; then
                # Not "every archive matches": some were never established either
                # way. Still exit 0 — unproven is not stale.
                echo "tree-sitter-freshness: PARTIAL — $TS_UNATTESTABLE_COUNT archive(s) unattestable and skipped; the rest match the sources on disk ($(ts_live_label))"
            else
                echo "tree-sitter-freshness: FRESH — the archive cargo built matches the sources on disk ($(ts_live_label))"
            fi
            return 0
            ;;
        stale)
            ts_unattestable_lines
            ts_dormant_lines
            {
                echo "tree-sitter-freshness: STALE — the archive cargo will link was NOT built from the sources now on disk:"
                ts_stale_lines
                echo "    ($(ts_live_label))"
                echo "    This is the false-GREEN case: the gate would verify a parser it never compiled."
                echo "    Run './scripts/tree-sitter-freshness.sh ensure' then rebuild."
            } >&2
            return 1
            ;;
        *)
            echo "ERROR: internal — unexpected scan status '$TS_SCAN_STATUS'" >&2
            return 1
            ;;
    esac
}

ts_mode_ensure() {
    echo "tree-sitter-freshness: ensure — attesting libtree_sitter_reify.a against $TS_DIR"

    # Open the RUN WINDOW first, before anything else can touch target/. verify.sh
    # runs `ensure` after tree-sitter-generate.sh and before every cargo leaf, so
    # stamping here means every build-script run the upcoming cargo wave performs
    # lands at or after the epoch — and the `check` leaf at the end of the plan
    # therefore attests EVERY archive this run built, not just the newest one.
    #
    # Stamped unconditionally, ahead of ts_scan and ahead of every early return:
    # a window that exists only on the repair path would leave the common
    # already-fresh run asserting at the old newest-marker-only scope.
    #
    # It cannot widen ensure's OWN scan below: every marker already on disk
    # predates a file created a moment ago, so the window is empty for this scan
    # and the live/dormant split ensure reasons about is unchanged.
    ts_write_epoch

    ts_scan

    local ledger="$TARGET_DIR/$LEDGER_NAME"

    case "$TS_SCAN_STATUS" in
        skip)
            # Nothing can be attested here, so nothing can be repaired either.
            # Forcing anyway would recompile parser.c on every run, forever.
            echo "tree-sitter-freshness: SKIP — $TS_SCAN_SKIP_REASON"
            return 0
            ;;
        none)
            echo "tree-sitter-freshness: no built archive under $TARGET_DIR; nothing to force"
            ts_write_ledger "$ledger"
            return 0
            ;;
        fresh)
            ts_unattestable_lines
            if [ "$TS_UNATTESTABLE_COUNT" -gt 0 ]; then
                echo "tree-sitter-freshness: PARTIAL — $TS_UNATTESTABLE_COUNT archive(s) unattestable and skipped; the rest match the sources on disk"
            else
                echo "tree-sitter-freshness: FRESH — every built archive matches the sources on disk"
            fi
            ts_write_ledger "$ledger"
            return 0
            ;;
        stale | dormant) ;;
        *)
            echo "ERROR: internal — unexpected scan status '$TS_SCAN_STATUS'" >&2
            return 1
            ;;
    esac

    # Stale, possibly alongside unattestable dirs. Report those first: they are
    # neither repaired nor counted against the verdict, so a reader who sees a
    # force happen must still be told which archives it could not vouch for.
    ts_unattestable_lines

    # Was a force already applied for exactly these bytes? If so, and the only
    # staleness left is DORMANT, those are dirs cargo will never rebuild — the
    # force cannot clear them, and re-applying it would buy a full parser.c
    # recompile on every verify run for no gain.
    #
    # The ledger is deliberately BYPASSED when a stale LIVE archive is present.
    # That archive is the one cargo will actually link, so it must be re-forced
    # on every run until it comes back fresh; letting the ledger silence it would
    # recreate the false GREEN this script exists to close — and it is exactly
    # the state a force that failed to take leaves behind.
    #
    # The bypass requires POSITIVE evidence, i.e. a determinable live dir. When
    # no run dir carries a cargo marker the scan treats every archive as live
    # (fail-closed for `check`), but that is an absence of information, not
    # evidence that a force failed — so here the ledger still applies and the
    # pre-liveness force-once-per-source-state behaviour is preserved.
    #
    # The ledger ALSO requires its mtime witness to still hold. A matching
    # fingerprint proves a force was applied for these bytes; it does not prove
    # the mtime bump survived, and a warm-lane reseed rewinds source mtimes to
    # 2020 while CoW-cloning the ledger intact. Honouring a cancelled force there
    # is a residual false GREEN of exactly the class this script closes, so a
    # rewind (or a missing witness) bypasses the short-circuit — see
    # ts_ledger_force_in_effect.
    if { [ "$TS_SCAN_STATUS" = "dormant" ] || [ -z "$TS_LIVE_DIR" ]; } \
        && [ -f "$ledger" ] && [ "$(cat "$ledger")" = "$CURRENT_FP" ] \
        && ts_ledger_force_in_effect "$ledger$LEDGER_MTIME_SUFFIX"; then
        ts_stale_lines
        ts_dormant_lines
        echo "tree-sitter-freshness: the force was already applied for these exact inputs;"
        echo "    leaving mtimes alone (ledger: $ledger)."
        echo "    Dormant fingerprint dirs are never rebuilt, so they stay stale forever;"
        echo "    without this guard every verify run would pay a full parser.c recompile."
        return 0
    fi

    # Name the rewind explicitly when it is what put us here: "the ledger already
    # matched, yet we are forcing again" is otherwise indistinguishable from a bug
    # in the short-circuit above.
    if [ -f "$ledger" ] && [ "$(cat "$ledger")" = "$CURRENT_FP" ] \
        && ! ts_ledger_force_in_effect "$ledger$LEDGER_MTIME_SUFFIX"; then
        echo "tree-sitter-freshness: the ledger matches these inputs, but the mtime bump it"
        echo "    stands for is no longer on disk (watched-input mtimes were rewound beneath"
        echo "    it — the warm-lane reseed signature, or a missing witness file). Re-forcing:"
        echo "    a ledger entry whose force has been undone would otherwise cancel the repair."
    fi

    echo "tree-sitter-freshness: forcing rebuild — a built archive does not match the sources on disk:"
    ts_stale_lines
    ts_dormant_lines
    echo "tree-sitter-freshness: bumping the mtime (content untouched) of every input build.rs watches,"
    echo "    so cargo must re-run the build script — and recompile — on the next invocation:"

    local force_rc=0
    ts_force_touch || force_rc=$?
    if [ "$force_rc" -ne 0 ]; then
        echo "tree-sitter-freshness: DETECTED BUT UNREPAIRABLE — the stale archive stands." >&2
        echo "    A gate that silently continued here would report GREEN against a parser it" >&2
        echo "    never compiled. Fix the tree, then re-run." >&2
        return 1
    fi

    ts_write_ledger "$ledger"
    return 0
}

main() {
    local mode="${1:---help}"
    case "$mode" in
        --list-inputs)
            ts_inputs
            ;;
        --print-fingerprint)
            # An unhashable host still exits 0: it has successfully reported the
            # degradation, and the callers map the UNAVAILABLE line to a SKIP.
            local fp_rc=0
            ts_fingerprint || fp_rc=$?
            [ "$fp_rc" -eq "$TS_RC_UNAVAILABLE" ] && return 0
            return "$fp_rc"
            ;;
        check)
            ts_mode_check
            ;;
        ensure)
            ts_mode_ensure
            ;;
        --help | -h)
            usage
            ;;
        *)
            echo "ERROR: unknown mode: $mode" >&2
            usage >&2
            return 2
            ;;
    esac
}

main "$@"
