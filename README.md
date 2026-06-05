# Reify

A text-first DSL for engineering design. Edit a `.ri` file, run `reify build`, get a STEP file. The engine is parametric, constraint-driven, dimensionally-typed (full SI), and incremental — change one parameter and only the affected subgraph re-evaluates.

Think OpenSCAD's text-first philosophy meets SolidWorks' constraint-driven parametrics, with a real type system underneath.

**Status:** pre-release alpha. Expect rough edges. License: AGPL-3.0-or-later.

---

## What to expect (for alpha testers)

Reify is in active development. Two things to set expectations:

- **Linux only right now.** Specifically Ubuntu 24.04. The setup script uses `apt` and the FreeCAD PPA for OCCT 7.8. macOS and Windows don't work yet.
- **First-time build is slow.** 5–15 minutes for `cargo build --release` on a cold cache, plus apt installs. Once warm, incremental rebuilds are fast. `setup-dev.sh` builds the elalish/manifold C++ libs once into `/opt/reify-deps/manifold/lib` (~5–10 min cold) via `scripts/build-manifold-deps.sh`; a `links`-override in `.cargo/config.toml` then links those prebuilt static libs, so `cargo build` never recompiles manifold from source.
- **Things will break.** This is the first time anyone outside the core team has installed it. Please report what fails — see "Feedback" below.

What does work, end-to-end:

- Parsing, type-checking, and constraint-solving against `.ri` source.
- Geometry generation via OpenCASCADE; STEP/STL export.
- A Tauri-based GUI with a 3D viewer, parameter panel, and live diagnostics.
- LSP server for editor integration (diagnostics, hover, go-to-def).
- 50+ example designs under `examples/` ranging from a single flange to multi-part assemblies.

What's flaky or missing:

- Some examples exercise features that are still under construction. If `reify check examples/<file>.ri` errors, try a different one; `examples/m5_geometry.ri` is the safest baseline.
- No installer, no binary releases. You build from source.

---

## System requirements

- Ubuntu 24.04 LTS (other distros not yet supported)
- ~10 GB free disk for the build cache
- `sudo` (for apt installs during setup)
- Node.js 20+ on PATH (`apt install nodejs npm` if you don't have it)

## Install

```sh
git clone https://github.com/leodearden/reify.git
cd reify
scripts/setup-dev.sh
cargo build --release
```

`setup-dev.sh` is idempotent — re-run it any time. It installs rustup, clippy, sccache, tree-sitter-cli, OCCT 7.8 (via the FreeCAD PPA), libslvs, the Tauri webview deps, the GUI's npm packages, and a conda-forge env at `/opt/reify-deps` carrying gmsh 4.15.2 + openvdb 13.0.0 (via micromamba — installed automatically if no conda-family installer is on PATH). It also builds the manifold C++ libs once into `/opt/reify-deps/manifold/lib` (see `scripts/build-manifold-deps.sh`), which `.cargo/config.toml` links as prebuilt static libs instead of recompiling manifold per build. At the end it runs a smoke test against `examples/m5_geometry.ri`.

## Try it

CLI:

```sh
./target/release/reify check examples/m5_geometry.ri
./target/release/reify build examples/m5_geometry.ri -o /tmp/flange.step
./target/release/reify test  examples/m5_geometry.ri
```

GUI (release mode — recommended for testers):

```sh
scripts/run-gui.sh examples/m5_geometry.ri
```

GUI (dev mode with HMR + devtools — for poking under the hood):

```sh
scripts/run-gui-dev.sh examples/m5_geometry.ri
```

For more, see [`docs/getting-started.md`](docs/getting-started.md).

## CLI commands

```
reify check  <file>                      Parse, type-check, solve constraints
reify build  <file> -o <output>          Build geometry and export (.step / .stl)
reify test   <file>                      Run @test-annotated structures
reify eval   <file>                      Evaluate and print every top-level value cell
reify gui    [--debug] <file>            Open file in GUI
reify gui-debug <file>                   Alias for `gui --debug`
reify lsp                                Start language server (stdin/stdout)
reify mcp-server [file] [--project-dir <dir>]   Start MCP server (stdin/stdout)
reify doc    <file> [-o <path>]          Generate documentation
reify cache  <subcmd>                    stats / clear / gc / export / import (FEA cache)
reify --version                          Print version
reify --help                             Show this list
```

See [`docs/fea-cache.md`](docs/fea-cache.md) for the FEA cache surface (directory, env vars, distribution recipes).

## Repository layout

```
crates/                 30 Rust crates (kernel, eval, constraints, LSP, CLI…)
gui/                    Tauri 2 + SolidJS + Three.js frontend
examples/               50+ .ri sample files
docs/                   Language spec, stdlib reference, design notes
tree-sitter-reify/      Tree-sitter grammar (regenerated on build)
scripts/                setup-dev.sh, run-gui.sh, run-gui-dev.sh
```

## Feedback

Open an issue at <https://github.com/leodearden/reify/issues>. Useful things to include:

- Output of `scripts/setup-dev.sh` if setup itself failed.
- The `.ri` file you ran and the full command output.
- `uname -a` and `lsb_release -a`.

If you hit a compile error, `cargo build --release 2>&1 | tail -50` is usually enough.

## License

AGPL-3.0-or-later. See `LICENSE` for the full text.
