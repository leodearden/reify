# PRD: Merge-gate compile-cost reduction — harness consolidation, eval compile-unit isolation, release-sensitivity leaf, ENGINE_VERSION_HASH narrowing

**Status: committed 2026-07-19. Evidence base: `docs/notes/merge-verify-cpu-survey-2026-07.md`. Program milestone: task 5254.**
**Shape: B + H** (high-stakes: verify pipeline + FEA seams) — §Contract + §Boundary-test sketch below are load-bearing, not decorative.

## 1. Consumer + user-observable surface (G1)

**Consumer of every mechanism here is the verify pipeline itself** — the merge/task/background gate run that compiles and links the workspace on every `merge_request`:

- **Harness consolidation** is consumed by the nextest debug pass (`scripts/verify.sh:1161`, the `cargo nextest run ${selector}…` line) and release pass (`scripts/verify.sh:1220-1241`, `emit_nextest_pass "$_RELEASE_ALL_FLAGS"`). Fewer `tests/*.rs` compile units ⇒ fewer link operations (`~0.5–1.5 s`/link, memory-serialized at 1–3 GiB RSS, `verify.sh:1183-1200`).
- **reify-eval-fea compile-unit isolation** (revived B/D) is consumed by the debug pass's incremental rebuild on a solver-module edit — a smaller reify-eval lib compile unit ⇒ a smaller downstream relink wave.
- **Release-sensitivity leaf extraction** is consumed by `scripts/release-scope-lib.sh` `release_declared_set()`/`release_sensitive_set()` and the release pass's `-p` selection: a shrunken `scripts/release-sensitive-crates.txt` ⇒ the release pass selects fewer crates (esp. dropping reify-eval's 397 binaries). Additionally consumed downstream by PRD `merge-gate-riders` (delta-conditional release skip — cross-PRD, G4).
- **ENGINE_VERSION_HASH narrowing** is consumed by the persistent FEA cache (`crates/reify-eval/src/engine_hash_algo.rs`, `build.rs`): a narrower hash input ⇒ an out-of-closure dep bump no longer invalidates the cache, avoiding a full eval recompile + `~794` relinks (survey).
- Program milestone **5254** (the terminal task wires into it).

**Observable surfaces** (measured, not asserted): `cargo nextest list` binary-count per crate (drops to target); `cargo nextest run --print-plan` for the release role (loses `-p reify-eval` after the eval-leaf); `test_release_scoped_scope.sh` grep-derived set (crates exit); measured link-wave count on a lib-touch rebuild; ENGINE_VERSION_HASH stability across an out-of-closure dep bump; runs.db merge-gate compile+link wall (program-level, checked by 5254 against the 2026-07-19 baseline).

## 2. Problem statement (measured 2026-07-19)

Compile+link is **≈ 55–65 % of merge-gate CPU** (survey §Cost baseline). The workspace produces **1,103 integration-test binaries** (reify-eval 397, reify-compiler 290, reify-cli 73, reify-syntax 61, reify-kernel-occt 48), each a separate link at 0.5–1.5 s, serialized on memory. Four independent levers, all zero test-selection-precision-loss, all ratified 2026-07-19:

1. **1,103 binaries is ~10–20× more than selection needs.** Test selection is crate-level (`-p`) or per-test; only **7 binaries** are named individually by filtersets/overrides. Every other `tests/*.rs` is a gratuitous compile unit.
2. **reify-eval's lib is a 199.5 kLOC compile unit.** A one-line solver edit rebuilds the whole unit and relinks its dependents. ~25–35 kLOC of solver modules are OCCT-free/Engine-free and can move to a leaf crate (revived task B).
3. **The release pass runs all 397 reify-eval binaries** because reify-eval is on `release-sensitive-crates.txt` — for release-sensitivity that lives in **8 src + 4 test** files. Five other small crates are on the list for **1–2** trivial `#[cfg(not(debug_assertions))]` sites each.
4. **ENGINE_VERSION_HASH hashes all of `Cargo.lock`** (`engine_hash_algo.rs` `CONTRIBUTORS_RELATIVE`'s `"../../Cargo.lock"`, task 3484). Any dep bump anywhere — a GUI-only or CLI-only crate — invalidates the persistent FEA cache and forces a full eval recompile + relink wave.

## 3. Sketch of approach

Four workstreams. **Ratified 2026-07-19 (operator-approved) — do not re-litigate the decisions; the design here is the execution.**

### W1 — Within-crate test-harness consolidation (1,103 → ~50–80 binaries)

Cargo emits one test binary per `tests/*.rs`. Consolidate many former standalone files into a small number of **harness compile units**, each a `tests/harness_<subsystem>.rs` that `mod`-declares the former files (now under `tests/harness_<subsystem>/<file>.rs`), **preserving each former file's module path** so `test(/^<mod>::/)` filtersets stay valid. nextest still schedules every `#[test]` fn independently across binaries (substrate verified, survey §G3) — per-test selection and scheduling are unchanged; only the link count drops.

**Ratified constraints (hard):**
- **Within-crate only.** Never merge across crates — `-p` scoping and the nextest `occt` package-group (`.config/nextest.toml:55-57`, matches by *package*) must keep crate granularity.
- **Harness compile-unit cap ≈ 10–20 kLOC** (~20–40 test files). Targets: reify-eval 397 → ~15–25 subsystem harnesses; reify-compiler 290 → ~10–15; reify-cli (≈13–17 kLOC) / reify-syntax / reify-kernel-occt → **1 each**. Net ~50–80 binaries, ~95 % link elimination, worst-case test-edit recompile ≈ 1 min (survey §Consolidation trade-off).
- **The 7 override-named binaries stay standalone forever** — they are addressed by `binary(X)`, which consolidation would break: `determinism`, `analytical_validation`, `modal_benchmarks` (reify-solver-elastic); `buckling_smoke`, `fea_diagnostics_e2e` (reify-eval-fea-tests); `tensegrity_t0a`, `representation_within_assertion` (reify-eval). Sources: `scripts/heavy-test-filter-lib.sh:48` (6 heavy atoms via `binary()`), `.config/nextest.toml:91-122` (5 nextest priority overrides via `binary()`; union = these 7).
- **Grouping principle: by subsystem module prefix** (so `test(/^<mod>::/)` matchers are stable; §Contract C1).
- **Exclusions from every consolidation leaf:** the 7 standalone binaries above **and** the 4 release-sensitive reify-eval integration tests owned by W3 (left standalone for the eval-leaf to relocate — avoids sweeping a release-selectable test into a debug-only harness).

**Guard-rename rule (same-diff, mandatory).** Six guards resolve binary names to disk and fail LOUDLY on rename — `tests/infra/{test_heavy_filter_atoms,test_nextest_slow_priority,test_verify_offline_partition,test_verify_gate_exclude_heavy,test_run_offline_deep,test_verify_role_prio}.sh`. Because the 7 standalone binaries never change, these should be untouched — but any leaf that does rename a referenced binary rewrites the guard in the **same diff** (not a sibling task).

**Anti-re-accretion drift guard (B+H boundary deliverable — see §Contract C2, lands before any consolidation leaf).**

**Bonus:** dedup `crates/reify-eval/tests/common/` (2,510 LOC compiled into 11–21 binaries today) as part of the reify-eval consolidation — a shared `mod common;` under the harness, compiled once per harness instead of once per former binary.

### W2 — Revive B (task 4935) + D (task 4936), re-premised on compile-unit isolation

M (task 4933, **done**) cancelled the reify-eval-fea decomposition's A/B/D on CPU grounds but **explicitly allowed a non-CPU reason**. B and D are currently **pending** (tied to `docs/prds/reify-eval-fea-decomposition.md`, Track-1 "OCCT-skip selection" premise). Re-premise both (via `update_task` at decompose) on **compile-unit isolation**:

- **B (4935):** create the OCCT-free, Engine-free `reify-eval-fea` crate and move the pure solver trampolines + co-located unit tests (`compute_targets/*`, `modal_ops`, `dynamics_*`, `trajectory_ops`, `multi_load_dispatch`, `solver_progress`) out of the 199.5 kLOC reify-eval lib. reify-eval depends on reify-eval-fea; registration stays in reify-eval (`compute_targets/mod.rs` `register_compute_fns`). **New premise:** moving ~25–35 kLOC of solver source out of reify-eval shrinks reify-eval's compile unit and the relink wave on a solver-module edit. The OCCT-skip selection win survives as a *bonus*, not the premise.
- **D (4936):** the Track-1 leaf — re-cast its selection-proof from "an OCCT change excludes reify-eval-fea" (still true, keep it) to **primarily** "a reify-eval-fea lib-touch triggers a measurably smaller relink wave than the pre-split reify-eval unit" (the compile-unit-isolation observable).
- **Dependencies:** preserve B's existing edges, including the real cross-PRD prereq **5072** (`register_production_compute_fns` bundler, `compute-fea-hardening` A1 — currently **pending**; the registration seam B relies on). Wire B/D into this PRD's graph and milestone 5254. Re-home their `prd_path` to this PRD.
- **Out of scope:** task 5028 (godfile stage-3 production splits) stays a **deferred do-not-implement bookmark** — confirmed, excluded from the commit_planning flip.

### W3 — Release-sensitivity leaf extraction (shrink `scripts/release-sensitive-crates.txt`)

The list today is exactly **9 crates**: reify-core, reify-eval, reify-eval-fea-tests, reify-expr, reify-gui, reify-mesh-morph, reify-runtime, reify-solver-elastic, reify-stdlib. (Re-derived 2026-07-19; **reify-ir is NOT on it** — task 5166, which would add it, is `infra-hold`; see G4.) The list drives the release pass; each crate's release-sensitivity is a debug-vs-release test-behavior delta (`#[cfg_attr(debug_assertions, ignore)]` / `#[cfg(not(debug_assertions))]` / `cfg!(debug_assertions)`), catalogued by the header of the file itself and drift-checked by `tests/infra/test_release_scoped_scope.sh` (list must equal the grep-derived set).

- **Trivial members (5 leaves, one crate each — cheap, independent, land first):** reify-core (`src/source_location.rs:274`), reify-stdlib (`src/loop_closure_solver.rs:1513`), reify-runtime (`src/warm_startable_assert.rs:97`), reify-expr (1 file), reify-mesh-morph (`src/diagnostics.rs:527,560`). Each has 1–2 `#[cfg(not(debug_assertions))]` / `cfg!(debug_assertions)` **test** sites that pin release no-op behavior. **Site-by-site conscious review** (§Contract C3): make the site profile-invariant *without changing the production `debug_assert!` safety semantics* — either (a) the test pins a language-guaranteed property (debug_assert elision in release) and adds no real coverage ⇒ delete it, letting the crate exit the list, **or** (b) the test pins a real release fallback path (e.g. reify-core's out-of-range clamp) ⇒ re-express it as a profile-invariant assertion that exercises the fallback without tripping the debug_assert, **or** (c) consciously *unify* the debug/release behavior with a recorded rationale. **Never silently drop or invert the debug-profile assertion** (memory: `feedback_silent_defaults_pattern`). The crate exits the list only when its grep-derived membership genuinely goes empty.
- **reify-eval leaf (1 leaf, downstream of B/D + reify-eval consolidation):** reify-eval's release-sensitivity lives in **8 src** (`morph_stage_b, engine_helpers, warm_pool, tolerance_format, engine_purposes, engine_admin, geometry_ops/tests, cache`) + **4 integration test** (`warm_state_donation, zv_shaped_ramp_db_reduction, input_shape_tots_compute_node, cross_sub_geometry_e2e`) files. Move the **4 integration tests** into a new leaf crate `reify-eval-release-tests` (dev-deps reify-eval; they only touch the public API, so relocation is clean); make the **8 src-embedded release-only unit tests** profile-invariant in place per C3, or relocate behind a minimal `pub(crate)`/`#[doc(hidden)]` test hook where they need private access. Result: reify-eval exits `release-sensitive-crates.txt`, the small leaf enters, and the release pass selects the leaf's handful of binaries instead of all 397 reify-eval binaries. (File sets are disjoint from B — none of the 12 are solver files — so B doesn't move them; the dependency on B/D + reify-eval consolidation is for layout stability and tests/ churn avoidance, not file overlap.)
- **Ownership (G4):** **this PRD owns every change to `release-sensitive-crates.txt` membership.** PRD `merge-gate-riders` owns the delta-conditional release-skip logic that *consumes* the shrunken list — it never edits the list. Re-derive the live set at each leaf's dispatch (main is moving; 5166 may land reify-ir).

### W4 — ENGINE_VERSION_HASH narrowing

`crates/reify-eval/src/engine_hash_algo.rs` `CONTRIBUTORS_RELATIVE` ends with `"../../Cargo.lock"` (category 5, transitive-dep version pin), and `build.rs:88-96` hashes every listed contributor. Replace the whole-`Cargo.lock` contribution with a hash over **only the `[[package]]` stanzas of reify-eval's own transitive dependency closure**:

- **Mechanism (deterministic, no build-time `cargo metadata`):** a checked-in closure manifest (the package names in reify-eval's transitive closure); `build.rs`/`engine_hash_algo.rs` parses `Cargo.lock`, selects entries whose name ∈ closure (sorted, canonical), hashes those instead of the whole file.
- **Soundness (G6):** invalidation must **over-approximate** — the closure may be a superset of the true closure, never a subset. A **drift guard** (gate test, §Contract C4) regenerates the closure via `cargo metadata -p reify-eval --edges=…` and asserts the checked-in manifest ⊇ actual; a *missing* dep fails the guard (the safe-fail direction — an unnarrowed dep would silently under-invalidate the cache).
- Preserve the `CONTRIBUTORS_RELATIVE` rename-panic invariant (`build.rs:88-96`): a renamed/removed contributor still panics loudly.

## 4. Pre-conditions / substrate (G3 — verified 2026-07-19)

No novel language substrate (no grammar/parser/semantic assumptions) — this is pure verify-pipeline infrastructure. Substrate facts re-verified against current main; **re-verify at decompose time** (main moving under red-main recovery):

- nextest schedules per-`#[test]` across binaries; `test(/regex/)` filtersets match `<module>::<test>`, `binary(X)` matches the compile-unit name (confirmed `.config/nextest.toml:91-122`). Consolidation preserves module paths ⇒ `test()` filtersets survive; `binary()` filtersets break ⇒ the 7 named binaries stay standalone.
- The `occt` nextest group matches by **package** (`.config/nextest.toml:55-57`) — consolidation-safe.
- `scripts/release-sensitive-crates.txt` = 9 crates (listed above); `test_release_scoped_scope.sh` enforces list == grep-derived set; `release-scope-lib.sh` derives the release `-p` selection.
- `engine_hash_algo.rs` `CONTRIBUTORS_RELATIVE` includes `"../../Cargo.lock"`; `build.rs` iterates it with a rename-panic.
- Six binary-name-resolving guards exist and pass today (all confirmed on disk).
- **Cross-PRD prereq 5072** (`compute-fea-hardening` A1, `register_production_compute_fns`) is **pending** — a real upstream dependency of B (4935), not a fiction. B stays blocked on it (existing edge preserved).

## 5. Contract (H component — the seam specification)

**C1 — Harness-layout contract.**
- Naming: `crates/<crate>/tests/harness_<subsystem>.rs`; former `tests/<file>.rs` becomes `crates/<crate>/tests/harness_<subsystem>/<file>.rs`, declared `mod <file>;` (or `#[path = "harness_<subsystem>/<file>.rs"] mod <file>;`). The submodule path **must equal** the former file stem so `test(/^<file>::/)` filtersets resolve unchanged.
- Grouping: by subsystem module prefix; one harness per subsystem, ≤ ~20 kLOC compile unit.
- Invariant I1: the 7 override-named binaries are never consolidated (they remain top-level `tests/<name>.rs`).
- Invariant I2: within-crate only — a harness never `mod`-includes a file from another crate.
- Invariant I3: no test `#[test]` fn is added or removed by a consolidation (only relocated); total count preserved (±documented moves).

**C2 — Anti-re-accretion drift guard** (`tests/infra/test_harness_kloc_cap.sh`, new; lands **before** any consolidation leaf; run-all-classification.manifest row in the same diff). Enumerates each consolidatable crate's `tests/*.rs`, asserts (a) each harness compile unit ≤ the kLOC cap, (b) every `tests/*.rs` is either a sanctioned harness or one of the 7 named overrides (no new gratuitous standalone binary re-accretes), (c) failure names the offending file. Emits a structured pass/fail line (not a log-scrape).

**C3 — Profile-invariance rule for release-sensitivity sites.** A site may be refactored profile-invariant **iff** the production-code `debug_assert!`/`cfg!(debug_assertions)` *safety* behavior is preserved or consciously unified with a recorded rationale; the release-only *test* is deleted only when it pins a language-guaranteed property, and re-expressed (not dropped) when it pins a real fallback path. Every W3 leaf records its per-site decision in its commit message / the measurements doc, cross-referencing `feedback_silent_defaults_pattern`.

**C4 — ENGINE_VERSION_HASH closure contract.** The checked-in closure manifest is a **superset-or-equal** of reify-eval's `cargo metadata` transitive closure; the drift guard fails on any missing member; hash-input selection is deterministic and order-independent (sorted stanzas).

## 6. Boundary-test sketch (H component — facing both producer and consumer)

| # | Scenario | Precondition | Postcondition (observable) | Faces |
|---|---|---|---|---|
| BT-1 | Consolidation preserves coverage | consolidate crate C's non-excluded `tests/*.rs` into harnesses | `cargo nextest list -p C` **#[test]-fn count unchanged** (±documented moves) before/after; binary count ↓ to target | producer (verify pipeline consumes fewer binaries) |
| BT-2 | Filterset stability | a `test(/^<mod>::/)` heavy/priority filterset references a consolidated test | the filterset still resolves to the same test set post-consolidation | consumer (nextest overrides) |
| BT-3 | Named binaries survive | consolidate reify-eval / reify-solver-elastic / reify-eval-fea-tests | the 6 heavy-atom + 5 priority `binary(X)` filtersets still resolve to on-disk `tests/<X>.rs`; the 6 name-resolving guards green | consumer (heavy-filter, offline, role-prio) |
| BT-4 | Compile-unit isolation win (D) | edit one line in a reify-eval-fea solver module | relink wave count < the pre-split reify-eval baseline (measured, `docs/measurements/`) | consumer (incremental debug rebuild) |
| BT-5 | Release-scope shrink | run W3 leaves | `test_release_scoped_scope.sh` green with the crate absent; `--print-plan` release role loses `-p <crate>` (and `-p reify-eval` after the eval-leaf) | consumer (release-scope-lib, release pass) |
| BT-6 | ENGINE_VERSION_HASH soundness | synthetic `Cargo.lock`: bump an **out-of-closure** dep | ENGINE_VERSION_HASH **unchanged** (FEA cache survives) | consumer (persistent FEA cache) |
| BT-7 | ENGINE_VERSION_HASH completeness | synthetic `Cargo.lock`: bump an **in-closure** dep | ENGINE_VERSION_HASH **changes** (sound invalidation) | consumer (persistent FEA cache) |
| BT-8 | Closure drift | add a new dep to reify-eval's closure without updating the manifest | C4 drift guard **fails** loudly | producer (hash narrowing) |

The consolidation integration-gate leaf (per crate) names **BT-1/BT-2/BT-3** as its observable signal; the eval-leaf names **BT-5**; the ENGINE_VERSION_HASH leaf names **BT-6/BT-7/BT-8**; D names **BT-4**.

## 7. Resolved design decisions

- **Consolidation is within-crate only**, and the 7 `binary()`-addressed binaries stay standalone — not negotiable (breaks selection otherwise).
- **kLOC cap ≈ 10–20** per harness; drift-guarded (C2) so harnesses don't re-accrete into 1,103 again.
- **B/D re-premised on compile-unit isolation** (non-CPU), not OCCT-skip CPU savings (which M cancelled). The OCCT-skip proof is retained as a bonus in D.
- **This PRD solely owns `release-sensitive-crates.txt` membership**; `merge-gate-riders` owns skip logic only (G4).
- **Release-sensitivity refactors preserve or consciously unify debug_assert semantics** — never silent (C3).
- **ENGINE_VERSION_HASH narrowing over-approximates** (superset-safe), drift-guarded (C4).
- **Sequencing (cheap-and-independent first):** (1) trivial release-sensitivity sites + ENGINE_VERSION_HASH; (2) the C2/C1 guards + harness-layout contract; (3) per-crate consolidation in small batches, one crate-subsystem per leaf (small crates first), gated by (2); (4) B → D; (5) eval-leaf (downstream of B/D and reify-eval consolidation, to avoid tests/ churn); (6) terminal task → milestone 5254. Narrow file locks throughout (memory: `feedback_orchestrator_narrow_locks_favor_upfront_design`).

## 8. Out of scope

- Test-selection-precision changes, dynamic step reordering, green-cache (owned by `merge-gate-health` / rejected in survey).
- Lint-order swap, run_all content-addressed skip, delta-conditional release-pass **skip logic** (PRD `merge-gate-riders`).
- retry_failed_only merge retry (PRD `verify-retry-failed-only`).
- Offline-lane / host-infra-lane enablement (item-4 config checklist; `offline-deep-test-lane.md`).
- Categorizer / flake / gate-bypass work (PRD `merge-gate-health`).
- Godfile production-module splits (task 5028, deferred bookmark).
- Cross-crate consolidation; the linker choice (rust-lld retained, mold rejected +26 %, `docs/notes/linker-rustlld-vs-mold-bench.md`).

## 9. Cross-PRD seams (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/merge-gate-riders.md` (unauthored) | consumes | shrunken `release-sensitive-crates.txt` for delta-conditional release skip | **this PRD** owns list membership; riders owns skip logic | queued (riders not yet authored; existing release pass is the live consumer, so G1 holds) |
| task **5166** (infra-hold) | conflicts | `release-sensitive-crates.txt` (5166's landing re-adds reify-ir) | 5166 owns its own add; W3 leaves re-derive the live set at dispatch | monitor — reify-ir NOT on the list as of 2026-07-19 |
| task **5072** (`compute-fea-hardening` A1, pending) | consumes | `register_production_compute_fns` registration bundler that B relies on | 5072 (upstream) | edge preserved on B (4935) |
| `heavy-test-filter-lib.sh` heavy atoms / offline lane | shares | the 6 `binary()` atom names (coordinator-owned offline config) | atom names stay valid — 6 named binaries never consolidated | invariant I1 |
| `docs/prds/merge-gate-health.md` (landed) | sibling | none — §6/§7 there explicitly reserves the release-sensitive list to this PRD | disjoint | no seam |
| milestone **5254** | produces | terminal task wired as a dep of 5254 | 5254 | queued |

No new contested-ownership pair introduced (checked against overlay §G4 list).

## 10. Decomposition plan (one bullet = one leaf; signals sketched, finalized + G6/G7/manifest-checked at decompose)

Every leaf wires into milestone **5254**. `metadata.files` tight-or-empty (broad refactors → `[]`); every new gate-resident guard carries its `run-all-classification.manifest` row **in the same diff** (hard `add_dependency`, never PRD-prose ordering — overlay drift-guard rule; esc-4914-162).

**Phase A — cheap & independent (land first):**
- **A1** reify-core release-sensitivity → profile-invariant (C3). Signal: `test_release_scoped_scope.sh` green with reify-core absent from the grep-derived set. Files: `crates/reify-core/src/source_location.rs`, `scripts/release-sensitive-crates.txt`.
- **A2** reify-stdlib → profile-invariant. Signal: reify-stdlib absent. Files: `crates/reify-stdlib/src/loop_closure_solver.rs`, list.
- **A3** reify-runtime → profile-invariant. Signal: reify-runtime absent. Files: `crates/reify-runtime/src/warm_startable_assert.rs`, list.
- **A4** reify-expr → profile-invariant. Signal: reify-expr absent. Files: `[]` (re-derive the exact file at dispatch), list.
- **A5** reify-mesh-morph → profile-invariant. Signal: reify-mesh-morph absent. Files: `crates/reify-mesh-morph/src/diagnostics.rs`, list.
- **A6** ENGINE_VERSION_HASH closure narrowing + C4 drift guard. Signal: **BT-6/BT-7/BT-8** — out-of-closure bump leaves the hash unchanged, in-closure bump changes it, closure-drift fails the guard. Files: `crates/reify-eval/src/engine_hash_algo.rs`, `crates/reify-eval/build.rs`, new closure manifest, new `tests/infra/test_engine_hash_closure.sh` + its manifest row.

**Phase B — consolidation contract (B+H integration gate; upstream of every consolidation leaf):**
- **B1** Harness-layout contract (C1) + anti-re-accretion drift guard (C2, `tests/infra/test_harness_kloc_cap.sh`) + its `run-all-classification.manifest` row. Signal: the guard green on the pre-consolidation tree; a seeded 21-kLOC harness fixture / a seeded stray `tests/stray.rs` **fails** it. Files: `tests/infra/test_harness_kloc_cap.sh`, `tests/infra/run-all-classification.manifest`.

**Phase C — per-crate consolidation (small batches; each depends on B1; each excludes the 7 named binaries + the 4 release-sensitive reify-eval integration tests):**
- **C-cli** reify-cli 73 → 1 harness. Signal: **BT-1/BT-2/BT-3** — `nextest list -p reify-cli` #[test] count unchanged, binary count → 1, filtersets/guards green. Files: `[]` (broad, within `crates/reify-cli/tests/`).
- **C-syntax** reify-syntax 61 → 1. Signal: BT-1/BT-2. Files: `[]`.
- **C-occt** reify-kernel-occt 48 → 1. Signal: BT-1/BT-2/BT-3 (guards). Files: `[]`.
- **C-eval-\*** reify-eval 397 → ~15–25 subsystem harnesses — **several leaves**, one subsystem prefix per leaf (fea/solver-e2e, geometry, engine, cache/warm, dynamics, topology-selector, tolerance, morph, …; final boundaries architect-chosen under the cap), plus the `tests/common/` dedup. Signal per leaf: BT-1/BT-2/BT-3 for that subsystem. Files: `[]`.
- **C-compiler-\*** reify-compiler 290 → ~10–15 — **several leaves**, one subsystem prefix per leaf. Signal per leaf: BT-1/BT-2. Files: `[]`.

**Phase D — eval compile-unit isolation (revived; `update_task` re-premise, not new files):**
- **D-B** revive task **4935** (B): re-premise on compile-unit isolation; preserve deps incl. 5072; re-home prd_path. Intermediate — unlocks D-D. (Existing signal updated to the compile-unit framing.)
- **D-D** revive task **4936** (D): selection-proof leaf. Signal: **BT-4** — measured relink-wave shrink on a reify-eval-fea lib-touch vs pre-split baseline (`docs/measurements/`), plus the retained OCCT-skip proof. Depends on D-B.

**Phase E — eval release-sensitivity leaf (downstream of D-D and the C-eval-\* leaves):**
- **E1** create `reify-eval-release-tests` leaf crate; move the 4 release-sensitive integration tests; make the 8 src unit tests profile-invariant (C3); reconcile `release-sensitive-crates.txt` (reify-eval out, leaf in). Signal: **BT-5** — `--print-plan` release role loses `-p reify-eval`, gains `-p reify-eval-release-tests`; `test_release_scoped_scope.sh` green. Files: new crate + the 4 test files + list + the 8 src files.

**Phase F — terminal:**
- **F1** program-tie task depending on A1–A6, all C-\*, D-D, E1 → wires into milestone **5254** (5254 itself is the no-code release gate; F1 is this PRD's terminal aggregation node). Signal: all upstream leaves done; the compile+link share of a merge gate re-measured against baseline (rolled up by 5254).

## 11. Open (tactical) questions

- Exact reify-eval / reify-compiler subsystem harness boundaries (architect-chosen at dispatch under the kLOC cap — module-prefix map derived from `ls crates/<c>/tests/`).
- C2 guard: kLOC measured as raw line count vs sloc — settle in the B1 leaf (raw line count is simplest and conservative).
- A6 closure regeneration command exact flags (`cargo metadata -p reify-eval` edge filter to include build+normal, exclude dev) — settle in-leaf against the sound (over-approximate) direction.
- E1: whether any of the 8 src unit tests genuinely need a `pub(crate)` test hook vs pure profile-invariant rewrite — per-site at dispatch (C3).
- Whether reify-eval consolidation should split `tests/common/` dedup into its own leaf or fold it into the first C-eval-\* leaf — tactical.
