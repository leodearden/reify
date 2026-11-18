# Reify

A text-first DSL for engineering design. Edit a `.ri` file, run `reify build`, get a STEP file. The engine is parametric, constraint-driven, dimensionally-typed (full SI), and incremental — change one parameter and only the affected subgraph re-evaluates.

Think OpenSCAD's text-first philosophy meets SolidWorks' constraint-driven parametrics, with a real type system underneath.

**Status:** pre-release alpha. Expect rough edges. License: AGPL-3.0-or-later.

---

## What to expect (for alpha testers)

Reify is in active development. Two things to set expectations:

- **Linux only right now.** Specifically Ubuntu 24.04. The release ships a `.deb` and an AppImage, both built for Ubuntu 24.04 (x86_64); macOS, Windows and other distros don't work yet.
- **Things will break.** Please report what fails — see "Feedback" below.

What the release ships, from the [GitHub releases page](https://github.com/leodearden/reify/releases):

- `Reify_<version>_amd64.deb` — **the headline asset.** One `apt install` gives you the `reify` CLI on your `PATH`, the `reify-gui` app and a desktop entry. It brings its own copies of OpenCASCADE, gmsh, OpenVDB and oneTBB, so there is no PPA, conda or build step.
- `Reify_<version>_amd64.AppImage` — the same GUI as a single file, for when you don't want to install a package. It needs a desktop session (GTK 3 and the usual GL libraries, which a stock Ubuntu desktop has). It carries the `reify` CLI inside it too; see [Install](#install).

What does work, end-to-end:

- Parsing, type-checking, and constraint-solving against `.ri` source.
- Geometry generation via OpenCASCADE; STEP/STL export.
- A Tauri-based GUI with a Start view (Open, New, Recent, Mode), a 3D viewer, an edges-in-black view, parameter panel, and live diagnostics.
- LSP server for editor integration (diagnostics, hover, go-to-def).
- 50+ example designs under `examples/` ranging from a single flange to multi-part assemblies.

What's flaky or missing:

- Some [examples](https://github.com/leodearden/reify/tree/main/examples) exercise features that are still under construction. If `reify check` errors on one, try a different one; `m5_geometry.ri`, the file you fetched in [Install](#install), is the safest baseline.
- Double-clicking a `.ri` file in your file manager does not open Reify yet (the package doesn't register the `.ri` file type). Open files from the Start view, or pass the path: `reify-gui file.ri`.

---

## System requirements

- Ubuntu 24.04 LTS, x86_64 (other distros not yet supported)
- A desktop session for the GUI; the `reify` CLI needs none.
- Node.js 18 or newer on your `PATH` for the GUI's Assistant mode, which runs a Node script. The `.deb` installs Ubuntu 24.04's own `nodejs` for you; with the AppImage, run `sudo apt install nodejs`. Building from source needs Node 20+ with npm. The `reify` CLI and the GUI's other modes need none.

## Install

1. Download `Reify_<version>_amd64.deb` from the newest `v0.x-alpha` prerelease on the [releases page](https://github.com/leodearden/reify/releases).
2. Install it (apt resolves the package's own dependencies):

   ```sh
   sudo apt install ./Reify_*_amd64.deb
   ```

3. Check it works. The examples are in this repository, not in the package, so fetch one:

   ```sh
   curl -fsSLO https://raw.githubusercontent.com/leodearden/reify/main/examples/m5_geometry.ri
   reify check m5_geometry.ri
   ```

   `main` moves on after each release, so if `reify check` rejects the file, fetch the copy that matches your release by putting its tag (for example `v0.2-alpha`) in the URL in place of `main`.

**AppImage instead.** Download `Reify_<version>_amd64.AppImage`, then:

```sh
chmod +x Reify_*_amd64.AppImage
./Reify_*_amd64.AppImage          # opens the GUI on its Start view
```

If your machine has no FUSE 2 (`libfuse2t64`), run it with `APPIMAGE_EXTRACT_AND_RUN=1 ./Reify_*_amd64.AppImage`. To get the CLI out of an AppImage, extract it once with `./Reify_*_amd64.AppImage --appimage-extract`; the CLI is then `squashfs-root/usr/bin/reify`.

## Try it

CLI:

```sh
reify check m5_geometry.ri
reify build m5_geometry.ri -o /tmp/flange.step
reify test  m5_geometry.ri
```

GUI: launch **Reify** from your desktop's application menu, or run `reify-gui` (add a `.ri` path to skip the Start view and open that file). With no file, the Start view lets you Open…, create a New… file, reopen a Recent one, and pick a Mode.

For more, see [`docs/getting-started.md`](docs/getting-started.md).

## CLI commands

```
reify check  <file>                      Resolve `auto` parameters and check every constraint
reify check  --strict <file>             ...and also fail on any INDETERMINATE constraint
reify build  <file> -o <output>          Build geometry and export (.step / .stl)
reify test   <file>                      Run @test-annotated structures
reify eval   <file>                      Evaluate and print every top-level value cell
reify run    <file>                      Alias for eval
reify report --bom <file>                Roll up a BOM / cost / waste / provenance report
reify explain <file>                     Print per-cell objective provenance: which objective chose each `auto` value
reify fix    --imports <file>...         Add the `import std.X` lines each file needs
reify gui    [--debug] <file>            Open file in GUI
reify gui-debug <file>                   Alias for `gui --debug`
reify lsp                                Start language server (stdin/stdout)
reify doc    <file> [-o <path>]          Generate documentation
reify cache  <subcmd>                    stats / clear / gc / export / import (FEA cache)
reify --version                          Print version
reify --help                             Show this list; `reify <command> --help` shows one command's flags
```

**Which commands let the solver choose `auto` values.** Every command above that evaluates a design, `check` (including `--strict`), `test`, `build`, `eval` / `run`, `report` and `explain`, resolves every `auto` before it reports, and so does the GUI viewport. Whether the editor and the GUI's definition-preview pane do yet, and how to write an `auto` and bound it so it resolves: the `solving` language-reference chunk, [`crates/reify-mcp/src/tools/chunks/solving.md`](crates/reify-mcp/src/tools/chunks/solving.md).

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

## For contributors (build from source)

Building from source is for working on Reify itself, not for trying it. It is Ubuntu 24.04 only too, and needs `sudo` (apt installs during setup), ~10 GB of disk for the build cache, and Node.js with npm on your `PATH` (see [System requirements](#system-requirements)).

```sh
git clone https://github.com/leodearden/reify.git
cd reify
scripts/setup-dev.sh
cargo build --release
```

The first build is slow: 5–15 minutes for `cargo build --release` on a cold cache, plus apt installs. Once warm, incremental rebuilds are fast.

`setup-dev.sh` is idempotent — re-run it any time. It installs rustup, clippy, sccache, tree-sitter-cli, OCCT 7.8 (via the FreeCAD PPA), libslvs, the Tauri webview deps, the GUI's npm packages, and a conda-forge env at `/opt/reify-deps` carrying gmsh 4.15.2 + openvdb 13.0.0 (via micromamba — installed automatically if no conda-family installer is on PATH). It also builds the manifold C++ libs once (~5–10 min cold) into `/opt/reify-deps/manifold/lib` (see `scripts/build-manifold-deps.sh`), which `.cargo/config.toml` links as prebuilt static libs instead of recompiling manifold per build. At the end it runs a smoke test against `examples/m5_geometry.ri`.

From a checkout, the binary is `./target/release/reify`:

```sh
./target/release/reify check examples/m5_geometry.ri
./target/release/reify build examples/m5_geometry.ri -o /tmp/flange.step
./target/release/reify test  examples/m5_geometry.ri
```

GUI (release mode):

```sh
scripts/run-gui.sh examples/m5_geometry.ri
```

GUI (dev mode with HMR + devtools — for poking under the hood):

```sh
scripts/run-gui-dev.sh examples/m5_geometry.ri
```

Installers are built with `scripts/build-bundle.sh` and published with `scripts/release-alpha.sh <tag>`.

## Feedback

Open an issue at <https://github.com/leodearden/reify/issues>. Useful things to include:

- Which asset you installed (`.deb` or AppImage) and the output of the failing install step; if you built from source, the output of `scripts/setup-dev.sh`.
- The `.ri` file you ran and the full command output.
- `uname -a` and `lsb_release -a`.

If you built from source and hit a compile error, `cargo build --release 2>&1 | tail -50` is usually enough.

## License

AGPL-3.0-or-later. See `LICENSE` for the full text.
