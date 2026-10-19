# PRD: Cargo-native harness layout — `tests/harness_x/main.rs`, an explicit `[[test]]` list, one Python guard over cargo's own data

**Status:** active. Authored 2026-10-18/19 by the gap-fill stream session for **cargo-native-harness-layout**
(`plans/gap-fill-deliberation-reify-20261016.md` §D-K8-1, with the stream's halves of §D-K8-2, §Uncontested
block → D-K8-3 and §Uncontested block → D-K8-10 (4)). Decomposed in the same session (2026-10-19): α #9396, β₁ #9397, β₂ #9398, β₃ #9399, β₄ #9400, β₅ #9401, γ #9402, δ #9403, ε₁ #9404, ε₂ #9405, Ω #9406.
Shape: **B+H** (contract §6, boundary-test sketch §7): the blast radius is five crates plus the verify pipeline.
Milestone: v0_6.

**Code anchors** verified against main `3f9015b3b5` (2026-10-18). `git diff --stat 04be9302ea..main` over the
guard files, `scripts/verify.sh`, `.config/nextest.toml` and the five crates' `Cargo.toml` is empty. Only
`docs/prds/merge-gate-compile-cost.md` changed: its 2026-10-16 gap-fill amendment, which this PRD cites. Main
moves fast: cite by symbol, and re-locate lines at implementation time.

**Finding keys owned (6):** fk-e2a244cfb575 (high), fk-b904603b81f2, fk-10c353ee8ed7 (measure/split half;
S19 θ #8700 holds the WARN half), fk-9e9f9b36fbcc (root prose), fk-e6dd1263aafe (fail-closed; weakened) — all
hotspot-survey-reify-20261015 — and fk-53a9afc0d2b9 (high), from review-all-reify-20261010, which moves here
from #8700. Statements: `docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json` and
`review/reports/review-reify-20261015.json`. §10 maps each key to its leaf.

---

## §1 Goal

Cargo compiles each top-level `tests/*.rs` as its own test binary. The C1 contract
(`docs/prds/merge-gate-compile-cost.md` §5) caps link count by consolidating tests into 45 harness units.
Each unit has a root `crates/<c>/tests/harness_<x>.rs` whose members live in `tests/harness_<x>/`. Because
rustc resolves a bare `mod` in that root against `tests/`, every member needs a `#[path]`. The failure modes
are silent: a bare `mod x;` binds the wrong file, an undeclared member is never compiled, and a stray
`tests/*.rs` becomes a new binary. A 3,308-line bash test (`tests/infra/test_harness_kloc_cap.sh`), an
893-line lib with an awk module-graph resolver (`tests/infra/harness-layout-lib.sh`), a 218-row baseline
manifest and a diff gate that fails open (`scripts/check-harness-baseline-registration.sh`) police them.

After this PRD:

- **A harness is cargo's own `tests/harness_x/main.rs` form.** A bare `mod foo;` resolves to
  `tests/harness_x/foo.rs`, so the `#[path]` mandate and the wrong-sibling mode cannot exist. Target names, nextest
  binary ids and test names are byte-identical (§2.5).
- **The test-target list is declared, not discovered.** The five consolidatable crates set `autotests = false`
  and list every harness root and grandfathered standalone as an explicit `[[test]]` entry. The baseline manifest
  retires into `Cargo.toml`. Growing the standalone set reds a Python verify step that fails closed.
- **One Python guard reads structured cargo data instead of parsing Rust.** `scripts/harness_layout.py` is the
  engine; `tests/infra/test_harness_layout.py` is its stdlib-unittest member, with a `.sh` wrapper. It checks:
  - every tracked test file is a cargo test target;
  - every `.rs` under a harness directory is in that unit's rustc dep-info;
  - the kLOC cap, on **code lines** from dep-info, with external includes attributed free;
  - no selector names a target that does not exist.
- **What dies:** the bash test, the layout half of the lib (the awk resolver included), the baseline manifest,
  the diff gate and its gate test.

User-observable: an agent adding a test writes `mod new_test;` in `tests/harness_x/main.rs`.

- A new `tests/foo.rs` without a `[[test]]` entry reds the guard member at any tier, naming the file, where
  today it would silently become a new binary.
- A new standalone `[[test]]` entry reds the registration step.
- A file dropped into `tests/harness_x/` without its `mod` line, or a unit over the code-line cap, reds the
  merge gate's post-nextest harness-layout step, naming the file or unit.
- After any build, `python3 scripts/harness_layout.py measure` prints every unit's code-line headroom.

## §2 Background: what the code does at `3f9015b3b5`

### §2.1 Inventory (seat S1; every figure's rule is in the session record)

- **45 harness roots:** reify-cli 2, reify-compiler 22, reify-eval 16, reify-kernel-occt 3, reify-syntax 2.
  Each is a pure `mod` list with no code, no `include!` and no inner attribute.
- **831 member files.** They sit one level deep only. All are tracked and all are declared, so 0 are
  undeclared. 94 carry an inner `#![cfg(…)]`/`#![allow(…)]`, which a move of the root does not touch.
- **850 out-of-line `mod` declarations in the roots:**
  - 831 are `#[path = "harness_<x>/<f>.rs"]` into the root's own directory, one per member;
  - **16 are `#[path = "common/…"]`**: `common/mod.rs` ×13, `common/differential.rs`,
    `common/eval_gate_support.rs` and `common/numeric_cell.rs`;
  - **3 are a bare `mod common;`**, in harness_cli, harness_fea_solver_e2e and harness_occt_measurement.

  The survey's "846 `#[path]`" and the framing seat's 912 counted other file sets; this PRD uses S1's.
- **Standalones** (top-level non-harness `tests/*.rs`) exist only in reify-eval: 220. That is the 218 baseline
  rows plus the override stems `tensegrity_t0a` and `representation_within_assertion`. The other five override
  stems live outside the five crates.
- **No `autotests`, `[[test]]`, `[[bench]]` or `[[example]]` anywhere** in the workspace. No `tests/*/main.rs`
  exists.
- **No build.rs reruns on `tests/` changes.** reify-cli, reify-eval and reify-kernel-occt have build scripts with
  their own `rerun-if-changed` sets; reify-compiler and reify-syntax have none.
- **Where the 16 escaping `#[path]`s sit:** 12 are in reify-compiler roots, 1 in harness_syntax and 3 in
  reify-eval roots. Derive with `git grep -c '#\[path = "common/' -- 'crates/<c>/tests/harness_*.rs'`.
- **Root prose:** 1,675 of 3,402 root lines are comments. They include 18 `re-accretion` lines (harness_engine
  17), 11 `esc-` cites in 11 roots and 34 kLOC/`CAP_LINES` lines. The 348 / 355 / 585 / "31 roots" counts are in
  the lib and the test header, not in roots.
- **References:** the registration script is named by 52 tracked files, 20 of them crate test files (reify-compiler 9,
  reify-eval 10, reify-constraints 1) and 8 capability-manifest twins of 4 other PRDs.

### §2.2 Where the guard runs, and what dep-info exists (seats S4, critic; corrected by the lead)

- **Dark-factory runs `test_command` and `lint_command` as separate, concurrent `verify.sh` processes at every
  role.** The commands are `dark-factory-orchestrator.yaml` `test_command` / `lint_command`, and they run
  concurrently through one `asyncio.gather` in dark-factory `orchestrator/src/orchestrator/verify.py`. The infra
  suite runs in the `test` process, where `verify.sh test` sets `DO_LINT=0 DO_TYPECHECK=0`:
  - **merge/background:** the wholesale `run_all.sh` line inside `build_plan`;
  - **task tier:** the `SELECTED_INFRA_GLOBS` leaf, which `select_harness_kloc_guard` feeds only under
    `--scope branch` and only for a crate whose tests the diff touches. Its `git diff --diff-filter=AM` drops
    pure renames (status R).

  No clippy or `cargo check` precedes the infra suite in that process. `add_test_passes`, which builds and runs
  the nextest debug pass, is emitted after `build_plan`. Seat S4's "clippy precedes the guard" is true only of a
  single-process `verify.sh all`, which dark-factory never runs.
- **The warm-lane seed touches sources on every acquire.** `scripts/seed-warm-lane.sh`'s `env!()` relink sets
  every tracked `tests/**/*.rs` that contains `env!("CARGO_MANIFEST_DIR"|…)` to now: 116 harness members today
  (S3). The seeded `.d` files keep the base's mtimes. So before a build in the current process, a seeded
  `.d` is older than the sources it lists.
- **After the debug nextest pass, every in-scope test target's dep-info is fresh.** The pass compiles each test
  binary in scope, the whole workspace at merge/background, and rewrites its `.d`. A unit cargo judged fresh has
  every source older than its fingerprint.
- **Dep-info:** `target/debug/deps/` holds 4–17 `<unit>-<hash>.d` files per root, and the two newest always
  agree on the `.rs` set (45/45). Clippy's check-mode `.d` lists the same `.rs` set as the test build's.
  - In-tree paths are workspace-relative. Escaping includes appear un-normalised, so a consumer must normalise
    them.
  - 21/45 units list a `tests/common/*.rs` external.
  - 24/45 list non-`.rs` `include_str!` data (`.ri` fixtures).
  - In the main checkout, harness_occt's newest `.d` predates 3 members added on 2026-10-13. A stale `.d` is a
    real state of a real checkout.
  - Consequence (R8): dep-info is read only by a planner step placed right after the debug nextest pass at
    merge/background roles (§5 D10). The run_all member reads cargo metadata and git only.

### §2.3 The measure (seats S2, S4; reconciled by the lead)

- **Code line** = a line whose `scripts/lib_rust_production_view.sh::RUST_LEXER_AWK` view is non-blank. That
  lexer drops comment text, and string-literal contents lex to nothing: the interior lines of a multi-line raw
  string are not code. The lead spot-checked this on a sample, because two seats had counted string interiors
  differently.
- **Totals over the 45 units' dep-info `.rs` sets:** 407,583 raw lines, 247,971 code lines (60.8%).
- **Largest units, raw / code:**

  | unit | raw | code |
  |---|---|---|
  | harness_syntax | 17,994 | 12,777 |
  | harness_topology_selector | 17,522 | 11,215 |
  | harness_fea_solver_e2e | 17,009 | 10,431 |
  | harness_cli | 16,733 | 9,763 |

- **Survey claims re-measured.** Confirmed: 17,994; 8 units ≥ 16,000 raw; 17 units > 12,000 raw. The comment
  shares (17/26/28%) are confirmed as comment-only/raw. Refuted: harness_geometry.rs's prose saying
  harness_fea_solver_e2e is "within ~700 lines of the 20 kLOC cap"; it is 2,991 lines under.
- `CAP_LINES=20000` (raw) and `WARN_PCT=90`. No unit is in the WARN band today.

### §2.4 The step-B substrate probe (G3; seat S3, real cargo 1.96 / nextest 0.9.136 runs)

Directory-driven membership **fails on stable Rust**:

| mechanism | verdict | evidence |
|---|---|---|
| build.rs writes an OUT_DIR mod list, `include!`d | FAIL | Relative `#[path]` and bare `mod` in an `include!`d file resolve against OUT_DIR; only absolute paths compile. Adding or touching any member re-runs the build script and recompiles the lib, the dependent crate and every test target (8 rustc units vs 1). Under the warm-lane seed's `env!()` relink the script re-runs on every acquire. Without the relink, OUT_DIR keeps the base worktree's absolute paths. |
| proc macro listing the directory (automod-style) | FAIL | A new member is silently not compiled until another member changes, both locally and in a copied target. `proc_macro::tracked_path` is unstable. |
| proc macro with its own build.rs watching the directory | FAIL (cost) | Correct, but every member edit recompiles every test target of the crate: 236 test roots in reify-eval. |
| explicit bare `mod` lines in `tests/harness_x/main.rs` | PASS | `mod foo;` → `foo.rs`; nested via `mod bar;` in `foo.rs`. External includes become `#[path = "../common/mod.rs"]`. An undeclared file is silently not compiled, and a directory listing diffed against the unit's `.d` detects it. |

### §2.5 Selector parity (S3)

Old layout (`tests/harness_par.rs` + `#[path]`) vs new (`tests/harness_par/main.rs` + bare `mod`):

- byte-identical cargo target name, nextest binary id (`plib::harness_par`), test names and binary metadata
  hash; only `src_path` differs;
- `binary(harness_par) & test(/^foo::/)` selects the same tests;
- cargo refuses `tests/x.rs` and `tests/x/main.rs` coexisting (duplicate target), so no transitional collision
  is possible.

### §2.6 Premise corrections (code wins; told to Leo 2026-10-19)

1. **The ruled subset check `git ls-files 'crates/*/tests/*.rs'` ⊆ test `src_paths` is false today.** The
   pathspec `*` crosses `/`, so it lists 895 nested module files, which are not targets. The contract that is
   true is: for every workspace package, every tracked `tests/<f>.rs` **and** every tracked `tests/<d>/main.rs`
   is the `src_path` of one of that package's `kind = ["test"]` targets (§6.3 check `registered`).
2. **Step A is not only "strip the `#[path]` on in-directory members."** 19 includes escape the directory. Under
   `main.rs` the 3 bare `mod common;` would resolve to `harness_x/common.rs`, and the 16 `common/…` paths to
   `harness_x/common/…`. All 19 become `#[path = "../common/…"]` in step A's diff (§5 D3).
3. **The bash guard's root detection globs `tests/harness_*.rs`.** `harness_layout_violations`,
   `harness_path_attr_violations`, `harness_undeclared_member_violations`, `harness_layout_unit_lines` and
   `harness_layout_declared_members` would silently stop measuring a moved root. The non-vacuity floors would
   not notice a one-crate move. They must learn both shapes before any move (α).
4. **R7's premise was false.** R7 assumed that clippy precedes the infra suite in the same process. Dark-factory
   runs test and lint concurrently in separate processes, and the warm-lane relink makes seeded dep-info stale
   (§2.2). A run_all member reading dep-info would report `dep-info-stale` in every gate lane. The critic found
   the task-tier half; the lead confirmed that merge has the same shape. Leo ruled R8 on 2026-10-19.
5. **Three of the brief's "other consumers" are comment citations, not calls.** These are
   `harness_layout_baseline_path` in `cited-test-path-lib.sh`, `_harness_layout_baseline_load` in
   `test_no_new_wallclock_rust_deadlines.sh`, and `harness_layout_in_scope_standalone` in `verify.sh`. The only
   code consumer outside the guards is `test_verify_scope.sh`'s B-KLOC-driftguard (`harness_layout_consolidatable_crates`),
   which S18 ζ #9112 deletes. S18 β #9097 then adds the selection BashCall.

## §3 Pre-conditions and already-filed owners

| task | what | relation |
|---|---|---|
| #8700 S19 θ (pending; in #8612's closure) | contract doc `docs/contributor/harness-layout-contract.md`, 35 header repoints, WARN-ratchet deletion | **upstream of α** (ruled; never the reverse). After θ this stream owns the doc's layout half. Its `x_finding_key` moves to `x_finding_part` (fk-53a9afc0d2b9 → ε₂). |
| #9112 S18 ζ (pending; closure) | `scripts/verify.sh` becomes the exec shim | **upstream of δ** (deleting the diff gate's plan step needs a planner edit) and hence of ε₁/ε₂ |
| #9097 S18 β (pending; closure) | `scripts/reify_verify/selection.py` incl. `select_harness_kloc_guard` (BashCall of `harness_layout_consolidatable_crates`) | **upstream of δ and ε₂** (both edit the selector) |
| #9303 S19 υ (pending) | `# run-all-bucket:` pragma replaces `run-all-classification.manifest` | δ registers the new member by whichever is live; no edge |
| #9312 S18 τ (pending; after ζ) | `# guards:` / `GUARDS = [...]` replaces `verify-pipeline-infra-tests.txt` | δ / ε₁ / ε₂ register by whichever is live; no edge |
| #9313 S18 υ (pending; after β) | `[package.metadata.reify-scope]`; `harness_layout_consolidatable_crates` becomes a metadata reader | no edge. The engine needs no crate set (§5 D7). The selector's crate set is S18's. |
| #7445 (pending) | native `test_*.py` discovery | until it lands the member needs its `.sh` wrapper and asserts no wall-clock upper bound |
| #5254 milestone | merge-verify CPU-reduction | no live consolidation leaf: its one open dependency #5286 waits on #5285 (release-sensitive, not harness). Nothing here waits on it. |
| #8146 (pending, low) | registration script's no-arg/stdin trap | interim fix, kept. If still open when δ deletes the script, δ cancels it with `x_subsumed_by` = δ (R5). |
| #8648, #8207, #7749, #5436, #8599, #7557, #7357 (pending) | add or partition harness members, or edit the baseline | sequenced by file locks, not edges. Each received a one-sentence details note at filing (G1 sentence-sized): re-read `docs/contributor/harness-layout-contract.md` at leaf time; after β a root is `tests/harness_x/main.rs` with bare `mod` lines; after δ a new root or standalone needs a `[[test]]` entry. δ moves any old-shape root still present. |
| #9352 syntax-lowering ω₂ (pending) | dissolves `ts_parser/tests.rs` into `harness_syntax_lowering` | re-reads the root at leaf time, per its own PRD. γ splits `harness_syntax`, not `harness_syntax_lowering`. |

**#8612 closure (G1):** recomputed 2026-10-18 through the task API, 240 non-terminal tasks. No leaf of this
stream is a prerequisite of any closure task. #8700, #9097 and #9112 are closure members that leaves here depend
on, which is follow-on form. #9171 and #9242 are not in the closure, and no closure task depends on either.

## §4 Rulings

### From the record (binding; full text in `plans/gap-fill-deliberation-reify-20261016.md`)

- **D-K8-1** (Leo, 2026-10-16): new stream, sequenced A → C → B, then a final Python leaf:
  - step A: `git mv` each root to `tests/harness_x/main.rs` and strip the in-directory `#[path]`s;
  - step C: `autotests = false` with an explicit `[[test]]` list; the baseline manifest retires and
    `check-harness-baseline-registration.sh` is deleted;
  - step B: directory-driven membership behind a G3 probe, settling on explicit bare `mod` lines if the probe
    fails;
  - final leaf: one stdlib-unittest Python member plus wrapper (kLOC cap on code lines from dep-info with
    external attribution, the subset check, the stale-selector check). It deletes `test_harness_kloc_cap.sh` and
    the lib's layout half; functions other consumers read keep their contract until D-K8-6.

  #8700 shrinks to the contract doc, the 35 header repoints and the WARN-ratchet deletion. Step A depends on
  #8700.
- **D-K8-2** (2026-10-16): the WARN tier is advisory (θ deletes the ratchet); "the comment-blind measure and
  numbered shards go to the cargo-native stream". Numbered shards were amended by R3 below.
- **D-K8-3** (uncontested block): deleting the harness-root accretion blocks, restated counts and escalation cites
  moves into step A; #8700 stays pointer-only; S23's exclusion stays true.
- **D-K8-10 (4)** (uncontested block): fk-e6dd1263aafe is absorbed by step C, which carries the fail-closed
  requirement.
- **G1** (2026-10-16): #8612's closure is protected; follow-on form only; no named exception applies here.

### This session (Leo, 2026-10-19; option chosen, with the lead's recommendation noted)

- **R1 — Step B settles on bare `mod` lines, folded.** "Bare mods, fold B (Recommended)."
  - The probe (§2.4) failed for every directory-driven mechanism.
  - Membership is explicit bare `mod` lines in `main.rs`, the state step A already produces.
  - There is no separate B leaf. The undeclared-member check is ε₁'s dep-info check, and Section 10's awk rule
    retires there.
  - The `harness_members!` self-check macro (probe M10) was offered and declined.
- **R2 — The cap is re-based to 12,000 code lines, after splitting harness_syntax first.** "12,000 + split
  first." The recommendation was 14,000; Leo chose the literal 60.8% translation. γ splits harness_syntax
  (12,777 code lines) before ε₁ changes the measure, so no unit reds on the day.
- **R3 — Size-driven splits cluster by subtopic.** Answer verbatim: "Now, in A0, but cluster by subtopic rather
  than simply sharding by number (`harness_<topic>_<subtopic>`) so that each file has a coherent theme". This
  amends D-K8-2's "numbered shards". The rule enters the contract doc in α.
- **R4 — Gate for fk-53a9afc0d2b9 is #9242.** "#9242 (Recommended)." `x_finding_run` is copied from #8700
  (review-all-reify-20261010).
- **R5 — Step C lands whole after ζ, with a planner step.** "After ζ, planner step (Recommended)."
  - δ follows ζ #9112 and β #9097.
  - It deletes the script, its gate test and its plan step. In their place goes a Python planner step that reds
    when the standalone `[[test]]` set grows against merge-base, and reds with `reason=cannot-derive-diff` when
    the base is underivable.
  - δ is the single owner of the script and the baseline manifest. Today's fail-open stays until then.
  - #8146 stays as the interim fix and closes as moot if it is still open when δ lands.
- **R6 — The final leaf splits for the one-run rule.** "Split F1/F2 (Recommended)."
  - ε₁ (survey keys; high; gate #9171): the code-line cap, the undeclared-member check, the hoisted lexer, and the
    deletion of the awk resolver and the bash rules they replace.
  - ε₂ (fk-53a9afc0d2b9; high; gate #9242): the stale-selector check, deletion of `test_harness_kloc_cap.sh` and
    the lib's layout remainder, and the selection retarget.
- **R7 — Dep-info is read, never built, and fails closed.** "Accept, fail closed (Recommended)."
  - The engine proves dep-info fresh.
  - A missing or stale `.d` is a FAIL with a named reason, never a SKIP.
  - The engine never runs cargo beyond `cargo metadata`.
  - Its lane premise ("clippy's check-mode dep-info, the guard runs after the clippy wave") was false; R8
    replaces it.
- **R8 — Dep-info checks run as a planner step after the debug nextest pass.** "Planner step after nextest
  (Recommended)", answering the question in §2.6 4.
  - ε₁ adds a planner step, `harness_layout.py check-dep-info`, emitted right after the debug nextest pass at
    merge and background roles. That pass has just built every workspace test target, so each `.d` is fresh by
    construction.
  - The run_all member runs the cargo-metadata checks live at every tier, and the dep-info checks only
    hermetically, against fixture `.d` files.
  - Accepted cost: a cap breach or an undeclared member is caught at the merge gate, not at the task gate.
  - R7's fail-closed rule holds inside the step.

## §5 Resolved design decisions

- **D1 — The target layout is cargo's own.** A harness unit is `crates/<c>/tests/harness_<topic>[_<subtopic>]/main.rs`.
  - Members are `tests/harness_…/<stem>.rs` (or nested `<stem>/…` through a `mod` in the member), declared by a
    bare `mod <stem>;`.
  - The only `#[path]` a root keeps is an include that escapes its directory, written relative to the directory
    (`#[path = "../common/mod.rs"]`).
  - Member-level `#[path = "../common/angle_expr.rs"]` (4 members, 6 standalones) is relative to the member file
    and unchanged.
- **D2 — α teaches the bash guard both root shapes before any move, at every site that globs roots.** That is:
  `harness_layout_violations`, `harness_path_attr_violations`, `harness_undeclared_member_violations`, Section
  5b/5c's `_s5bc_scan`, the fused Section 6/10 live loop that sets `_s6s10_live_roots` and the members floor, and
  the lib walk.
  - α includes a fixture tree holding only `main.rs` roots, so the guard is proved green and non-vacuous after β₅,
    when no `harness_*.rs` root is left anywhere. Each per-crate move is then a pure mechanical diff that the guard
    measures, and an intermediate tree with both shapes is green.
  - The old shape ends at δ: from δ on, a new old-shape root is a standalone `[[test]]` entry, which the
    registration step reds.
- **D3 — Step A rewrites the 19 escaping includes.** 3 bare `mod common;` and 16 `#[path = "common/…"]` become
  `#[path = "../common/…"]`. It is a correctness item: without it the roots do not compile.
- **D4 — Step A splits per crate after α, smallest first.** β₁ reify-cli validates α on the smallest move. β₂–β₄
  (syntax, kernel-occt, compiler) then run in parallel, with disjoint `crates/<c>/tests/**` locks. β₅ reify-eval
  (16 roots, 247 members, most of the root prose) lands last.
  - Each β is "quiet-window" by construction: its lock lists the crate's roots, so it dispatches when no
    in-progress task holds them.
  - In-flight tasks whose `metadata.files` name an old root path get the moved path at plan time (BRE
    acquire-before-edit). Such tasks number 174 (20 in the closure). No task-record sweep is needed.
- **D5 — Root prose goes in step A (D-K8-3).** A rewritten root is one `//!` pointer line to
  `docs/contributor/harness-layout-contract.md` plus its `mod` list. Accretion blocks, restated counts, `esc-` cites
  and guard-artifact mentions are deleted. "Which harness" becomes a rule in the doc: topic first, then headroom
  read from `python3 scripts/harness_layout.py measure` (from ε₁; before that, the bash guard's PASS lines). The
  esc-5056-11 cite becomes the ruling's sentence in the doc: "the grandfather baseline only shrinks; growing it
  is superseded" (Leo, 2026-07-22).
- **D6 — The engine lives in `scripts/`, the test in `tests/infra/`.** `scripts/harness_layout.py` (stdlib,
  Python 3.12 `tomllib`) is the guard implementation, a CLI plus importable functions.
  `tests/infra/test_harness_layout.py` drives it through hermetic fixtures (`--root` / `--target-dir`) and one live
  run, and `tests/infra/test_harness_layout.sh` is the wrapper. This is fk-53a9afc0d2b9's remedy in the new
  world: the guard is no longer a test file, the contract is in docs, and every name has one owner (heuristics 6,
  9, 13).
- **D7 — Units and scope come from cargo metadata, not a crate list.** A harness unit is any `kind = ["test"]`
  target whose `src_path` is `tests/<d>/main.rs`. A consolidatable crate is one with `autotests = false`
  (from δ). The engine therefore never reads `harness_layout_consolidatable_crates` (heuristics 11, 12). Which
  diffs select the member at task tier stays the selector's question, and S18 owns it: the β BashCall, then #9313's
  metadata reader.
  - Once #9313 adds `[package.metadata.reify-scope].harness_consolidatable`, the same fact has two spellings.
    δ's `test-entry-shape` check reds a crate whose `autotests = false` and `harness_consolidatable = true`
    disagree, from whichever leaf lands second. The equivalence is enforced, not left to prose (heuristic 11).
- **D8 — The `registered` check is the corrected subset check (§2.6 1) and lands with δ.** δ is the change that
  makes an unregistered file silently uncompiled, so the check that catches it lands in the same diff and needs
  only `cargo metadata`. In autotests-on crates it is a tautology by construction. δ also enforces the entry
  shape in `autotests = false` crates:
  - every `[[test]]` has an explicit `path`;
  - `name` equals the file stem, or the directory name for `main.rs`;
  - a `main.rs` unit's directory is named `harness_*`, which the selector's `harness_*/*` arm relies on.
- **D9 — The registration step is diff-scoped and fails closed (R5).** `scripts/harness_layout.py
  check-registration [--base <rev>]`:
  - compares the `tomllib` view of each `autotests = false` crate's flat-path `[[test]]` entries at the base and
    at the working tree;
  - reds `reason=standalone-added` for every entry present in the tree and absent at the base, naming the entry and
    the ruling. This is membership, as in fk-e6dd1263aafe's proposal (rows(HEAD) ⊆ rows(base)): renaming a
    grandfathered standalone is refused from δ on, and the remedy is to fold it into a harness;
  - reds `reason=cannot-derive-diff` when no base can be derived.

  Without `--base`, the engine derives the base exactly as `check-harness-baseline-registration.sh::_derive_added_from_git`
  does today:
  - `MERGE_HEAD` during a merge;
  - `HEAD^1` on a merge commit;
  - `merge-base main HEAD` otherwise.

  That is a 1:1 port. The derivation is then the engine's, as it was the script's. If S18's `ScopeDecision`
  later carries a base, the emitter passes it as `--base` (cross-reference line to S18). The emitter that
  replaces the old `add_tool` line runs under the same role and scope conditions as today's step, the hook
  `--scope staged` path included. A two-branch merge cannot slip past it: each branch's addition reds on its own.
- **D10 — Dep-info is the module-graph authority (R7; fk-b904603b81f2's preferred remedy).** For each unit:
  - `cargo metadata --no-deps --locked --offline` runs once (0.17 s) for `workspace_root`, `target_directory` and
    the targets;
  - candidates are `<target_directory>/debug/deps/<name>-*.d`, parsed as Makefile rules (comments stripped,
    continuations joined, relative entries normalised against `workspace_root`);
  - the newest candidate **whose first prerequisite is the unit's `src_path`** is chosen;
  - freshness is proved: every prerequisite exists; no `.rs` prerequisite is newer than the `.d`; every tracked
    `.rs` under the unit's directory is a prerequisite.

  Failures are reported in this order: `dep-info-missing`, `dep-info-missing-file`, `dep-info-stale`, then
  `undeclared-member`. A stale `.d` is never reported as undeclared. There is no SKIP and no cargo build. A
  checkout with no fresh build gets `dep-info-missing` or `dep-info-stale`, with the hint
  `cargo nextest run --no-run -p <crate>`.

  **Where it runs (R8).** `harness_layout.py check-dep-info` is a planner step, emitted right after the debug
  nextest pass at merge and background roles. That pass has just compiled every workspace test target, so every
  `.d` the step reads is fresh by construction, and the freshness proof is the redundant check (heuristic 10).
  - It never runs in run_all or at task tier: those lanes have no in-process build ahead of them (§2.2).
  - The emitter lives in S18's `build` module, beside the nextest emitter. A follow-on edit after ζ is allowed by
    G1.
  - The same subcommand is the developer's tool after any local build.
- **D11 — The measure (R2).** A code line is a line of a `.rs` dep-info prerequisite whose
  `RUST_LEXER_AWK` view is non-blank. Comments, doc comments and string-literal interiors are not code; attributes
  are code.
  - Non-`.rs` prerequisites (`include_str!` data) are excluded from the count but kept in the freshness proof.
  - External `.rs` includes count per including unit, because rustc compiles a copy into each (#5620, PRD §7).
    Dep-info attributes them with no resolver.
  - `CAP_CODE_LINES = 12000` lives in the engine and nowhere else; the manifest's check matches only
    `^CAP_CODE_LINES = `. The advisory WARN prints at 90% and never gates (D-K8-2).
  - The doc names the constant and the command, not the number.
  - The PASS line carries `code_lines cap root_lines module_lines module_files external_lines external_files
    data_files`.
- **D12 — One lexer (heuristic 11).** `RUST_LEXER_AWK` stays the only Rust lexer.
  `scripts/assert-module-scc.py`'s spawn helpers (`LEXER_LIB`, `LEXER_SPAWN`, `LEXER_DRIVER`, `split_lines`,
  `lex_crate`) are hoisted into `scripts/rust_lexed_source.py`, and both the engine and assert-module-scc.py import
  it. Two adoption-ratchet rows guard it:
  - `RUST_LEXER_AWK` over `scripts/*.py`, with the ceiling set to the hoisted module's own count;
  - `RUST_LEXER_AWK` over `tests/infra/*.py`, ceiling 0.

  So a second spawn or a Python re-lexer reds. No Python re-lexer is written.
- **D13 — Stale selectors: two decision rules, by surface (ε₂).** The surfaces and atoms are today's:
  `binary(<name>)` and `--test <name>` in `.config/nextest.toml`, `scripts/` and `tests/infra/`.
  - **Structured surfaces** are the `filter =` strings of `.config/nextest.toml` and the value of
    `REIFY_HEAVY_NEXTEST_FILTER`. An atom there must name a live cargo-metadata target, per package when it is
    `package(X) & binary(Y)`. This is stronger than today: it also catches a selector naming a target that
    never existed.
  - **Free text** is every other line of `scripts/` and `tests/infra/`, comments included. Today's rule holds:
    an atom is stale only if its name is a harness member stem, i.e. a basename under `crates/*/tests/harness_*/`,
    excluding `main`. Prose that merely mentions `binary(Y)` does not red. The member's own fixture strings are
    excluded by path.
  - Non-vacuity: at least 18 structured atoms are scanned (10 in nextest.toml and 8 in the heavy filter today),
    plus a hermetic must-fire per rule.
- **D14 — Splits cluster by subtopic (R3).** When a unit nears the cap, the split carves a coherent subtopic into
  `harness_<topic>_<subtopic>/main.rs` (precedent: `harness_syntax_lowering`). It does not cut a numbered shard.
  Each resulting unit's members share one theme, and the new target name is the selector.
- **D15 — Verdict grammar (one home: the engine).** Every line starts at column 0, so
  `run_all.sh::_ra_collect_fail_detail` lifts it:

  ```
  HARNESS_LAYOUT FAIL crate=<c> unit=<target> file=<path> reason=<token> [key=value …]
  HARNESS_LAYOUT WARN crate=<c> unit=<target> reason=approaching-cap code_lines=<n> cap=<n>
  HARNESS_LAYOUT PASS crate=<c> unit=<target> code_lines=<n> cap=<n> …
  HARNESS_LAYOUT SUMMARY units=<n> violations=<n> warnings=<n>
  ```

  `reason` is a closed vocabulary, an enum in the engine (heuristic 12): `exceeds-cap`, `dep-info-missing`,
  `dep-info-missing-file`, `dep-info-stale`, `undeclared-member`, `unregistered-test-file`, `test-entry-shape`,
  `orphan-test-target`, `stale-selector`, `standalone-added`, `cannot-derive-diff`, `metadata-unavailable`.

  The wrapper runs the `.py` directly, not under `assert`, so the lines pass through un-indented, and then asserts
  its exit code.
- **D16 — A new invariant, INV-TEST-1 (heuristic 10).** "No tracked integration-test code is silently
  uncompiled: every flat `tests/*.rs` and `tests/*/main.rs` of a workspace package is a cargo test target, and
  every `.rs` under a harness unit's directory is a prerequisite of that unit's dep-info."
  - The registration half is enforced at every tier by the run_all member, and in the task-tier selection.
  - The member half is enforced at merge/background by the post-nextest step (R8).
  - δ adds the `docs/invariants.md` row (`partial: enforced(test)`) and a census row in
    `docs/legibility/design-invariants.md`'s umbrella table (the census obligation). ε₁ flips both to enforced.
- **D18 — A `cfg` gate goes inside the member, not on its `mod` line.** An outer `#[cfg(…)] mod x;` that is false
  on the gate host leaves `x.rs` unread by rustc, so absent from dep-info, and the step reds it
  `undeclared-member`. An inner `#![cfg(…)]` (94 live members) keeps the file read.
  - The contract doc states the rule, and the engine's `undeclared-member` hint names it.
  - The live roots' outer `#[cfg(unix)]` / `#[cfg(target_os = "linux")]` mod lines are true on every gate host.
    They stay, and the hint covers the day one is not.
- **D17 — Residual cost, stated.** B's probe failed, so a new member is still one registration line in a shared
  `main.rs`, and concurrent tail-appends still conflict. Smaller roots and subtopic splits limit it; nothing
  removes it. Revisit when `proc_macro::tracked_path` stabilises (§12).

## §6 Contract (B+H)

### §6.1 Layout (target state, normative home: `docs/contributor/harness-layout-contract.md`)

| rule | enforced by | where / from |
|---|---|---|
| a harness unit is `tests/harness_*/main.rs`; members are bare `mod`; only escaping includes keep `#[path]` (`../…`) | rustc resolution (a wrong path does not compile) | every build / β |
| every flat `tests/*.rs` and `tests/*/main.rs` is a `[[test]]` target; entries carry `path`, `name` = stem; `autotests = false` ⇔ `harness_consolidatable` | engine `registered` / `test-entry-shape` | run_all member, every tier / δ |
| the standalone set never grows (membership, fail closed) | engine `check-registration` | planner step, every role the old diff gate ran in / δ |
| every `.rs` under a unit dir is in the unit's dep-info; `cfg` gates go inside members (D18) | engine `undeclared-member` | post-nextest planner step, merge/background / ε₁ |
| code lines per unit ≤ `CAP_CODE_LINES` | engine `exceeds-cap` | post-nextest planner step, merge/background / ε₁ |
| no selector names a dead target (D13's two rules) | engine `stale-selector` | run_all member, every tier / ε₂ |
| within-crate only (I2); the 7 override binaries stay standalone (I1); no `#[test]` added or removed by a move (I3) | I1: `[[test]]` list; I3: nextest-list parity in each move's done criteria; I2: review | unchanged |

### §6.2 Engine interface — `scripts/harness_layout.py`

```
python3 -I -B scripts/harness_layout.py check [--root DIR]
    the cargo-metadata + git checks live at the leaf's stage (δ: registered, test-entry-shape;
    ε₂: + stale-selector); no dep-info read; exit 0 iff no FAIL. The run_all member runs it live.
python3 -I -B scripts/harness_layout.py check-registration [--base REV] [--root DIR]   (δ)
    diff-scoped standalone-growth check; base derived as §5 D9 when --base is absent
python3 -I -B scripts/harness_layout.py check-dep-info [--root DIR] [--target-dir DIR]   (ε₁)
    freshness, undeclared-member, exceeds-cap over every harness unit's dep-info; the
    post-nextest planner step (R8)
python3 -I -B scripts/harness_layout.py measure [--root DIR] [--target-dir DIR]          (ε₁)
    PASS/WARN lines with code-line headroom for every unit, SUMMARY; the read-out the contract
    doc points at (needs a prior build)
```

Importable functions return frozen dataclasses (`Violation`, `UnitMeasure`), one per check:
`registered_violations`, `entry_shape_violations`, `registration_violations` (δ), `dep_info_violations`,
`cap_violations` (ε₁), `stale_selector_violations` (ε₂). The delivered checks bind to these names. `Reason` is an
`enum.Enum` that gains each leaf's tokens with that leaf, and the printer is the only formatter.

Exit codes are 0 clean, 1 violations and 2 input error (metadata unavailable, a malformed `Cargo.toml`, an
unreadable `.d`). The member and the step report an input error as a FAIL naming the input, never OK.

### §6.3 Check catalogue

| check | inputs | FAIL reasons | runs |
|---|---|---|---|
| `registered` | cargo metadata targets × `git ls-files` per package `tests/` | `unregistered-test-file`, `orphan-test-target` | member (live, every tier) |
| `test-entry-shape` | `tomllib` of each `autotests = false` manifest (+ `reify-scope` key once #9313 lands) | `test-entry-shape` | member (live) |
| `check-registration` | `tomllib` at the base and at the working tree | `standalone-added`, `cannot-derive-diff` | planner step (live) |
| freshness + `undeclared-member` | dep-info, `git ls-files` under the unit dir | `dep-info-missing`, `dep-info-missing-file`, `dep-info-stale`, `undeclared-member` | post-nextest step (live); member (hermetic) |
| cap | dep-info `.rs` set, `rust_lexed_source.lexed_lines` | `exceeds-cap` (WARN advisory at 90%) | post-nextest step (live); member (hermetic) |
| `stale-selector` | structured atoms × target names; free-text atoms × member stems | `stale-selector` | member (live) |

### §6.4 Registration, selection and full-gate routing

- **run_all:** the wrapper `tests/infra/test_harness_layout.sh`. Its bucket is `pool`, by `# run-all-bucket: pool`
  if #9303 has landed, else a `run-all-classification.manifest` row, in δ's diff (the overlay's same-diff
  drift-guard rule).
- **Task-tier selection:** `GUARDS = [...]` in the `.py` if #9312 has landed, else
  `scripts/verify-pipeline-infra-tests.txt` rows. The guarded artifacts are the engine, the lexer module (ε₁),
  `scripts/lib_rust_production_view.sh` (ε₁), `.config/nextest.toml`, `scripts/heavy-test-filter-lib.sh` (ε₂) and
  the five `crates/<c>/Cargo.toml`.
- **`select_harness_kloc_guard`** in `scripts/reify_verify/selection.py` (S18 β):
  - δ adds the wrapper as an output, and `crates/<c>/Cargo.toml` as a trigger, since a new `[[test]]` entry is now
    the re-accretion vector;
  - ε₂ drops `test_harness_kloc_cap.sh` and renames the selector to `select_harness_layout_guard` if no other
    reader names it.
- **Planner steps:** δ's `check-registration` emitter replaces the old `add_tool` line; ε₁'s `check-dep-info`
  emitter sits right after the debug nextest pass at merge/background roles. Both live in S18's `build` module.
- **Full-gate routing:** `tests/infra/*.{sh,py}` already requires the full gate. `scripts/harness_layout.py` and
  `scripts/rust_lexed_source.py` do not today (rc 1).
  - δ and ε₁ make `bash scripts/verify-pipeline-guard.sh requires-full-gate <each>` exit 0 by the mechanism live
    when they land:
    - an emitter-declared gate path, if ζ's `--print-gates` derives Python step leaves;
    - otherwise a `scripts/verify-pipeline-paths.txt` row.
  - BT14 asserts this.

## §7 Boundary-test sketch

Both sides of each seam. The member's hermetic fixtures (a tiny workspace with fixture `Cargo.toml`s, fixture
`.d` files and `git init`) carry the engine side. The live tree and the planner carry the production side.

| # | scenario | precondition | postcondition |
|---|---|---|---|
| BT1 | move parity (β) | crate before/after its β diff | `cargo nextest list -p <crate>` binary ids and test names identical; guard PASS lines name the same unit count for the crate |
| BT2 | dual-shape guard (α) | fixture tree A with one `harness_a.rs` + `#[path]` root and one `harness_b/main.rs` + bare-`mod` root; fixture tree B with ONLY `main.rs` roots | both units measured (module_lines > 0); on B the live-shaped floors (roots ≥ 1, members ≥ 1, module-wired and external-wired witnesses) hold; Section 6 does not flag `harness_b`'s bare `mod`; a bare `mod common;` in `harness_b/main.rs` is flagged |
| BT3 | escaping include | `main.rs` with `#[path = "../common/mod.rs"] mod common;` | compiles (live, every β); dep-info lists `tests/common/mod.rs` as external |
| BT4 | unregistered file (δ) | `autotests = false` fixture; add `tests/stray.rs` and `tests/harness_new/main.rs` without entries | `FAIL … reason=unregistered-test-file` naming each; autotests-on fixture with the same files: PASS |
| BT5 | entry shape (δ) | `[[test]]` without `path`; `name` ≠ stem; `tests/foo/main.rs` unit not named `harness_*`; `autotests = false` with `harness_consolidatable = false` (once the key exists) | `FAIL reason=test-entry-shape` each |
| BT6 | standalone growth (δ) | fixture repo: base commit with N flat entries; working tree with one added, one renamed, one removed | added and renamed → `FAIL reason=standalone-added` naming each; removal alone → PASS; no `main` ref, no merge state, `--base deadbeef` → `FAIL reason=cannot-derive-diff`, rc ≠ 0; each of the three base derivations (§5 D9) exercised |
| BT7 | stale beats undeclared (ε₁) | fixture `.d` older than a listed `.rs`, plus a tracked `.rs` absent from it | `reason=dep-info-stale`, no `undeclared-member` line |
| BT8 | undeclared member (ε₁) | fresh `.d`; tracked `tests/harness_x/orphan.rs` not listed; a member behind an outer `#[cfg(any())]` | `FAIL reason=undeclared-member` naming each, the second with D18's hint |
| BT9 | missing dep-info (ε₁) | empty target dir | `FAIL reason=dep-info-missing` with the build hint; never PASS, never SKIP |
| BT10 | cap (ε₁) | fixture unit at 12,001 code lines (comment-padded to 20,000 raw); one at 11,999 code with 30,000 raw | first FAIL `exceeds-cap`; second PASS (comments and string interiors do not count) |
| BT11 | attribution (ε₁) | two units including one `tests/common/x.rs` of 500 code lines; a unit with a 5,000-line `include_str!` fixture | each including unit's `external_lines` = 500; data excluded from `code_lines`, listed as `data_files` |
| BT12 | stale selector (ε₂) | fixture `nextest.toml` with `binary(gone)` and `package(p) & binary(harness_x)`; a fixture script comment `# see binary(some_prose)` and one naming a moved member stem | `binary(gone)` FAILs; the live atom passes; the prose comment passes; the member-stem comment FAILs |
| BT13 | live member (every leaf from δ) | main after the leaf | `bash tests/infra/test_harness_layout.sh` green at task tier and in run_all; non-vacuity: ≥ 37 packages read, ≥ 45 harness units registered, ≥ 18 structured selector atoms (ε₂) |
| BT14 | planner seams (δ, ε₁) | `--print-plan` at merge, background, task, offline and hook-staged roles | the registration step appears wherever `check-harness-baseline-registration.sh --from-git` appeared; `check-dep-info` appears right after the debug nextest pass at merge/background and nowhere else; `requires-full-gate` exits 0 for the engine (and the lexer module from ε₁) |
| BT15 | live dep-info (ε₁) | merge-tier plan on main after ε₁ | the `check-dep-info` step is green, measures ≥ 45 units, ≥ 1 external, and a mutation (an orphan `.rs` under a unit dir) reds it naming the file |

## §8 Cross-PRD relationship (G4)

| other PRD / artefact | direction | seam | owner | status |
|---|---|---|---|---|
| `docs/prds/merge-gate-compile-cost.md` §5 C1/C2, §7 | amends | the dated 2026-10-16 addendum already supersedes C1/C2 step by step. ε₁ appends one dated line: cap re-based to code lines (`CAP_CODE_LINES`, R2) and subtopic splits (R3). | this PRD (ε₁) | addendum present; ε₁ line queued |
| `docs/prds/v0_6/run-all-runner-infra-harness.md` (S19) | consumes | θ #8700 creates the contract doc; this stream owns its layout half from α on. #9303 pragma registration. D11's WARN amendment is the applier's. | θ: S19; layout half: this PRD | θ pending |
| `docs/prds/v0_6/verify-structured-plan-and-role-table.md` (S18) | consumes | `selection.py::select_harness_kloc_guard` (δ, ε₂ edit it, after β); in the `build` module, the registration emitter replacing the `add_tool` line (δ) and the `check-dep-info` emitter after the debug nextest pass at merge/background (ε₁), both after ζ; full-gate routing of the engine (§6.4); #9312 GUARDS registration; #9313 keeps `harness_layout_consolidatable_crates` alive for the selector, and δ's `test-entry-shape` enforces its equivalence with `autotests = false`; if `ScopeDecision` later carries a base, the registration emitter passes it as `--base` | S18 owns the modules; δ/ε₁/ε₂ make follow-on edits | β/ζ pending |
| `docs/prds/v0_6/syntax-lowering-contract.md` | neighbour | ω₂ #9352 adds members to `harness_syntax_lowering`. γ splits `harness_syntax`. Both re-read the root at leaf time. | each its own | no edge |
| capability manifests naming `check-harness-baseline-registration.sh` (4 PRDs, 8 files) | affected | prose/evidence mentions; δ deletes the script | those PRDs (cross-reference line, not edited here) | — |
| delivered checks that the moves would flip (decompose-time scan of all 1,024 tasks with checks, 2026-10-19) | affected | five checks read a moved or deleted path. Those with non-terminal dependents were widened by this session as three writes each, to pass both before and after: #5789 `harness-layout-ratchet` (angle-units-surface-convergence twins), #6745 `reify-eval-tests-must-be-harness-submodules` (gui-on-demand-measurement twins), #8700 `contract-doc-home` (run-all-runner-infra-harness twins). #5477/#5479 (done, no non-terminal dependents) are left as they are. Each moving or deleting leaf re-scans at landing. | this PRD | widened |
| `docs/invariants.md`, `docs/legibility/design-invariants.md` | extends | INV-TEST-1 row + census row (δ, ε₁) | this PRD | queued |
| G4 row (program doc §Gap-fill deliberation) | — | `harness layout ([[test]] list, tests/<harness>/main.rs, residual Python guard) \| cargo-native-harness-layout \| S19 θ #8700 (contract doc), S18 β #9097` | this PRD | adopted |

No reciprocal "the other owns it" remains. #9313's `harness_consolidatable` key and δ's `autotests = false` state
the same fact for the same five crates. δ's `test-entry-shape` check enforces their agreement (§5 D7). Whether
#9313 derives the key from `autotests` instead is a cross-reference line to S18.

## §9 Decomposition plan

Every leaf below is follow-on to #8612's closure: it depends on closure tasks, and none is depended on by one.
Every edit under `crates/**` runs the workspace suite, and every edit under `tests/infra/` or to a verify-pipeline
file takes the full gate.

**Every leaf that moves or deletes a file re-runs the delivered-check scan at landing.** That covers β₁–β₅, γ,
δ, ε₁ and ε₂, scanning every task's `metadata.delivered_checks` for paths and patterns naming what the leaf
moves or deletes. Each such check is widened (three writes: producer task metadata, the `.yaml` twin, the `.md`
twin, resubmitting the complete list) and reported in the commit message. This is the #9112 pre-landing
precedent. This session already widened the three known cases (§8).

| label | id | title | prio | depends on | user-observable signal |
|---|---|---|---|---|---|
| **α** | #9396 | Guard learns the `tests/harness_x/main.rs` root shape at every root-glob site; contract doc's layout half (main.rs form, subtopic splits, placement rule) | medium | #8700 | a fixture tree with only `harness_*/main.rs` roots is measured green and non-vacuous; a bare `mod common;` in such a root reds |
| **β₁** | #9397 | Step A, reify-cli: roots to `main.rs`, `#[path]` stripped, `mod common` re-pathed, root prose deleted | medium | α | `crates/reify-cli/tests/harness_cli/main.rs` holds bare `mod` lines; `cargo nextest list -p reify-cli` identical |
| **β₂** | #9398 | Step A, reify-syntax | medium | β₁ | as β₁ for reify-syntax |
| **β₃** | #9399 | Step A, reify-kernel-occt | medium | β₁ | as β₁ for reify-kernel-occt |
| **β₄** | #9400 | Step A, reify-compiler (22 roots, 12 `common` includes) | medium | β₁ | as β₁ for reify-compiler |
| **β₅** | #9401 | Step A, reify-eval (16 roots); heavy-filter atom test repointed; the last root prose | medium | β₁–β₄ | as β₁ for reify-eval; no `esc-5056-11` / `re-accretion` line left in any root |
| **γ** | #9402 | Split harness_syntax by subtopic (`harness_syntax_<subtopic>/main.rs`) ahead of the code-line cap | medium | β₂ | every reify-syntax unit ≤ 10,800 code lines; `cargo nextest list -p reify-syntax` test names identical modulo binary id |
| **δ** | #9403 | Step C: `autotests = false` + explicit `[[test]]` lists; baseline + diff gate retired; engine `check` / `check-registration`; planner step fails closed; INV-TEST-1 | medium | β₁–β₅, #9112, #9097 | a stray `tests/foo.rs` reds `test_harness_layout.sh` naming it; a new or renamed standalone `[[test]]` reds the registration step; `harness-layout-baseline.manifest` is gone |
| **ε₁** | #9404 | Engine `check-dep-info` (code-line cap, freshness, undeclared member) as a post-nextest merge-gate step; lexer hoisted; bash cap/undeclared rules and awk resolver deleted (B folded) | **high**, gate #9171 | δ, γ | the merge gate's `check-dep-info` step reds an orphan `.rs` under `tests/harness_x/` naming it; `python3 scripts/harness_layout.py measure` prints 12,000-cap headroom per unit after a build |
| **ε₂** | #9405 | Engine stale-selector check; `test_harness_kloc_cap.sh` and the lib's layout remainder deleted; selection retargeted | **high**, gate #9242 | ε₁, #9097 | a `binary(gone)` in `.config/nextest.toml` reds the member; `tests/infra/test_harness_kloc_cap.sh` no longer exists |
| **Ω** | #9406 | PRD close: SHIPPED header, landed ids, AS-AUTHORED freeze | low | α–ε₂ | the committed header |

**Edges onto gates:** `#9171 ← ε₁` (fk-e2a244cfb575, high), `#9242 ← ε₂` (fk-53a9afc0d2b9, high, R4). Both were
checked against the live closure. No edge is added on #8612.

### Leaf detail

**α — dual-shape guard, contract doc's layout half** (x_finding_part fk-e2a244cfb575, fk-9e9f9b36fbcc,
fk-10c353ee8ed7).

- **Every root-glob site learns `harness_*/main.rs`.** Each iterates `harness_*.rs` ∪ `harness_*/main.rs`, with
  the module dir `${root%.rs}` for the first and `dirname` for the second:
  - `test_harness_kloc_cap.sh::harness_layout_violations`;
  - `harness_path_attr_violations`;
  - `harness_undeclared_member_violations`;
  - Section 5b/5c's `_s5bc_scan`;
  - the fused Section 6/10 live loop (`_s6s10_live_roots` and its per-crate member count).
- **Lib:** `_harness_layout_walk_unit`, `harness_layout_unit_lines` and `harness_layout_declared_members` resolve a
  bare `mod` in a `main.rs` root against its own directory, like the existing `mod.rs` branch, and
  `_HL_WALK_MODDIR_PREFIX` is that directory.
- **Section 6** skips bare `mod` in a `main.rs` root unless it would resolve outside the directory onto a retained
  sibling, i.e. `mod common;` under `main.rs` is flagged. Section 7's stem derivation excludes `main`.
- **BT2 with both fixture trees.** Tree B (only `main.rs` roots) proves every floor the guard asserts on the live
  tree still holds once β₅ leaves no old-shape root. Name the floors in the commit message: Section 5b/5c's
  witnesses, Section 6/10's roots ≥ 13 and members ≥ 300, and their live counts after each β.
- **Restated counts:** delete what θ left in the two guard files (348, 355, 585, "31 roots"), each replaced by the
  command that derives it (D-K8-3).
- **Contract doc** (created by #8700): the layout section states the `main.rs` form (D1, D3, D18), with a
  transition line: both shapes are accepted until β₁–β₅ land, and δ ends the old shape. It also gains:
  - the subtopic split rule (D14, R3, quoting `harness_<topic>_<subtopic>`);
  - the "which harness" rule (D5), with the esc-5056-11 cite replaced by the ruling's sentence;
  - a note that task texts written before this stream name `tests/harness_x.rs` for what is now
    `tests/harness_x/main.rs`.
- **Done when** BT2 is green on both trees and the live guard is green with every floor met.

**β₁–β₅ — step A per crate** (β₁–β₄: x_finding_part fk-e2a244cfb575, fk-9e9f9b36fbcc; β₅: x_finding_key
fk-9e9f9b36fbcc, x_finding_part fk-e2a244cfb575).

- For every root of the crate: `git mv crates/<c>/tests/harness_<x>.rs crates/<c>/tests/harness_<x>/main.rs`;
  delete each `#[path = "harness_<x>/<f>.rs"]` line, leaving `mod <f>;`. Keep the existing order (§12 Q1).
- Rewrite each escaping include to `#[path = "../common/…"]` (D3). The bare `mod common;` sites are in harness_cli
  (β₁), harness_occt_measurement (β₃) and harness_fea_solver_e2e (β₅). The `common/…` `#[path]` sites are 12 in
  β₄, 1 in β₂ and 3 in β₅. Re-derive with §2.1's command at leaf time.
- Root prose → one `//!` pointer line (D5). In β₄ the one member citing esc-5056-11 gets the same pointer.
- β₅ also points `tests/infra/test_heavy_filter_atoms.sh` at `harness_fea_solver_e2e/main.rs` and deletes
  harness_geometry's stale "~700 lines" claim with the rest of the prose.
- `tests/infra/affected-crates-direct-sets.tsv` rows that name old root paths are a pinned input corpus, not a
  tree oracle. Leave them; #9319 retires B13.
- **Done when:**
  - BT1: record the `cargo nextest list -p <crate>` test count in the commit message;
  - BT3 compiles;
  - `bash tests/infra/test_harness_kloc_cap.sh` is green, its PASS lines include every moved unit, and every
    floor holds. Run it explicitly: the task-tier selector's `--diff-filter=AM` drops renames. The merge tier runs
    it in the pool;
  - the delivered-check re-scan is done.

**γ — harness_syntax subtopic split** (x_finding_part fk-10c353ee8ed7; ruling R2/R3).

- Carve at least one coherent subtopic of `crates/reify-syntax/tests/harness_syntax/` into
  `harness_syntax_<subtopic>/main.rs`, so every resulting unit is ≤ 10,800 code lines (below the 90% advisory
  line of the 12,000 cap) under D11's rule.
- 55 members total 12,620 code lines. Candidate themes include declarations and members (`member_*`,
  `sub_decl_*`, `field`, `port`, `trait`, `type_alias`, `unit_decl`, `constraint_*`), literals (`numeric_separators`,
  `radix_literals`, `scientific_notation`, `interpolated_string`, `undef_literal`) and function bodies (`fn_*`,
  `lambda`). The leaf picks the seam by member theme, not by size alone.
- Moved members keep their stems, so test names are unchanged and only the binary id changes for them. A
  `binary(harness_syntax)` / `--test harness_syntax` selector naming a moved member's tests is updated in the same
  diff. None exists today (S1 §6).
- If δ has landed, add the `[[test]]` entry in the same diff.
- **Done when:**
  - the measure shows every reify-syntax unit ≤ 10,800 code lines: the bash guard's walk plus
    `rust_lexed_source`/`RUST_LEXER_AWK`, or `harness_layout.py measure` after a build if ε₁ is live;
  - the nextest test inventory is identical modulo binary id;
  - the guard is green, and the delivered-check re-scan is done.

**δ — step C** (x_finding_key fk-e6dd1263aafe, x_finding_part fk-e2a244cfb575; rulings D-K8-10 (4), R5).

- **Manifests:** each of the five `crates/<c>/Cargo.toml` gets `autotests = false` under `[package]` and an explicit
  `[[test]]` entry (`name`, `path`) for every harness unit and every standalone. reify-eval has 16 + 220, the
  others only their harness units. Any old-shape root still present (landed after β) moves to `main.rs` in this
  diff.
- **Engine and member:**
  - `scripts/harness_layout.py` gets `check` (`registered`, `test-entry-shape`, D7's `reify-scope` agreement once
    the key exists) and `check-registration`, with D9's membership semantics and its three base derivations;
  - `tests/infra/test_harness_layout.py` covers BT4–BT6 hermetically and BT13 live. Its module docstring points
    at `docs/contributor/harness-layout-contract.md`, which #8700's widened `contract-doc-home` check reads;
  - `tests/infra/test_harness_layout.sh` is the wrapper, in pass-through form (D15).

  Register the member per §6.4 in this diff.
- **Planner (S18 seam):**
  - In the `build` module, the emitter that rendered `./scripts/check-harness-baseline-registration.sh --from-git`
    now emits `harness_layout.py check-registration` under the same role and scope conditions, the hook
    `--scope staged` path included. Its gate paths make `requires-full-gate` exit 0 (§6.4, BT14).
  - `select_harness_kloc_guard` gains the wrapper and the `crates/<c>/Cargo.toml` trigger.
  - Enumerate every lane that runs the step, fixture repos included, and prove each derives a base. A fixture
    repo that drives a real verify with no `main` ref (e.g. `tests/infra/test_verify_semaphore_e2e.sh` Section B)
    gets one. A lane that genuinely cannot derive a base reds by design; name it in the commit message.
- **Bash guard, at function and global level.** After δ, `bash tests/infra/test_harness_kloc_cap.sh` must still run
  under `set -u` and every surviving floor must be non-vacuous. Delete or move:
  - delete rule (b), Sections 2, 3 (baseline arm), 3b, 3c, 6, 8 and 9, `_bare_mod_decls` and
    `harness_path_attr_violations`;
  - delete the `BASELINE` and `OVERRIDE_BINARIES` globals and Section 5's baseline assertion;
  - drop `harness_layout_violations`' baseline argument, along with every hermetic caller's fixture baseline;
  - move the fused Section 6/10 live loop's undeclared-member half into Section 10, so Section 10 sets its own
    `_s10_live_*` state;
  - keep `_emit`.
- **Lib:** delete the baseline functions (`harness_layout_baseline_*`, `_harness_layout_baseline_load`),
  `harness_layout_override_stems` and `harness_layout_in_scope_standalone`. The override "not a cap-overflow
  destination" rule moves to the contract doc.
- **Deleted files:** `tests/infra/harness-layout-baseline.manifest`, `scripts/check-harness-baseline-registration.sh`
  and `tests/infra/test_harness_baseline_registration_gate.sh`.
- **Every live reader repointed.** Run `git grep -l -e check-harness-baseline-registration -e harness-layout-baseline`
  at leaf time. The known ones:
  - `tests/infra/test_verify_pipeline_guard.sh::_PAIR_E_PLAN_LEAF_GROUND_TRUTH`;
  - `tests/infra/test_verify_ld_library_path_scope.sh`'s `expect_scrubbed` line naming the script;
  - `tests/infra/test_verify_scope.sh`;
  - the 20 crate test files naming the script (→ contract doc);
  - the comment citations in `cited-test-path-lib.sh`, `test_no_new_wallclock_rust_deadlines.sh` and
    `tests/infra/wallclock-rust-deadline-baseline.txt`, which become the `[[test]]` list;
  - the contract doc's registration section, now the `[[test]]` list, which also drops α's transition line.

  ζ may already have rewritten some of these.
- **INV-TEST-1:** the `docs/invariants.md` row (`partial: enforced(test)`) and the design-invariants census row
  (D16).
- **#8146:** if it is still open, cancel it with reason "script deleted by δ" and `metadata.x_subsumed_by` = δ.
  Check for spawned children first.
- **Done when:**
  - BT4–BT6, BT13 and BT14 are green;
  - `bash tests/infra/test_harness_kloc_cap.sh` is green with its floors met;
  - `cargo nextest list` inventories for the five crates are identical to before;
  - the commit message lists the per-crate `[[test]]` counts;
  - the delivered-check re-scan is done.

**ε₁ — dep-info step, measure, undeclared member; B folded** (x_finding_key fk-e2a244cfb575, fk-b904603b81f2,
fk-10c353ee8ed7; high; gate #9171; rulings D-K8-1, D-K8-2, R1, R2, R7, R8).

- **Measure first.** Run the cap over every unit (after a build). For each unit over 12,000 code lines at
  implementation time, stop and escalate naming it, so a γ-shaped subtopic-split leaf is filed and wired ahead of
  ε₁. There is no cap bump (R2) and no in-leaf multi-crate split. harness_topology_selector is at 11,215 today.
- **Lexer:** hoist `scripts/rust_lexed_source.py` (D12) with both adoption-ratchet rows; assert-module-scc.py
  imports it.
- **Engine:**
  - `check-dep-info` and `measure`: the locator and freshness proof (D10), the cap (D11, `CAP_CODE_LINES = 12000`,
    WARN advisory) and `undeclared-member` with D18's hint;
  - the member gains BT7–BT11 hermetically.
- **Planner (R8):** the `check-dep-info` emitter right after the debug nextest pass at merge/background roles (BT14,
  BT15). Full-gate routing for the lexer module (§6.4).
- **Bash guard:**
  - delete rule (a), Sections 1–1f, 4, 4b, 5, 5b, 5c and 10, `harness_undeclared_member_violations`, and
    `run_harness_layout_scan` if nothing else calls it;
  - keep `_emit` and Section 7, which ε₂ ports;
  - after ε₁, `bash tests/infra/test_harness_kloc_cap.sh` runs under `set -u` and is green.
- **Lib:** delete the awk resolver `_harness_layout_norm_path`, `_harness_layout_mod_decls`,
  `_harness_layout_walk_unit`, `harness_layout_unit_lines` and `harness_layout_declared_members`.
- **Docs:**
  - contract doc: the measure (D11, naming the constant and the `measure` command), the membership rule (R1: bare
    `mod`; an undeclared member reds the merge gate; D18's `cfg` rule);
  - `docs/prds/merge-gate-compile-cost.md`: one dated line in its 2026-10-16 gap-fill amendments (§8 row);
  - INV-TEST-1 → `enforced(test)`; census row → shipped.
- **Done when:** BT7–BT11, BT14 and BT15 are green; the live `check-dep-info` step passes on the merge plan at 12,000;
  and the delivered-check re-scan is done.

**ε₂ — stale selectors, bash guard deleted** (x_finding_key fk-53a9afc0d2b9; high; gate #9242; x_finding_run
review-all-reify-20261010; rulings R4, R6).

- **Engine:** the `stale-selector` check with D13's two rules, in `check`. The member gains BT12 and BT13's
  ≥ 18-atom floor.
- **Deleted:** `tests/infra/test_harness_kloc_cap.sh` with its registration (manifest row or pragma; infra-map rows
  or guards), and every remaining layout function of `tests/infra/harness-layout-lib.sh`.
  `harness_layout_consolidatable_crates` stays until #9313, or whatever #9313 left.
- **Selection:** `selection.py` selects only the new wrapper (§6.4). Repoint every remaining reader of the old path
  (`git grep -n test_harness_kloc_cap`: S18 members, docs/notes, the contract doc's guard section).
- **Done when:**
  - BT12 and BT13 are green;
  - `tests/infra/test_harness_kloc_cap.sh` does not exist;
  - `git grep -n test_harness_kloc_cap -- scripts tests .config ':!tests/infra/affected-crates-direct-sets.tsv'` is
    empty;
  - the delivered-check re-scan is done, widening #8700's `kloc-warn-ratchet-deleted` if a dependent of #8700 is
    still open.

**Ω — PRD close.**

- Status → SHIPPED, naming the landed leaf ids, with the AS-AUTHORED freeze paragraph and the LIVE / AS-AUTHORED
  map. §6.1 stays LIVE only until the contract doc is the sole home; then it is AS-AUTHORED too.
- Apply the same header to the `.capability-manifest.md`.
- A cancelled sibling counts as satisfied.

### G7 walk (reify family; INV-SF-1..7, umbrella)

- **Umbrella `nothing-vacuous-and-unowned`:** the new detector carries a census row in its own change (δ adds it,
  ε₁ flips it).
- **INV-SF-3 (declared intent consumed or diagnosed):** the stream's purpose is that a placed test file is
  compiled or diagnosed (INV-TEST-1).
- **INV-SF-5 (placeholders owned and loud):** no SKIP path exists (R7), and `cannot-derive-diff` replaces a silent
  PASS (R5).

No other INV-SF applies: there is no `.ri` surface, diagnostic code or Undef path. No waivers.

## §10 Finding-key disposition

| key | run | sev | closing leaf (`x_finding_key`) | parts | gate |
|---|---|---|---|---|---|
| fk-e2a244cfb575 | hotspot-survey-reify-20261015 | high | ε₁ | α, β₁–β₅, δ | #9171 |
| fk-b904603b81f2 | hotspot-survey-reify-20261015 | medium | ε₁ | — | — |
| fk-10c353ee8ed7 (measure/split half) | hotspot-survey-reify-20261015 | medium | ε₁ | α, γ | — |
| fk-9e9f9b36fbcc | hotspot-survey-reify-20261015 | medium | β₅ | α, β₁–β₄ | — |
| fk-e6dd1263aafe | hotspot-survey-reify-20261015 | low | δ | — | — |
| fk-53a9afc0d2b9 | review-all-reify-20261010 | high | ε₂ (moved from #8700, which keeps it as `x_finding_part`) | — | #9242 |

## §11 Out of scope

- Directory-driven membership. The probe failed (§2.4); revisit only on the §12 trigger.
- Consolidating more standalones: the 218 grandfathered reify-eval binaries stay, now as `[[test]]` entries, and
  their folding (e.g. #7749) is separate work.
- Crates beyond the five: autotests stays on and the `registered` check is a tautology there.
- `[package.metadata.reify-scope]` (S18 υ #9313) and the selector's crate set.
- The run_all bucket and guards headers (S19 υ #9303, S18 τ #9312): consumed, not edited.
- `docs/legibility/confusion-codebook.yaml`'s mention of `harness_layout_unit_lines`: a machine-written artifact,
  refreshed by its own census, not edited here.

## §12 Open questions (tactical)

1. **Member ordering.** Alphabetic insertion would spread concurrent `mod` additions (D17), but reordering can
   change `macro_rules!` textual scope between members. β keeps the existing order. Whether the contract doc
   recommends sorted insertion for new members is decided in α.
2. **Revisit trigger for step B.** Stabilisation of `proc_macro::tracked_path`, or a cargo mechanism that
   fingerprints a directory per test target. Whoever lands that toolchain bump files the re-probe. No task now.
3. **Subtopic seam for γ.** The leaf picks it by member theme within D14 (§9 γ lists candidates).
