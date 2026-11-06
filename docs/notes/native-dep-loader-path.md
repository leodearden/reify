# Native-dep runtime loader path

**Task #8654 (S21 γ) | 2026-10-15** — owning PRD:
`docs/prds/v0_6/native-dep-table-and-loader-path.md` (D5–D8).

This note covers which directories a process gets on `LD_LIBRARY_PATH` at
**run** time, and why. Build-time discovery of the same deps (include/lib
candidates, `has_<dep>` cfgs) is a separate concern that lives in
`reify_build_utils`.

## The two consumer classes

`scripts/lib_native_ld_path.sh` is the one place that makes this decision.
Each consumer names its class and gets the class's dirs in loader search order.

| Class | Consumers | Dirs, in search order | Why |
|---|---|---|---|
| `cargo-target` | every binary cargo/nextest runs, via `.cargo/run-with-occt.sh` | `/opt/reify-deps/lib` if it holds `libTKernel.so*`; then `/snap/freecad/current/usr/lib` if it exists | a test binary's RUNPATH can miss a deps lib it `NEED`s (below) |
| `workspace-bin` | `reify`, `reify-gui`, via `gui_launch_env_pin` in `scripts/lib_gui_launch.sh` | `/snap/freecad/current/usr/lib` if it exists | their RUNPATH already covers all three deps, and the deps dir is a conda prefix that is hostile to their non-Rust children |

Both roots can be overridden with `REIFY_NATIVE_LD_SNAP_DIR` and
`REIFY_NATIVE_LD_DEPS_DIR`. The tests use these to drive the snap branch on
hosts that have no `/snap/freecad`.

## Measurements

The PRD's premise check (§3 item 1) was measured on 2026-10-12, against main
`8e6ffcac9d` with binaries built 2026-10-11. It was re-measured on 2026-10-15
on the task-8654 branch:

- `target/debug/reify-gui` (built 2026-10-15):
  - RUNPATH is `[/opt/reify-deps/tbb-pin:/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib:/opt/reify-deps/lib]`.
  - `env -u LD_LIBRARY_PATH ldd` reports **0** `not found`.
- `target/debug/reify` (built 2026-10-13): **0** `not found`.
- The `reify_kernel_occt` lib-test binary (built 2026-10-15):
  - RUNPATH is `[/usr/lib/x86_64-linux-gnu]` only.
  - It `NEED`s `libgmsh.so.4.15` and `libopenvdb.so.13.0` through its
    dev-dependency on `reify-eval`, and both are `not found` without
    `LD_LIBRARY_PATH`.
  - Through the runner, `cargo nextest run -p reify-kernel-occt` passed all
    734 tests.

## Which packages bake which RUNPATH

**Rule:** a package's RUNPATH covers only the deps that its own `build.rs`
emits rpath for. A `cargo:rustc-link-arg` reaches that package's own targets,
never its dependents'.

| Mechanism | Packages (deps) |
|---|---|
| `emit_rpath_for_bins` | `reify-cli`, `gui/src-tauri` (occt, gmsh, openvdb) |
| `emit_rpath_for_tests` | `reify-cli` (openvdb); `reify-config` (occt, gmsh, openvdb); `reify-eval` (gmsh, openvdb); `reify-kernel-conformance` (occt, gmsh); `reify-mesh-morph` (gmsh); `reify-solver-elastic` (gmsh) |
| unscoped `-Wl,-rpath` | each of `reify-kernel-occt`, `-gmsh`, `-openvdb`, for its own dep only |
| `emit_tbb_pin_for_bins` / `_for_tests` | `reify-cli` (both), `gui/src-tauri` (bins) |

To re-derive the table:
`git grep -n 'emit_rpath_for_\|emit_tbb_pin_for_\|-Wl,-rpath' -- 'crates/*/build.rs' gui/src-tauri/build.rs`.

## LD_LIBRARY_PATH outranks DT_RUNPATH

The loader searches `LD_LIBRARY_PATH` before `DT_RUNPATH` and before
`ld.so.cache`. Two consequences follow:

- **The deps conda prefix must not be on `LD_LIBRARY_PATH` without a
  reason.** `/opt/reify-deps/lib` ships hundreds of system sonames, and any
  dir on `LD_LIBRARY_PATH` hands its libraries to every child process. This is
  why `workspace-bin` never includes it. It is also why `scripts/verify.sh`
  scrubs it from tool plan lines; the hazard is detailed in the header of
  `tests/infra/test_verify_ld_library_path_scope.sh`.
- **The tbb pin must lead `LD_LIBRARY_PATH`.** Leading the RUNPATH is not
  enough. `gui_launch_env_pin` therefore prepends the pin last, so that it
  ends up first.

`/etc/ld.so.conf.d/reify-deps.conf` does not exist on the dev host (checked
with `ls` on 2026-10-15). So `/opt/reify-deps/lib` is on no default loader
path, and only RUNPATH or `LD_LIBRARY_PATH` can reach it.

Why a reify binary loads both OCCT 7.8 (directly) and 7.9 (via gmsh) is
covered in CLAUDE.md → "Native deps".

## Consumers

- `.cargo/run-with-occt.sh` uses `cargo-target`.
- `scripts/lib_gui_launch.sh`'s `gui_launch_env_pin` makes the only
  `LD_LIBRARY_PATH` mutation of a GUI launch: the `workspace-bin` dirs first,
  then the tbb pin. It is used by `scripts/run-gui.sh`,
  `scripts/run-gui-dev.sh` and `gui/test/visual/lib_e2e_smoke.sh`.
- `scripts/verify.sh` `apply_env` keeps its own copy of the `cargo-target`
  dirs (its two `_PLAN_LD_LIBRARY_PATH` lines). S18's Python rewrite will
  replace that copy with the class.
- Section I of `tests/infra/test_verify_ld_library_path_scope.sh` is a
  ratchet that keeps any other hand-rolled prepend out.

## Out of scope: the root fix

A `cargo-target` class without the conda prefix needs every package whose
test binaries `NEED` a deps lib to emit rpath for that lib. A
NEEDED-vs-RUNPATH scan would enforce this. That work is recorded in the
PRD's §5 and in the "RUNPATH completeness" breadcrumb in §9.
