# Capability manifest — `merge-gate-compile-cost`

Mechanizes G3 + G6 per leaf for `docs/prds/merge-gate-compile-cost.md`. Each binding ties a leaf's asserted capability to **evidence**; any **FAIL** (`declared-only | test-only | producer-absent | producer-downstream | producer-extent-short | fixture-ERROR | bound≤floor | rejection-absent`) blocks queueing until resolved. Verified against main @ `bde340c59c`, 2026-07-19.

**Machine-readable twin.** `merge-gate-compile-cost.capability-manifest.yaml` is the stamper-facing sidecar. It was **migrated 2026-07-28** from the original hand-stamped interim format (which pre-dated the enforced schema and so failed `CapabilityManifestDoc` validation — the stamper skipped this PRD silently) to `schema_version: 1` per `shared/src/shared/capability_manifest.py`: one `tasks[]` entry per PRD label below, each capability carrying a `grep`/`script`/`manual` `delivered_check`. The same migration added the **CMP-3..CMP-6** leaves (5693–5696) filed 2026-07-28 to own the reify-compiler remainder. Content with no field in the new schema is preserved in the sidecar's header comment: the program milestone (**5254**, reached via F1), the revived-task provenance (D-B/D-D), and the cross-PRD dep (4935 → 5072, recorded as a real `add_dependency` edge, not sidecar metadata).

**Domain notes.** This PRD is **verify-pipeline / build-graph infra** — no `.ri` DSL syntax, no result-field production. The reify field-population sentinel (`Value::Undef`) and grammar-fixture checks are therefore **N/A by construction** (recorded per binding, not silently skipped). The G6 surface here is (a) **rejection-mechanism** bindings — the C2 kLOC-cap guard and the C4 closure-drift guard must be *observed to fire* on a seeded violation, not merely defined; and (b) **numeric claims** — all of them (1103→~50-80 binaries, ~10-20 kLOC cap, 25-35 kLOC moved, ~95% link elimination) are ratified survey ranges, not accuracy bounds on a numerical method, so no method error-floor applies. The one coverage invariant (test-count preservation) is derived from `cargo nextest list` output, **never assumed**.

**Consumers are all wired on main today** (anti-orphan): the nextest debug/release passes (`scripts/verify.sh:1161`, `:1220-1241`), `scripts/release-scope-lib.sh` `release_declared_set`/`release_sensitive_set`, `tests/infra/test_release_scoped_scope.sh`, the persistent FEA cache (`engine_hash_algo.rs`/`build.rs`), and the 6 binary-name-resolving guards. PRD `merge-gate-riders` is an *additional future* consumer of the shrunken list; G1 is satisfied by the existing release pass regardless.

---

## A1–A5 — trivial-member release-sensitivity → profile-invariant

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | Each crate's release-sensitive site verified on disk 2026-07-19: reify-core `src/source_location.rs`, reify-stdlib `src/loop_closure_solver.rs`, reify-runtime `src/warm_startable_assert.rs`, reify-expr `tests/field_eval_tests.rs`, reify-mesh-morph `src/diagnostics.rs`. Grep mechanisms A/B/C per `release-sensitive-crates.txt` header. |
| Anti-orphan / wired | **PASS** | Consumed by `test_release_scoped_scope.sh` (list == grep-derived set) and the release pass `-p` selection — both on main. |
| Rejection/observable | **PASS** | Signal = crate **absent** from the grep-derived set → `test_release_scoped_scope.sh` green with the crate removed from the list (observed, not asserted). |
| C3 silent-defaults guard (G6) | **PASS by construction** | The debug-profile `debug_assert!` safety semantics are preserved or consciously unified with recorded rationale (C3); never silently dropped/inverted (`feedback_silent_defaults_pattern`). Per-site decision recorded in each leaf. |
| Field-population / Grammar | **N/A** | No result field, no `.ri` syntax. |

## A6 — ENGINE_VERSION_HASH narrowed to reify-eval's dependency closure

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `engine_hash_algo.rs` `CONTRIBUTORS_RELATIVE` includes `"../../Cargo.lock"`; `build.rs:88-96` iterates it with a rename-panic (verified). `Cargo.lock` is parseable `[[package]]` TOML; `cargo metadata -p reify-eval` yields the closure. |
| Anti-orphan / wired | **PASS** | Consumed by the persistent FEA cache key (`engine_hash_algo.rs`) — on main. |
| Rejection-mechanism (G6 branch 4) | **PASS (observed by BT-8)** | New C4 drift guard `tests/infra/test_engine_hash_closure.sh`: a closure member missing from the checked-in manifest **fails** the guard (seeded, observed). |
| Numeric-floor (G6) | **N/A** | No accuracy bound; soundness is set-inclusion (over-approximate manifest ⊇ actual closure), not a numeric tolerance. |
| Soundness (BT-6/BT-7) | **PASS (observed)** | Out-of-closure dep bump ⇒ hash unchanged; in-closure bump ⇒ hash changes — both exercised by the leaf's own fixture test. |

## B1 — harness-layout contract (C1) + anti-re-accretion guard (C2)

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | nextest per-`#[test]` scheduling across binaries + `test(/^mod::/)` filtersets + `binary(X)` addressing all verified in `.config/nextest.toml`; cargo auto-discovers `tests/*.rs` targets (no Cargo.toml edit needed). |
| Anti-orphan / wired | **PASS** | The guard is consumed by the run-all suite (its `run-all-classification.manifest` row, same diff) — gate-resident. |
| Rejection-mechanism (G6 branch 4) | **PASS (observed)** | Signal requires the guard to **fail** on a seeded 21-kLOC harness fixture and on a seeded stray `tests/stray.rs` — observed to fire, not just defined. |
| Drift-guard registration | **PASS** | Same-diff `run-all-classification.manifest` row (overlay rule; esc-4914-162) — declared in the leaf. |

## C-cli / C-syntax / C-occt / EVAL-1..3 / CMP-1..6 — within-crate consolidation

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | Cargo one-binary-per-`tests/*.rs`; `mod`-include preserves module path → `test()` filtersets stable (§4). Binary counts verified: cli 73, syntax 61, occt 48, eval 397, compiler 290. |
| Anti-orphan / wired | **PASS** | Consumed by the debug/release nextest passes — fewer link units. |
| Coverage invariant (BT-1, G6) | **PASS (derived, not assumed)** | `#[test]`-fn count from `cargo nextest list -p <crate>` before/after, asserted equal (±documented moves) in the leaf. |
| `binary()` stability (BT-3) | **PASS** | The 7 override-named binaries (`heavy-test-filter-lib.sh:48` 6 atoms + `.config/nextest.toml:91-122` `representation_within_assertion`) are **excluded** from consolidation → the 6 name-resolving guards stay green. Verified the 7 names on disk. |
| Extent match (anti-short) | **PASS** | Each leaf's scope is its own crate's `tests/` only (within-crate constraint I2); EVAL-3 additionally **excludes** the 4 release-sensitive integration tests (owned by E1) so they stay standalone for relocation. CMP-2 and CMP-5 additionally **exclude** `geometry_chunk_smoke.rs` and `stdlib_chunk_geometry_ops_smoke.rs` (owned by task **5477**, which absorbs them into `harness_doc_chunks/` and removes their baseline rows itself). |
| Signal scope (corrected 2026-07-28, esc-5284-2) | **PASS** | Every consolidation leaf's signal is **batch-local** (PRD §10: "several leaves, one subsystem prefix per leaf. Signal per leaf: BT-1/BT-2") — its own files collapse to harness roots, its own rows leave `harness-layout-baseline.manifest` in the same diff, `#[test]` count unchanged for the batch, C2 guard green. The crate-wide `~10–15` band in PRD §3 W1 is a **cross-leaf aggregate owned by F1**, never a per-leaf gate; restating it on one batch made an unreachable RED. The reify-compiler line is completed by **CMP-3..CMP-6** (5693–5696, chained 5284 → 5693 → 5694 → 5695 → 5696, all dependencies of F1/5286); CMP-6 is the terminal leaf and re-derives its residual scope live at dispatch. |
| kLOC cap enforced upstream | **PASS** | Every consolidation leaf depends on B1 (C2 guard live before consolidation). |

## D-B (revive 4935) / D-D (revive 4936) — reify-eval-fea compile-unit isolation

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | `reify-eval` solver modules (`compute_targets/*`, `modal_ops`, `dynamics_*`, …) exist and are OCCT/Engine-free (4935's own verified move-list). Registration seam `register_production_compute_fns` is task **5072** (upstream, pending). |
| DAG-direction (anti-inversion) | **PASS** | 5072 is **upstream** of 4935 (existing edge preserved); D-D depends on D-B (4935). |
| Producer extent (anti-short) | **PASS** | B moves the solver source + co-located unit tests; the compile-unit-isolation premise is delivered by B itself (the ~25-35 kLOC move), not a downstream task. |
| Observable (BT-4) | **PASS (measured)** | D-D's signal = measured relink-wave shrink on a reify-eval-fea lib-touch vs the pre-split reify-eval baseline (`docs/measurements/`), + retained OCCT-skip proof. |
| Re-premise soundness (G6 branch 3) | **PASS** | M (4933, done) explicitly allowed a non-CPU reason; compile-unit isolation is that reason. No capability is demanded that the dependency set can't produce. |

## E1 — reify-eval release-sensitivity leaf crate

| Check | Verdict | Evidence |
|---|---|---|
| Substrate exists (G3) | **PASS** | reify-eval release-sensitive set (fresh grep, 2026-07-19): 8 src + 4 integration tests, all on disk. The 4 integration tests touch only reify-eval's public API → relocatable to a dev-dep leaf crate. |
| DAG-direction | **PASS** | Depends on D-D (4936) and EVAL-3 (reify-eval consolidation settled) — both upstream. |
| Producer extent (anti-short) | **PASS** | Leaf covers exactly the 8 src + 4 test extent; the 8 src unit tests made profile-invariant in place (C3) or behind a minimal test hook — extent matched, not name-matched. |
| Rejection/observable (BT-5) | **PASS (observed)** | `--print-plan` release role **loses** `-p reify-eval`, **gains** `-p reify-eval-release-tests`; `test_release_scoped_scope.sh` green — observed, not asserted. |
| Ownership (G4) | **PASS** | This PRD solely owns `release-sensitive-crates.txt` membership; re-derive live set at dispatch (5166 may add reify-ir). |

## F1 — program-terminal aggregation → milestone 5254

| Check | Verdict | Evidence |
|---|---|---|
| Execution path declared | **PASS** | `task_kind="deterministic"`, `execution_class="operational"` (no-code aggregation gate; depends on every workstream leaf; single edge into milestone 5254). |
| Anti-orphan | **PASS** | Consumed by milestone **5254** (`add_dependency(5254 → F1)`). |
