# Capability manifest — native-dep-table-and-loader-path

PRD: `docs/prds/v0_6/native-dep-table-and-loader-path.md`. The evidence was bound on 2026-10-12 against main `8e6ffcac9d`. Every S21 anchor is byte-unchanged since the review's pinned tree `a030ee7528`. Symbol evidence was checked by `git grep` in the authoring session. The loader facts were probed with `readelf -d` / `env -u LD_LIBRARY_PATH ldd` on binaries built 2026-10-11. No `.ri` fixtures: this PRD touches no language surface, so the grammar gate and the docs-truth gate do not fire.
Machine-readable twin: `native-dep-table-and-loader-path.capability-manifest.yaml`.

## α — native-deps.toml as the single declaration

- `toml-decodable-in-build-graph` (capability→producer):
  - `toml = "0.8"` is already a workspace dependency, and `toml 0.8.2` is in `Cargo.lock`.
  - `serde` is a workspace dependency; `derive` is added at the crate.
  - No new crate enters the lock.
  - Delivered check: manual. The table file is α's own deliverable, so a presence grep would false-RED before landing. α's row-set unit test is the behavioural cover.
  - PASS.
- `bash-side-reader-available`:
  - `/usr/bin/python3` is 3.12.3, and `python3 -I -c "import tomllib"` exits 0.
  - The guard is emitted via `add_tool`, so its python child runs on the scrubbed ambient loader path.
  - PASS.
- `mirror-and-parsers-exist-today` (signal premise). These artefacts are present today and are deleted by α:
  - the six `_rust_*` parsers in `tests/infra/test_occt_deps_preflight.sh`
  - the three `# BEGIN <dep>-candidates` blocks, `OCCT_SNAP_ROOT` and the sentinel-keyed `case` in `scripts/check-manifold-deps.sh`
  - `// BEGIN occt-xde-toolkits` in `crates/reify-kernel-occt/build.rs`

  PASS.
- `kernel-suites-nonvacuous-baseline` (G6): the silent-vacuity check compares per-crate passed counts from one captured run (`scripts/cargo-test-tally.py`) with main's. The floor is "equal to main and non-zero", never a literal number. PASS.

## β — `detect(dep)`

- `hand-written-cfg-sites-baseline` (signal premise). `git grep` over `*build.rs` finds:
  - `rustc-cfg=has_`: 10
  - `rustc-check-cfg=cfg(has_`: 10
  - `rerun-if-env-changed=(OCCT|GMSH|OPENVDB)_`: 6
  - `reify_build_utils::find(`: 10

  The ratchet target is 0 for all four. Delivered check: grep, `rustc-cfg=has_` absent from the 8 migrated `build.rs`. PASS.
- `include-env-rerun-missing-today`:
  - `find()` declares no rerun line.
  - `emit_rpath_*` declares `lib_env` only.
  - The five consumers emit no `*_INCLUDE_DIR` rerun.

  So the `cargo build -v` dirty-reason signal really differs before and after. PASS.
- `rerun-heuristic-posture-unchanged` (G6): every crate that gains a rerun declaration already emits a `rerun-if-*` directive, so cargo's default package-scan heuristic is already off. `gui/src-tauri` without `--features gui` is re-checked at implementation (PRD §3 item 7). PASS.

## γ — loader-path helper and GUI launch

- `workspace-bins-need-no-ld-library-path` (G6): `target/debug/reify` and `target/debug/reify-gui` have 0 unresolved libraries with no `LD_LIBRARY_PATH`. This backs D6, where the GUI launchers and the e2e smoke carry no deps dir. PASS.
- `cargo-target-class-needs-deps-dir` (G6):
  - The `reify_kernel_occt` lib-test binary has RUNPATH `[/usr/lib/x86_64-linux-gnu]` only.
  - It NEEDs `libgmsh.so.4.15` and `libopenvdb.so.13.0` through its dev-dependency on `reify-eval`.
  - With no `LD_LIBRARY_PATH`, both are `not found`.

  This backs D5 keeping the deps dir for the cargo-target class. The runner's "No rpath is baked in" is false, but the deps-dir prepend it guards is real. PASS.
- `four-copies-exist-today` (signal premise): the D8 prepend pattern returns 7 lines outside `lib_gui_launch.sh`. Delivered check: grep, `SNAP_OCCT_LIB` absent from `run-gui.sh`, `run-gui-dev.sh` and `.cargo/run-with-occt.sh`. PASS.
- `no-snap-on-dev-host` (G3): `/snap/freecad` is absent on the dev host, so every snap assertion is fixture-driven. PASS.

## δ — verify.sh derives from the helper

- `apply-env-computation-is-separable`: only the two prepend lines in `apply_env` change. The ambient capture, the phase-boundary export and the `add_tool` scrub are separate statements under `tests/infra/test_verify_ld_library_path_scope.sh`. PASS.
- `full-gate-route` (G3): `verify-pipeline-guard.sh requires-full-gate scripts/verify.sh` exits 0. PASS.

## ω — PRD close

- `terminal-stamp`: a process capability under the overlay's decompose-close obligations. Delivered check: grep, `Status.*SHIPPED` present. PASS.
