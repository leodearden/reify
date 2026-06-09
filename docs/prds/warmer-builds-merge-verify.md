# PRD — Warmer (faster) builds for the merge-gate verify

**Status:** active · version-agnostic infrastructure foundation · authored 2026-06-09.
**Source design:** `docs/design/warmer-builds-merge-verify.md` (measured single-session profiling pass, 2026-06-09; baselines from live production state — orchestrator journal, `sccache --show-stats`, `cargo metadata`, `verify.sh --print-plan`). Re-locate every cited symbol at implementation time — `main` moves fast; the line is a hint, cite-by-symbol.
**Scope guard (load-bearing):** the merge gate **must stay full-scope, full-correctness** (verify-scope-contract **C2**; `scripts/verify.sh:348` force-`--scope all` for `DF_VERIFY_ROLE=merge`, drift-tested by #4059). This PRD makes the gate *cheaper to run*, **never narrower**. Every lever below buys speed from **warmth / faster tooling / better scheduling**, not from coverage. See §10.

---

## 1. Goal — collapse the ~90-min serial merge lane without touching gate scope

A merge-gate verify (`DF_VERIFY_ROLE=merge` → `hooks/pre-merge-commit` → `verify.sh all --profile both --scope all`) runs **serial** (`_MERGE_AHEAD_BOUND=1`) in a **freshly-created empty-`target/`** worktree, so wall-time directly bounds landing throughput and every attempt re-pays cold build cost. **Measured: median ≈ 90 min, range 57–148 min** over 16 consecutive real attempts; the lane ran back-to-back cold full verifies for ~25 h straight in the originating red-main livelock, most *failing* (the #1688 thrash signature).

**User-observable end state (the consumer is the orchestrator merge queue + every human waiting on a land):**

| | Today (cold) | After Phase 1 | After Phases 1+2+4 |
|---|---:|---:|---:|
| Typical land (leaf delta) | ~90 min | ~25–35 min | ~15–22 min |
| Worst case (reify-core delta) | ~110–148 min | ~40–60 min | ~25–40 min |
| Merge-gate CPU-seconds (contended box) | baseline | −60–80% | −70–85% |
| Scope / correctness | full | **full (unchanged)** | **full (unchanged)** |

The wall-time figures are **projections, not gated thresholds** — see §9's signal framing (G6): no task freezes a guessed minute-count into a RED test; each task asserts a *measured improvement direction + a recorded delta vs the cold baseline*, with these numbers as the expectation.

## 2. Background — why it is cold every time, and why sccache is not the lever

**Root cause (cold every attempt).** dark-factory `git_ops.py` `_create_merge_worktree` (≈:1404) runs `git worktree add --detach .worktrees/_merge-<uuid8> <ref>`, and `cleanup_merge_worktree` (≈:1446) removes it afterward. No `CARGO_TARGET_DIR` override, no target reuse / symlink / warmth of any kind, fresh `target/` from zero (a *task* worktree's `target/` measures **177 GB** on disk; the merge worktree rebuilds a comparable tree from scratch each attempt, then deletes it). `prune_stale_merge_worktrees` (≈:1491) force-removes leftover `_merge-*` worktrees.

**sccache is on (`RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`) and is *not* the lever.** Two reframing findings from `sccache --show-stats`:
1. The expensive artifacts are **non-cacheable by design** — dominant non-cacheable reasons are `crate-type` (75,942) and `multiple input files` (83,332). sccache warms the per-crate dependency **rlib codegen** (the opt-3 dep graph — high value), but does **not** serve the workspace's `bin`/`test` final compiles — exactly the **≈745 test binaries** that run cold on every fresh worktree.
2. The fresh-`_merge-<uuid>` path **suppresses even cacheable hits** — sccache's input hash folds absolute paths (debuginfo, `CARGO_MANIFEST_DIR`, `file!`), so the ~60% cross-worktree Rust hit rate is a *ceiling depressed by path churn*, not a floor. A stable merge path lifts it for free.

**Workspace shape (`cargo metadata`).** 32 crates, **711 integration-test targets** + 31 lib + 3 bin unit-test binaries ⇒ **≈745 test binaries linked per profile per attempt**. Concentration: `reify-eval` 239, `reify-compiler` 187. The merge plan compiles the workspace **3–4× over** (clippy-all-targets, gui-check, debug-test, release-test [release already trimmed by **#4390**, done]), each pass paying the uncached test-binary + **bfd-link** cost. Active linker is **GNU `ld` (bfd) 2.42** (rustc default) — slowest option, no link caching anywhere.

**The three uncached cost centres (design §2), in priority order:**
- **(A)** repeated cold workspace compile → attacked by **target warmth** (Phase 1, the keystone).
- **(B)** ≈745 cold bfd links per profile → attacked by a **faster linker** (Phase 2) + **less debuginfo** (Phase 3).
- **(C)** serial OCCT test exec + GUI floor → attacked by **OCCT→nextest unification** (Phase 4); a floor build-warmth cannot touch.

## 3. Sketch of approach — five independently-shippable, independently-measurable phases

| Phase | Lever | Repo | Effort/Risk | Attacks |
|---|---|---|---|---|
| **0** | one controlled instrumented baseline (pin the A/B/C split) | reify (out-of-band) | one ~90-min off-peak run | measurement |
| **1** (keystone) | **persistent warm merge worktree + `target/` at a FIXED path**, reset-in-place per attempt | **dark-factory** | M / Low–Med | (A) |
| **2** | switch off bfd → faster linker (benchmark `rust-lld` vs `mold`) | reify | S / Low | (B), cold+warm |
| **3** | trim debug debuginfo (lean profile) | reify | S / Low | (B) + 177 GB disk |
| **4** | fold OCCT crates into the nextest pool (drop the separate serial gated pass) | reify | M / Med | (C) |
| **5** | measure `CARGO_INCREMENTAL=1` on the persistent lane only (A/B vs Phase 1) | reify | S / Med | experiment |
| companion | retire **#4447**'s 60→90 min timeout bump once warm holds | reify | S / Low | band-aid removal |

Land in order, re-measure after each. Phases **2, 3, 4 are independent of Phase 1** (they also speed the 24 task lanes). Phase **5 depends on Phase 1** (needs the private, stable target). The companion depends on Phase 1 + #4447.

## 4. Resolved design decisions

- **D1 — One PRD; Phase 1 filed as a `dark_factory:` task.** The keystone is structurally cross-repo (`git_ops.py` / `merge_queue.py` + a yaml knob) and reify cannot build/test dark-factory, but DF tasks share this fused-memory (`project_root=/home/leo/src/dark-factory`; e.g. #1687/#1688 live there). The effort stays one coherent PRD; at decompose, Phase 1 is filed against the DF project and the reify Phase-5 + companion tasks depend on it cross-project (the established `dark_factory:NNNN` edge pattern). *(Leo, 2026-06-09.)*
- **D2 — Phase 0 is out-of-band, not a queued task.** A one-shot off-peak instrumented full verify dispatched by the orchestrator onto the already-contended box could worsen the very livelock. Instead this PRD's decompose session **spawns Phase 0 directly during a reify-orchestrator stop→benchmark→restart window** (a restart we want anyway, so warm-build config can take effect). Phase 0 *informs* Phase 1 tuning; it does not gate the reify phases. *(Leo, 2026-06-09.)*
- **D3 — Phase 2 benchmarks both linkers in-task, keeps the winner; `rust-lld` is the tie-break.** `mold` 2.30.0 is host-installed (fastest on the 2.8 GB OCCT/OpenVDB/gmsh static stack) and `rust-lld` ships with the toolchain (zero host install, travels reproducibly across worktrees/hosts). On a near-tie prefer `rust-lld` — a missing `mold` must never be able to break every link on the load-bearing merge path. The task records the measured delta + chosen linker in a bench doc. *(Leo, 2026-06-09.)*
- **D4 — Approach = bare B + a Phase-1 invariants subsection (§10), not full B+H.** The reify phases are infrastructure wiring of *existing* capabilities (no novel substrate — none of the overlay's load-bearing seams: FEA / ComputeNode / persistent-naming / multi-kernel / grammar). The single sharp seam — reify-verify ↔ DF persistent worktree — carries a real correctness invariant (reset-in-place determinism; concurrent cargo on one `target/` is unsafe; serial-lane-only; periodic from-scratch safety-valve) that §10 captures as a contract, without the weight of a full boundary-test harness. *(Leo, 2026-06-09.)*
- **D5 — Stale-premise correction on Phase 4 ↔ task 3767.** The design says "coordinate with task 3767 Stage 2 (same migration)." **3767 is DONE** and chose a *different* mechanism — a host-wide **counting semaphore on `scripts/cargo-test-occt-gated.sh`** (commit 7b33357598), giving bounded multi-process OCCT concurrency *for the 24 task lanes*. It did **not** fold OCCT into the nextest pool; `.config/nextest.toml`'s `occt` test-group (`max-threads = 1`) is still inert (the OCCT crates remain *excluded* from the nextest pass and run via the separate gated `cargo test … -- --test-threads=1` pass). Crucially, the merge lane is **serial** (`_MERGE_AHEAD_BOUND=1`), so 3767's *cross-worktree* semaphore does not touch the merge gate's OCCT floor — that floor is still single-process `--test-threads=1`. Phase 4 is therefore a **new, distinct** task that *builds on* 3767's done semaphore (it supplies the cross-worktree bound) by raising the `occt` group's `max-threads` and routing OCCT crates through the nextest scheduler. Re-measure: 3767 may already have captured part of the floor on the task lanes, but not on the gate.

## 5. Pre-conditions for activating

- **Phase 0** (out-of-band) precedes Phase 1 tuning — pins the (A)/(B)/(C) split off-peak so Phase 1's projection is validated against a real per-step breakdown, not log archaeology.
- **Phase 5** is hard-gated on **Phase 1** (needs the isolated, stable, serial persistent target before `CARGO_INCREMENTAL=1` is even coherent — incremental is mutually exclusive with sccache and only permissible on that one private lane).
- **Companion (retire #4447)** is gated on **Phase 1** (warm verifies must demonstrably finish inside the pre-bump 60 min) **and #4447** (the 60→90 bump must have landed to be reverted; #4447 is currently `in-progress`).
- Phases **2, 3, 4** have **no upstream gate** — substrate is verified present (§7) and they are independent of Phase 1.

## 6. Substrate verification (G3) — all present, no novel substrate

This is **pure-infrastructure wiring of existing capabilities — G3 is otherwise a no-op** (no `.ri` grammar surface), but each assumed tool/symbol was verified at authoring (2026-06-09, `main 55c166430a`):

| Capability | Phase | Evidence (verified) |
|---|---|---|
| `mold` 2.30.0 on PATH | 2 | `command -v mold` → `/usr/bin/mold` |
| `rust-lld` + `gcc-ld/ld.lld` bundled | 2 | `<sysroot>/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld` + `…/gcc-ld/ld.lld` present (rustc 1.96.0) |
| `[target.x86_64-unknown-linux-gnu]` rustflags slot | 2 | `.cargo/config.toml:2` (already holds `runner`); manifold `links` override is a *separate* table (`:21`) — additive, no conflict |
| `.config/nextest.toml` `occt` test-group | 4 | present (`[test-groups] occt = { max-threads = 1 }` + `package(...)` override), explicitly "staged for Stage 2" |
| `cargo-test-occt-gated.sh` + `occt-touching-crates.txt` | 4 | exist; counting-semaphore form landed by 3767 |
| `verify.sh:348` C2 `--scope all` merge guard | scope guard | present; drift-tested #4059 |
| DF `_create_merge_worktree`:1404 / `cleanup_merge_worktree`:1446 / `prune_stale_merge_worktrees`:1491 | 1 | present in `orchestrator/src/orchestrator/git_ops.py` |
| DF `_MERGE_AHEAD_BOUND=1`:103 / `merge_verify_cold_command_timeout_secs` 7200 s | 1 | present in `orchestrator/src/orchestrator/merge_queue.py` |
| #4447's debug `outer_timeout` 60→90 bump | companion | task #4447 `in-progress` (the bump this companion reverts) |

## 7. Cross-PRD / cross-repo relationship (G4)

The one genuine seam is reify-verify ↔ dark-factory; the rest are same-incident siblings with no contested ownership. No reciprocal "the other owns it."

| Other | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| dark-factory `git_ops.py` / `merge_queue.py` | reify verify **consumes** the warm worktree DF provides | persistent fixed-path worktree lifecycle (reset-in-place, prune-exempt) + serial-lane routing + `git.persistent_merge_worktree` yaml knob | **Phase-1 DF task** (this batch, `dark_factory` project) | queued |
| #4447 (debug `outer_timeout` 60→90) | this PRD's **companion reverts** it | `verify.sh` debug `outer_timeout` | companion task (this batch); gated on Phase 1 + #4447 | queued |
| #4390 (release-pass scoping) | already in the baseline | `verify.sh` release pass `-p` set | #4390 | **done / landed** |
| #4448 (fail-fast ordering + cheap-gate parallelism) | sibling, same incident — bounds the *failing* path; warm-build is the *happy-path* complement | `verify.sh` step ordering | #4448 (separate, `pending`) | independent — land alongside |
| task 3767 (OCCT counting semaphore) | Phase 4 **builds on** it (supplies the cross-worktree bound) | `cargo-test-occt-gated.sh` + `.config/nextest.toml` `occt` group | 3767 (**done**); Phase 4 folds OCCT into nextest | done / built-on (see D5) |
| #1687 / #1688 (DF skip-verify SHA pin / thrash signature) | sibling incident fixes | `merge_queue` re-merge skip path / thrash keying | separate DF tasks | independent |

## 8. (intentionally folded into §3/§9 — no separate "why deferred")

This PRD is **active**, not deferred: every phase is shippable now. The only sequencing is the §5 dependency set.

## 9. Decomposition plan — task DAG with observable signals (G2)

Greek labels here; task IDs assigned at decompose. **Phase 0 is NOT a filed task** (D2). All wall-time numbers are *expectations*; each signal asserts **measured improvement direction + a recorded delta vs the cold baseline**, never a frozen minute-threshold (G6 — avoid the esc-3453 "guessed bound frozen into a RED test" failure).

- **κ — Phase 1 · dark-factory · persistent warm merge worktree (the keystone).** *(intermediate: unlocks δ + companion; also a first-class deliverable.)* Replace the create-fresh/destroy `_merge-<uuid>` lifecycle *for the verifying serial lane* with a single persistent worktree at a fixed path (e.g. `.worktrees/_merge-verify`), reset-in-place per candidate (`git reset --hard <merge-commit> && git clean -xfd -e target`, `target/` retained), exempt from `prune_stale_merge_worktrees`, routed only under `_MERGE_AHEAD_BOUND=1`. Gate behind `git.persistent_merge_worktree` yaml (default off; reify opts in). **Signal:** orchestrator journal shows a warm merge-gate verify (`verify start`/`verify end` pair) completing far below the cold-baseline median **and** the worktree persisting across attempts (not pruned) **and** the §10 safety-valve from-scratch verify still passing. *Modules:* `orchestrator/src/orchestrator/git_ops.py`, `merge_queue.py`, defaults yaml. *(Repo: dark-factory.)*
- **α — Phase 2 · reify · switch off bfd.** Add target-scoped `rustflags = ["-Clink-arg=-fuse-ld=<winner>"]` to `.cargo/config.toml`'s `[target.x86_64-unknown-linux-gnu]` (x86_64-linux only; wasm/emscripten keep bfd). Benchmark `rust-lld` vs `mold` on a representative relink; keep the winner, `rust-lld` tie-break (D3). Confirm no crate passes a bfd-specific linker arg (design checked: only manifold's `static=…`/`stdc++`, both linkers handle). **Signal:** `.cargo/config.toml` carries the `-fuse-ld` flag; a committed bench doc records chosen linker + bfd-vs-winner link-time delta on a real relink; a full `cargo build` links via the new linker with no regression (a normal task verify stays green). *Leaf.*
- **β — Phase 3 · reify · trim debug debuginfo.** A dedicated lean profile (or `debug = 1` / `split-debuginfo = "unpacked"` for dev tests) in `Cargo.toml`, preserving enough for test backtraces. **Signal:** `target/` size shrinks measurably (recorded delta) **and** a deliberately-panicking test still resolves `file:line` in its backtrace. *Leaf.* (Best measured *after* α so the debuginfo delta is on the new linker — soft ordering, not a blocking dep.)
- **γ — Phase 4 · reify · fold OCCT into the nextest pool.** Raise `.config/nextest.toml`'s `occt` group `max-threads` to a bounded N (FD/memory-headroom-capped, per 3767's rationale), route the OCCT-touching crates through the nextest pass, and drop the separate `cargo-test-occt-gated.sh --test-threads=1` pass from `verify.sh`. Per-process address-space isolation keeps OCCT race-free (3767's established insight). Builds on 3767 (done), not a re-do (D5). **Signal:** OCCT crates run inside nextest (green, race-free over K repeat runs on an idle box) with the separate gated pass removed from the merge plan, and a bench doc records the OCCT-phase wall-time delta. *Leaf.*
- **δ — Phase 5 · reify · measure `CARGO_INCREMENTAL=1` on the persistent lane only.** *(depends_on κ.)* A/B the persistent merge lane with vs without `CARGO_INCREMENTAL=1` (lane-scoped — never global; it is mutually exclusive with sccache and would break the 24 task lanes' cross-worktree rlib sharing). **Signal:** a committed A/B bench doc (Phase-1-alone vs Phase-1+incremental on the same lane) **plus** an adopt/reject decision wired to the lane-scoped config (adopt *only* if it wins). *Leaf.*
- **ε — companion · reify · retire #4447's timeout bump.** *(depends_on κ + #4447.)* Revert the debug `outer_timeout` 60→90 (and the consistency `gated_timeout` 3600→5400) bump once warm verifies demonstrably finish inside 60 min. **Signal:** the timeout config is reverted **and** the orchestrator journal shows a warm merge verify completing inside the reverted budget with no `exit 124` timeout regression. *Leaf.*

**DAG:** α, β, γ independent leaves. κ (DF) → δ, κ + #4447 → ε. Phase 0 out-of-band (D2).

## 10. Phase-1 correctness invariants & safety valves (the §D4 contract)

Phase 1 sits on the load-bearing path to `main`; a stale-fingerprint false-GREEN would land unverified code — the exact failure the gate exists to prevent. κ MUST hold all of:

1. **Reset-in-place determinism.** Per attempt: `git reset --hard <merge-commit>` then `git clean -xfd -e target` (everything *except* `target/` cleaned), so the source tree is bit-identical to a fresh checkout of that commit while `target/` is retained. Correctness rests on cargo's own fingerprinting recompiling precisely the changed crates + their reverse-dep closure — *exactly* how normal local dev reuses `target/` across commits.
2. **Fixed path, not a moving `CARGO_TARGET_DIR`.** The worktree path itself must be stable (e.g. `.worktrees/_merge-verify`), not a fixed `CARGO_TARGET_DIR` under changing worktree paths — otherwise path-sensitive fingerprints/debuginfo (`CARGO_MANIFEST_DIR`, `file!`) invalidate the warmth and re-suppress sccache hits. Stable path is *also* the §1.2 sccache-hit-rate bonus.
3. **Serial-lane-only invariant.** The warm worktree is single-consumer **only because** the lane is serial (`_MERGE_AHEAD_BOUND=1`). Concurrent cargo against one `target/` is unsafe. **If `_MERGE_AHEAD_BOUND` is ever raised >1, Phase 1 must become a small worktree pool or revert** — a startup guard should assert `_MERGE_AHEAD_BOUND==1` whenever `git.persistent_merge_worktree` is on.
4. **Prune exemption.** `prune_stale_merge_worktrees` force-removes `_merge-*`; the warm worktree MUST be exempt (a distinct name / keep-list) or it gets eaten between attempts.
5. **Ephemeral path retained for non-building probes.** Speculative / conflict-probe merges stay on the ephemeral `_merge-<uuid>` path (they don't build); only the *verifying* serial attempt uses the warm worktree.
6. **Periodic from-scratch safety valve.** Every Nth land (or nightly) runs one from-scratch verify in a throwaway worktree to catch any fingerprint-staleness corner case; a divergence between warm and cold result is a hard alarm, not a silent pass.
7. **No `.mcp.json` concern (verified).** Merge worktrees check out the committed `.mcp.json` (`:3939`) but never run `setup-worktree-debug-port.sh` and host no MCP client (headless verify); reset-in-place just re-checks-out the committed default. The skip-worktree hygiene matters only for dispatched-agent *task* worktrees.

## 11. Out of scope

- **Narrowing the merge-gate scope** — **FORBIDDEN** (C2 / §1 scope guard). Speed comes from warmth, never coverage.
- **Global `CARGO_INCREMENTAL=1`** — mutually exclusive with sccache; breaks cross-worktree rlib sharing for the 24 task lanes + merge lane. Permissible only on the isolated serial persistent lane (δ), measured.
- **Sharing one persistent `target/` across concurrent merges** — safe only at `_MERGE_AHEAD_BOUND=1` (invariant 3).
- **Reusing a *task* worktree's `target/` for merges** — different flags/contention; the warm worktree is dedicated to the merge lane.
- **Skipping the gate / SHA-pinning as a throughput fix** — that is #1687's separate, deliberately-bounded concern; warm builds make the *real* gate cheap, so skipping is unnecessary.
- **Raising the verify timeout further as a throughput fix** — #4447 treats the symptom; Phase 1 removes the cause (companion ε reverts it).
- **lever #6 sccache stable-hashing, lever #7 nextest `archive`, lever #8 codegen-units tuning** — mostly subsumed by Phase 1's fixed path / low leverage; kept as fallbacks if a persistent worktree is rejected. Not filed.
- **#4448 fail-fast ordering** — separate task, lands alongside (the *failing*-path complement); not part of this PRD.
- **Durable per-step timing emission** (design §7: "emit per-step durations to the event store on success too") — a small DF `verify.py` change that would make future tuning / Phase-5 A/B / safety-valve monitoring measurable without log archaeology. Optional DF follow-up; not filed here. (`reify-jobserver-canary.service` `failed`-state is an unrelated glance item, §7.)

## 12. Open questions (tactical — surfaced, not blocking)

1. **Phase 4 `occt` group `max-threads` value.** The bounded N for FD/memory headroom. **Suggested resolution:** reuse 3767's chosen cap rationale (`docs/notes/multi-process-occt-bench.md`); confirm on an idle box. Decide during γ.
2. **Phase 3 mechanism — dedicated lean profile vs `debug=1`/`split-debuginfo`.** **Suggested resolution:** `split-debuginfo = "unpacked"` for dev tests keeps backtraces while moving debug data out of the link; fall back to `debug=1` if backtraces degrade. Decide during β.
3. **Phase 1 safety-valve cadence (N).** Every-Nth-land vs nightly. **Suggested resolution:** nightly + every 20th land, tightened if any warm/cold divergence is ever observed. Decide during κ.
4. **Phase 5 adoption threshold.** How much incremental must beat Phase-1-alone to adopt. **Suggested resolution:** adopt only on a ≥15% lane-wall win with no correctness divergence; else keep sccache-only. Decide during δ.
