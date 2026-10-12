# PRD — native-dep table and the loader-path decision (review-all S21)

**Status:** draft — pass 1 of the review-all-reify-20261010 program (stream S21), awaiting the lead's rulings on §9 before commit and decompose.
**Program:** `plans/review-all-program-reify-20261012.md`, stream **S21** (size S, risk low, mode agent, wave 1, no upstream seam). Ruling: §Resolved decisions → "Uncontested block" (native-deps.toml read by lib.rs and check-manifold-deps.sh; OCCT directory decision in lib_gui_launch.sh) and §Contradictions ("the OCCT directory decision joins lib_gui_launch.sh").
**Findings closed:** `fk-ce79499430e2` (high), `fk-765ff49d2de5` (medium), `fk-e05e85e98f91` (medium). No mechanical filings exist for them (report §Mechanical filings: all three `open → this stream`).
**Invariants established:** none promoted to this stream. Every extraction leaf names its adoption ratchet (program INV candidate 5, extract-with-ratchet). The existing CLAUDE.md native-deps invariant (one preflight arm per `NativeDep` variant) is kept and gets a by-construction enforcement (§4 D3).
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). Every S21 anchor is byte-unchanged since the review's pinned tree `a030ee7528` (`git diff --stat a030ee7528..main` is empty for every path below). Anchors are cited by symbol. Re-locate lines at implementation time.

---

## 1. Consumer and user-observable surface (G1, G2)

The work has three consumers. Each one exists today, and none of them is a new seam.

- **The native-dep build wiring.** Eight `build.rs` files call `reify_build_utils`: three kernel crates (`reify-kernel-occt`, `-gmsh`, `-openvdb`) and five consumers (`reify-cli`, `reify-eval`, `reify-kernel-conformance`, `reify-mesh-morph`, `gui/src-tauri`), plus two rpath-only callers (`reify-config`, `reify-solver-elastic`). The surface they observe is whether `has_<dep>` is set and stays correct when the environment changes. When that cfg is wrong, every `#[cfg(has_<dep>)]` test silently stops compiling (CLAUDE.md "THE SILENT-VACUITY RULE").
- **The verify pipeline's native-dep preflight.** `scripts/check-manifold-deps.sh` is emitted by `verify.sh` as `add_tool "./scripts/check-manifold-deps.sh"` on every `RUN_RUST=1` verify, and the merge gate and every task-scope Rust verify read its `[ok]`/`[error]` lines. It is guarded by `tests/infra/test_occt_deps_preflight.sh`.
- **Every process that loads a native-dep-linked binary.**
  - The cargo target runner (`.cargo/config.toml` → `runner = ".cargo/run-with-occt.sh"`, which nextest also honours for every test process).
  - The GUI launchers `scripts/run-gui.sh` and `scripts/run-gui-dev.sh`.
  - The e2e smoke lifecycle `gui/test/visual/lib_e2e_smoke.sh` (eight runners source it).
  - `scripts/verify.sh::apply_env`.

  The surface they observe is the `LD_LIBRARY_PATH` each process gets and the reason it is set.

The user is a developer or agent who changes a native dep. Examples: a new candidate dir, a sentinel rename, a fourth dep, or a deps-tree refresh. Today they edit 2–4 files in lockstep under a formatting-sensitive awk parser. After this PRD they edit one row, or one helper.

## 2. Problem (the three findings, re-verified at main)

**fk-ce79499430e2 — the per-dep discovery data is declared twice.** `NativeDep`'s match arms in `crates/reify-build-utils/src/lib.rs` hold the data:
- `include_candidates` and `lib_candidates` (priority-ordered)
- `include_sentinel` and `lib_sentinel`
- the `/snap/freecad` fallback root and its sentinel→subdir map in `find_dir_with_override`

`scripts/check-manifold-deps.sh` repeats all of it by hand in its `# BEGIN <dep>-candidates` marker blocks, the `OCCT_SNAP_ROOT` default and `dep_find_dir`'s `case "$sentinel"`. The OCCT XDE toolkit set also lives twice: `crates/reify-kernel-occt/build.rs`'s `// BEGIN occt-xde-toolkits` block and the guard's `OCCT_XDE_TOOLKITS`.

`tests/infra/test_occt_deps_preflight.sh` enforces parity by reading Rust source as text with six awk parsers: `_rust_dep_list`, `_rust_dep_scalar`, `_rust_snap_root`, `_rust_snap_map`, `_rust_native_dep_variants` and `_rust_occt_xde_toolkits`. They are anchored on `fn <name>`, a 4-space `^    }$` brace, quote-splitting and comment markers. Several bash-side parsers sit beside them (`_bash_snap_map`, `_bash_block_array`, `_bash_block_scalar` and the rest). The snap fallback dispatches on the sentinel *string*. If a sentinel is renamed consistently on both sides, it orphans the fallback on both sides and parity stays green. Re-measured at main:
- 201 of 529 lines of lib.rs are `//` prose.
- 15 lines name the mirror, the guard or the preflight test.

**fk-765ff49d2de5 — `find()` owns neither its rerun inputs nor its cfg.**
- `find()` reads `include_env()` and `lib_env()`. Both are private. It declares neither as a rerun input.
- `emit_rpath_for_bins` and `emit_rpath_for_tests` declare `rerun-if-env-changed` for `lib_env` only.
- Consumers re-derive the cfg by hand: 10 `cargo:rustc-cfg=has_<dep>` sites plus 10 matching `cargo::rustc-check-cfg` lines across 8 `build.rs` files. The three kernel `build.rs` files also hand-write 6 env-name literals.
- `reify-cli`, `reify-eval`, `reify-kernel-conformance`, `reify-mesh-morph` and `gui/src-tauri` gate `has_<dep>` on `find()` with no `*_INCLUDE_DIR` rerun declaration. A kernel and its consumer can therefore disagree on `has_<dep>` after an include-only env change.

**fk-e05e85e98f91 — the loader-path policy is copied four times, and the copies differ.**

| copy | snap dir | deps dir | note |
|---|---|---|---|
| `.cargo/run-with-occt.sh` | yes | yes (gated on `libTKernel.so*` present) | rationale "No rpath is baked in" is false; calls /opt/reify-deps "where OCCT 7.9 is installed" |
| `scripts/run-gui.sh` §5 and `scripts/run-gui-dev.sh` §7 | yes | no | each says it "Mirrors .cargo/run-with-occt.sh" |
| `gui/test/visual/lib_e2e_smoke.sh` §2a | no | yes | says it "mirrors the .cargo/run-with-occt.sh logic" |
| `scripts/verify.sh::apply_env` | yes | yes | its own computed export, phase-scoped and scrubbed for `add_tool` lines |

A fifth copy is in a test: `tests/infra/test_verify_ld_library_path_scope.sh` re-implements the snap/deps computation to build its expected value.

## 3. Premise checks run at authoring (G3, G6)

These are measured facts. The design in §4 rests on them.

1. **RUNPATH is baked, but not everywhere the deps dir is needed.**
   - `readelf -d target/debug/reify` (built 2026-10-11) shows RUNPATH `/opt/reify-deps/tbb-pin:…:/opt/reify-deps/lib:/usr/lib/x86_64-linux-gnu:…`.
   - `env -u LD_LIBRARY_PATH ldd` reports **0** `not found` for `target/debug/reify` and `target/debug/reify-gui`, and for the lib-test binaries of `reify-kernel-gmsh`, `-openvdb`, `reify-eval` and `reify-config`.
   - **But** the `reify-kernel-occt` lib-test binary has RUNPATH `[/usr/lib/x86_64-linux-gnu]` only. It `NEED`s `libgmsh.so.4.15` and `libopenvdb.so.13.0` through its dev-dependency on `reify-eval`, and `ldd` without `LD_LIBRARY_PATH` reports both `not found`.
   - So the runner's deps-dir prepend **is load-bearing** for cargo-run binaries. Its stated reason is wrong (RUNPATH *is* baked, by every `emit_rpath_*` caller). The real reason is that a package's RUNPATH only covers the deps its own `build.rs` emits rpath for.
   - The workspace bins (`reify`, `reify-gui`) need nothing from `LD_LIBRARY_PATH`.
2. **`/etc/ld.so.conf.d/reify-deps.conf` does not exist on the dev host.** `crates/reify-kernel-occt/build.rs` says `libTKernel.so.7.9` "only exists under" it. γ corrects that rationale.
3. **`/snap/freecad` is absent on the dev host.** On such hosts the snap branch of every copy is unexercised at runtime, so it must be covered by fixture-driven tests.
4. **TOML is readable on both sides with no new crate and no hand parser.**
   - `toml = "0.8"` is already a workspace dependency (`Cargo.toml`), and `toml 0.8.2` is in `Cargo.lock`.
   - `/usr/bin/python3` is 3.12.3, and `python3 -I -c 'import tomllib'` succeeds.
5. **`check-manifold-deps.sh` already runs on the scrubbed ambient loader path.** It is emitted via `add_tool`, so a `python3` it spawns does not inherit the conda prefix.
6. **Cfg-site counts reproduce exactly.** `git grep` finds 10 `rustc-cfg=has_(occt|gmsh|openvdb)` sites and 10 `rustc-check-cfg=cfg(has_…)` lines in 8 `build.rs` files, plus 6 kernel `rerun-if-env-changed=(OCCT|GMSH|OPENVDB)_*` literals.
7. **Rerun-heuristic posture.** Every consumer that gains a rerun declaration from `detect()` already emits at least one `rerun-if-*` directive today (via `emit_rpath_*` or its own `rerun-if-changed`). Adding `rerun-if-env-changed` therefore does not switch any crate off cargo's default "any file in the package" heuristic. β re-checks `gui/src-tauri` without the `gui` feature at implementation time, because it calls `find(OpenVdb)` outside the feature gate.
8. **Gate routing.**
   - `scripts/verify-pipeline-guard.sh requires-full-gate` answers 0 (full gate) for `scripts/check-manifold-deps.sh`, `scripts/verify.sh`, `tests/infra/test_occt_deps_preflight.sh` and `scripts/verify-pipeline-infra-tests.txt`.
   - It answers 1 for `.cargo/run-with-occt.sh`, `scripts/lib_gui_launch.sh`, `crates/reify-build-utils/src/lib.rs` and a would-be `crates/reify-build-utils/native-deps.toml`.
   - At merge role a no-source diff escalates to the full gate (dark-factory INV-1, CLAUDE.md "Landing on main").
   - `scripts/verify-pipeline-infra-tests.txt` already maps `crates/reify-build-utils/src/lib.rs`, `scripts/check-manifold-deps.sh` and `scripts/setup-dev.sh` to `tests/infra/test_occt_deps_preflight.sh`.

## 4. Resolved design decisions

**D1 — One data file, `crates/reify-build-utils/native-deps.toml`, is the only declaration of per-dep discovery data.**
- **Rows.** One row per dep, keyed by the dep's lowercase name (`occt`, `gmsh`, `openvdb`).
- **Columns:**
  - `include_env` and `lib_env`
  - `cfg` (`has_occt`, …)
  - `include_candidates` and `lib_candidates` (ordered arrays; order is load-bearing and per-dep, never shared)
  - `include_sentinel` and `lib_sentinel`
  - an optional `snap` table (`root`, `include_subdir`, `lib_subdir`), present on the OCCT row only
  - an optional `xde_toolkits` array, OCCT only
- **Header.** The file's own header comment names its readers: `reify_build_utils` and `scripts/check-manifold-deps.sh` (via `scripts/native_deps_table.py`). It also states that row order is not meaningful but list order is (h13).
- **Out of the table.** Data that only one side uses stays where it is:
  - `OCCT_ACCEPTED_SONAMES` (guard only, pinned against `setup-dev.sh` by preflight section 7)
  - the hint/install text
  - the base OCCT link-toolkit list in `reify-kernel-occt/build.rs`

  Moving them would add no parity and widen the change.

**D2 — Rust reads the table with `include_str!` and decodes it once with the `toml` crate.**
- `reify-build-utils` gains `toml` (workspace) and `serde` with `derive` as normal dependencies. It is itself only ever a `[build-dependencies]` entry, so they land in the host build-script graph. Cost: a one-time host compile of `toml`/`toml_edit`/`serde`, sccache-shared across worktrees. No new crate enters `Cargo.lock`.
- `NativeDep` stays a Rust enum. Each variant maps to its row key through one exhaustive `match`.
- Parse errors and a missing row are build-script panics with the file path in the message. The table is repo-owned data, so a malformed table is a bug, not an environment.
- The snap fallback is keyed by the **row** (`dep.row().snap`), not by sentinel text. That closes the rename-orphans-fallback hole.
- **Rejected:** a hand-written TOML subset parser. That is the h12 ad-hoc-parser smell the finding names.
- **Rejected:** `.tsv`. The ruling named `.toml`, and list-valued cells would need a second in-band separator.

**D3 — Bash reads the same file through one small stdlib reader, and the arm set is driven by the table.**
- **The reader.** `scripts/native_deps_table.py` (`python3 -I`, stdlib `tomllib`, no third-party dependency) prints the rows as shell-quoted (`shlex.quote`) bash declarations. `check-manifold-deps.sh` evaluates them.
- **The table path.** The guard reads `${REIFY_NATIVE_DEPS_TABLE:-$REPO_ROOT/crates/reify-build-utils/native-deps.toml}`. The defaulted variable is the guard's one fixture seam, following the `OCCT_SNAP_ROOT` precedent, and it replaces `OCCT_SNAP_ROOT`: the snap root now comes from the (fixture) table.
- **Fail-closed:**
  - If the reader fails (missing file, malformed TOML, or a row missing a required column), the guard reds with "native-deps table unreadable" naming the path.
  - The guard iterates the table's rows and dispatches each one to its arm: OCCT's bespoke arm (SONAME pin plus XDE presence) or `dep_presence_arm` with that dep's hint. A row with no arm reds the guard and names the row.
- **The invariant chain.** Together with D2's exhaustive variant→row `match` and a Rust unit test that the table's row set equals the variant set, this gives *variant ⇔ row ⇔ arm* by construction. Each link is observable when it fails.
- **Reader hygiene.** The reader is invoked on the guard's own (scrubbed) loader path. The reader's header names that requirement.

**D4 — `reify_build_utils::detect(dep: NativeDep) -> Option<LibLoc>` is the only discovery entry point.**
- It emits, in order:
  - `cargo::rustc-check-cfg=cfg(<row.cfg>)`, always
  - `cargo:rerun-if-env-changed=` for **both** `include_env` and `lib_env`, always
  - `cargo:rustc-cfg=<row.cfg>`, only when both dirs resolve
- It returns the `LibLoc`, so kernels keep linking from it.
- `find()` stops being `pub`. The compiler then rejects any `build.rs` that calls it.
- `emit_rpath_for_bins` and `emit_rpath_for_tests` keep their current contract. Their `lib_env` rerun line becomes redundant with `detect()`'s for crates that call both, which is harmless.
- **The adoption ratchet** (INV candidate 5) is a section of `tests/infra/test_occt_deps_preflight.sh`. It `git grep`s tracked `*build.rs` files for:
  - `rustc-cfg=has_`
  - `rustc-check-cfg=cfg(has_`
  - `rerun-if-env-changed=(OCCT|GMSH|OPENVDB)_`
  - `reify_build_utils::find(`

  It requires **zero** hits. The baseline today is 10 + 10 + 6 + 10.

**D5 — The loader-path decision lives in one sourced lower lib, `scripts/lib_native_ld_path.sh`.**
- **Who sources it.** `scripts/lib_gui_launch.sh` and `.cargo/run-with-occt.sh` both source it. The ruling's "or a lower lib that it and .cargo/run-with-occt.sh both source" branch is taken. The runner cannot sensibly source the GUI lib: it would inherit the GUI lib's source-time snapshot and GUI-only functions.
- **The classes.** It exposes one function that prints the ordered dirs for a **consumer class**, so callers stay stateless (h7), plus a thin prepend wrapper. There are exactly two classes, matching premise check 1:
  - `cargo-target`: any binary cargo or nextest runs. Its RUNPATH may not cover every deps lib it transitively `NEED`s. Dirs: the snap OCCT lib dir if it exists, then `/opt/reify-deps/lib` if it exists. Today's runner predicate (`libTKernel.so*` present) is kept unchanged, so behaviour does not move.
  - `workspace-bin`: `reify` and `reify-gui`, whose RUNPATH comes from `emit_rpath_for_bins` for all three deps. Dirs: the snap OCCT lib dir if it exists. **Never** the deps dir, because that dir is the conda prefix that is hostile to every non-Rust child (sidecar, bun, node, the claude CLI).
- **Defaults.** The dir roots are defaulted variables so fixtures can drive both branches on a host with no `/snap/freecad`.
- **Reading the table is rejected.** The runner wraps every nextest test process, so a `python3` spawn per test would cost CPU across thousands of processes, and CPU is the scarce resource here. The helper stays pure bash. Its two constants are runtime-loader facts, distinct from D1's build-time discovery candidates. Its header says so.

**D6 — `lib_gui_launch.sh` owns the GUI OCCT-dir prepend, and the pin-last ordering moves from caller discipline into one function.**
- `gui_launch_env_pin` (its name is kept, because `test_run_gui_scripts.sh` Test 28 and three callers name it) does the steps in order:
  1. the WebKit default
  2. the `workspace-bin` prepend via `lib_native_ld_path.sh`
  3. the tbb-pin prepend, **last**
- The ORDERING CONTRACT comment changes from "call this LAST, after the launcher's own snap prepend" to "the only LD_LIBRARY_PATH mutation a launcher makes". The source-time inherited-path snapshot is unchanged.
- The run-gui.sh §5 and run-gui-dev.sh §7 snap stanzas are deleted.
- `lib_e2e_smoke.sh` §2a's deps prepend is deleted, and its §2b call is unchanged. The smoke's binary is `reify-gui` (premise check 1: 0 unresolved without `LD_LIBRARY_PATH`), and the §2a comment's TBB-12.18 rationale is superseded by #5192/#7254's pin. The deps prepend also currently leaks the conda prefix into the smoke's `npm install`, sidecar build, `cargo build` and vite children.

  This is the one behaviour change in the PRD. The evidence for it is above, and γ's signal includes a real smoke run.

**D7 — The runner's and verify.sh's dir sets come from the helper's `cargo-target` class, and the facts are stated once.**
- `.cargo/run-with-occt.sh` becomes "source the lib, prepend `cargo-target`, exec".
- `verify.sh::apply_env` computes `_PLAN_LD_LIBRARY_PATH` from the same class. Its phase-boundary export, `REIFY_AMBIENT_LD_LIBRARY_PATH` capture and `add_tool` scrub are the scope contract that `test_verify_ld_library_path_scope.sh` guards. They are unchanged, because they are not a copy of the dir policy (§9 Q1 sequences this against S18).
- **The new note.** `docs/notes/native-dep-loader-path.md` states once:
  - the two classes and why each exists (premise check 1, dated)
  - which packages bake which RUNPATH (`emit_rpath_for_bins` / `emit_rpath_for_tests` callers)
  - the LD_LIBRARY_PATH-outranks-DT_RUNPATH hazard
  - the 7.8-direct / 7.9-via-gmsh fact, by pointer to CLAUDE.md "Native deps"

  The runner, the helper header, `reify-kernel-occt/build.rs`'s SONAME-pin comment and `verify.sh`'s apply_env comment point to it instead of restating it. CLAUDE.md's Pointers table gains one row.
- The false "No rpath is baked in" and "OCCT 7.9 is installed" sentences are deleted.

**D8 — No new `tests/infra` member unless §9 Q2 rules otherwise.**
- Helper behaviour and the loader-path ratchet go into existing members:
  - `test_run_gui_scripts.sh` for the GUI ordering and the class outputs
  - `test_verify_ld_library_path_scope.sh` for verify and the runner
- This avoids adding a `run-all-classification.manifest` row, which is S19's seam (G4 table).
- The loader-path ratchet (INV candidate 5) counts loader-path prepends in tracked `*.sh` files: lines matching `LD_LIBRARY_PATH="\$[A-Za-z_]+\$\{_?[A-Z_]*LD_LIBRARY_PATH:\+` (the `git grep -E` form), excluding `scripts/lib_native_ld_path.sh` and `scripts/lib_gui_launch.sh`. The baseline at main is **7**: runner ×2, run-gui.sh, run-gui-dev.sh, lib_e2e_smoke.sh, verify.sh ×2. γ takes it to 2, because verify.sh's two `_PLAN_LD_LIBRARY_PATH` lines are exempt until δ. δ takes it to 0.

## 5. Out of scope

- **Making `reify-kernel-occt` (and any other package whose test binaries pick up gmsh/openvdb through dev-deps) emit RUNPATH for those deps.** That root fix would let the `cargo-target` class drop the conda prefix too. It needs a workspace-wide NEEDED-vs-RUNPATH scan this stream does not own. §9 Q3. Breadcrumb trigger: a non-Rust child of a cargo-run binary misbehaves under the conda prefix.
- **`libslvs` discovery in `reify-constraints/build.rs`** (pkg-config, outside `NativeDep`). It is named in the finding's next-change scenario, and no finding asks for it to join the table.
- **Porting `check-manifold-deps.sh` or `test_occt_deps_preflight.sh` to Python.** The policy's port arm fires on a flake touch, not on this refactor. These leaves edit the existing bash members in place.
- **The OCCT SONAME pin set and its `"7.8"` build.rs fallback literal.** Not mirrored with Rust beyond that fallback; unchanged.

## 6. Cross-PRD relationship and seam owners (G4)

| shared artefact | owner | this PRD's use |
|---|---|---|
| `scripts/check-manifold-deps.sh` gate arms (INVARIANT: one arm per `NativeDep` variant) | **S21** (this PRD) | α moves the invariant's enforcement from a lexical parse to table-driven dispatch plus a Rust row-set test plus the behavioural per-row loop |
| `scripts/lib_gui_launch.sh` ordering contract (pin step last) once it owns the OCCT dir prepend | **S21** | γ, D6 |
| `scripts/verify.sh` (whole file is S18's port target; verify-pipeline file, full gate per landing) | **S18** (wave 2) | δ edits `apply_env`'s dir computation only (§9 Q1) |
| `run-all-classification.manifest` rows for any new wrapper | **S19** | none by default (D8); §9 Q2 |
| `scripts/verify-pipeline-paths.txt` and the config-only fast path | **S18** | none: the table sits under a crate dir and the reader is `.py`. α re-runs the oracle on the final file set (§8 α) |
| `scripts/verify-pipeline-infra-tests.txt` | unowned in the program table | α adds rows `crates/reify-build-utils/native-deps.toml` and `scripts/native_deps_table.py` → `tests/infra/test_occt_deps_preflight.sh`; γ adds `scripts/lib_native_ld_path.sh` → the two members D8 names |
| CLAUDE.md "Native deps" bullets | unowned (S23 burns prose down in wave 4) | α rewrites the completeness-check sentence; γ adds the Pointers row |

There is no later-wave dependency. The only coupling to a later wave is δ → S18, where S21 lands first and S18 ports the derived form.

## 7. Boundary tests (G5 — bare B suffices)

G5 heuristic: the stakes are build-graph correctness and the merge gate, but each seam is single-repo and both ends are owned by this PRD. Approach **B** with explicit two-way tests per seam:

- **Rust ⇔ table:**
  - a unit test that the decoded row set equals the variant set
  - a unit test that the snap fallback resolves a fixture `<root>/<rev>/<subdir>/<sentinel>` for the OCCT row and **does not** scan for a row with no `snap` table
  - the existing override and empty-override unit tests, kept
- **Bash ⇔ table:** with a fixture table:
  - a fourth row reds the guard naming the row
  - a malformed table reds naming the path
  - the snap branch resolves a fixture snap root
  - an exported-empty `<DEP>_LIB_DIR` falls through to the fixture candidates. This is the bash half of the empty-override rule, which has no behavioural bash test today.
- **Helper ⇔ callers:**
  - class outputs over fixture roots, in both presence states
  - `gui_launch_env_pin` leaves the pin first and the snap dir second
  - the runner exports the `cargo-target` dirs
  - the ratchet

## 8. Decomposition plan (one leaf per bullet; labels only — task ids at decompose)

- **α — native-deps.toml as the single declaration** · priority high · keys `fk-ce79499430e2` · INV: none promoted; ratchet = the six `_rust_*` parsers and the marker-block mirror are deleted, which the D3 row-set test and the dispatch fail-close make unnecessary.
  - **Deliverable:**
    - D1 table; D2 decode in lib.rs; the snap fallback keyed by row
    - `reify-kernel-occt/build.rs` takes `xde_toolkits` from the table (`// BEGIN occt-xde-toolkits` markers deleted)
    - D3 reader and table-driven guard (the marker blocks, `OCCT_SNAP_ROOT` and the sentinel `case` deleted)
    - `test_occt_deps_preflight.sh`: sections 5, 10 and 12-lexical and section 13's set-equality deleted, along with the six `_rust_*` parsers and the bash block parsers they paired with. The section-12 behavioural loop and section 13's presence half derive their dep list and sentinels from the reader. Add the §7 bash cases.
    - verify-pipeline-infra-tests.txt rows
    - lib.rs prose reduced to what the code needs
    - CLAUDE.md's completeness-check sentence rewritten to the new mechanism
  - **Signal (G2):**
    - `scripts/check-manifold-deps.sh` on the dev host prints byte-identical `[ok]` lines before and after.
    - With a fixture table holding a fourth row, the guard exits non-zero naming it.
    - `git grep -cE 'check-manifold-deps|mirror|MIRROR|test_occt_deps_preflight' crates/reify-build-utils/src/lib.rs` = 0.
    - The six `_rust_*` function names are absent from the test.
    - `scripts/cargo-test-tally.py` over one captured `cargo test -p reify-kernel-occt -p reify-kernel-gmsh -p reify-kernel-openvdb` run reports the same non-zero passed counts as on main (the silent-vacuity check).
  - Before landing, re-run `verify-pipeline-guard.sh requires-full-gate` on the final file set (§6).
- **β — `detect(dep)` owns rerun, check-cfg and cfg** · priority medium · keys `fk-765ff49d2de5` · INV: extract-with-ratchet (candidate 5), ratchet member = the D4 section of `tests/infra/test_occt_deps_preflight.sh` (baseline 10/10/6/10 → 0) · depends on α (same file, and env/cfg names come from the table).
  - **Deliverable:**
    - D4 `detect`; `find` made private
    - all 8 `build.rs` migrated: 10 cfg + 10 check-cfg + 6 env literals deleted
    - the `reify-mesh-morph` "copied from reify-solver-elastic" header line goes with its block
  - **Signal (G2):**
    - After `GMSH_INCLUDE_DIR=<other dir>`, `cargo build -p reify-eval -v` reports reify-eval's build script dirty on that env var. Today it does not.
    - The ratchet section is green at 0.
    - The per-crate passed counts for `reify-eval`, `reify-kernel-conformance` and `reify-mesh-morph` (one captured run, cargo-test-tally) equal main's.
- **γ — one loader-path helper; GUI launch owns the OCCT dir** · priority medium · keys `fk-e05e85e98f91` · INV: extract-with-ratchet, ratchet member = the D8 loader-path prepend count in `tests/infra/test_verify_ld_library_path_scope.sh` (baseline 7 → 2, verify.sh's two lines exempt until δ).
  - **Deliverable:**
    - D5 `scripts/lib_native_ld_path.sh`
    - D6 `gui_launch_env_pin`; the run-gui.sh, run-gui-dev.sh and lib_e2e_smoke.sh §2a stanzas deleted
    - the runner rewritten per D7
    - `test_run_gui_scripts.sh`: its two `grep -qF '/snap/freecad/current/usr/lib'` source asserts replaced by behavioural asserts (fixture snap root → the launched env carries it second, after the pin), and its fixture-repo copies of lib_gui_launch.sh also copy the lower lib
    - `docs/notes/native-dep-loader-path.md`
    - the false runner and `reify-kernel-occt/build.rs` rationales corrected by pointer
    - CLAUDE.md Pointers row
  - **Signal (G2):**
    - `scripts/run-gui-dev.sh examples/<any>.ri` on the dev host comes up (debug MCP health endpoint answers) with `LD_LIBRARY_PATH` leading with `/opt/reify-deps/tbb-pin` and no `/opt/reify-deps/lib` entry.
    - One `gui/test/visual/run_find_uses_smoke.sh` run passes.
    - `cargo nextest run -p reify-kernel-occt` still loads (the `cargo-target` class keeps the deps dir, premise check 1).
    - The ratchet reads 2.
- **δ — verify.sh derives its plan loader path from the helper** · priority low · keys none — it delivers the verify.sh share of `fk-e05e85e98f91`, which γ carries as its `x_finding_key` · INV: the γ ratchet's verify.sh exemption removed (→ 0) · depends on γ · full gate (verify-pipeline file).
  - **Deliverable:**
    - `apply_env`'s snap/deps computation replaced by the `cargo-target` class. The capture, export, scrub and their comments stay, re-pointed to the note.
    - `test_verify_ld_library_path_scope.sh`'s expected-value re-implementation replaced by a call to the same helper.
  - **Signal (G2):**
    - `scripts/verify.sh --print-plan` emits a byte-identical `export LD_LIBRARY_PATH=` ENV line before and after on the dev host.
    - `test_verify_ld_library_path_scope.sh` is green.
    - The ratchet reads 0.
- **ω — PRD close** · priority low · depends on α, β, γ, δ · process leaf per the overlay's decompose-close obligations.
  - **Deliverable:** task-id backfill, `Status: SHIPPED` freeze header with the LIVE/AS-AUTHORED map, and the matching manifest header.
  - **Signal:** the committed header.

Each leaf carries `metadata.x_finding_key` (list), `x_finding_run = "review-all-reify-20261010"`, `source = "review-all"`. No new gate-resident test file is added (D8), so no drift-guard registration leaf is needed. If §9 Q2 rules for a new member, its wrapper and manifest row land in γ's own diff.

## 9. Open questions (for the lead before commit)

1. **δ's sequencing against S18.** verify.sh is S18's whole port target. Nothing in the G4 table names verify.sh itself as a seam, and the brief lists "verify.sh's phase-scoped export" as S21-deletable.
   - **(a) default:** δ lands in wave 1 before S18 starts, and S18 ports the derived form. Cost: one more full-gate landing.
   - **(b)** Drop δ. Hand S18 a cross-reference line so its Python `apply_env` calls the helper's `cargo-target` class. γ's ratchet then keeps a verify.sh exemption until S18 lands.

   Note: "delete the export" is read as "delete verify.sh's own copy of the dir policy". The phase-scoped export and the scrub stay, because removing a belt-and-braces export needs per-plan-line evidence this PRD does not have.
2. **New infra member vs existing members (D8).** CLAUDE.md says new infra tests are Python; S19 owns manifest rows for new wrappers.
   - **(a) default:** extend `test_run_gui_scripts.sh` and `test_verify_ld_library_path_scope.sh`. No new row, no S19 dependency.
   - **(b)** A new `tests/infra/test_native_ld_path.py` plus a `.sh` wrapper plus one manifest row, consuming S19's row format. This is cleaner per h6, at the cost of a same-wave cross-stream dependency.
3. **Root-cause RUNPATH completeness (§5).**
   - **(a) default:** out of scope, breadcrumbed.
   - **(b)** Add a leaf: every package whose test binaries `NEED` a deps lib emits rpath for it, checked by a NEEDED-vs-RUNPATH scan, after which the `cargo-target` class can drop the conda prefix. This widens S21 past its ruled remedy.
