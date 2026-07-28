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
# static yaml value can name.  Closing that upstream gap is tracked as reify
# task #5724 (blocked, pending cross-repo refile); until then the redirect is
# load-bearing and this script makes it cheap and durable.
#
# Consequence without this script: the FIRST dependency-touching agent run
# after every boot re-downloads the crates.io index and the whole dep tree on a
# live task's critical path, and a second full copy of both caches accumulates
# under /tmp alongside the never-shared ~/.cargo / ~/.npm originals.
#
# ── WHAT THIS SCRIPT DOES ─────────────────────────────────────────────────────
#   1. SEED   — hardlink-pre-seeds the redirect destinations from the host's
#               real caches (`cp -al`), so a sandboxed agent starts warm.
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
#   scripts/setup-agent-cache-redirect.sh [--seed-only]
#
#   --seed-only   Seed the caches ONLY: no sudo, no systemctl, no tmpfiles conf
#                 and no unit install.  This is the mode the boot unit runs, so
#                 the boot path is entirely unprivileged.
#
# Environment:
#   REIFY_AGENT_CACHE_ROOT  Parent of the redirect destinations (default: /tmp)
#   REIFY_TMPFILES_DIR      tmpfiles.d dir to write (default: /etc/tmpfiles.d)
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

_usage() {
    echo "Usage: $(basename "$0") [--seed-only]" >&2
    echo "" >&2
    echo "  Provision the /tmp toolchain-cache redirect for the landlock-" >&2
    echo "  sandboxed agent roles (task 5729): hardlink pre-seed, tmpfiles" >&2
    echo "  age-clean exclusion, and a boot-persistent re-seed oneshot." >&2
    echo "" >&2
    echo "  --seed-only   Seed only — no sudo, no systemctl (the boot path)." >&2
}

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)   _usage; exit 0 ;;
        --seed-only) SEED_ONLY=1; shift ;;
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
# points CARGO_HOME / npm_config_cache at.
CACHE_ROOT="${REIFY_AGENT_CACHE_ROOT:-/tmp}"
CARGO_DEST="$CACHE_ROOT/reify-agent-cargo-home"
NPM_DEST="$CACHE_ROOT/reify-agent-npm-cache"

TMPFILES_DIR="${REIFY_TMPFILES_DIR:-/etc/tmpfiles.d}"
TMPFILES_CONF="$TMPFILES_DIR/reify-agent-caches.conf"

# ── seed sources ──────────────────────────────────────────────────────────────
# The deliberate narrowing (measured on this host): registry/cache is 181M and
# registry/index 81M, but registry/src is 1.1G and _npx 443M.  registry/src is
# purely DERIVED — cargo extracts it from the .crate tarballs already in
# registry/cache — so seeding cache+index removes ALL network from the cold
# start (the dominant cost) at a quarter of the bytes and ~4x fewer inodes.
#
# It also confines every hardlink-shared inode to a content-addressed,
# write-new-then-rename store (cargo's .crate tarballs, npm's cacache), so a
# sandboxed agent writing under /tmp can never write THROUGH a shared inode
# into the host's ~/.cargo / ~/.npm.  Hardlinking the mutable extracted src
# tree would put exactly that write-through risk on the tree PROBLEM B is
# about; cargo re-extracts src locally as fresh, unshared files instead.
CARGO_SRC_ROOT="${CARGO_HOME:-$HOME/.cargo}"
NPM_SRC_ROOT="${npm_config_cache:-$HOME/.npm}"

# ── seed_one <src> <dst> — hardlink-mirror one tree ───────────────────────────
# `cp -al --update=none` both hardlinks and re-runs cleanly, topping up newly
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
# THREE GUARDS, each independent and each FAIL-OPEN (warn + `return 0`).  They
# never abort the caller: this script runs unconditionally from setup-dev.sh
# and from a boot oneshot, and a cache pre-seed is an optimisation — no failure
# here justifies breaking a contributor's environment setup or a boot.  Each
# warn is worded distinctly so a caller (and the test suite) can tell which
# path was taken.
seed_one() {
    local src="$1" dst="$2"
    local src_real dst_real dst_parent dst_parent_real src_dev dst_dev

    # GUARD 1 — missing/unreadable source.  A fresh contributor machine has no
    # ~/.npm at all.  Skip THIS pair only; the caller still seeds the others.
    # Checked BEFORE the mkdir so no empty destination tree is left behind — an
    # empty dir looks seeded to a later reader and to `test -d`.
    if [ ! -d "$src" ] || [ ! -r "$src" ]; then
        _warn "seed source missing or unreadable, skipping: $src"
        return 0
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
        return 0
    fi
    case "$dst_real/" in
        "$src_real"/*)
            _warn "degenerate seed target (destination is nested inside the source), skipping: $dst"
            return 0
            ;;
    esac

    # GUARD 3 — cross-device.  `cp -al` cannot hardlink across filesystems, and
    # GNU cp does NOT fall back to a byte copy — it fails.  Skipping with a
    # clear warn beats both that bare error and the alternative some future
    # edit might reach for (a real `cp -a`), which would silently duplicate 6+
    # GB into what might be a tmpfs.  This is what turns the measured
    # same-device premise (/tmp and $HOME both on /dev/nvme2n1p5 today) into an
    # ENFORCED precondition rather than a standing assumption, so a future host
    # that mounts /tmp as tmpfs degrades loudly instead of filling RAM.
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
        return 0
    fi

    mkdir -p "$dst" || {
        _warn "could not create seed destination, skipping: $dst"
        return 0
    }
    cp -al --update=none "$src/." "$dst/" || {
        _warn "hardlink seed failed (see above), continuing: $src -> $dst"
        return 0
    }
    return 0
}

# ── seed_caches — the SEED step ───────────────────────────────────────────────
seed_caches() {
    seed_one "$CARGO_SRC_ROOT/registry/cache" "$CARGO_DEST/registry/cache"
    seed_one "$CARGO_SRC_ROOT/registry/index" "$CARGO_DEST/registry/index"
    seed_one "$NPM_SRC_ROOT/_cacache"         "$NPM_DEST/_cacache"
    _ok "seed pass complete ($CARGO_DEST, $NPM_DEST)"
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

    # Best-effort apply so the exclusion takes effect now rather than at the
    # next boot.  Non-fatal: the conf on disk is what actually matters.
    if command -v systemd-tmpfiles >/dev/null 2>&1; then
        if [ -w "$TMPFILES_DIR" ] || [ "$(id -u)" = "0" ]; then
            systemd-tmpfiles --create "$TMPFILES_CONF" >/dev/null 2>&1 \
                || _warn "systemd-tmpfiles --create did not apply cleanly (non-fatal)"
        else
            sudo systemd-tmpfiles --create "$TMPFILES_CONF" >/dev/null 2>&1 \
                || _warn "systemd-tmpfiles --create did not apply cleanly (non-fatal)"
        fi
    fi
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
# `Before=orchestrator-reify.service` inline — matching sccache.service in
# setup-dev.sh's install_build_services() — orders the re-seed ahead of the
# orchestrator without needing a drop-in.  NOTHING Requires= it: a failed seed
# degrades to today's cold-start behaviour rather than blocking orchestrator
# startup, the same fail-open posture as the warm-lane units.
install_boot_unit() {
    local unit_dir unit_path self

    # Bus probe, same shape as setup-dev.sh's install_build_services() caller:
    # a CI box or container has no --user bus and must skip cleanly.
    if ! systemctl --user show-environment >/dev/null 2>&1; then
        _warn "no systemd --user bus — skipping the boot re-seed unit install"
        return 0
    fi

    # The unit must name an ABSOLUTE path: systemd has no working directory to
    # resolve a relative one against.
    self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

    unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    unit_path="$unit_dir/reify-agent-caches.service"
    mkdir -p "$unit_dir" || {
        _warn "could not create $unit_dir — skipping the boot re-seed unit install"
        return 0
    }

    cat > "$unit_path" <<EOF
[Unit]
Description=Re-seed the landlock agent toolchain-cache redirect under /tmp (reify task 5729)
Documentation=file://$self
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
ExecStart=$self --seed-only

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
