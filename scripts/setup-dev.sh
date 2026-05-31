#!/usr/bin/env bash
# Setup development dependencies for reify.
# Idempotent — safe to re-run; skips already-installed components.
#
# Usage: ./scripts/setup-dev.sh
set -euo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*"; }

for arg in "$@"; do
    case "$arg" in
        -h|--help)
            sed -n '2,5p' "$0"
            exit 0
            ;;
        *)
            err "Unknown flag: $arg"
            exit 2
            ;;
    esac
done

need_sudo=false

# ---------- OS preflight ----------
#
# This script targets Ubuntu 24.04 LTS specifically: it uses apt, the
# FreeCAD PPA for OCCT 7.8, and Ubuntu-24.04 package names. Fail fast
# with a clear message on anything else, rather than half-installing and
# leaving the user to debug apt errors.

if [ ! -f /etc/os-release ]; then
    err "This script supports Ubuntu 24.04 only (no /etc/os-release found)."
    exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
if [ "${ID:-}" != "ubuntu" ] || [ "${VERSION_ID:-}" != "24.04" ]; then
    err "This script supports Ubuntu 24.04 only. Detected: ${PRETTY_NAME:-unknown}"
    err "Other distros aren't supported yet — please file an issue at"
    err "  https://github.com/leodearden/reify/issues"
    err "describing your platform if you'd like to help port it."
    exit 1
fi
ok "Ubuntu 24.04 detected"

# ---------- rustup + stable toolchain ----------

if command -v rustup &>/dev/null; then
    ok "rustup $(rustup --version 2>/dev/null | head -1 | awk '{print $2}')"
else
    if [ -d "$HOME/.rustup/toolchains" ] && ! command -v rustup &>/dev/null; then
        info "Toolchains exist but rustup binary missing — installing rustup"
    fi
    info "Installing rustup (stable toolchain)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    ok "rustup installed"
fi

# Ensure cargo/rustup are on PATH for the rest of this script
export PATH="$HOME/.cargo/bin:$PATH"
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

# ---------- clippy ----------

if cargo clippy --version &>/dev/null; then
    ok "clippy $(cargo clippy --version 2>/dev/null | awk '{print $2}')"
else
    info "Installing clippy..."
    rustup component add clippy
    ok "clippy installed"
fi

# ---------- System packages (apt) ----------

check_apt() {
    dpkg -s "$1" &>/dev/null
}

APT_PACKAGES=()

# C compiler + headers (needed for bindgen / cxx builds)
if ! check_apt gcc || ! check_apt libc6-dev; then
    APT_PACKAGES+=(build-essential)
fi

# libslvs (SolveSpace constraint solver)
if ! check_apt libslvs1-dev; then
    APT_PACKAGES+=(libslvs1-dev)
fi

# pkg-config (used by several build.rs scripts)
if ! command -v pkg-config &>/dev/null; then
    APT_PACKAGES+=(pkg-config)
fi

# clang (needed by bindgen for tree-sitter-cli build)
if ! command -v clang &>/dev/null; then
    APT_PACKAGES+=(clang)
fi

# cmake (needed by manifold3d crate's build.rs — it cmake-builds the
# elalish/manifold C++ tree on first cargo build of reify-kernel-manifold)
if ! command -v cmake &>/dev/null; then
    APT_PACKAGES+=(cmake)
fi

# Tauri 2 webview deps — required to build reify-gui on Linux.
# Without these, `cargo build -p reify-gui` fails with cryptic linker errors.
TAURI_DEPS=(
    libwebkit2gtk-4.1-dev
    libsoup-3.0-dev
    libjavascriptcoregtk-4.1-dev
    librsvg2-dev
    libxdo-dev
    libayatana-appindicator3-dev
    libssl-dev
)
for pkg in "${TAURI_DEPS[@]}"; do
    if ! check_apt "$pkg"; then
        APT_PACKAGES+=("$pkg")
    fi
done

# ---------- OCCT 7.8 (via FreeCAD PPA) ----------

occt_version_ok=false
if check_apt libocct-foundation-dev; then
    installed_ver=$(dpkg -s libocct-foundation-dev 2>/dev/null | grep '^Version:' | grep -oP '\d+\.\d+' | head -1)
    if [ "$installed_ver" = "7.8" ]; then
        occt_version_ok=true
        ok "OCCT $installed_ver"
    else
        warn "OCCT $installed_ver installed, need 7.8 — will upgrade via PPA"
    fi
fi

if ! $occt_version_ok; then
    # Add PPA if not already present
    if ! grep -rq "freecad-maintainers/occt-releases" /etc/apt/sources.list.d/ 2>/dev/null; then
        info "Adding FreeCAD OCCT PPA..."
        need_sudo=true
        sudo add-apt-repository -y ppa:freecad-maintainers/occt-releases
    fi
    APT_PACKAGES+=(
        libocct-foundation-dev
        libocct-modeling-algorithms-dev
        libocct-modeling-data-dev
        libocct-data-exchange-dev
    )
fi

if [ ${#APT_PACKAGES[@]} -gt 0 ]; then
    info "Installing apt packages: ${APT_PACKAGES[*]}"
    need_sudo=true
    sudo apt-get update -qq
    sudo apt-get install -y -qq "${APT_PACKAGES[@]}"
    ok "apt packages installed"
else
    ok "all apt packages present"
fi

# ---------- conda-forge env: gmsh + openvdb ----------
#
# Reify links libgmsh (FEA tet meshing, reify-kernel-gmsh) and libopenvdb
# (sparse SDF / voxel grids, reify-kernel-openvdb). Ubuntu's apt has stale
# versions (gmsh 4.12.1, openvdb 10.0.1); upstream is much fresher
# (4.15.2 / 13.0.0). conda-forge ships pre-built binaries at upstream-
# current with multi-platform support, so we use it for these two even
# on Ubuntu.
#
# Strategy:
#   - Probe for an existing conda-family installer (micromamba, mamba,
#     conda). If found, use it.
#   - Otherwise, install micromamba (single static binary, ~13 MB) into
#     /usr/local/bin.
#   - Create the env at /opt/reify-deps from environment.yml.
#   - Per-crate build.rs in reify-kernel-{gmsh,openvdb} embeds RPATH to
#     /opt/reify-deps/lib so libgmsh + libopenvdb + their transitive deps
#     (TBB, Imath, Blosc) resolve at runtime, scoped to just the crates that
#     need them (no global linker-cache change).
#   - build.rs scripts in reify-kernel-{gmsh,openvdb} probe
#     /opt/reify-deps/include for headers.

CONDA_BIN=""
for cmd in micromamba mamba conda; do
    if command -v "$cmd" &>/dev/null; then
        CONDA_BIN="$(command -v $cmd)"
        ok "$cmd $($cmd --version 2>/dev/null | head -1 | awk '{print $NF}')"
        break
    fi
done

if [ -z "$CONDA_BIN" ]; then
    info "Installing micromamba to /usr/local/bin..."
    need_sudo=true
    micromamba_tar="/tmp/micromamba.tar.bz2"
    if [ ! -f "$micromamba_tar" ]; then
        curl -fsSL "https://micro.mamba.pm/api/micromamba/linux-64/latest" -o "$micromamba_tar"
    fi
    sudo tar -xjf "$micromamba_tar" -C /usr/local bin/micromamba
    CONDA_BIN="/usr/local/bin/micromamba"
    ok "micromamba installed"
fi

# /opt/reify-deps owned by current user so env install doesn't need sudo.
if [ ! -d /opt/reify-deps ]; then
    info "Creating /opt/reify-deps (owned by $USER)..."
    need_sudo=true
    sudo mkdir -p /opt/reify-deps
    sudo chown -R "$USER:$USER" /opt/reify-deps
fi

# Detect whether the env already has the required versions.
if [ -f /opt/reify-deps/lib/libgmsh.so.4.15.2 ] \
    && [ -f /opt/reify-deps/lib/libopenvdb.so.13.0.0 ]; then
    ok "reify-deps env: gmsh 4.15.2 + openvdb 13.0.0"
else
    info "Creating reify-deps conda-forge env (gmsh=4.15.2 + openvdb=13.0.0)..."
    info "  This downloads ~150 MB on first install (~3-5 min)."
    "$CONDA_BIN" create -y -p /opt/reify-deps -f environment.yml
    ok "reify-deps env created"
fi

# Note: libgmsh + libopenvdb resolution is handled by per-crate RPATH (see
# crates/reify-kernel-{gmsh,openvdb}/build.rs); no global ld.so.conf.d wiring
# needed.

# ---------- sccache ----------

if command -v sccache &>/dev/null; then
    ok "sccache $(sccache --version 2>/dev/null | awk '{print $2}')"
else
    info "Installing sccache..."
    cargo install sccache --locked
    ok "sccache installed"
fi

# ---------- build-accelerator systemd --user services ----------
#
# Build infra installed as systemd --user units so it survives reboots and
# does NOT silently revert to defaults (a depleted/un-tuned default once pinned
# the orchestrator's merge verifies to -j1 with idle cores):
#   * sccache.service              — sccache server with a tuned cache cap. The
#                                    10 GiB default is far too small for the
#                                    orchestrator's back-to-back full-workspace
#                                    verifies across worktrees; a single
#                                    target/debug is ~60-80 GiB.
#   * reify-jobserver.service      — shared 32-token cargo jobserver FIFO so
#                                    concurrent verifies don't oversubscribe
#                                    cores. PartOf=orchestrator-reify.service
#                                    re-seeds it whenever the orchestrator
#                                    restarts (a restart SIGKILLs in-flight
#                                    rustc, each permanently leaking its token).
#   * reify-jobserver-canary.{service,timer} — every 5 min, re-seed the FIFO if
#                                    tokens have leaked; acts ONLY while the
#                                    build is idle, so it can never disrupt an
#                                    in-flight verify.
#
# Cache size overridable via REIFY_SCCACHE_SIZE (default 100G). Skipped when no
# systemd --user bus is available (e.g. CI).

install_build_services() {
    local unit_dir="$HOME/.config/systemd/user"
    local sccache_bin="$HOME/.cargo/bin/sccache"
    local repo_dir size
    repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    size="${REIFY_SCCACHE_SIZE:-100G}"
    mkdir -p "$unit_dir"

    cat > "$unit_dir/sccache.service" <<EOF
[Unit]
Description=sccache build cache server (${size} cap) for reify verify/builds
Documentation=https://github.com/mozilla/sccache
After=network.target
Before=orchestrator-reify.service

[Service]
Type=oneshot
RemainAfterExit=yes
Environment=SCCACHE_CACHE_SIZE=${size}
Environment=SCCACHE_IDLE_TIMEOUT=0
ExecStartPre=-${sccache_bin} --stop-server
ExecStart=${sccache_bin} --start-server
ExecStop=${sccache_bin} --stop-server

[Install]
WantedBy=default.target
EOF

    cat > "$unit_dir/reify-jobserver.service" <<'EOF'
[Unit]
Description=Shared cargo jobserver FIFO (32 tokens) for reify orchestrator
# PartOf= re-seeds the pool when the orchestrator restarts (a restart SIGKILLs
# in-flight verify rustc, each permanently losing the FIFO token it held).
# Inert if orchestrator-reify.service isn't installed.
PartOf=orchestrator-reify.service

[Service]
Type=simple
ExecStartPre=/bin/bash -c 'rm -f /tmp/reify-jobserver && mkfifo /tmp/reify-jobserver'
ExecStart=/bin/bash -c 'exec 7<>/tmp/reify-jobserver; printf "%%032s" | tr " " "+" >&7; exec sleep infinity'
ExecStopPost=/bin/rm -f /tmp/reify-jobserver
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
EOF

    cat > "$unit_dir/reify-jobserver-canary.service" <<EOF
[Unit]
Description=Re-seed the cargo jobserver FIFO if tokens have leaked (idle-only check)

[Service]
Type=oneshot
ExecStart=${repo_dir}/scripts/jobserver-canary.sh
StandardOutput=journal
StandardError=journal
EOF

    cat > "$unit_dir/reify-jobserver-canary.timer" <<'EOF'
[Unit]
Description=Periodic cargo jobserver depletion check (every 5 min)

[Timer]
OnBootSec=300
OnUnitActiveSec=300
AccuracySec=15s

[Install]
WantedBy=timers.target
EOF

    chmod +x "$repo_dir/scripts/jobserver-canary.sh"
    systemctl --user daemon-reload
    systemctl --user enable --now sccache.service reify-jobserver.service reify-jobserver-canary.timer
}

if systemctl --user show-environment &>/dev/null; then
    info "Installing build-accelerator services (sccache ${REIFY_SCCACHE_SIZE:-100G} + cargo jobserver + leak canary)..."
    if install_build_services; then
        ok "build-accelerator services installed, enabled & started"
    else
        warn "build-accelerator service install hit an error (see above) — non-fatal"
    fi
else
    warn "no systemd --user bus — skipping build-accelerator service install"
fi

# ---------- cargo-nextest ----------
#
# scripts/verify.sh runs the non-OCCT workspace test tail through nextest (one
# global pool over hundreds of test binaries). verify.sh falls back to plain
# `cargo test` when nextest is absent, so this is a performance dependency, not
# a hard one — but the orchestrator/hook fast path expects it present.

if cargo nextest --version &>/dev/null; then
    ok "cargo-nextest $(cargo nextest --version 2>/dev/null | head -1 | awk '{print $2}')"
else
    info "Installing cargo-nextest..."
    cargo install cargo-nextest --locked
    ok "cargo-nextest installed"
fi

# ---------- tree-sitter-cli ----------

if command -v tree-sitter &>/dev/null; then
    ok "tree-sitter $(tree-sitter --version 2>/dev/null | awk '{print $2}')"
else
    info "Installing tree-sitter-cli..."
    # bindgen needs gcc include path for stdbool.h on some systems
    gcc_include=$(find /usr/lib/gcc -name stdbool.h -printf '%h\n' 2>/dev/null | head -1)
    if [ -n "$gcc_include" ]; then
        BINDGEN_EXTRA_CLANG_ARGS="-I$gcc_include" cargo install tree-sitter-cli --locked
    else
        cargo install tree-sitter-cli --locked
    fi
    ok "tree-sitter-cli installed"
fi

# ---------- Node.js + npm ----------

if command -v node &>/dev/null && command -v npm &>/dev/null; then
    ok "node $(node --version), npm $(npm --version)"
else
    err "node/npm not found. Install Node.js 20+ (e.g. via nvm or nodesource)."
    exit 1
fi

# ---------- GUI dependencies (npm ci) ----------

if [ -d gui ] && [ -f gui/package-lock.json ]; then
    info "Installing GUI npm dependencies..."
    (cd gui && npm ci --ignore-scripts)
    ok "gui npm packages"
fi

# ---------- Verification ----------

info "Running verification build (cargo check)..."
cargo check --workspace 2>&1
ok "cargo check passed"

# ---------- Smoke test ----------
#
# Build the release CLI and run it against the simplest example. This
# catches link-time problems (OCCT, tree-sitter) that `cargo check` misses,
# and gives the user a known-good first command to copy.

info "Building release CLI for smoke test (this may take 5-15 min on first run)..."
cargo build --release -p reify-cli
ok "release binary at target/release/reify"

info "Smoke test: reify check examples/m5_geometry.ri"
if ./target/release/reify check examples/m5_geometry.ri; then
    ok "smoke test passed"
else
    err "smoke test FAILED — please file an issue at"
    err "  https://github.com/leodearden/reify/issues"
    err "with the output above and your platform info (uname -a, lsb_release -a)."
    exit 1
fi

echo
ok "Development environment ready."
echo
echo "Try these next:"
echo "  ./target/release/reify build examples/m5_geometry.ri -o /tmp/flange.step"
echo "  scripts/run-gui.sh examples/m5_geometry.ri"
echo
echo "More: docs/getting-started.md"
