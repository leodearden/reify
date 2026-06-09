# Capability manifest — warmer-builds-merge-verify

Mechanizes G3 + G6 per leaf for `docs/prds/warmer-builds-merge-verify.md`. Verified 2026-06-09 at `main fb2637861a` (PRD-commit base). This is a **pure-infrastructure** PRD wiring existing build tooling — there is **no novel `.ri` grammar surface** (the reify grammar gate is N/A), and **no result-field / `Value::Undef` population** surface (the FEA empty-value sentinel is N/A). All numeric figures in the PRD are *expectations*, not RED-test thresholds; the signals assert **measured-improvement-direction + a recorded delta**, so the G6 numeric-floor check is N/A by construction (nothing to under-shoot). Every binding below is **PASS** — no `declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `fixture-ERROR` / `bound≤floor`.

Evidence commands were run from `/home/leo/src/reify` (reify substrate) and `/home/leo/src/dark-factory` (DF substrate).

## κ — Phase 1 · dark-factory · persistent warm merge worktree (keystone; intermediate + deliverable)

| Capability the signal asserts | Evidence | Verdict |
|---|---|---|
| `git_ops.py` merge-worktree lifecycle to replace | `grep:orchestrator/src/orchestrator/git_ops.py` `_create_merge_worktree`:≈1404, `cleanup_merge_worktree`:≈1446, `prune_stale_merge_worktrees`:≈1491 | PASS (wired-on-main) |
| serial-lane routing hook + cold timeout | `grep:orchestrator/src/orchestrator/merge_queue.py` `_MERGE_AHEAD_BOUND = 1`:103, `merge_verify_cold_command_timeout_secs` 7200 s | PASS |
| `.worktrees/_merge-verify` fixed path is reset-in-place-safe | mechanism = normal cargo `target/` reuse across commits (fingerprint recompiles changed crates + reverse-dep closure); §10 invariants 1–2 | PASS (mechanism-basis) |
| projected `~25–35m` warm wall-time | **expectation, not a gated bound** — signal = journal `verify start/end` delta below cold-baseline median + worktree-persists + safety-valve cold verify still green | PASS (numeric-floor N/A) |

*Downstream consumers (intermediate):* δ (Phase 5), ε (companion). Cross-project: filed in `dark_factory` project; reify δ/ε depend via qualified `dark_factory:<id>` edges.

## α — Phase 2 · reify · switch off bfd (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| `mold` linker on PATH | `command -v mold` → `/usr/bin/mold` (`mold 2.30.0`) | PASS (present) |
| `rust-lld` + `gcc-ld/ld.lld` bundled (zero host install) | `<sysroot>/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld` + `…/gcc-ld/ld.lld` present (rustc 1.96.0) | PASS (present) |
| target-scoped `rustflags` slot | `grep:.cargo/config.toml:2` `[target.x86_64-unknown-linux-gnu]` (holds `runner`; `rustflags` is additive; manifold `links` override at `:21` is a separate table) | PASS (wired) |
| no crate passes a bfd-specific linker arg | design-checked: only manifold `rustc-link-lib = static=…/stdc++` (both linkers handle); re-confirm at impl | PASS (re-verify in α) |
| linker speedup (`lld` 2–5× / `mold` 3–10× vs bfd) | **benchmarked in-task**, winner recorded in bench doc; not a frozen bound | PASS (numeric-floor N/A) |

## β — Phase 3 · reify · trim debug debuginfo (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| `Cargo.toml` profile accepts `split-debuginfo`/`debug=1` | standard cargo profile keys — no novel substrate | PASS |
| line tables retained for test backtraces | config-level; signal verifies a deliberate test panic resolves `file:line` | PASS |
| `target/` size shrink | measured delta recorded; not a gated bound | PASS (numeric-floor N/A) |

## γ — Phase 4 · reify · fold OCCT into the nextest pool (leaf)

| Capability | Evidence | Verdict |
|---|---|---|
| `.config/nextest.toml` `occt` test-group | `grep:.config/nextest.toml` `[test-groups] occt = { max-threads = 1 }` + `package(reify-kernel-occt)\|package(reify-eval)\|package(reify-cli)\|package(reify-config)` override — present, "staged for Stage 2" | PASS (substrate present; γ activates by raising `max-threads` + routing) |
| `cargo-test-occt-gated.sh` + `occt-touching-crates.txt` + verify.sh OCCT routing to drop | exist; counting-semaphore form landed by **producer:task-3767** (done, commit 7b33357598) | PASS (built-on; see PRD D5) |
| per-process OCCT race-freeness | established by 3767's address-space-isolation insight + `docs/notes/multi-process-occt-bench.md`; signal re-asserts via K idle-box repeat runs | PASS |
| OCCT-phase wall-time drop | measured; **re-measure** — 3767's semaphore already helps the task lanes but NOT the serial merge gate (D5) | PASS (numeric-floor N/A) |

**Note (D5):** the `occt` group is currently **inert** (`max-threads=1`, OCCT crates excluded from the nextest pass, run via the separate gated `--test-threads=1` pass). This is *staged substrate*, not a fiction — γ makes it live. Not a `declared-only` FAIL: the group + crate-list + gated script all exist on main; γ is the wiring task that consumes them.

## δ — Phase 5 · reify · `CARGO_INCREMENTAL=1` on the persistent lane (leaf, depends_on κ)

| Capability | Evidence | Verdict |
|---|---|---|
| an isolated, stable, serial persistent `target/` to enable incremental on | **producer:κ** (Phase-1 DF task), DAG-direction: κ is **upstream** of δ (qualified `dark_factory:<id>` external dep) | PASS (anti-inversion OK) |
| A/B adopt threshold (≥15% lane-wall win) | an **adoption decision** in §12 Open Q, not a RED-test premise; lane-scoped, reject-by-default | PASS (numeric-floor N/A) |

## ε — companion · reify · retire #4447's timeout bump (leaf, depends_on κ + #4447)

| Capability | Evidence | Verdict |
|---|---|---|
| #4447's debug `outer_timeout` 60→90 (+`gated_timeout` 3600→5400) bump to revert | **producer:#4447** (`in-progress`, upstream dep) — the bump must exist to be reverted | PASS (anti-inversion OK) |
| warm verify finishes inside the reverted 60 min budget | **producer:κ** (Phase-1), upstream; the 60 min is the *pre-existing restored config value*, not a guessed bound; signal = journal warm verify inside budget, no `exit 124` | PASS (numeric-floor N/A) |

---

**Manifest verdict: CLEAR.** All bindings PASS on verified substrate. No FAIL value present; the batch is unblocked for queueing. The only cross-project edges are δ→κ and ε→κ (qualified `dark_factory:<id>`), plus the same-project ε→#4447.
