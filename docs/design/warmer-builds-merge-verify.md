# Design — Warmer (faster) builds for the merge-gate verify

**Status:** design + research, **/prd-candidate** — do not implement from this doc directly; hand off via `/prd` (decompose) or `/do`. This is research + design only; no code, config, or orchestrator changes were made producing it.
**Provenance:** single-session design + profiling pass (2026-06-09), spawned from the red-main merge-queue-livelock `/deb` investigation. All baselines below are **measured from live production state** (orchestrator journal, `sccache --show-stats`, `cargo metadata`, `verify.sh --print-plan`, on-disk worktrees) — not estimated, except where a line is explicitly labelled *(derived)*.
**Code anchors** are as of `main 55c166430a`; main moves fast, so **re-locate every symbol at implementation time** — cite-by-symbol, the line is a hint.
**Scope guard:** the merge gate **must stay full-scope, full-correctness** (verify-scope-contract C2). This doc makes the gate *cheaper to run*, never *narrower*. See §8 "What NOT to do".
**Companions (same incident, do not duplicate):** reify **#4448** (verify.sh fail-fast ordering + bounded cheap-gate parallelism — the *doomed-path* fix), reify **#4447** (debug timeout 60m→90m — a band-aid this design should retire), reify **#4390** (release-pass scoping — **already landed**), dark-factory **#1687** (skip-verify SHA pinning) / **#1688** (thrash signature). Warm-build is the **throughput complement** to #4448: #4448 bounds the *failing* path to minutes; warm-build shortens the *happy/landing* path.

---

## 1. Problem — the serial merge lane spends ~90 min per attempt re-doing cold work

A merge-gate verify (`DF_VERIFY_ROLE=merge` → `hooks/pre-merge-commit` → `scripts/verify.sh all --profile both --scope all`) runs in a **freshly created, empty-`target/` git worktree** and is **serial** (`_MERGE_AHEAD_BOUND=1`, dark-factory `merge_queue.py:103`). So merge-gate wall-time directly bounds landing throughput, and every attempt re-pays cold build cost.

**Measured merge-verify durations** — 16 consecutive real attempts, from `journalctl --user -u orchestrator-reify` `merge_queue` `verify start`/`verify end` pairs (2026-06-08 → 06-09):

| Task | Wall | Passed | | Task | Wall | Passed |
|---|---:|:--:|---|---|---:|:--:|
| 4079 | **97m** | ✓ | | 2995 | **80m** | ✗ |
| 4093 | **144m** | ✗ | | 4164 | **80m** | ✗ |
| 4093 (retry) | **148m** | ✗ | | 4318 | **79m** | ✗ |
| cargo-run-prebuilt-fix | **57m** | ✗ | | 4421 | **85m** | ✗ |
| 4369 | **144m** | ✗ | | 4412 | **109m** | ✗ |
| 4331 | **110m** | ✓ | | 4284 | **92m** | ✗ |
| 3437 | **90m** | ✗ | | (3 pre-rebased) | **~0m** | ✓ |

- **Median ≈ 90 min; range 57–148 min.** The 85 min in the spawn brief sits squarely in the distribution.
- The lane ran **back-to-back full verifies for ~25 h straight**, most **failing** — the thrash signature (#1688). #4447 confirms the **debug pass alone exceeds the 60 min `outer_timeout`** cold, which is *why* it is being bumped to 90 min. Warm builds make that bump unnecessary.
- A minority of lands are instant: `merge_queue` logs `skipping re-verification (pre-rebased, main unchanged)` — those bypass the gate entirely and are **not** the target here. The target is the **cold full verify** that every non-trivial land must pay.

### 1.1 Why it is cold every time (root cause)

dark-factory `git_ops.py` `_create_merge_worktree` (≈:1404) runs **`git worktree add --detach <path> <ref>`** into `.worktrees/_merge-<uuid8>` (resolves via symlink to `/media/leo/data_lv_1/leo/reify-build/worktrees/`), then `cleanup_merge_worktree` (≈:1446) `git worktree remove --force`s it after. There is:
- **no `CARGO_TARGET_DIR`** override (orchestrator sets none; cargo uses the worktree-local `target/`),
- **no target reuse, symlink, or warmth** of any kind across attempts (verify.py `_is_verify_cold` always reports cold for `_merge-*`, so the 7200 s cold timeout applies),
- a **fresh `target/` built from zero** every attempt. (A *task* worktree's `target/` measures **177 GB** on disk — the merge worktree rebuilds a comparable tree from scratch each time, then deletes it.)

So every merge attempt re-pays: full cargo fingerprint/dep-graph assembly from empty, **every test-binary compile + link**, **all test execution**, **all build scripts**, and the **entire GUI step** — keeping only what sccache can serve.

### 1.2 sccache is on, and it is *not* the lever

`RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0` (incremental is deliberately off so sccache artifacts are shared across the ~300 worktrees). `sccache --show-stats` (cumulative, 100 GiB cap, 36 GiB used):

```
Cache hits rate (Rust)        59.92 %      Compilations            16134
Cache hits (Rust)             23091        Compilation failures      142
Cache misses (Rust)           15444        Average compiler         3.700 s
Non-cacheable calls          160946        Average cache read hit   0.002 s
Non-cacheable reasons:  crate-type 75942   multiple input files 83332  …
```

Two findings that **reframe the problem**:

1. **The expensive artifacts are non-cacheable by design.** The dominant non-cacheable reasons are `crate-type` (75,942) and `multiple input files` (83,332). sccache caches per-crate **library `rlib` codegen** (the 23k Rust hits — these are mostly the dependency graph, which `[profile.dev.package."*"] opt-level = 3` makes *expensive* to produce, hence high value to cache). It does **not** serve the workspace's **`bin`/`test` final compiles** — and those are exactly the ~745 test binaries (see §1.3). **sccache warms the deps; the merge-gate long pole is the uncached workspace + test-binary compile/link, which runs cold on every fresh worktree.**
2. **The fresh worktree path actively suppresses even the cacheable hits.** sccache's input hash folds in absolute paths (debuginfo, `CARGO_MANIFEST_DIR`, `file!`). Every merge runs under a **new** `_merge-<uuid>` path, perturbing those inputs — so the cross-worktree Rust hit rate (≈60%) is a *ceiling depressed by path churn*, not a floor. A **stable** merge path would lift it for free (§5, Phase 1 bonus).

### 1.3 Workspace shape — where the uncached cost lives

`cargo metadata --no-deps`:
- **32 workspace crates** (31 lib/proc-macro + 3 bin), **711 integration-test targets**, 10 build scripts.
- Each integration-test file is its **own crate → its own binary that must be linked**. Plus 31 lib unit-test binaries + 3 bin unit-test binaries ⇒ **≈745 test binaries linked per profile per attempt.**
- Concentration: `reify-eval` **239** test targets, `reify-compiler` **187**, `reify-syntax` 57, `reify-kernel-occt` 37, `reify-cli` 29, `reify-expr` 25, `reify-solver-elastic` 20.
- **Linker (CORRECTED 2026-06-09, post-`/prd` empirical probe, esc-4449-206):** the active linker is **already `rust-lld` (LLD 22.1.2)** — rustc 1.96 defaults to its bundled lld on this target. Verified: a default-linked binary's `.comment` reads `Linker: LLD 22.1.2`, and `-Clink-arg=-fuse-ld=bfd` *removes* that line (so bfd is opt-in and currently **unused**; no repo override forces it). My earlier "all ~745 binaries link with bfd, the slowest option" claim was **wrong** — an inference from a PATH probe (`ld.lld`/`mold` absent on PATH) that missed rust-lld living inside the rustc sysroot. **Consequence:** the ~745 links are uncached (sccache never caches linking) but already run on a *fast* linker, so the "swap off bfd" win evaporates. The only remaining linker lever is a marginal **rust-lld → `mold` 2.30.0** (now installed) A/B plus a guard that nothing accidentally forces bfd — see the re-scoped Phase 2. **Importantly, every measured number in this doc already reflects rust-lld**, so the ~90-min baseline and compile anchors are unchanged by this correction.

### 1.4 The plan the merge gate actually runs

From `verify.sh --print-plan` semantics (`scripts/verify.sh` `build_plan`/`add_test_passes`) for `all --profile both --scope all` (merge role; note `--include-infra` is **not** passed by `pre-merge-commit`, so infra checks are skipped at merge):

1. `check-manifold-deps.sh`, `tree-sitter-generate.sh` — preflight, ~seconds.
2. **`cargo clippy --workspace --all-targets -- -D warnings`** — compiles **all** targets (lib + bins + **711 tests**) through clippy-driver. A full second compile of the workspace; deps sccache-hit, workspace+test crates cold.
3. **`cargo check -p reify-gui --features gui --tests`** — the `#[cfg(feature="gui")]` surface, a third partial compile.
4. **Debug test pass:** OCCT-gated `cargo test -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config -- --test-threads=1` (**serial execution**, gated by `cargo-test-occt-gated.sh`) **+** ungated `cargo nextest run --workspace --exclude <4 occt crates>` (compiles ~all test bins, then runs **4789 tests**).
5. **Release test pass:** scoped to release-sensitive crates only (**already trimmed by #4390**) — gated + ungated, incl. the release-only buckling/eigensolve tests.
6. **GUI:** `(cd gui && npm ci && npm run typecheck && npm test)` + sidecar `npm ci && typecheck` + tree-sitter `npm ci`.

The workspace is thus compiled **3–4 times over** (clippy-all-targets, gui-check, debug-test, release-test), each pass paying the uncached test-binary + link cost.

---

## 2. Measured / derived time breakdown of a cold ~90-min merge verify

Anchored on cargo's **own** self-reported timings harvested from failed-merge journal dumps (verify.py buffers step output and dumps it to the journal *only on failure* — a passed verify logs only "All checks passed" and the detail dies with the deleted worktree; see §7), plus the structural counts above. Mid-points; release is post-#4390 (trimmed).

| Phase | What dominates the cost | sccache helps? | Est. wall (cold, contended) | Basis |
|---|---|:--:|---:|---|
| Preflight (manifold/tree-sitter) | scripts | n/a | ~1 min | plan |
| `clippy --workspace --all-targets` | cold compile of workspace + 711 test crates via clippy-driver | deps only | **~12–20 min** | *(derived)* full all-targets compile |
| `cargo check -p reify-gui --features gui` | gui-feature compile | deps only | ~2–4 min | *(derived)* |
| Debug test **compile + link** | ~745 test-bin compile; 745 links (already **rust-lld**) | deps only | **~10–15 min** | **measured** `Finished dev … in 6m13s–11m53s` |
| Debug **OCCT-gated exec** (`--test-threads=1`) | **serial** run of reify-eval(239)/occt/cli/config tests | n/a (exec) | **~10–18 min** | *(derived)* serial floor |
| Debug ungated **nextest exec** | 4789 tests, parallel | n/a (exec) | ~2–2.5 min | **measured** `Summary [107–151s] 4789 tests` |
| Release pass (post-#4390 subset) | release-subset compile/link + heavy release-only tests | deps only | ~8–15 min | *(derived)* |
| GUI (`npm ci`+`tsc`+vitest) + sidecar + tree-sitter | install + typecheck + unit | npm cache | ~5–8 min | **measured** npm lines + `Finished dev` adjacency |

**Raw sum ≈ 50–85 min**; on a box running 24 task lanes that share the 32-token jobserver, the merge verify gets ~half the box and wall stretches to the observed **80–110 min** (this contention *is* the livelock mechanism the `orchestrator.yaml` 48→24 comment describes).

**The three uncached cost centres, in priority order:**
- **(A) Repeated cold workspace compile** (clippy-all-targets + debug-test + gui-check + release) — the biggest bucket. Attacked by **target warmth** (Phase 1).
- **(B) ~745 uncached links per profile** — embedded in every compile bucket. **Already on fast rust-lld** (corrected; not the bottleneck first labeled), so this bucket is smaller than the first draft implied. Reduced further by **less debuginfo** (Phase 3) and by **fewer relinks under warmth** (Phase 1); a rust-lld→mold swap (Phase 2) is now only a marginal check, not a lever.
- **(C) Serial OCCT test execution + GUI** — a floor that build-warmth cannot touch. Attacked by **OCCT-into-nextest process isolation** (Phase 4).

---

## 3. Levers evaluated and ranked

Effort: S/M/L. Risk: Low/Med. "Wall" = expected merge-gate wall-clock on a *typical* (leaf-ish) delta. "CPU-s" = CPU-seconds removed from the contended box (the throughput/livelock-relevant metric).

| # | Lever | Effort | Risk | Wall | CPU-s | Repo | Notes |
|---|---|:--:|:--:|---|---|---|---|
| **1** | **Persistent, reused merge worktree + `target/` at a FIXED path** (reset-in-place per attempt; serial lane only) | **M** | **Low–Med** | **~90→~25–35m** | **~60–80%** | **DF** | The keystone. Only the merge delta + reverse-dep closure recompiles/relinks; everything else is a cargo fingerprint hit. Stable path also lifts sccache hit-rate (§1.2). Safe *because* the lane is serial. |
| **2** | **Linker — rust-lld → `mold` A/B + bfd-guard** (CORRECTED: rust-lld is *already* the default; this is no longer a "switch off bfd" win) | **S** | **Low** | **~0 (marginal)** | **small** | reify | The ~745 links already run on fast rust-lld (LLD 22.1.2), so the big linker win the first draft claimed does not exist. Residual value: (a) benchmark `mold` 2.30.0 vs rust-lld on a representative relink — likely a tie or small win on the 2.8 GB OCCT-static-stack binaries; (b) a **non-regression guard** that nothing in `.cargo/config.toml`/`RUSTFLAGS` accidentally forces bfd. **Demoted** below Phases 3–4; keep only if mold measurably wins. x86_64-linux only (target-scoped). |
| **3** | **Cut debug debuginfo** (`debug=1`/`line-tables-only` or `split-debuginfo` on a dedicated merge profile) | **S** | **Low** | **−2–5m** | small | reify | Less link input → faster links + far smaller `target/` (helps the 177 GB problem & disk-pressure pruning). Keep enough for test backtraces. |
| **4** | **OCCT crates → nextest process-per-test** (the `.config/nextest.toml` `occt` group is *already staged* for this; task 3767 Stage 2) | **M** | **Med** | **−8–15m** | exec | reify | Parallelizes the serial `--test-threads=1` OCCT floor (C). Per-process OCCT-global isolation comes free from nextest's process model; cross-worktree contention stays bounded by the existing semaphore. Attacks the floor warmth can't. |
| **5** | **`CARGO_INCREMENTAL=1` for the persistent merge lane only** (disables sccache *there*) | **S** | **Med** | experiment | tbd | reify/DF | Only coherent *after* Phase 1 (needs a private, stable target). Incremental beats sccache for the *changed* crates; unchanged crates are already built. Measure vs Phase 1 alone; lane-scoped so it cannot regress the 24 task lanes' cross-worktree sharing. |
| **6** | **sccache stable input hashing** (`--remap-path-prefix`, fixed paths) | **S** | **Low** | minor | minor | reify | Mostly *subsumed* by Phase 1's fixed path. Worth it only if a persistent worktree is rejected. |
| 7 | nextest `archive` (build bins once, reuse) | M | Low | — | — | reify | Largely subsumed by Phase 1 (persistent target reuses bins *and* fingerprints *and* clippy/check artifacts, not just test bins). Keep as fallback if a persistent worktree is infeasible. |
| 8 | codegen-units / test-profile opt-level tuning | S | Low | minor | minor | reify | Low leverage here; deps already opt-3, workspace already opt-0. Not a priority. |
| — | **Narrow the merge gate scope** | — | — | — | — | — | **FORBIDDEN** (C2). See §8. |

---

## 4. Recommended phased plan

Each phase is independently shippable and measurable. Land in order; re-measure after each.

**Phase 0 — one controlled instrumented baseline (validation, off-peak).** The brief permits *at most one* instrumented run; the box was at load 89 (PSI avg10 38%) during this study with live merges in flight, so the breakdown in §2 was derived from production logs rather than a competing 85-min build. Before Phase 1, do **one** off-peak run of `verify.sh all --profile both --scope all` in a throwaway worktree with `CARGO_PROFILE_*` + `cargo build --timings` + `/usr/bin/time -v` per step and a pre/post `sccache --show-stats` delta, to pin the (A)/(B)/(C) split exactly. ~1 run, ~90 min, no production impact.

**Phase 1 — persistent warm merge worktree + target (the keystone; dark-factory).** Replace the create-fresh/destroy `_merge-<uuid>` lifecycle *for the verifying lane* with a **single persistent worktree at a fixed path** (e.g. `.worktrees/_merge-verify`), **reset in place** to each candidate merge commit (`git reset --hard <merge-commit> && git clean -xfd -e target`) with **`target/` retained**, exempt from `prune_stale_merge_worktrees`, used only under the serial `_MERGE_AHEAD_BOUND=1` lane. Expected **~90 → ~25–35 min** typical and **~60–80% of merge-gate CPU-seconds removed** — which is the metric that ends the livelock. *Correctness:* this is exactly how normal local dev reuses `target/` across commits; cargo fingerprints recompile precisely the changed crates + reverse-deps. A **fixed path** (not a fixed `CARGO_TARGET_DIR` under changing worktree paths) is essential so both source paths *and* artifacts are stable — otherwise path-sensitive fingerprints/debuginfo invalidate the warmth. Speculative/conflict-probe merges may stay ephemeral (they don't build). **Safety valve:** every Nth land (or nightly) do one from-scratch verify to catch any fingerprint-staleness corner case.

**Phase 2 — linker: rust-lld → mold A/B + a bfd-guard (DEMOTED; corrected premise).** The original "switch off bfd" framing was wrong: rust-lld (LLD 22.1.2) is *already* the default active linker (empirically verified, esc-4449-206), so there is no slow-linker to escape. This phase shrinks to: (1) **benchmark `mold` 2.30.0 vs the default rust-lld** on a representative relink (`-Clink-arg=-fuse-ld=mold` in a target-scoped `rustflags`) — adopt only if it measurably wins, likely a tie/small gain; (2) add a cheap **non-regression guard/test** that nothing forces bfd (the slow path) by accident. No correctness risk either way; both are target-scoped so wasm/emscripten are untouched. This is now a *minor* item — sequence it after Phases 3–4, not before.

**Phase 3 — trim debug debuginfo.** A dedicated lean profile (or `debug = 1` / `split-debuginfo = "unpacked"` for dev tests) cuts link time and shrinks `target/` (eases the 177 GB / disk-pressure-prune dynamic that the ENOSPC retry path exists to handle). Verify test backtraces stay adequate.

**Phase 4 — fold OCCT tests into nextest.** Realize the already-staged `.config/nextest.toml` `occt` test-group (drop the separate `cargo-test-occt-gated.sh --test-threads=1` pass; run OCCT crates inside the nextest pool pinned to the `occt` group, isolated per-process, cross-worktree-bounded by the semaphore). Attacks floor (C) — the part Phases 1–3 cannot. Coordinate with task 3767 Stage 2 (same migration).

**Phase 5 — measure `CARGO_INCREMENTAL=1` on the persistent lane only.** A/B against Phase 1 alone. Adopt only if it wins on the dedicated lane; never globally.

**Phase 6 (design sketch, NOT yet PRD-ready) — extend warmth to the 24 task lanes via CoW-seeded warm-lane pool.** Phases 1–5 warm only the *serial merge* lane; Phase 6 asks whether the durable warm `target/` can also seed the *concurrent task* lanes cheaply (copy-on-write, deltas-only on disk). It is materially less mature than 0–5 — it carries unresolved empirical forks (filesystem substrate, does-a-seeded-lane-actually-skip-the-rebuild, provisioning) and a value question. **See §10 for the full sketch.** Gated behind Phase 1 (κ) landing *and* a de-risking spike; do not queue from this doc.

**Land alongside (not part of this design):** #4448 fail-fast ordering (bounds the *failing* path), and **retire #4447's 60→90 timeout bump** once Phase 1 lands (warm verifies finish well inside 60 min, so the band-aid is no longer needed).

---

## 5. Cross-repo (dark-factory) changes required

Phase 1 is the only structurally cross-repo lever; the rest are reify-local (`.cargo/config.toml`, `Cargo.toml`, `.config/nextest.toml`, `scripts/verify.sh`).

- **`orchestrator/src/orchestrator/git_ops.py`** — add a "persistent verify worktree" path beside `_create_merge_worktree`/`cleanup_merge_worktree`: create-once-at-fixed-path, **reset-in-place** per attempt (retain `target/`), and **exempt it from `prune_stale_merge_worktrees`** (which today force-removes `_merge-*`; it must not eat the warm one). Keep the ephemeral path for speculative/conflict probes.
- **`orchestrator/src/orchestrator/merge_queue.py`** — route the **verifying** attempt (serial, `_MERGE_AHEAD_BOUND=1`) through the persistent worktree; keep the rest of the train logic. The cold-vs-warm timeout split (`merge_verify_cold_command_timeout_secs` 7200 s) can fall back toward the warm budget once warmth holds.
- **Config knob** — gate the new behaviour behind a yaml key (e.g. `git.persistent_merge_worktree: true`) so it is opt-in per project and trivially revertible. Default off; reify opts in.
- **No `.mcp.json` concern on the merge lane.** The spawn brief flagged `.mcp.json` skip-worktree hygiene as a caveat; verified it **does not apply**: merge worktrees check out the committed `.mcp.json` (`:3939`) but never run `setup-worktree-debug-port.sh` and host no MCP client (headless verify). A reset-in-place persistent worktree just re-checks-out the committed default each time. (The skip-worktree hygiene matters only for *dispatched-agent task* worktrees.)
- **Invariant to preserve:** the warm worktree is single-consumer *only because the lane is serial*. If `_MERGE_AHEAD_BOUND` is ever raised >1, Phase 1 must become a small pool or revert — concurrent cargo on one `target/` is unsafe.

---

## 6. Expected end-state

| | Today (cold) | After P1 | After P1+P4 |
|---|---:|---:|---:|
| Typical land (leaf delta) | ~90 min | ~25–35 min | **~15–22 min** |
| Worst case (low-level/reify-core delta) | ~110–148 min | ~40–60 min | **~25–40 min** |
| Merge-gate CPU-seconds (contended box) | baseline | −60–80% | −70–85% |
| Scope / correctness | full | **full (unchanged)** | **full (unchanged)** |

(The combined column is **P1+P4**, not P1+P2+P4 — with the linker premise corrected, Phase 2 contributes ≈0; the gain beyond P1 is the OCCT exec-floor parallelization (P4). Phase 3 debuginfo-trim adds a little more, mostly on disk/link-input size.)

The throughput win compounds: a 3–5× shorter serial lane *and* a large CPU-seconds reduction together remove the contention that froze main for 2 h in the originating incident — without touching gate scope.

---

## 7. Incidental findings (worth a follow-up, out of scope here)

- **No durable merge-verify breakdown is persisted.** verify.py dumps step output to the journal **only on failure**; a *passed* merge verify's per-step timing dies with the deleted worktree. A tiny "emit per-step durations to the event store on success too" change would make future tuning measurable without log archaeology. (Useful for Phase 0 / regression tracking.)
- **`reify-jobserver-canary.service` is in `failed` state** (observed in `systemctl --user`). Unrelated to this design but worth a glance — the jobserver FIFO is load-bearing for the 32-token sharing model.
- The `Summary […] 11370 tests` lines in the journal (vs the merge-gate ungated **4789**) come from wider non-merge invocations; the merge-gate ungated nextest count is **4789** (OCCT-gated crates run separately and are not in that total).

---

## 8. What NOT to do

- **Do NOT narrow the merge-gate scope.** verify-scope-contract **C2** forbids it; `verify.sh:348` force-`--scope all` for `DF_VERIFY_ROLE=merge` is the guard, backed by drift test #4059. Narrowing the gate is precisely the ingress risk that caused the red-main incident. This design buys speed from **warmth**, never from **coverage**.
- **Do NOT set `CARGO_INCREMENTAL=1` globally.** It is mutually exclusive with sccache; turning it on workspace-wide would break the cross-worktree rlib sharing that 24 task lanes + the merge lane depend on (the documented rationale in `CLAUDE.md` / `orchestrator.yaml`). Incremental is permissible **only** on the isolated, stable, serial persistent merge lane (Phase 5), measured.
- **Do NOT share one persistent `target/` across concurrent merges.** Safe only while `_MERGE_AHEAD_BOUND=1`. Raising the bound requires a pool or a revert.
- **Do NOT reuse a *task* worktree's `target/` for merges.** Different flags/contention; the warm worktree must be dedicated to the merge lane.
- **Do NOT "fix" throughput by skipping the gate / pinning SHAs.** That is #1687's separate, deliberately-bounded concern. Warm builds make the *real* gate cheap, so skipping it is unnecessary — keep the full gate, just warm.
- **Do NOT raise the verify timeout further as a throughput fix.** #4447's 60→90 bump treats the symptom; Phase 1 removes the cause and should let it be reverted.

---

## 9. PRD-readiness

This design is **PRD-ready for Phases 0–5** (queued 2026-06-09 as κ/α/β/γ/δ + companion ε). It has a measured baseline, a ranked option table with effort/risk/expected savings, a phased plan where each phase is independently shippable + measurable, and an explicit cross-repo seam (Phase 1 ↔ dark-factory `git_ops.py`/`merge_queue.py`). Natural decomposition: **Phase 1** (dark-factory, the keystone) and **Phases 2–5** (reify-local) are separable PRDs/tasks; Phase 0 is a one-shot measurement spike that should precede Phase 1. **Phase 6 (§10) is explicitly excluded** — it is a design sketch with open empirical forks and a value question, and must go through a de-risking spike before it could be `/prd`'d. Hand off Phases 0–5 via `/prd`; do **not** implement from this doc directly.

---

## 10. Phase 6 (design sketch) — warm-lane pool + CoW seeding to the task lanes

**Status:** design sketch — **PRD'd and δ integration gate landed**. Phase 6 has been filed as `docs/prds/warm-lane-pool-cow-seeding.md` (generalized to a *unified* task-dispatch + merge-speculation CoW pool, 2026-06-17, the PRD of record), and its δ end-to-end integration gate has **LANDED** (task 4662, `tests/infra/test_warm_lane_pool.sh`, commit 12e810b2f4). This section is preserved as the historical design sketch — the dead ends ruled out and the reasoning remain on record. See the PRD for the current source of truth; do not implement directly from this section.

### 10.1 Goal and shape

Phases 1–5 warm the *serial merge* lane. Phase 6 asks: can the durable warm `target/` (Phase 1's keystone) also **seed the 24 concurrent task lanes**, so each task worktree starts from a warm `target/` instead of building cold — at **deltas-only disk cost** via copy-on-write?

The robust shape is **not** "CoW-seed today's fresh-per-task worktrees." It is a **fixed-path pool of reused warm lanes** (`_lane-0 … _lane-N`), each **reset-in-place** per task (so mtimes move only on changed files and the path is stable), **CoW-seeded once** from the warm merge `target/`. Fixed paths are load-bearing — they make the path-sensitivity problems below evaporate; CoW is what makes N warm lanes affordable on disk.

### 10.2 Why the naive versions fail — empirical findings (this session)

Three results from direct measurement, each of which steers toward the fixed-path-pool shape:

1. **Symlink-to-a-pool is DEAD.** rustc canonicalizes its cwd (`getcwd()` resolves symlinks): a binary built via `/tmp/cowtest/link → …/real` embedded **`…/real`**. So a stable symlink over varying real worktrees still bakes the *varying* real path into debuginfo. Verified.
2. **Embedded paths are absolute + fully resolved, but contamination is concentrated and remappable.** A live task binary embeds ~9,340 `~/.rustup` + ~1,150 `~/.cargo/registry` hits (already path-stable across worktrees) vs only **22** worktree-path hits. `--remap-path-prefix=<wt>=<const>` drives the worktree-path hits to **0** (verified). So there are **three path-sensitivity vectors**: (a) embedded debuginfo/panic paths in *output* artifacts → fixed by `--remap-path-prefix`; (b) cargo's *internal* `target/.fingerprint/**/dep-*.d` bookkeeping → **fixed by a stable path, NOT confirmed fixable by remap alone** (this is the strongest argument for fixed-path lanes over remap-over-varying-paths); (c) mtime freshness → §10.2.3.
3. **mtime normalization is needed and feasible.** `-Z checksum-freshness` (content-hash freshness, which would dissolve the problem) is **nightly-only**; this is stable cargo 1.96, so freshness is **mtime-based**. A fresh checkout stamps sources at wall-clock *now* (newer than seeded artifacts) → cargo rebuilds. Fix: after checkout, make sources **older** than the seed (`find <src> -exec touch -d <old>` — O(source files), negligible vs the 177 GB `target/`), *or* use `git-restore-mtime` (commit-time mtimes; installable, not currently on the box). Works only if the seed's fingerprint *flag-hashes* match — they do, since every lane builds under the same `verify.sh` env. Reset-in-place lanes mostly sidestep this (git only re-touches changed files).

### 10.3 CoW substrate — overlayfs vs native-CoW FS (agent-verified, 2026-06-10)

The current worktree FS is **ext4**, which has **no reflink/CoW** (`cp --reflink=always` → `Operation not supported`, verified). So Phase 6 needs a substrate change. Two routes, agent-verified against kernel docs:

- **overlayfs — viable in theory, disqualified on this box.** The cross-session claim "overlayfs requires a read-only lower layer" is **misleading**: the read-only lower is exactly the design (shared immutable base + per-worktree writable upper), and *"lower layers may be shared among several overlay mounts … a very common practice."* But two agent-confirmed facts rule it out **here**: (1) **mount privilege** — unprivileged overlay rides the user-namespace machinery (Linux 5.11+ `FS_USERNS_MOUNT`), and **userns is broken on this box** (the same root cause as the bwrap→landlock switch), so it would need real root / a privileged mount helper; (2) **whole-file copy-up** — *"the file is first copied from the lower filesystem to the upper"* on first write, so every relinked test binary / regenerated `.rmeta` duplicates in full, gutting the disk savings. (Separately, pointing the lower at the *live, evolving* merge `target/` would hit the kernel's "don't mutate a mounted lower" rule — cited, not independently re-verified, and moot given the above.)
- **native-CoW FS — the recommended route.** Both deliver **block-level, both-sides-independently-writable** CoW (agent-verified verbatim):
  - **XFS-reflink (recommended default).** `reflink=1` is the `mkfs.xfs` default on modern xfsprogs; `cp -a --reflink=always` clones a directory tree, shared extents, deltas-only, both sides writable. **The boring, robust choice** — fewest fragmentation footguns on a rewrite-heavy build workload. Cost: per-file `cp` walk (tens of thousands of files, ~seconds/clone); no subvolume snapshot.
  - **btrfs subvolume snapshot (alternative).** *Writable by default*, **O(1) instantaneous** (metadata-only) — the cheapest clone primitive *if* `target/` is its own subvolume. But our workload (shared base + heavy artifact rewrites) is close to btrfs's **worst case** for CoW write-amplification/fragmentation; `chattr +C` only partially mitigates (a snapshotted file still takes one CoW on first write to a shared block, and loses checksums/compression).
  - **Verdict: RESOLVED — XFS-reflink chosen (spike #4641 §7 / Q2 SAFE).** XFS-reflink is stable over reset-in-place cycles: no fragmentation accumulation, no space leak, no perf drift over 5 cycles. Btrfs deferred. See `docs/design/phase6-xfs-reflink-cow-spike-results.md` §7.

### 10.4 Merge-worker-write safety (the seed keeps evolving)

The seed is not static — the merge worker rewrites the merge `target/` on every land. Native-CoW handles this **by construction**: a reflink/snapshot clone is **point-in-time and independent**, so the merge worker's later writes allocate new blocks and never touch existing clones. The only effects are **divergence** (disk) and **staleness** (an older clone is warm-but-staler → re-seed periodically) — never corruption. This is the decisive correctness advantage over overlay's frozen-lower model. **Seed at a quiescent moment** (between merges, when the serial lane is idle and `target/` is consistent — never mid-build); a `cp --reflink` in that window is near-instant.

### 10.5 Interaction with the orchestrator isolation model

A pool of reused fixed-path lanes replaces the *fresh-per-task* worktree model, which touches:
- **per-worktree `.mcp.json` debug-port** wiring (`setup-worktree-debug-port.sh`, the esc-4202-61 hygiene) — a reused lane must re-provision or retain its port deterministically;
- **`lock_depth` / `max_per_module`** scheduler assumptions tied to worktree identity;
- **landlock** workspace scoping (the sandbox bounds writes to the worktree path — a stable pooled path is actually *simpler* here, but must be re-confirmed).
None are blockers, but they make Phase 6 an **orchestrator-model change**, not a drop-in — bigger than Phase 1, same shape.

### 10.6 Provisioning reality

- ext4 **cannot be converted in place** to a reflink FS. Need a fresh FS on the build volume: carve a new LV from the LVM VG, `mkfs.xfs` (or `mkfs.btrfs`), mount, repopulate the warm seed, clone per lane.
- **Open: VG free extents.** The 4.5 TB "free" is *inside* the existing ext4 LV — **not** the same as unallocated VG space. Provisioning a new LV needs free PEs in `vgroup0` (check `vgs`/`pvs`); may require shrinking the data LV. Confirm before committing.
- No kernel work: btrfs and XFS are standard in 6.x; the running kernel already supports both.

### 10.7 Value gating + de-risking spike (DONE — all questions resolved, review concluded positively)

**Spike #4641 (task 4641, 2026-06-17) answered all four gating questions** — full results in `docs/design/phase6-xfs-reflink-cow-spike-results.md`:

1. **Q1 PROMISING.** A seeded + mtime-normalized lane ran the full merge gate in **9 min 31 s vs 22 min 29 s cold** (~58% total wall-clock, ~70% / ~12.6 min off compile-link, ~904 of ~940 unit-compiles skipped). The §10.2 path-sensitivity vectors (a) and (b) are **moot** — cargo freshness is path-independent (383==383 Fresh units across a rename, identical unit hashes; spike §4/§6.1); no remap or bind-mount needed for warmth to transfer.
2. **XFS-reflink chosen (§10.3 bake-off resolved).** Q2 SAFE — XFS-reflink is fragmentation/space/perf-stable over reset-in-place cycles (spike §7). Btrfs deferred.
3. **VG free-extent question moot (§10.6 resolved).** The spike validated a **loopback XFS image** on the ext4 `data_lv` (D2) — no VG surgery needed.
4. **Speedup justifies the orchestrator change** — ~58% total on a representative workload; the CoW substrate is mechanically stable.

**Both #4469 triggers** held (Phase 1 κ landed + spike done). The desirability review resolved **positively** (#4469 done_provenance commit af3aece1a1); this design was `/prd`'d as `docs/prds/warm-lane-pool-cow-seeding.md`. See that PRD for the current source of record.
