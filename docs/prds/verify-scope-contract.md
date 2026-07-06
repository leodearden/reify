# PRD — Verification scope contract: per-task narrowing with a full merge gate

**Status:** authored 2026-05-29. Version-agnostic infrastructure foundation (not tied to a v0.x language milestone).

## 0. Context & supersession

`scripts/verify.sh` is the single source of truth for verification, shared by `orchestrator.yaml` (per-task `test`/`lint`/`typecheck`) and the git hooks (`hooks/project-checks`, `hooks/pre-merge-commit`). Today the orchestrator runs **`--scope all`** for every task — full-workspace clippy + nextest + OCCT gated pass + GUI, regardless of what the task changed. Verification wall-clock is the dominant throughput bottleneck for the orchestrator.

This PRD is the **design-first half** of the verify-throughput program (points 2 + 4 of the 2026-05-28 investigation). The **low-risk half** — heavy-test relocation (A), two-tier debug/release by role (1), and command fusion (3) — is being implemented directly in a separate session and **must land on main first** (see §5). This PRD assumes that post-A/1/3 `verify.sh`.

It introduces a deliberate change to the **per-task vs merge-gate correctness contract**: per-task verification is narrowed to what a task branch actually changed (plus everything that could break as a result), while the **merge gate stays full-workspace**. Rare breakage classes (release-only, downstream-crate) are caught at the merge boundary — the right place, since per-task verify reruns on every TDD iteration.

## 1. Consumer & user-observable surface (G1)

- **Named consumer:** `orchestrator.yaml`'s three per-task verify commands (`test_command` / `lint_command` / `type_check_command`), realized through `scripts/verify.sh`. The mode and its consumer are wired in the **same task** (T1) — no producer-orphan.
- **User-observable surface:**
  - `scripts/verify.sh --print-plan` — the existing faithful oracle of what runs. The narrowed plan for a given branch/staged diff is deterministic and directly assertable (this is the leaf signal for most tasks, via the existing `tests/infra/test_verify_scope.sh` harness).
  - Orchestrator task throughput (recorded as before/after actuals on representative branch shapes — §7 / T6).
- This is build/CI infrastructure, **not** an in-engine Reify seam, so the `engine-integration-norm.md` §3 sub-check does not apply.

## 2. Sketch of approach

**Phase 1 — `--scope branch` family gating.** A new scope mode that classifies the branch's changed files through the *existing* `decide_scope` file→family logic (`verify.sh:224-277`), so whole check families drop out when the branch doesn't touch them:
- changed-file set = `git diff --name-only --diff-filter=ACMR <merge-base>` where `<merge-base> = git merge-base main HEAD` — diffing the **fork point against the working tree** captures committed *and* uncommitted task work (the orchestrator commits before verifying, but this is conservative). Filtered through the existing `grep -v '^\.task/'`.
- routed through `decide_scope`'s existing `case` arms → `RUN_RUST` / `RUN_GUI` / `RUN_OCCT_GATE`. A docs-only branch → empty plan; a non-OCCT crate branch → no OCCT gated pass; a `gui/src` branch → no Rust.
- the existing `MERGE_HEAD` guard (`verify.sh:145-149`) still forces `--scope all`.
- `orchestrator.yaml`'s three commands switch `--scope all` → `--scope branch`.

**Phase 2 — affected-crate reverse-dependency narrowing.** `decide_scope` gains a fourth output: the **affected-crate set** = *changed crates ∪ their reverse-dependency closure* (every workspace crate that transitively depends on a changed crate, so anything that could break is rebuilt/retested). Computed by a new `scripts/affected-crates-lib.sh` reusing the `cargo metadata` resolve-graph technique already in `scripts/occt-scope-lib.sh:occt_touching_set` (which walks *forward* closure; this walks the *reverse* adjacency). When the set is a bounded list (≠ `ALL`), the test passes, the OCCT gated pass, **and** clippy + `cargo check` run with `-p <crate>...` instead of `--workspace`/`--exclude`. Workspace-global changes (`Cargo.toml`, `Cargo.lock`, `.cargo/*`, `tree-sitter-reify/*`, toolchain) → `ALL` sentinel → no narrowing.

## 3. Contract (B+H)

The invariants any implementation must hold. These are the load-bearing guarantees; the boundary tests in §4 enforce them.

**C1 — `all` is sacred.** `--scope all` always means full workspace: `RUN_RUST=RUN_GUI=RUN_OCCT_GATE=1`, every cargo pass `--workspace`, **zero** `-p` narrowing. Narrowing logic is gated behind `scope ∈ {branch, staged}`; it is structurally unreachable for `all`.

**C2 — The merge gate never narrows.** `hooks/pre-merge-commit` keeps `verify.sh all --profile debug --scope all`. The orchestrator's post-merge verify (`DF_VERIFY_ROLE=merge`) keeps `--scope all` via its command. **Defensive belt-and-braces:** `verify.sh` forces `SCOPE=all` whenever `DF_VERIFY_ROLE=merge` (mirroring the existing `MERGE_HEAD` force), so a future caller cannot accidentally hand the merge gate a narrowing scope.

**C3 — Reverse-closure completeness (non-transitive dev-deps).** The affected-crate set must include the changed crates and every workspace crate B whose *test-compile closure* — the union of (i) every crate B normal/build-depends on transitively, including B itself, and (ii) for each crate D that B *directly* dev-depends on, every crate D normal/build-depends on transitively, including D itself — contains a changed crate. This matches cargo's actual compile semantics for `cargo test`/`clippy --all-targets`: dev-deps do not propagate transitively, so a dev-dependency of one of B's dev-dependencies is never part of what gets compiled when testing B. If crate A is changed and B's test-compile closure contains A, B is in the set. Under-approximation = shipped breakage that only surfaces at merge; this is the single most dangerous failure mode and T5's boundary test exists to catch it.

**C4 — Global changes force `ALL`.** Any change to a workspace-global file (`Cargo.toml`, `Cargo.lock`, `.cargo/*`, `tree-sitter-reify/*`, `rust-toolchain*`) yields the `ALL` sentinel → `--workspace`, no narrowing. This mirrors `decide_scope`'s existing conservative `gate=1` arms for those paths.

**C5 — Fail safe, fail wide.** Any failure to compute the branch diff or the affected set (detached HEAD, missing `main` ref, `cargo metadata` error, unrecognized path) falls back to the *widest* applicable scope (`ALL` / family-on), never the narrowest. Unrecognized file paths keep `decide_scope`'s existing `rust=1;gui=1;gate=1` conservative arm.

**Non-crate allowlist (§5 companion to C4/C5).** Not every path that fails to map to a crate is "unmappable" in the C5 sense. `scripts/affected-crates-lib.sh:_is_noncrate` allowlists `docs/**` (documentation), `gui/src/**` (frontend-only, no Rust compile unit), and `tests/infra/**` (shell/python infra test scripts, which run as their own verify step and never affect Rust crate compilation or test outcomes) as *known* non-crate paths: they contribute zero crates to the direct set and must **narrow** (possibly to an empty affected set), never falling through to C5's fail-wide-to-`ALL` branch — that branch is reserved for paths matching no known rule at all. `tests/infra/**` was added to this allowlist by commit `ab021821fa` (esc-4967-33): the previous allowlist (`docs/**`/`gui/src/**` only) forced `ALL` on tests/infra-only diffs, needlessly widening scope for changes that cannot affect any crate's compilation or test outcome.

**C6 — `staged` opt-in only.** `--scope staged` (the main-branch `hooks/project-checks` gate) keeps the **full crate set** by default. Affected-crate narrowing applies to `staged` only when an explicit opt-in flag (e.g. `--narrow`) is passed — for urgent, low-risk, narrow commits. Default `staged` behaviour is unchanged.

**Reverse-closure definition (C3, precise; supersedes the original all-kind transitive reverse BFS described below).** From a single `cargo metadata --format-version 1` resolve graph, build two adjacency maps among workspace members: `adj_normal[pkg]` (dep kind `null` or `build`) and `adj_dev[pkg]` (dep kind `dev`). Define `normal_closure(id)` as the BFS-reachable set over `adj_normal` only, starting from `id` (inclusive). For every workspace member `B`, its test-compile closure is `normal_closure(B) ∪ ⋃{normal_closure(d) : d ∈ adj_dev[B]}`; `B` is affected iff that closure intersects the seed ids (the changed crates). This is `occt_touching_set`'s forward algorithm (`scripts/occt-scope-lib.sh:59-109`, "what does X pull in") reused verbatim and parameterized on the changed-crate seed ids instead of a hardcoded OCCT id — so `affected_crates` seeded with `reify-kernel-occt` necessarily equals `occt_touching_set`.

The original definition here — reverse adjacency `R[dep] ∋ pkg` over *every* dependency edge of any kind (`null`/`build`/`dev`) among workspace members, BFS-walked transitively and seeded by the changed crates — over-approximated cargo's actual semantics: it let a chain of two-or-more dev edges (e.g. `reify-kernel-occt` ←dev– `reify-eval` ←dev– `reify-eval-fea-tests`) pull a crate into the affected set even though its test binary never actually links the changed crate. Cargo does not propagate dev-deps transitively — only the *first* dev-hop from the tested crate itself matters. (Forward closure in `occt_touching_set` answers "what does X pull in"; the reverse question here is "which crates' test-compile closures pull in a changed crate" — a fresh per-member walk, same `cargo metadata` substrate.)

**Release-sensitivity scoping (orthogonal to C1/C2; task 4390).** The merge-gate RELEASE pass is sensitivity-scoped to the crates whose tests depend on `debug_assertions` or `overflow-checks` — the only behavioral delta between debug and release builds. (3rd-party numeric deps are already `opt-level=3` in both profiles per `Cargo.toml:155-158`, so they never flip.) This set (currently 8 crates) is enumerated in `scripts/release-sensitive-crates.txt` and guarded by the drift test `tests/infra/test_release_scoped_scope.sh`, which asserts that the declared list equals the grep-derived set. Three detection mechanisms are used:

- **Mechanism A** — `#[cfg_attr(debug_assertions, ignore …)]` attribute: tests that are skipped in debug but run in release.
- **Mechanism B** — `#[cfg(not(debug_assertions))]` attribute: code blocks compiled only in release (where `debug_assert!` is elided).
- **Mechanism C** — runtime `cfg!(debug_assertions)` macro expression: tests that assert different outcomes in debug vs release via an inline boolean. Motivating case: `reify-mesh-morph/src/diagnostics.rs:511` (`record_quality_remesh_pass_never_touches_a_counter`) asserts `outcome.is_err() == cfg!(debug_assertions)` — the release-only no-op path (silent early return instead of `debug_assert!` unwind) is exercised only by the release run. Mechanisms A and B (attribute-only detectors) missed this, which would have silently dropped `reify-mesh-morph` from the release pass and left the no-op path uncovered at the merge gate.

All three mechanisms are detected by anchored grep patterns documented in `scripts/release-sensitive-crates.txt` and `scripts/release-scope-lib.sh`.

This scoping is **distinct from the C1/C2 branch-diff narrowing**: C1/C2 forbid narrowing the *crate set* tested by the merge gate (no per-task `-p` scope); this narrows which *build profile* each crate is retested under. The DEBUG full-workspace pass is completely unchanged and covers every crate; **total merge-gate cfg-detectable coverage is preserved**. Re-running only the release-sensitive crates in release loses no **cfg-detectable** coverage because non-sensitive crates have no identified cfg-gated divergence across profiles.

**Detector gap (convention-enforced).** The three grep mechanisms detect only profile divergence expressed via explicit cfg tokens. They do not detect: (1) bare `debug_assert!` elision used as a test-observable side-effect rather than a correctness guard — a test that only passes because `debug_assert!` fires in debug but is elided in release, without wrapping the assertion in `cfg!(debug_assertions)`, is invisible to the detector; (2) arithmetic overflow-panic expectations that rely on the `overflow-checks` flip without any cfg guard. A test in an unlisted crate that silently changes observable outcome across profiles due to either gap would not run in the narrowed RELEASE pass. **Project convention:** any test whose observable result (pass/fail) differs between debug and release builds MUST express that difference via one of the three detected cfg mechanisms (`#[cfg_attr(debug_assertions, …)]`, `#[cfg(not(debug_assertions))]`, or `cfg!(debug_assertions)`). A test that does not is invisible to the drift guard and constitutes a latent merge-gate coverage gap. This convention is the code-review complement to the automated drift test — the drift test enforces `declared == derived` for the detectable set only.

Within the RELEASE pass, the OCCT ∩ release-sensitive intersection (`reify-eval` only) remains flock-serialized with `REIFY_OCCT_TEST_TIMEOUT=4800` and `--test-threads=1`; the remaining 7 non-OCCT sensitive crates run at full nextest concurrency. The three other OCCT crates (`reify-kernel-occt`, `reify-cli`, `reify-config`) have zero release-sensitive tests and correctly drop out of the RELEASE pass entirely — they remain fully covered by the DEBUG full-workspace gated pass.

## 4. Boundary-test sketch (B+H)

Scenarios facing **both** sides of the contract, realized as new scenarios in the existing hermetic `tests/infra/test_verify_scope.sh` harness (`plan_has`/`plan_lacks`/`plan_cmdcount` over `--print-plan`) plus a crate-set drift test modeled on `tests/infra/test_occt_gated_scope.sh`.

| # | Side | Precondition (changed files) | Postcondition (asserted in `--print-plan`) |
|---|---|---|---|
| B1 | per-task narrow | docs-only branch | empty plan (`plan_cmdcount` 0) |
| B2 | per-task narrow | one non-OCCT crate | no `cargo-test-occt-gated.sh`; `-p <crate>` present; **no `--workspace`** |
| B3 | per-task narrow | `gui/src` frontend only | GUI checks only, no cargo |
| B4 | **downstream catch (C3)** | change crate A where B depends on A | affected set (and `-p` flags) **includes B** across test + clippy + check |
| B5 | **merge gate full (C1/C2)** | `--scope all` (any inputs) | every cargo pass `--workspace`, **zero `-p`** |
| B6 | **role guard (C2)** | `DF_VERIFY_ROLE=merge --scope branch` | scope forced to `all` (full plan) |
| B7 | global force (C4) | `Cargo.lock` changed | `ALL` → `--workspace`, no `-p` |
| B8 | crate-set drift | the reverse-closure lib vs a cargo-metadata oracle | declared computation == derived set (like the OCCT drift test) |
| B9 | staged opt-in (C6) | staged crate change, no flag / with `--narrow` | full crate set without flag; narrowed with flag |

The integration-gate task (T5) names **B4 + B5** as its observable signal — closing the G2/G5 loop: the value of the whole PRD is "narrows per-task **without** letting breakage through to main", and B4/B5 are the two-way proof.

## 5. Pre-conditions for activating

- **HARD: A/1/3 landed on main.** The forked low-risk session edits `verify.sh` (two-tier role + command fusion + heavy-test relocation) and `orchestrator.yaml`. This PRD edits the same files. Its tasks must not run until A/1/3 is on main, or they will conflict and be authored against the wrong baseline. Tracked as a §6 seam; the batch stays `deferred` until A/1/3 lands (activation decided at decompose time).
- Substrate (all verified 2026-05-29, G3): `git merge-base main HEAD` resolves in orchestrator task worktrees (local `main` ref present); `decide_scope` is structured to extend; `cargo metadata` resolve graph is available; `cargo nextest`/`clippy`/`check` accept `-p`; `--print-plan` oracle exists; `tests/infra/test_verify_scope.sh` + `run_all.sh` auto-discovery exist.

## 6. Cross-PRD relationship & seam ownership (G4)

No Reify *feature*-PRD seams (this is orthogonal build infra). The one coordination seam is with the forked implementation work:

| Other work | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| Forked **A/1/3** (heavy-test relocation, two-tier role, command fusion) | this PRD consumes its result | `scripts/verify.sh` baseline + `orchestrator.yaml` per-task commands + `DF_VERIFY_ROLE` role plumbing | **A/1/3** owns the role/profile/fusion changes; **this PRD** owns the `--scope branch` mode + affected-crate narrowing | blocked-on-A/1/3 (must land first) |

This PRD's C2 role-guard is **independent** of A/1/3's role-driven *profile* default — it adds only a defensive `merge ⇒ all` scope force, so the merge gate is safe even if A/1/3's role semantics change.

## 7. Decomposition plan

Two phases; Phase 2 depends on Phase 1's branch-diff plumbing. Every leaf names a `--print-plan`-oracle / artifact signal (never "a unit test passes on synthetic input").

**Phase 1 — `--scope branch` family gating**

- **T1 (leaf)** — Add `--scope branch` to `verify.sh` (merge-base diff → existing `decide_scope`; `MERGE_HEAD` still forces `all`; fail-wide on detached/no-main per C5) **and** switch `orchestrator.yaml`'s `test`/`lint`/`typecheck` commands to `--scope branch`. *Signal:* new `test_verify_scope.sh` scenarios B1–B3 pass (docs-only → empty plan; non-OCCT crate → no gated pass; gui-only → no Rust); `orchestrator.yaml` commands read `--scope branch`. *Consumer wired in-task (G1).*
- **T2 (leaf)** — Merge-gate contract guard: add the C2 defensive `DF_VERIFY_ROLE=merge ⇒ force SCOPE=all` in `verify.sh`, and an infra drift test asserting B5 + B6 (merge gate / role=merge produce a full `--workspace` plan with zero `-p`) and that `hooks/pre-merge-commit` still passes `--scope all`. *Signal:* the guard test fails if anyone narrows the merge gate; passes today. *Depends: T1.*

**Phase 2 — affected-crate reverse-dependency narrowing**

- **T3 (intermediate → unlocks T4)** — `scripts/affected-crates-lib.sh`: given a changed-file list, emit the affected workspace crate set per the C3 reverse-closure definition; global files → `ALL` (C4); reuse the `cargo metadata` technique from `occt-scope-lib.sh`. *Signal:* crate-set drift test (B8) — change to a low-level crate (e.g. `reify-core`) yields all its dependents; a leaf crate yields just itself; `Cargo.lock` yields `ALL`. *Unlocks T4.*
- **T4 (leaf)** — Wire the affected set into `verify.sh`: when `scope=branch` (or `staged` **with** the `--narrow` opt-in, C6) and the set ≠ `ALL`, build `-p <crate>...` for the nextest tail, the OCCT gated pass (set ∩ OCCT), clippy, and `cargo check`, instead of `--workspace`/`--exclude`; `ALL`/`scope=all` keep current behaviour. *Signal:* B2/B7/B9 — single-crate branch emits `-p <crate> -p <dependents>` and no `--workspace` across test+clippy+check; `Cargo.lock` branch still `--workspace`. *Depends: T1, T3.*
- **T5 (leaf, B+H integration gate)** — Boundary test proving the contract two ways: B4 (a change to crate A puts dependent B in the narrowed plan → downstream breakage still caught) and B5 (merge gate unaffected). *Signal:* the B4+B5 infra test. *Depends: T3, T4.*
- **T6 (leaf, validation/outcome)** — Record per-task verify wall-clock (and plan-step count via `--print-plan`) for representative branch shapes — docs-only, single non-OCCT crate, OCCT crate, gui-only — under `--scope branch`(+narrowing) vs `--scope all`, committed as a measurement note. Records **actuals**, asserts no guessed threshold (G6-safe). *Signal:* committed before/after artifact showing the narrowed plans. *Depends: T1, T4.*

DAG: `T1 → {T2, T4}`; `T3 → T4`; `T4 → {T5, T6}`; `T1 → T6`. Whole batch blocked-on-A/1/3 (§5).

## 8. Out of scope

- Heavy-test relocation, two-tier debug/release, command fusion (forked A/1/3).
- A shared cross-worktree `CARGO_TARGET_DIR` / sccache tuning (separate concern).
- `cargo nextest --changed-since` adoption (rejected: experimental + git-coupled; the deterministic `cargo metadata` reverse-closure is preferred and consistent with `occt-scope-lib.sh`).
- Changing what the OCCT gated pass *is* (still serialized/single-thread); Phase 2 only changes *which* crates it runs when it runs.

## 9. Open (tactical) questions

- Exact name of the `staged` opt-in flag (`--narrow` vs `--affected`) — tactical.
- Whether T6's measurement lives in `docs/` or a `data/` artifact — tactical.
- Whether to memoize the `cargo metadata` call across the gated/ungated/clippy plan steps within one `verify.sh` run (perf of the tooling itself) — tactical, local.
