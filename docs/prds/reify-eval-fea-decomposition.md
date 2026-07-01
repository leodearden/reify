# reify-eval crate decomposition for OCCT-scoped FEA test selection (Design X, measure-first)

**Status:** deferred / **measure-gated** — 2026-07-01. Supersedes blocked task **3768** ("Decompose reify-eval 2421-test monolith into finer crates for dep-driven test selection"). Ratifies the **Design X** ruling (Leo, 2026-07-01) over the architect's proposed Design Y (esc-3768-97).
**Depends on:** task **3766** (done — `scripts/verify.sh` reverse-dep `--scope branch` machinery, merged `31df17e`).
**Relationship:** refines `docs/prds/verify-scope-contract.md` (Lever S touches its `affected-crates-lib.sh` reverse-closure; must preserve contract invariants C1–C6, especially **C2 merge-gate-never-narrows**).

## Goal

On a typical OCCT-touching branch change, the verify pipeline should run **fewer** tests — skipping FEA test groups that provably cannot be affected by an OCCT change — cutting **total test CPU**, not just wall-clock. `reify-eval` today is a 2421-test monolith whose one integration binary is ~379 s of a ~465 s run; crate-level reverse-dep selection is coarse precisely because everything OCCT-adjacent lives in that one crate.

User-observable proof: a committed selection-proof infra test (`tests/infra/test_fea_crate_selection.sh`) shows that a change to `reify-kernel-occt` yields an `affected-crates` set that **excludes** the new pure-FEA crate — so its tests are not selected on OCCT changes.

## Background — the two selection mechanisms (corrected premise)

The architect's Design Y (esc-3768-97) rested on the claim that a crate dev-depping `reify-eval` is "re-tainted OCCT-touching," forcing a full sibling inversion up to `reify-cli`. **Direct verification of the substrate falsifies that claim.** There are two *independent* scope machineries that treat dev-deps **oppositely**:

1. **OCCT gate membership** — `scripts/occt-scope-lib.sh` `occt_touching_set` (`:101-104`). A crate is OCCT-touching iff `reify-kernel-occt` is in its own **normal/build** closure OR in the normal/build closure of one of its **direct** dev-deps. Dev-deps are **non-transitive** here. `reify-eval`'s OCCT edge is **dev-dep-only** (`crates/reify-eval/Cargo.toml:83`, `reify-kernel-occt` under `[dev-dependencies]`; its normal deps are all OCCT-free). ⇒ A crate that merely dev-deps `reify-eval` is **NOT** OCCT-touching. The Design-Y "forced sibling topology" premise does not hold.

2. **Test selection** — `scripts/affected-crates-lib.sh` reverse-closure (`:115-124`). Walks **all** dep kinds (normal/build/**dev**) **transitively**. This is *stricter* than cargo's real compile semantics.

A third verified fact (agent team, 2026-07-01): **the FEA solve is already OCCT-independent.** Engine-driven FEA e2e tests use the kernel-less `make_simple_engine` (`crates/reify-test-support/src/helpers.rs`); a `box(...)` stays a deferred `GeometryHandle` and the solve consumes a **synthetic** box-tet mesh (task 4091), not an OCCT mesh. Only **3** FEA e2e files genuinely realize geometry via OCCT (`fea_face_selector_bc_e2e.rs`, `dynamics_body_mass_props.rs`, `rigid_moment_of_inertia_autoderive_smoke.rs`).

### The achievable prize (what can/can't be skipped on an OCCT change)

A test binary can be **skipped** on an OCCT change only if its host crate has **no** workspace dep-path (any kind) reaching `reify-kernel-occt`.

| FEA test group | ~size | Skippable on OCCT change? |
|---|---|---|
| Pure solver **unit** tests (no Engine; build `VolumeMesh` in-test) | ~206 | **Yes** — moved to `reify-eval-fea`, which has no OCCT path. (Design X **and** Y both deliver this.) |
| Engine-driven **synthetic-mesh** FEA e2e (need the Engine, not OCCT) | ~30 files / ~80 fns | **Only after Lever P + Lever S** — moved to an OCCT-free crate (P), then `affected-crates` aligned to cargo's non-transitive dev-dep semantics (S). |
| Genuinely **geometry-driven** FEA e2e (need OCCT) | 3 files / ~5 fns | **No** — correctly rerun; they touch OCCT. |

Design Y's entire incremental value over Design X collapses to **signal #3** (a FEA-*only* change also skips `reify-eval`'s tests) — and it buys **zero** additional OCCT-skip (its migrated e2e land in `reify-cli`, still OCCT-touching), at the cost of a foundation crate + full registration inversion + ~30 e2e migrations. **Rejected** in favour of Design X + the P/S track.

## Resolved design decisions (ratified)

1. **Design X, not Design Y.** `reify-eval` *depends on* the new pure-FEA crate; registration stays in `reify-eval`; **no** `reify-cli` registration inversion. Rationale: the corrected premise (§Background mechanism 1) removes Y's justification, and Y buys only signal #3.
2. **Measure first.** Task **M** profiles the FEA test CPU by category and writes `docs/measurements/fea-decomposition-selection.md` with a go/no-go. A/B/D and P/S are **filed but held `deferred`** until M's numbers clear the flip-condition (§Pre-conditions). This validates the premise empirically before paying for the split (the achievable prize is bounded — see table).
3. **Signal #3 is rescoped.** "Touching one sub-area selects only that crate's tests" is *not* literally achievable for engine-driven FEA e2e tests (the Engine welds them to `reify-eval`). It is delivered for the **pure solver unit tests** (via B, OCCT direction) and, conditionally, for the ~30 engine-driven e2e (via P+S). Full sibling inversion (Y) for the residual is **out of scope** (documented deferred alternative).
4. **Lever P is scheduled iff M shows a worthwhile win** (Leo, 2026-07-01). **Lever S is scheduled, depending on P** (Leo, 2026-07-01) — the two compose to skip the ~30 engine-driven synthetic-mesh e2e on OCCT changes.
5. **Lever S is a precision fix, not a coverage cut.** Aligning `affected-crates-lib.sh` to cargo's non-transitive dev-dep compile semantics (which `occt-scope-lib.sh` already models) only stops selecting binaries that *cannot* be affected by the change (the changed crate is not compiled into them). **C2 (merge gate forces `--scope all`) is preserved**, so any residual mis-selection can at worst shift a failure from branch-verify-time to merge-gate-time detection — it can never reach `main` undetected. Coverage of `main` is unchanged. (See §Contract, §Boundary tests.)

## Contract (the crate + selection seam — H component)

The ComputeFn contract itself is **not** being designed here — it ships in `docs/prds/v0_3/compute-node-contract.md` §4/§5. This PRD relocates it and pins the crate-topology + selection invariants.

**Crate topology (Design X):**
```
reify-compute-contract   (NEW, OCCT-free foundation)
      ▲                ▲
reify-eval-fea ────────┘   (NEW, OCCT-free, Engine-free; deps: contract + reify-solver-elastic/fdm/stdlib + faer)
      ▲
reify-eval  ───────────────►  reify-compute-contract   (re-exports contract types; keeps OCCT dev-deps; registration stays here)
```

**Invariants (each is a boundary-test assertion):**
- **INV-1 OCCT-free FEA crate.** `cargo tree -p reify-eval-fea -e no-dev | grep reify-kernel-occt` is empty; `occt_touching_set` excludes `reify-eval-fea`. Same for the P e2e crate (`-e normal,build`, dev-dep on `reify-eval` allowed).
- **INV-2 re-export compatibility.** `reify-eval` re-exports `ComputeFn/ComputeOutcome/RealizedContent/RealizationReadHandle/StructuredComputeDetail/DispatchError/CancellationHandle/ElasticResult/ShellChannels`; downstream (`reify-cli/mcp/lsp/runtime/mesh-morph`, `gui/src-tauri`) compiles **unchanged**.
- **INV-3 registration unchanged.** `reify_eval::compute_targets::register_compute_fns(&mut Engine)` still registers every solver trampoline (now imported from `reify-eval-fea`); no dispatch-table regression (inventory-linkage preserved — cf. `reify-eval/Cargo.toml:121-149` dead-strip invariants).
- **INV-4 mechanism-1 selection (OCCT skip).** A `reify-kernel-occt` source change's `affected-crates` set **excludes** `reify-eval-fea` (Track 1) and, post P+S, the OCCT-free e2e crate (Track 2).
- **INV-5 mechanism-2 merge-gate completeness (C2).** With `DF_VERIFY_ROLE=merge`/`MERGE_HEAD`, `verify.sh` forces `scope=all` regardless of the Lever-S refinement. The narrowing is branch-verify-only.

## Boundary-test sketch (the integration-gate signals — closes G2)

| # | Facing | Scenario | Precondition | Postcondition (asserted) |
|---|---|---|---|---|
| BT-1 | Track-1 producer | reify-eval-fea is OCCT-free | B landed | `cargo tree -p reify-eval-fea -e no-dev` shows no `reify-kernel-occt`; `test_occt_gated_scope.sh` green with `reify-eval-fea` absent from `occt-touching-crates.txt` |
| BT-2 | Track-1 selection | OCCT change skips FEA unit tests | B landed | `tests/infra/test_fea_crate_selection.sh`: a `reify-kernel-occt` change's affected set **excludes** `reify-eval-fea` |
| BT-3 | consumer compat | downstream unbroken | A landed | `cargo build --workspace` green; `reify-cli`/`gui` compile with no source edits beyond re-export path |
| BT-4 | Track-2 producer | e2e crate is OCCT-free | P landed | `cargo tree -p <e2e-crate> -e normal,build` shows no `reify-kernel-occt`; the ~30 e2e run in the fast pool, not the OCCT gate |
| BT-5 | Track-2 selection | OCCT change skips synthetic-mesh e2e | P+S landed | `tests/infra/test_affected_crates_lib.sh`: a `reify-kernel-occt` change's affected set **excludes** the OCCT-free e2e crate (dev-dep-non-transitive) |
| BT-6 | merge-gate safety | S never narrows the merge gate | S landed | `test_verify_scope.sh` MG-* scenarios: `DF_VERIFY_ROLE=merge` ⇒ `scope=all` unchanged (C2) |

## Pre-conditions for activating

- **M gate (the flip-condition for A/B/D/P/S).** M's `docs/measurements/fea-decomposition-selection.md` reports, for a representative OCCT-change branch verify, the FEA test CPU split across {pure unit, synthetic-mesh e2e, geometry e2e}. **Proceed** (flip A/B/D — and P/S — `deferred`→`pending`) iff the **skippable-on-OCCT-change** FEA test CPU is material: provisionally **≥ ~60 s** or **≥ ~15 %** of the OCCT-gated test CPU (Track 1 counts the pure-unit slice; the P/S track counts the ~30 synthetic-mesh e2e). If below, **cancel** A/B/D/P/S and keep `reify-eval` monolithic. (Threshold is provisional — refine against M's actual numbers; see Open questions.)
- 3766 done (met).

## Cross-PRD relationship

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/verify-scope-contract.md` | refines | `affected-crates-lib.sh` reverse-closure dev-dep semantics (Lever S); C2 preservation | **this PRD** (task S) | queued (S) |
| `docs/prds/v0_3/compute-node-contract.md` | consumes | `ComputeFn`/`ComputeOutcome`/`RealizedContent` types (relocated to `reify-compute-contract`) | compute-node-contract (shipped) | wired |

**No cross-repo dark-factory seam.** Unlike warm-lane / cpu-governance, this is entirely reify-side: new crates + `scripts/{occt-touching-crates.txt,affected-crates-lib.sh}` + `tests/infra/*`. `occt-touching-crates.txt` is self-policing (drift test auto-derives from `cargo metadata`), and the `--scope all` plan-count is unchanged by crate growth (throughput sentinel `all=14` untouched), so no orchestrator-side wiring is required.

## Decomposition plan

Two tracks, both gated on **M**. Track 2 (P→S) is largely independent of Track 1 (A→B→D): P only needs `reify-eval`'s public `register_compute_fns`, not the split.

- **M — measure FEA test CPU by category** (`docs/measurements/fea-decomposition-selection.md`). *Signal:* committed doc with per-category CPU numbers (pure-unit / synthetic-mesh-e2e / geometry-e2e) for a representative OCCT-change branch verify + a go/no-go against the §Pre-conditions threshold. *Unlocks:* A, P. *Modules:* `docs/measurements/`. No source lock (reads/runs tests only).
- **A — extract `reify-compute-contract` (OCCT-free foundation).** Move `ComputeFn/ComputeOutcome/StructuredComputeDetail/DispatchError` (`engine_compute.rs`), `RealizationReadHandle/RealizedContent` (`engine_compute.rs`), `CancellationHandle` (`graph.rs`), `ElasticResult/ShellChannels` (`persistent_cache.rs`) into a new crate; `reify-eval` re-exports (INV-2). *Signal:* `cargo build --workspace` green + downstream compiles unchanged + `cargo tree -p reify-compute-contract` OCCT-free. *Depends:* M. *Unlocks:* B.
- **B — create `reify-eval-fea` (OCCT-free, Engine-free).** Move pure solver trampolines (`compute_targets/*`, `modal_ops`, `dynamics_ops`, `dynamics_psd`, `trajectory_ops`, `multi_load_dispatch`, `solver_progress`) + their ~206 co-located unit tests; deps = contract + `reify-solver-elastic/fdm/stdlib` + `faer`; `reify-eval` depends on it; registration stays in `reify-eval` (INV-3). *Signal:* workspace builds + all tests pass + `cargo tree -p reify-eval-fea -e no-dev` OCCT-free (INV-1); the ~206 unit tests now run under `reify-eval-fea`. *Depends:* A. *Unlocks:* D.
- **D — scope wiring + selection proof (Track-1 LEAF).** Keep `reify-eval-fea` OUT of `scripts/occt-touching-crates.txt`; update `tests/infra/test_occt_gated_scope.sh` (self-policing — assert OCCT-free); reconcile `scripts/release-sensitive-crates.txt` if any `#[cfg_attr(debug_assertions,ignore)]` FEA tests move; add `tests/infra/test_fea_crate_selection.sh` proving a `reify-kernel-occt` change excludes `reify-eval-fea` (BT-2); document the example in the measurements doc. *Signal:* `test_fea_crate_selection.sh` + `test_occt_gated_scope.sh` green (BT-1, BT-2). *Depends:* B.
- **P — OCCT-free integration crate for engine-driven synthetic-mesh FEA e2e.** Create a new integration-test crate (dev-deps `reify-eval` + `reify-test-support`, **NOT** `reify-kernel-occt`/`-gmsh`); move the ~30 synthetic-mesh FEA e2e files into it; **leave the 3 geometry-driven e2e in `reify-eval`**. *Signal:* new crate builds + the ~30 e2e pass in the fast pool (not the OCCT gate) + `cargo tree -p <e2e-crate> -e normal,build` OCCT-free (INV-1, BT-4); `test_occt_gated_scope.sh` still green. *Depends:* M. *Unlocks:* S.
- **S — align `affected-crates-lib.sh` to cargo's non-transitive dev-dep semantics (Track-2 LEAF).** Refine the reverse-closure so a crate's dev-deps are only walked for the direct crate (mirror `occt-scope-lib.sh`'s `normal_closure`-of-direct-dev-deps model); add a `tests/infra/test_affected_crates_lib.sh` scenario pinning that a `reify-kernel-occt` change **excludes** the P e2e crate (BT-5); preserve C2 (BT-6). *Signal:* `test_affected_crates_lib.sh` green with the new dev-dep-non-transitive assertion + `test_verify_scope.sh` MG-* green (C2 intact). *Depends:* P.

## Out of scope for this PRD

- **Design Y (full sibling inversion).** Registration inversion up to `reify-cli` + e2e migration out of `reify-eval` to satisfy signal #3 *literally* for engine-driven tests. Documented deferred alternative; revisit only if a measured need for FEA-only-change isolation emerges after Track 1/2 land.
- **Making `reify-eval` itself OCCT-free** (evicting all geometry integration tests). Massive blast radius; not justified by the achievable prize.
- **Coverage-based test-impact (e.g. `cargo-difftests`).** Correctness risk for a geometry kernel (stale coverage map silently skips a regression test); explicitly rejected in 3768's own scope note.

## Open questions (tactical — decide during M / impl)

1. **The exact go/no-go threshold.** §Pre-conditions gives a provisional `≥60 s` / `≥15 %`. Refine against M's real per-category numbers when M lands. *Suggested:* Leo confirms the number when reviewing M's doc.
2. **The P e2e crate's name/location.** `reify-eval-fea-tests` (top-level integration crate) vs folding into an existing test crate. *Suggested:* new top-level `reify-eval-fea-tests` crate. Decide during P.
3. **Whether `reify-eval` still qualifies for `release-sensitive-crates.txt`** after B moves solver tests. Decide during D by re-running the grep derivation.
