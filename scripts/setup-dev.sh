#!/usr/bin/env bash
# Setup development dependencies for reify.
# Idempotent — safe to re-run; skips already-installed components.
# Usage: ./scripts/setup-dev.sh
set -euo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*"; }

need_sudo=false

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

# ---------- sccache ----------

if command -v sccache &>/dev/null; then
    ok "sccache $(sccache --version 2>/dev/null | awk '{print $2}')"
else
    info "Installing sccache..."
    cargo install sccache --locked
    ok "sccache installed"
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

info "Running verification build..."
cargo check --workspace 2>&1
ok "cargo check passed"

echo
ok "Development environment ready."
