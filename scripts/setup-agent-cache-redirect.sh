#!/usr/bin/env bash
# scripts/setup-agent-cache-redirect.sh — Provision the /tmp toolchain-cache
# redirect used by the landlock-sandboxed orchestrator agent roles (task 5729).
#
# ── WHY /tmp AT ALL ───────────────────────────────────────────────────────────
# Task 5332 sandboxes three orchestrator agent roles (implementer, debugger,
# simple_task) with landlock, and redirects their CARGO_HOME / npm_config_cache
# into /tmp via a `role_env_overrides` block in dark-factory-orchestrator.yaml.
# /tmp is not a free choice: dark-factory's compute_write_set() grants no
# ~/.cargo / ~/.npm carve-out, so /tmp is the one host-global writable root a
# static yaml value can name.  Closing that upstream gap is tracked as
# dark_factory:3162; until then the redirect is load-bearing and this script
# makes it cheap and durable.
#
# Consequence without this script: the FIRST dependency-touching agent run
# after every boot re-downloads the crates.io index and the whole dep tree on a
# live task's critical path, and a second full copy of both caches accumulates
# under /tmp alongside the never-shared ~/.cargo / ~/.npm originals.
#
# ── WHAT THIS SCRIPT DOES ─────────────────────────────────────────────────────
#   1. SEED   — pre-seeds the redirect destinations from the host's real
#               caches so a sandboxed agent starts warm: `cp -al` (hardlink)
#               for the immutable stores, `cp -a` (real copy) for the two
#               index trees cargo/cacache rewrite in place, since a shared
#               inode there would let a sandboxed write reach back into
#               ~/.cargo / ~/.npm.  See the seed-sources note below.
#   2. EXCLUDE— writes /etc/tmpfiles.d/reify-agent-caches.conf `x` lines so
#               systemd-tmpfiles' age-clean pass cannot gut the seeded trees
#               while the orchestrator stays up.
#   3. PERSIST— installs a systemd --user oneshot that re-seeds after the
#               boot-time `D /tmp` wipe.
#
# Steps 2 and 3 cover two DIFFERENT tmpfiles mechanisms and neither substitutes
# for the other — see install_tmpfiles_exclusion() and install_boot_unit().
#
# ── MEASURED PREMISE ──────────────────────────────────────────────────────────
# `cp -al` is near-free ONLY while the source and destination share a
# filesystem.  On this host /tmp, $HOME, ~/.cargo and ~/.npm are all on
# /dev/nvme2n1p5 and /tmp is NOT a tmpfs, so the seed costs directory entries
# and no data blocks.  That premise is ENFORCED, not assumed: seed_one()'s
# cross-device guard skips rather than silently falling back to a real
# multi-GB byte copy if /tmp is ever remounted elsewhere.
#
# Called by scripts/setup-dev.sh (unconditionally, non-fatal) and by
# reify-agent-caches.service at boot (with --seed-only).
# Standalone installer, hermetically testable — mirror of
# scripts/install-warm-lane-units.sh; tests in
# tests/infra/test_agent_cache_redirect.sh.
#
# Usage:
#   scripts/setup-agent-cache-redirect.sh [--seed-only | --print-paths]
#
#   --seed-only   Seed the caches ONLY: no sudo, no systemctl, no tmpfiles conf
#                 and no unit install.  This is the mode the boot unit runs, so
#                 the boot path is entirely unprivileged.
#
#   --print-paths Print the resolved destinations as KEY=VALUE lines
#                 (CARGO_DEST, NPM_DEST, TMPFILES_CONF) and exit, touching
#                 nothing.  Exists so the PRODUCTION defaults are observable
#                 without running the seeder against the real /tmp: every other
#                 mode is only ever exercised in tests with
#                 REIFY_AGENT_CACHE_ROOT pointed at a temp dir, which means the
#                 `/tmp` default and both basenames would otherwise be pinned by
#                 nothing.  test_agent_cache_redirect.sh Block G uses it to
#                 cross-check those defaults against the CARGO_HOME /
#                 npm_config_cache values in dark-factory-orchestrator.yaml's
#                 role_env_overrides — the coupling that makes them load-bearing.
#
# Environment:
#   REIFY_AGENT_CACHE_ROOT  Parent of the redirect destinations (default: /tmp)
#   REIFY_TMPFILES_DIR      tmpfiles.d dir to write (default: /etc/tmpfiles.d)
#   REIFY_MAIN_CHECKOUT     OVERRIDE for the stable checkout the installed
#                           unit's ExecStart is pinned to.  When unset, that
#                           path is DERIVED from git by
#                           scripts/lib_main_checkout.sh; if the derivation
#                           fails, or names a tree holding no executable copy of
#                           this script, the unit falls back to the invoking
#                           copy and says so — see install_boot_unit()
#   CARGO_HOME              Seed source root  (default: $HOME/.cargo)
#   npm_config_cache        Seed source root  (default: $HOME/.npm)
#   XDG_CONFIG_HOME         User config dir   (default: $HOME/.config)
#
# Fail-open by contract: every provisioning step degrades to a warn + continue
# when its substrate is absent (no source caches on a fresh contributor
# machine, no sudo, no systemd --user bus, cross-device /tmp).
# Exits 0 on success and on every such degraded path; exits 2 on a CLI misuse.

set -euo pipefail

# ── helpers ───────────────────────────────────────────────────────────────────
_info() { echo "[setup-agent-cache-redirect] INFO:  $*" >&2; }
_ok()   { echo "[setup-agent-cache-redirect] OK:    $*" >&2; }
_warn() { echo "[setup-agent-cache-redirect] WARN:  $*" >&2; }

# ── CLI guard ─────────────────────────────────────────────────────────────────
SEED_ONLY=0
PRINT_PATHS=0

_usage() {
    echo "Usage: $(basename "$0") [--seed-only | --print-paths]" >&2
    echo "" >&2
    echo "  Provision the /tmp toolchain-cache redirect for the landlock-" >&2
    echo "  sandboxed agent roles (task 5729): hardlink pre-seed, tmpfiles" >&2
    echo "  age-clean exclusion, and a boot-persistent re-seed oneshot." >&2
    echo "" >&2
    echo "  --seed-only   Seed only — no sudo, no systemctl (the boot path)." >&2
    echo "  --print-paths Print the resolved destinations and exit; no writes." >&2
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)     _usage; exit 0 ;;
        --seed-only)   SEED_ONLY=1; shift ;;
        --print-paths) PRINT_PATHS=1; shift ;;
        *)
            echo "$(basename "$0"): unexpected argument: $1" >&2
            _usage
            exit 2
            ;;
    esac
done

# ── path constants — the SINGLE source of truth ───────────────────────────────
# Declared once and consumed by BOTH the seeder and the tmpfiles renderer.  A
# second literal on the renderer side could drift out of step with the seeder,
# silently un-protecting the live directory from age-cleaning with no visible
# symptom until a long-uptime build failed with a missing-source error.  These
# two basenames must match the paths task 5332's `role_env_overrides` block
# points CARGO_HOME / npm_config_cache at — a coupling ACROSS files, and so the
# one this script cannot enforce by construction.  Renaming either side leaves
# the seeder cheerfully reporting "4 of 4 source pairs seeded" while depositing
# ~1G at a path no sandboxed agent ever reads.  Pinned from the outside:
# test_agent_cache_redirect.sh Block G compares `--print-paths` output against
# the values parsed out of dark-factory-orchestrator.yaml.
CACHE_ROOT="${REIFY_AGENT_CACHE_ROOT:-/tmp}"
CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

TMPFILES_DIR="${REIFY_TMPFILES_DIR:-/etc/tmpfiles.d}"
TMPFILES_CONF="$TMPFILES_DIR/reify-agent-caches.conf"

# Emitted on STDOUT (the three _info/_ok/_warn helpers all write to stderr), so
# a caller can consume this without filtering diagnostics.  Placed here, before
# any seeding, so the mode provably touches nothing.
if [ "$PRINT_PATHS" = "1" ]; then
    echo "CARGO_DEST=$CARGO_DEST"
    echo "NPM_DEST=$NPM_DEST"
    echo "TMPFILES_CONF=$TMPFILES_CONF"
    exit 0
fi

# ── seed sources ──────────────────────────────────────────────────────────────
# Two independent decisions here — WHICH trees, and BY WHICH MECHANISM.
#
# (1) WHICH.  Measured on this host: registry/cache is 181M and registry/index
#     81M, but registry/src is 1.1G and _npx 443M.  registry/src is purely
#     DERIVED — cargo extracts it from the .crate tarballs already in
#     registry/cache — so seeding cache+index removes ALL network from the cold
#     start (the dominant cost) at a fraction of the bytes and inodes.  _npx and
#     _cacache/tmp are churn, not build inputs.  Neither is seeded.
#
# (2) HARDLINK vs REAL COPY.  A hardlink is safe ONLY for a tree that is never
#     mutated IN PLACE: a shared inode means a write under /tmp also lands in
#     the host's ~/.cargo / ~/.npm — outside the write set the landlock sandbox
#     exists to enforce.  Verified on this host, only two of the four trees
#     qualify, so the seeder runs in two modes:
#
#       registry/cache       link — immutable .crate tarballs; cargo replaces
#                                   them wholesale, never edits one.
#       _cacache/content-v2  link — hash-addressed blobs; cacache writes a temp
#                                   file and renames it into place.
#       registry/index       COPY — carries cargo's sparse-index
#                                   .cache/<a>/<b>/<crate> files, which cargo
#                                   REWRITES IN PLACE whenever it revalidates
#                                   an index entry.
#       _cacache/index-v5    COPY — cacache's bucket files, which it mutates by
#                                   APPENDING an entry line (plain in the
#                                   on-disk format: "\n<hash>\t<json>" per
#                                   entry, one bucket appended to per insert).
#
#     Both stores validate and re-fetch on a corrupt entry, so a write-through
#     would degrade rather than destroy — but it would silently bypass the
#     sandbox's write containment, which is not a trade this script gets to
#     make on the sandbox's behalf.
#
#     ACCEPTED RESIDUE on the two link-mode trees: a hardlink shares the whole
#     INODE, not just its contents, so mode/owner/mtime are shared too.  A
#     sandboxed agent that chmods or touches a seeded .crate tarball or
#     content-v2 blob changes it in the host's ~/.cargo / ~/.npm as well —
#     outside the landlock write set, by the same mechanism an in-place content
#     write would use.  That channel is pinned by observation, not by this
#     paragraph: test_agent_cache_redirect.sh B21/B22 chmod+touch a seeded
#     link-mode file and assert the write-through, so the boundary is a
#     characterised fact and a future move to copy-mode would turn those
#     assertions red rather than passing silently.
#
#     Why it is accepted rather than closed by copying: (a) the blast radius is
#     a permission bit or a timestamp on a content-addressed entry that both
#     stores locate by HASH and re-fetch on validation failure — a re-download,
#     never corruption of a different crate/package; (b) neither tool chmods or
#     touches an existing cache entry on a normal path (both replace by rename),
#     so nothing but a deliberately misbehaving agent reaches it; and (c) the
#     alternative is copying content-v2, which is 4.8G of real bytes on THIS
#     host, re-paid at every boot (see install_boot_unit's cost note).  The
#     in-place-mutation hazard the copy-mode split exists for is a routine tool
#     behaviour; this one is not.
#
#     The two COPY trees are ~950M together and are only re-copied after a /tmp
#     wipe (`--update=none` makes a warm re-run a near no-op).  Dropping them
#     instead of copying them would cost far more than it looks: without
#     index-v5 every cacache lookup misses and npm re-downloads the whole tree
#     the seeded content-v2 already holds, and without registry/index cargo
#     re-fetches the very sparse index the seed exists to avoid.
CARGO_SRC_ROOT="${CARGO_HOME:-$HOME/.cargo}"
NPM_SRC_ROOT="${npm_config_cache:-$HOME/.npm}"

# ── seed_one <src> <dst> [link|copy] — mirror one tree ────────────────────────
# MODE (default `link`) picks between `cp -al` (hardlink, near-free, only valid
# for a tree nothing mutates in place) and `cp -a` (real byte copy, for the two
# index trees cargo/cacache rewrite in place — see the seed-sources note above).
# Everything else about the two modes is identical, guards included.
#
# `--update=none` applies to both and both re-run cleanly, topping up newly
# added entries without erroring on the ones already there.
#
# `--update=none` is load-bearing, and NOT merely for the trivially-idempotent
# case: probed on coreutils 9.4, a bare `cp -al` re-linking an entry that is
# still the same hardlink is a silent no-op, but one whose destination holds a
# DIFFERENT inode under the same name dies with "cannot create hard link: File
# exists" and fails the whole run.  That divergent state is the redirect's
# normal steady state — a sandboxed agent runs with CARGO_HOME=$CARGO_DEST and
# downloads crates straight into it.  `--update=none` skips those names, which
# is also the semantics we want: the DESTINATION wins, because re-linking would
# replace a file the agent owns with the host's copy, unlinking through a path
# the agent may still hold open.  (`-n` behaves the same but coreutils now
# warns it is non-portable.)
#
# THREE GUARDS, each independent and each FAIL-OPEN: they warn and `return 3`,
# never a nonzero that propagates.  They must not abort the caller — this
# script runs unconditionally from setup-dev.sh and from a boot oneshot, and a
# cache pre-seed is an optimisation; no failure here justifies breaking a
# contributor's environment setup or a boot.  Each warn is worded distinctly so
# a caller (and the test suite) can tell which path was taken.
#
# RETURN CONTRACT: 0 = this pair was actually seeded, 3 = skipped by a guard.
# seed_caches() counts the two so an all-skipped run cannot report OK — a green
# `systemctl --user status` on a run that seeded nothing is the same silent
# failure class this script exists to close.
seed_one() {
    local src="$1" dst="$2" mode="${3:-link}"
    local src_real dst_real dst_parent dst_parent_real src_dev dst_dev
    local cp_mode_flag

    case "$mode" in
        link) cp_mode_flag="-al" ;;
        copy) cp_mode_flag="-a"  ;;
        *)
            _warn "unknown seed mode '$mode' (expected link|copy), skipping: $src"
            return 3
            ;;
    esac

    # GUARD 1 — missing/unreadable source.  A fresh contributor machine has no
    # ~/.npm at all.  Skip THIS pair only; the caller still seeds the others.
    # Checked BEFORE the mkdir so no empty destination tree is left behind — an
    # empty dir looks seeded to a later reader and to `test -d`.
    if [ ! -d "$src" ] || [ ! -r "$src" ]; then
        _warn "seed source missing or unreadable, skipping: $src"
        return 3
    fi

    src_real="$(realpath -m "$src" 2>/dev/null)" || src_real="$src"
    dst_real="$(realpath -m "$dst" 2>/dev/null)" || dst_real="$dst"

    # GUARD 2 — degenerate target.  Two shapes, both reached in practice:
    #   (a) dst == src: a landlock-sandboxed agent already runs with
    #       CARGO_HOME pointed at the redirect, so a setup-dev.sh invocation
    #       from inside the sandbox asks us to copy a tree onto itself.
    #   (b) dst nested inside src: `cp -al` would recurse into its own output,
    #       a self-feeding copy that never converges.
    # Skip WITHOUT touching the destination — a guard that "protected" the tree
    # by emptying it would be worse than the bug.
    if [ "$dst_real" = "$src_real" ]; then
        _warn "degenerate seed target (destination is the source itself), skipping: $dst"
        return 3
    fi
    case "$dst_real/" in
        "$src_real"/*)
            _warn "degenerate seed target (destination is nested inside the source), skipping: $dst"
            return 3
            ;;
    esac

    # GUARD 3 — cross-device.  `cp -al` cannot hardlink across filesystems, and
    # GNU cp does NOT fall back to a byte copy — it fails.  Skipping with a
    # clear warn beats both that bare error and a silent multi-GB duplication
    # into what might be a tmpfs.  Applied in `copy` mode too, deliberately: a
    # byte copy across the boundary would SUCCEED there, quietly writing ~944M
    # of index trees into a RAM-backed /tmp.  This is what turns the measured
    # same-device premise (/tmp and $HOME both on /dev/nvme2n1p5 today) into an
    # ENFORCED precondition rather than a standing assumption, so a future host
    # that mounts /tmp as tmpfs degrades loudly instead of filling RAM.
    #
    # NOT the only source of EXDEV, and deliberately not treated as such.
    # Observed live on this host (2026-07-29): running this script from inside a
    # landlock-sandboxed agent role fails every link-mode pair with "Invalid
    # cross-device link" while src and dst report the SAME st_dev, because
    # landlock answers a link across two of its hierarchies with EXDEV rather
    # than EACCES when LANDLOCK_ACCESS_FS_REFER is not granted.  No stat-based
    # check can see that, so the guard is a cheap filter for the common case,
    # not a complete one — the `cp` failure path below is what actually carries
    # the contract, and it is fail-open for exactly this reason.  (That case is
    # benign in production: the boot oneshot and a contributor's setup-dev.sh
    # both run unsandboxed; only an agent re-running setup-dev.sh hits it, and
    # it degrades to the copy-mode pairs plus a warn.)
    #
    # The device is read from the nearest EXISTING ancestor of the destination,
    # since the destination itself is usually not created yet at this point.
    dst_parent="$dst"
    while [ ! -e "$dst_parent" ] && [ "$dst_parent" != "/" ]; do
        dst_parent_real="$(dirname "$dst_parent")"
        [ "$dst_parent_real" = "$dst_parent" ] && break
        dst_parent="$dst_parent_real"
    done
    src_dev="$(stat -c %d "$src" 2>/dev/null)" || src_dev=""
    dst_dev="$(stat -c %d "$dst_parent" 2>/dev/null)" || dst_dev=""
    if [ -z "$src_dev" ] || [ -z "$dst_dev" ] || [ "$src_dev" != "$dst_dev" ]; then
        _warn "seed source and destination are on different filesystems (hardlinks impossible), skipping: $src -> $dst"
        return 3
    fi

    mkdir -p "$dst" || {
        _warn "could not create seed destination, skipping: $dst"
        return 3
    }
    cp "$cp_mode_flag" --update=none "$src/." "$dst/" || {
        _warn "$mode seed failed (see above), continuing: $src -> $dst"
        return 3
    }
    return 0
}

# ── seed_caches — the SEED step ───────────────────────────────────────────────
# Counts real seeds vs guard skips.  Every guard in seed_one is fail-open, so
# without this count a run in which ALL FOUR pairs were skipped — no source
# caches, a degenerate target, or a /tmp remounted onto another device — would
# still end with a cheerful "OK: seed pass complete", and
# `systemctl --user status reify-agent-caches.service` would show a clean
# oneshot while every agent run paid a full cold start forever.
_SEEDED=0
_SKIPPED=0
seed_counted() {
    if seed_one "$@"; then
        _SEEDED=$((_SEEDED + 1))
    else
        _SKIPPED=$((_SKIPPED + 1))
    fi
}

seed_caches() {
    # link = immutable stores (safe to share inodes with the host caches);
    # copy = trees cargo/cacache rewrite in place.  See the seed-sources note.
    seed_counted "$CARGO_SRC_ROOT/registry/cache"      "$CARGO_DEST/registry/cache"      link
    seed_counted "$CARGO_SRC_ROOT/registry/index"      "$CARGO_DEST/registry/index"      copy
    seed_counted "$NPM_SRC_ROOT/_cacache/content-v2"   "$NPM_DEST/_cacache/content-v2"   link
    seed_counted "$NPM_SRC_ROOT/_cacache/index-v5"     "$NPM_DEST/_cacache/index-v5"     copy

    if [ "$_SEEDED" -eq 0 ]; then
        _warn "seed pass complete but NOTHING was seeded (all $_SKIPPED source pairs skipped) — agents will cold-start"
    else
        _ok "seed pass complete: $_SEEDED of $((_SEEDED + _SKIPPED)) source pairs seeded ($CARGO_DEST, $NPM_DEST)"
    fi
    return 0
}

# ── install_tmpfiles_exclusion — the EXCLUDE step ─────────────────────────────
# Writes /etc/tmpfiles.d/reify-agent-caches.conf, excluding both redirect
# destinations from systemd-tmpfiles' AGE-CLEAN pass.
#
# SCOPE — read this before assuming the boot unit below is redundant.  `x`
# covers ONLY the `--clean` path (systemd-tmpfiles-clean.timer), which is what
# the man page means by "exclude paths from clean-up as controlled with the Age
# parameter".  It does NOT exclude a path from the boot-time `--remove` that
# /usr/lib/tmpfiles.d/tmp.conf's `D /tmp 1777 root root 30d` performs.  PROBED
# hermetically (systemd 255) with `systemd-tmpfiles --create --remove
# --root=<fakeroot>` — the exact verbs systemd-tmpfiles-setup.service runs —
# against a fake root holding that `D /tmp` line plus `x /tmp/keepme`, with an
# unexcluded /tmp/wipeme as control: BOTH were removed.  Hence install_boot_unit.
#
# The two mechanisms are complementary and both required:
#   `x` here          → the timer cannot gut a seeded tree while we stay up.
#   the boot oneshot  → the seed comes back after `D /tmp` empties it.
#
# The `x` verb specifically, not `X`: `X` excludes the directory itself but
# leaves its CONTENTS cleanable, which is exactly the partial-gutting failure
# below.  That failure is nasty because it is not atomic w.r.t. cargo's own
# state — the timer can delete files under registry/src/<crate>/ while leaving
# .cargo-ok in place, after which cargo skips re-extraction and the build fails
# with a missing-source error that looks nothing like a cache problem.  npm's
# cacache has the same content-vs-index split.
install_tmpfiles_exclusion() {
    local desired tmp_conf

    # Rendered from the SAME CARGO_DEST/NPM_DEST constants the seeder consumes,
    # so the conf cannot drift from the directories it is protecting.  A second
    # literal here is the desync bug tests/infra/test_agent_cache_redirect.sh
    # Block G exists to catch.
    # NO BACKTICKS in this heredoc body.  The delimiter is deliberately
    # UNQUOTED so $CARGO_DEST / $NPM_DEST expand — which also means backticks
    # in the prose would be COMMAND SUBSTITUTIONS, silently executing the
    # quoted word and replacing it with its output.  That is not hypothetical:
    # an earlier revision wrote `x` (not `X`) here and shipped a conf whose
    # comment header had those words blanked, after running the real X server.
    # Block F asserts on non-comment lines, so it stayed green; F15 now covers
    # it.  Use plain quotes in prose here.
    desired="$(cat <<EOF
# /etc/tmpfiles.d/reify-agent-caches.conf — generated by
# scripts/setup-agent-cache-redirect.sh (reify task 5729).  Do not edit by hand.
#
# Excludes the landlock-sandboxed agent roles' toolchain-cache redirect (task
# 5332: CARGO_HOME / npm_config_cache under /tmp) from systemd-tmpfiles'
# age-clean pass.  Without these lines systemd-tmpfiles-clean.timer deletes
# INDIVIDUAL FILES out of a live cache while the orchestrator stays up: it can
# remove files under registry/src/<crate>/ while leaving .cargo-ok in place,
# after which cargo skips re-extraction and the build fails with a
# missing-source error that looks nothing like a cache problem.  npm's cacache
# has the same content-vs-index split.
#
# The verb is lowercase 'x', not 'X': 'X' would exclude the directory but
# leave its CONTENTS cleanable, i.e. exactly the partial-gutting above.
#
# NOTE: 'x' covers the age-clean pass ONLY.  It does NOT survive the boot-time
# 'D /tmp' wipe in /usr/lib/tmpfiles.d/tmp.conf; the re-seed after that is
# reify-agent-caches.service, installed by the same script.
x $CARGO_DEST
x $NPM_DEST
EOF
)"

    # Compare before writing: setup-dev.sh is re-run freely, and re-sudoing an
    # already-correct conf is both noise and an unnecessary privilege ask.
    if [ -f "$TMPFILES_CONF" ] && [ "$(cat "$TMPFILES_CONF")" = "$desired" ]; then
        _info "tmpfiles exclusion already current: $TMPFILES_CONF"
        return 0
    fi

    tmp_conf="$(mktemp "${TMPDIR:-/tmp}/reify-agent-caches.conf.XXXXXX")" || {
        _warn "could not stage the tmpfiles exclusion conf — skipping"
        return 0
    }
    printf '%s\n' "$desired" > "$tmp_conf"

    # Prefer a plain write when we already can (root, or a writable target dir —
    # the hermetic-test case), and only reach for sudo otherwise.
    if [ -w "$TMPFILES_DIR" ] || [ "$(id -u)" = "0" ]; then
        if install -m 0644 "$tmp_conf" "$TMPFILES_CONF" 2>/dev/null; then
            _ok "wrote tmpfiles exclusion: $TMPFILES_CONF"
        else
            _warn "could not write the tmpfiles exclusion conf at $TMPFILES_CONF — age-cleaning stays live"
            rm -f "$tmp_conf"
            return 0
        fi
    elif command -v sudo >/dev/null 2>&1 \
         && sudo install -m 0644 "$tmp_conf" "$TMPFILES_CONF" 2>/dev/null; then
        _ok "wrote tmpfiles exclusion: $TMPFILES_CONF"
    else
        _warn "could not write the tmpfiles exclusion conf at $TMPFILES_CONF (no sudo?) — age-cleaning stays live"
        rm -f "$tmp_conf"
        return 0
    fi
    rm -f "$tmp_conf"

    # NO `systemd-tmpfiles --create` apply step, deliberately.  `x` is
    # IGNORE_PATH: it creates nothing, and it is re-read from /etc/tmpfiles.d on
    # every `--clean` invocation of systemd-tmpfiles-clean.timer.  Writing the
    # conf IS the activation — an apply pass would be a no-op that also cost a
    # second sudo prompt right after the install above, which the
    # compare-before-write check exists to avoid.  Pinned by F16.
    return 0
}

# ── install_boot_unit — the PERSIST step ──────────────────────────────────────
# Installs a systemd --user oneshot that re-seeds the redirect after every boot.
#
# NOT REDUNDANT WITH THE `x` LINES ABOVE, though it is easy to mistake for it.
# /usr/lib/tmpfiles.d/tmp.conf carries `D /tmp 1777 root root 30d`, and `D`'s
# boot-time `--remove` empties /tmp wholesale.  `x` excludes a path from the
# AGE-BASED `--clean` only — probed, see install_tmpfiles_exclusion.  So without
# this unit the pre-seed dies at the first reboot, and the first
# dependency-touching agent run after every boot pays a full cold start on a
# live task's critical path.  It fails silently, which is the exact failure
# class this whole script exists to close.
#
# ORDERING IS GUARANTEED, NOT INCIDENTAL: systemd --user managers start after
# the system manager's basic.target, which is ordered after sysinit.target and
# therefore after systemd-tmpfiles-setup.service.  So this oneshot provably
# runs AFTER the `D /tmp` wipe rather than racing it.
#
# ExecStart uses --seed-only, so the boot path is entirely unprivileged: no
# sudo is reachable (or needed) from a non-interactive --user boot context.
#
# ExecStart is pinned to the STABLE MAIN-CHECKOUT path, not to whatever
# checkout invoked us — host convention, matching the inline comments on
# deploy/systemd/reify-warm-lane.service and reify-warm-lane-gc.service.  This
# is not cosmetic: setup-dev.sh is routinely run from a warm-lane worktree
# (/home/leo/src/warm-lanes/worktrees/_lane-N), and a ${BASH_SOURCE[0]}-derived
# ExecStart would pin the unit at a path that vanishes the moment that lane is
# reclaimed and re-seeded.  From then on every boot fails with status=203/EXEC
# and the redirect is never re-seeded — and because nothing Requires= this
# unit, that failure is completely silent.  $self is kept only as the fallback
# for a checkout whose main checkout cannot be resolved, or that resolves to a
# tree holding no executable copy (a contributor clone).
#
# WHAT THE MOVE ONTO THE SHARED RESOLVER COSTS ON THIS HOST, so the trade is
# recorded rather than discovered later: before task 6864 an unset
# REIFY_MAIN_CHECKOUT meant the hardcoded /home/leo/src/reify, so on the reify
# host the pin was UNCONDITIONAL — no derivation existed that could fail.  It is
# now derived, and a derivation failure (git missing from PATH in whatever
# context setup-dev.sh runs under, an unreadable repo, this script copied
# outside any worktree, an anchor failing the resolver's --show-toplevel
# round-trip) degrades to $self — a warm-lane path, i.e. exactly the 203/EXEC
# shape above.  Accepted because the degrade is ANNOUNCED: the fallback warn
# names the remedy (set REIFY_MAIN_CHECKOUT), where the old hardcoded
# default was silently wrong on every host that was not this one.
#
# `Before=orchestrator-reify.service` inline — matching sccache.service in
# setup-dev.sh's install_build_services() — orders the re-seed ahead of the
# orchestrator without needing a drop-in.  NOTHING Requires= it: a failed seed
# degrades to today's cold-start behaviour rather than blocking orchestrator
# startup, the same fail-open posture as the warm-lane units.
#
# COST OF THAT ORDERING, so the trade is legible rather than implied: the seed
# sits ON the orchestrator's startup critical path, and the boot `D /tmp` wipe
# means it is always the cold case.  MEASURED on this host, 2026-07-29
# (/dev/nvme2n1p5, page cache warm — a true post-boot run reads colder and will
# be slower, so these are lower bounds):
#     copy  ~/.cargo/registry/index    81M / 2208 inodes   2.8 s
#     copy  ~/.npm/_cacache/index-v5  870M / 5159 inodes  18.9 s
#     link  (measured 2.8 s per 5159 inodes) → registry/cache 1227 inodes ~0.7 s
#           and _cacache/content-v2 8023 inodes ~4.3 s
#   ≈ 27 s of components; an end-to-end `--seed-only` run measured 37 s wall.
# So the orchestrator starts roughly half a minute later, ONCE per boot.  The
# thing bought is that no agent pays a crates.io index + full dep-tree
# re-download on a live task's critical path, which is minutes, and lands on a
# task the scheduler has already committed to.
#
# The alternative — drop `Before=` and let the seed race the orchestrator — was
# considered and rejected.  It is close to safe (cargo's .package-cache flock
# serializes access and `--update=none` makes a half-seeded tree safe to top
# up), but `cp -a` is not atomic per file, so an agent racing it can read a
# truncated index entry or .crate and take the re-fetch path anyway: the same
# cost, now nondeterministic and on the critical path rather than off it.
# Half a minute of deterministic boot latency is the cheaper side of that trade.
install_boot_unit() {
    local unit_dir unit_path self stable unit_exec script_dir main_checkout remedy

    # Bus probe, same shape as setup-dev.sh's install_build_services() caller:
    # a CI box or container has no --user bus and must skip cleanly.
    if ! systemctl --user show-environment >/dev/null 2>&1; then
        _warn "no systemd --user bus — skipping the boot re-seed unit install"
        return 0
    fi

    # The unit must name an ABSOLUTE path: systemd has no working directory to
    # resolve a relative one against.
    self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

    # WHERE THE STABLE MAIN CHECKOUT COMES FROM (task 6864): the shared resolver
    # in scripts/lib_main_checkout.sh, which setup-dev.sh's
    # install_build_services() already uses for the same purpose.  It honours
    # $REIFY_MAIN_CHECKOUT verbatim when set and otherwise DERIVES the answer
    # from git, so a contributor clone or a relocated tree resolves to its own
    # real main checkout instead of a hardcoded host path that does not exist
    # there.  Sourced from $(dirname "$self") rather than a second
    # ${BASH_SOURCE[0]} dance: $self is already absolute and symlink-resolved
    # just above, so its dirname is the correct per-invocation anchor.
    #
    # Sourced HERE and not at top level on purpose: --seed-only returns before
    # this function is ever reached, so the unprivileged boot path stays
    # bit-for-bit unchanged (test_agent_cache_redirect.sh H16-H21).
    #
    # THE THREE GUARDS BELOW ARE NOT EQUALLY LOAD-BEARING, and the difference is
    # measured (by deleting each in turn and re-running Part C of
    # tests/infra/test_host_global_unit_pinning.sh) rather than assumed:
    #
    #   `2>/dev/null || true` on the CALL — MANDATORY.  This script runs under
    #     `set -euo pipefail`, and reify_main_checkout returns non-zero when it
    #     cannot resolve; an assignment from an unguarded command substitution
    #     inherits that status and ABORTS a deliberately fail-open provisioning
    #     step, taking setup-dev.sh's whole run with it.  Dropping it reds C6.
    #
    #   `declare -F` — REDUNDANT, kept for intent.  It reads as the guard for a
    #     lib that is present and readable and sources cleanly but defines
    #     nothing (source guard already satisfied by an exported
    #     _REIFY_LIB_MAIN_CHECKOUT_SH_SOURCED, a renamed function, a truncated
    #     file).  But the `|| true` above already absorbs the resulting
    #     command-not-found into an empty string, so deleting `declare -F`
    #     leaves Part C — C9 included, which is the case built for exactly that
    #     shape — entirely green.  Kept because it states the intent, and
    #     because setup-dev.sh's install_build_services() spells it the same way.
    #
    #   `|| true` on the SOURCE — likewise redundant against every state Part C
    #     reaches (deleting it leaves the Part green), because a lib that
    #     sources at all returns 0.  It guards the shape the tests do not model:
    #     a lib that FAILS mid-source (syntax error, a trailing command that
    #     exits non-zero).  Cheap, unproven, retained.
    #
    # Do not "simplify" by deleting the first one on the strength of the other
    # two — it is the only one of the three that is doing work.
    script_dir="$(dirname "$self")"
    main_checkout=""
    if [ -r "$script_dir/lib_main_checkout.sh" ]; then
        # shellcheck source=scripts/lib_main_checkout.sh
        . "$script_dir/lib_main_checkout.sh" || true
    fi
    if declare -F reify_main_checkout >/dev/null 2>&1; then
        main_checkout="$(reify_main_checkout "$script_dir" 2>/dev/null || true)"
    fi

    # Prefer the stable main checkout over the invoking one; fall back to $self
    # (and say so) when this is not that checkout, so a contributor clone still
    # gets a working unit rather than one pointed at a path they do not have.
    #
    # The empty guard is not belt-and-braces: an unresolved $main_checkout
    # interpolated unconditionally yields the ROOT-RELATIVE
    # `/scripts/setup-agent-cache-redirect.sh`, a file that exists nowhere.  The
    # -x guard means the UNIT is still correct either way — it falls back to
    # $self — but the warn would name that nonsensical path instead of saying
    # the resolution failed.  `${main_checkout:-<unresolved>}` is setup-dev.sh's
    # exact idiom for the same condition; the two host-global installers must
    # not give an operator two vocabularies for one thing.
    #
    # NOT gated on `$main_checkout != <invoking checkout>`: when the invoking
    # checkout IS the main one but carries no executable copy, the unit is still
    # about to be pinned at a tree that cannot run it, and a `!=` gate would
    # swallow exactly that case.  See setup-dev.sh's install_build_services().
    stable=""
    [ -n "$main_checkout" ] && stable="$main_checkout/scripts/setup-agent-cache-redirect.sh"
    if [ -n "$stable" ] && [ -x "$stable" ]; then
        unit_exec="$stable"
    else
        unit_exec="$self"
        # `<unresolved>` on its own tells an operator what failed but not what
        # to do about it, and the unit they are left with is the ephemeral one.
        # Name the remedy in the same breath — but ONLY on the unresolved
        # branch: when the derivation succeeded and the tree simply holds no
        # executable copy, setting the override changes nothing.
        remedy=""
        [ -n "$main_checkout" ] || \
            remedy=" — set REIFY_MAIN_CHECKOUT to a checkout holding an executable copy to pin it at a stable path"
        _warn "no executable at the stable main-checkout path (${main_checkout:-<unresolved>}) — pinning the boot unit at the invoking copy instead: ${self}${remedy}"
    fi

    unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    unit_path="$unit_dir/reify-agent-caches.service"
    mkdir -p "$unit_dir" || {
        _warn "could not create $unit_dir — skipping the boot re-seed unit install"
        return 0
    }

    cat > "$unit_path" <<EOF
[Unit]
Description=Re-seed the landlock agent toolchain-cache redirect under /tmp (reify task 5729)
Documentation=file://$unit_exec
# WHY THIS EXISTS, given /etc/tmpfiles.d/reify-agent-caches.conf already
# excludes these paths: those \`x\` lines cover systemd-tmpfiles' AGE-CLEAN pass
# only.  /usr/lib/tmpfiles.d/tmp.conf's \`D /tmp 1777 root root 30d\` empties
# /tmp at every boot via \`--remove\`, which \`x\` does NOT exclude a path from
# (probed with \`systemd-tmpfiles --create --remove --root=<fakeroot>\`: an
# x-excluded dir was removed alongside the unexcluded control).  Without this
# oneshot the pre-seed dies at the first reboot and the first
# dependency-touching agent run pays a full cold start on a live task's
# critical path.
#
# Ordering is guaranteed rather than raced: --user managers start after the
# system manager's basic.target, itself ordered after sysinit.target and hence
# after systemd-tmpfiles-setup.service — so this runs AFTER the wipe.
Before=orchestrator-reify.service

[Service]
Type=oneshot
RemainAfterExit=yes
# --seed-only: no sudo, no systemctl.  The boot path is unprivileged by
# construction, because a non-interactive --user context cannot sudo.
#
# The path is the STABLE main checkout, not the checkout that ran setup-dev.sh:
# a warm-lane worktree path would disappear the next time that lane is
# reclaimed, leaving a 203/EXEC failure at every boot that nothing surfaces.
ExecStart=$unit_exec --seed-only

[Install]
WantedBy=default.target
EOF

    systemctl --user daemon-reload || {
        _warn "systemctl --user daemon-reload failed — boot re-seed unit not activated"
        return 0
    }
    systemctl --user enable --now reify-agent-caches.service >/dev/null 2>&1 || {
        _warn "could not enable reify-agent-caches.service — boot re-seed not active"
        return 0
    }
    _ok "boot re-seed unit installed and enabled: $unit_path"
    return 0
}

# ── main ──────────────────────────────────────────────────────────────────────
seed_caches

if [ "$SEED_ONLY" = "1" ]; then
    _info "--seed-only: skipping tmpfiles exclusion and boot-unit install"
    exit 0
fi

install_tmpfiles_exclusion
install_boot_unit

exit 0
