# PRD — prose rehoming and symbol citations (review-all S23)

**Milestone:** v0_6 · **Status:** pass 1.5 (authored 2026-10-15; the lead's rulings on all eight open questions and an opus/high critic pass are folded, §12; ready to decompose) · **Approach:** bare B (prose and pointer edits in place, plus one table-driven test and one helper relocation; no new mechanism, no new seam).

**Program:** `plans/review-all-program-reify-20261012.md`, stream **S23** (wave 4, the program's last stream; agent mode, size L, risk low). Ruling: §Resolved decisions → "S23 Prose rehoming and symbol citations", read with §Invariant candidates (candidate 3), §Contradictions ("one reify-audit PCITE lane owned by S24 instead of four line-cite lints"), the §Lead amendments (notably **one home per invariant id**, Leo S12 R7, and reify's four-site lockstep definition) and the §Shared conventions (S24 same-diff obligations). This PRD does not reopen any of them.

**Findings owned (29):** fk-465b5eec51e2, fk-4186fb4f7289, fk-cb27e6ba131b, fk-91aaee6b8fde, fk-5f3dcd2ae885, fk-8c68164d0151, fk-25c46b3f4c72, fk-632e4850d7d0, fk-e4b87a3c5f90, fk-b0a7b8818be4, fk-e90cf61da43a, fk-d0569c316d46, fk-7fbe91c20e82, fk-9e30579d2c52, fk-70d435aa0170, fk-3ac9bc99b963, fk-67ca81ff9067, fk-22068b67e813, fk-679840f73698, fk-eb5a9e37eb9f, fk-15e94643506d, fk-2e1e82c1d1c6, fk-e2730bcb8111, fk-a401e42a7b49, fk-420be0954017, fk-863955f49c19, fk-5b6284e261af, fk-a26634db4007, fk-14195f0a11d2. The key-to-leaf map is §9; every key lands on exactly one leaf or on a §11 disposition line.

**Invariant:** INV-DOC-1 (tracked code comments cite code by symbol, never by `<file>.<ext>:<line>`), promoted by S24 ο #8822 as invariant candidate 3 through PLINECITE. S23 establishes no new invariant and carries no lint of its own. It burns down INV-DOC-1's grandfathered ledger, and every burn-down leaf cites INV-DOC-1 as the invariant it strengthens.

**Code anchors** were re-measured against main `9bdef68093` (2026-10-15) by six read-only measurement seats; the review's anchors are dated `a030ee7528` (2026-10-10). Almost every other-stream leaf named below is filed but **pending**, so most S23 target files are byte-identical to the anchor today and will be moved, split or partly rewritten before S23 dispatches. Line numbers in §2 are dated evidence at `9bdef68093`. Leaves cite by symbol; re-locate everything at implementation time, at the post-move paths.

---

## 1. Consumer and user-observable surface (G1, G2)

The consumer is the **next agent that edits one of these files.** Today it follows a comment pointer such as `seed-warm-lane.sh:563` or `See design decision in plan.json` and lands on unrelated code, an untracked file, or a claim the code contradicts (INV-BUILD-1 "still proposed", "all 49 GeometryOp variants", PTODO "all Medium severity", "Both tets have volume 1/6"). It has to re-derive the construct by grep, or it trusts the stale claim. The finding records measured this repeatedly: 34 of 44 sampled infra cites stale, 16 of 16 in the analysis stdlib, 11 of 11 in constraints and geometry, 6 of 6 in the OCCT crate.

The work has three observable surfaces, all of which already exist:

- **The PLINECITE merge gate** (`reify-audit --pattern PLINECITE`, gated by `tests/infra/test_reify_audit_plinecite.sh` after S24 ο #8822). Its ledger `crates/reify-audit/plinecite-baseline.txt` holds one `<path> :: <cite>` row per grandfathered cite. A new cite reds a merge; a ledger row with no live cite is a Medium `stale-baseline-entry` that `plinecite-baseline-gen` prunes (shrink-only by default). **The burn-down signal is a ledger with no rows for a leaf's paths, and the gate still exiting 0.**
- **`cargo test -p reify-core`**, where 136 per-variant DiagnosticCode derive tests become one table-driven round-trip test.
- **The comments themselves**, read by `git grep`: every false claim a finding measured is either gone or replaced by a one-line pointer to the claim's single home. Each such leaf carries a `delivered_check` (an `absent` grep scoped to the false sentence).

This PRD introduces no mechanism that needs a consumer. The one code relocation (ε₂, `HexWedgeMeshOutcome`) moves an existing helper next to its named future caller, #4746.

## 2. Problem, re-measured at main `9bdef68093`

### 2.1 The population has grown and moved since the ruling

The ruling says "~700 file:line citations", "334 infra and ~150 crate line cites". Those were slice samples. Measured over the scope PLINECITE will gate after #8822 (the program rule: comment lines matching `[A-Za-z0-9_./-]*[A-Za-z0-9_-]\.(rs|ri|sh|py|ts|tsx|toml|yaml|yml):[0-9]+`):

| scope (after #8822) | cite occurrences |
|---|---|
| `crates/*/src/**.rs` (less reify-audit) | 883 |
| `crates/*/{tests,benches}/**.rs` | 962 |
| `crates/**/*.ri` | 10 (all in #8305's scope) |
| `tests/infra/**` | 314 (309 comment lines; 38 point only at external packages) |
| `scripts/**` | 70 lines (25 external only) |
| `hooks/*` | 0 |
| `gui/src/**/*.{ts,tsx}` | 182 (133 in `reifyGrammarCorpus.test.ts`, #8316's scope) |
| `gui/src-tauri/src/**.rs` | 59 |
| **total** | **≈ 2,480** |

Today's committed ledger (#6935, landed) has 742 rows, because it covers only `crates/*/src/**.rs` and stdlib `.ri` with `.rs`/`.ri` targets. #8822 regenerates it once with `--admit-new` over the widened scope. **That regenerated ledger, not this table, is S23's binding population:** each leaf is defined by a path set, and its done-state is "no ledger row under these paths".

Outside PLINECITE's scope but in S23's findings:
- `crates/reify-audit/` (excluded by #6935 and S24 D16): 51 cite lines, plus the ten false statements of fk-a26634db4007 (nine named by the finding, plus `bin/reify-audit.rs`'s "all three detectors").
- Dangling untracked-plan pointers (`plan.json`, "plan design decision #N", "plan reuse-item #N", "plan step-N"): about 110 comment lines across `crates/` reference `plan.json`, 14 of them in reify-solver-elastic src (unchanged since the anchor), plus 14 numbered-decision pointers in reify-gcode. The repo tracks no `plan.json`.
- `esc-NNNN-N` rationale pointers in infra tests, which no dispatched role can read.
- Python docstrings: `scripts/flake-density-report.py` carries `run_all.sh:NNN` cites inside a docstring, and `CodeView` comment leaders may not see docstrings.

### 2.2 False claims still present at main

All of these were re-read at `9bdef68093`. The owning leaf is in §9; claims another stream already removes are in §8.

| finding | claim | truth at main |
|---|---|---|
| fk-eb5a9e37eb9f | `probe_realization_cache` rustdoc and inline block: INV-BUILD-1 "still `proposed`", type-level reset "future work" | docs/invariants.md has INV-BUILD-1 `enforced(type)`; `reset_per_build_state` is the exhaustive destructure. The registry row is right; only the code comments are wrong. |
| fk-e2730bcb8111 | `GEOMETRY_OP_DESCRIPTORS`: "all 49 GeometryOp variants", "unverified copy until L2"; `ParentRole`: "see `parent_handles_for_op` for the authoritative source"; `FeatureId::content_hash_bytes` `#[allow(dead_code)]` "only caller is the golden test"; `lookup_result_face` "exercised only by tests"; a `G-allow` citing done #3623 | 55 rows, 55 variants; `descriptor_for` is read by the engine; value.rs calls `content_hash_bytes` in production; manifold `kernel.rs` calls `lookup_result_face` inside a `debug_assert!` (the "#4263 not landed" half is still true) |
| fk-15e94643506d | DiagnosticCode body: `crates/reify-types/src/value.rs::SampledField`; 25 line-coordinate `Origin:` pointers (both sampled stale) | reify-types was retired in 3f3da9f03d; `SampledField` is in reify-ir |
| fk-2e1e82c1d1c6 | `hex_wedge_mesh_diagnostic`: "blocked on VolumeMesh realization (task #2947)", mirrors "task #2991" | #2947 cancelled, #2991 done; the live blocker is #4746 (pending), whose description names these diagnostics as its "already-built" emitters |
| fk-a26634db4007 | reify-audit: PTODO "all Medium severity" / "exit-neutral" (lib.rs, ptodo.rs, pdiag.rs); "currently ships three detectors"; every `Pattern` variant carries `P<N>`; Medium "reserved for metadata-cleanliness"; `AuditContext` "never owns a connection or spawns processes"; `RealGitOps` "Untested by the slice-1 integration suite"; pdoccover "stubs — filled in by the TDD steps"; and a tenth, found by the critic: `bin/reify-audit.rs` usage "all three detectors" | `ptodo::Kind::severity` maps Untracked, BareIgnore, Orphaned and GAllowOrphaned to High; 19 patterns, 11 without a digit; `ptodo::open_tasks_db` opens tasks.db; `tests/real_git_ops.rs` has 13 tests |
| fk-e90cf61da43a | reify-eval `dispatcher.rs` module doc: `ReprKind` has 4 variants, BFS bounded at 4 | 5 variants (BRep, Mesh, Sdf, Voxel, VolumeMesh) |
| fk-e4b87a3c5f90 | `error_estimator.rs` `two_tet_fan_mesh`: "Both tets have volume 1/6" | tet1 has volume 1/3 (det 2); qoi.rs's test copy documents the discrepancy instead of fixing it |
| fk-b0a7b8818be4 | elastic_static module doc: "1×1×6 hex blocks … 36 tets, 28 nodes"; `element_stress_anisotropic` "Mirrors `element_von_mises_anisotropic`" (named 9 times) | `synthetic_grid_counts` gives nx = 60 for the documented smoke case; the mirrored function was deleted in 8de74b4c3b |
| fk-420be0954017 | reify-eval lib.rs: crate-attribute comment "See `reify-types::value::SampledField`"; `cell_commit` / `detectors` "no external callers yet" | retired crate; 4 and 2 calling files |
| fk-863955f49c19 | ty.rs `//!` (305 lines): "single normative home of the quantity-slot conformance rule"; `crates/reify-types/src/value.rs` | the rule is implemented in reify-compiler `conformance/mod.rs`; `dimensioned-construction-strictness.md` §12 (its "Out of scope") names ty.rs as the home, so the PRD is not a home either |
| fk-22068b67e813 | mechanism.rs: "#2538 will reconcile"; four `plan.json` pointers | #2538 is done |
| fk-4186fb4f7289 | `test_jcodemunch_index_units.sh`: `with-jcodemunch-serve.sh:248-257` "enumerates the four sites" | `lib_jcodemunch_pin.sh` calls itself "the SINGLE definition site" |
| (found at authoring) | `scripts/affected-crates-lib.sh` cites #9056 twice | #9056 is cancelled, an orphaned cite (S18 ζ #9112 deletes the surrounding `reify_is_inert_path` block; confirm after ζ) |

### 2.3 Reviewer-answering prose, still present

These shares were re-measured with the findings' own rules:

| file | production prose share at main |
|---|---|
| reify-eval `structural_classifier.rs` | 0.79 (`classify_cell`: 181 doc lines over 23 code lines) |
| `gui/src/stores/unitLadder.ts` | 0.84 |
| reify-lsp `diagnostics/eval_guard.rs` | 0.78 |
| reify-eval `dispatcher.rs` | 0.72 |
| reify-core `ty.rs` | 0.70 (305-line `//!`) |
| reify-eval `lib.rs` | 0.675 |
| reify-eval `engine_compute.rs` | 0.657 |
| `scripts/seed-warm-lane.sh` | 0.599 (`_close_inherited_fds`: a 53-line block over a 7-line function) |
| reify-constraints `registry.rs` `solve_inner` | 261 of 446 lines (a 150-line block over a 7-line literal) |

The measurement-and-history markers the findings quote are all still present at main: "measured at 0.249s … on the live 254-worktree store", "amendment 2", "History (task #4700 → #4710)", "RE-MEASURED (task #5711 step-7)", "ORDERS OF MAGNITUDE", "the falsehood this comment carried until amendment round 2", "Measured, not assumed, by", eigensolve's task-4055 timing table, "478 tests pass", and mechanism.rs's "measured at 1.3 m on the 5-arg form".

### 2.4 What other streams already absorb (premise corrections, G6)

- **fk-91aaee6b8fde is closed.** #8537 (done, merge `7def1f6e97`) fixed `run_all.sh`, `test_harness_kloc_cap.sh` and the named verify.sh cites in `test_compute_trampoline_registration_wired.sh`. Residual in those cites: 0. The trampoline test's 17 crate-path cites were not in the finding; they go to α₅ and S18 ν #9111.
- **fk-3ac9bc99b963's structural proposal is S26 θ #9069.** It adds `validate_parameter_value`, deletes every grammar/floor helper and their prose in unitLadder.ts, and its signal includes "`5 mm` is accepted". The skeptic's unverified claim holds: `parse_value_string` → `resolve_quantity_suffix` accepts `5 mm`. Premise correction: the two ASYMMETRY headings sit in two docblocks (`quantityUnitAlphabet`'s and `acceptsBareNumber`'s), not one. S23 keeps only the display-helper residue (ζ₅).
- **fk-67ca81ff9067's 11 contradicted multi-DOF sites are all owned by S6 or #8545** (§8). S23 keeps the residual narration and cites.
- **INV-BUILD-1's registry row is correct.** No S23 false-claim correction touches a `docs/invariants.md` status cell (see §6 D10; the stale cells INV-EVAL-4/5 and INV-FEA-2 are S24 ξ #8821's, §12 OQ-6).
- **`reset_dispatch_tallies` deletion** (fk-eb5a9e37eb9f's proposal) is delivered by S1 θ #9046, and the `reset_per_build_state` rework by S1 ε #9043.
- **reify-eval lib.rs compat re-exports.** fk-420be0954017 proposes retiring them, which collides with S5 D12 ("the crate-root re-export stays reify-eval's public surface for front ends"). S23 only rewords the shim comments as public-API comments.
- **Two global.rs facts are stale.** global.rs prose share is 0.375 now (#8373), not 0.644, and its anchor cites are gone. Its one `plan.json` pointer is in the `AssemblyMode` test that elastic-static-solver-performance ξ #8375 deletes.
- **shell_result.rs** status prose and pointer are gone (#8528, done).
- **`compliance_cell_f64`'s ~65-line block** (fk-632e4850d7d0) is the INV-AD-4 declarations-only deliverable of angle-dimension-completion ι. It stays.
- **The kinematic capability matrix home.** fk-22068b67e813 proposes `docs/prds/v0_2/kinematic-constraints.md`, but that PRD is itself stale (its FD-fallback line). The live matrix is code (`JointKind`/`Joint`, S6 C1/C3) plus S6 ω₁ #8855's chunk and stdlib reference §13. S23 points there and writes no prose matrix.
- **reify-gcode's numbered decisions come from two untracked plans whose numbers collide** (task 3805's #1/#2/#8 and task 3863's #2–#7). `trajectory-input-shaping.md` §8 carries none of the four rationales. Three are already inline; the uppercase-only Klipper reason is unrecoverable from the tree, the task and the commits.
- **fk-7fbe91c20e82's code half** (named-struct return, `want_optimality` enum, branching on `ObjectiveConsumption`) lands on a function that five pending non-program tasks edit (#6903, #6677, #5416, #6681, #7964). Ruled (§12 OQ-2 (b)): δ₂ rehomes the prose only; pass 2 puts the refactor on #6903 as a details note plus `x_finding_part` fk-7fbe91c20e82.
- **The finding's "Kind::severity per pattern listed in `--help`"** is not adopted by S24 (§5.1's `Kind` has no `severity()`). S23 points the docs at `ptodo::Kind::severity`; it does not add a `--help` table (§11).

## 3. Sketch of approach

A sweep, area by area, in the order the code streams finish moving files. Each leaf owns a **path set**. It does four things there, all excluding the prose another stream's leaf owns (§8):

1. **Line cites → symbol pointers.** Every grandfathered ledger row under the path set is resolved, using the pointer forms of D2. Then, in the same diff, run `cargo run -p reify-audit --bin plinecite-baseline-gen -- --project-root . --write` (shrink-only) and commit the pruned ledger. A file the leaf moves runs `--rename` first (S24's same-diff obligation).
2. **Reviewer-answering prose → its home.** Measurements, review rounds, amendment narratives, rejected alternatives, plan-step status, task histories and test-pin lists leave the code (D3). Durable rationale goes to the PRD or docs section it belongs to, and the code keeps a one-line pointer. Everything else is quoted in the leaf's commit message under a `Rehomed:` trailer keyed by `file::symbol`, so `git log -S` still finds it.
3. **False claims → corrected or pointed.** Each claim in §2.2 is checked against the code at implementation time. It is either corrected or replaced by a pointer to the claim's single home (D10: never restate an invariant's status in code).
4. **Dangling pointers → readable ones.** `plan.json` and numbered-plan pointers, `esc-NNNN-N` pointers, retired-crate paths and cancelled-task cites are replaced by the inline one-line rationale, a PRD section, or a live `#NNNN` (D5).

Two leaves change code, both small:
- **ε₁** shrinks DiagnosticCode docs to one line per variant. One `strum::IntoEnumIterator`-driven round-trip test replaces 136 per-variant derive tests, with adoption-ratchet rows that keep it that way (D8).
- **ε₂** relocates `HexWedgeMeshOutcome` and `hex_wedge_mesh_diagnostic` from reify-core to sit beside `p2_substitution_diagnostic` in reify-eval, re-cited to #4746.

Ordering is the substance of this stream. Most target files are moved, split or partly rewritten by pending wave 1–3 leaves. A leaf that de-numbers a cite or rehomes a paragraph before then either collides with the mover or rewrites prose the mover deletes. So **every leaf depends on every other-stream leaf that moves, splits or rewrites prose in its path set** (§8, §10). The residual sweeps (θ₁–θ₄, ζ₅) depend on the close leaves of the streams that touch their crates.

## 4. Pre-conditions and substrate (G3), verified at main `9bdef68093`

- **PLINECITE exists** (#6935, done): `crates/reify-audit/src/plinecite.rs`, the 742-row ledger, the shrink-only `plinecite-baseline-gen` (`--write`, `--admit-new`), and the `plinecite:allow — <reason>` escape (`ALLOW_TOKEN`).
- **The widened scope, `--rename` and the merge gate are S24 ο #8822 (pending).** Every burn-down leaf except ε₁ depends on #8822. ε₁ does not, because diagnostics.rs is already in #6935's scope (27 rows today).
- **`strum` is already a reify-core dependency.** `ty.rs` derives `EnumDiscriminants` with `EnumIter`/`EnumCount` on `TypeDiscriminants`, and `serde_json` is already a dev-dependency.
- **The adoption-ratchet engine exists:** `tests/infra/adoption-ratchet.manifest`, `tests/infra/adoption_ratchet.py`, gated by `test_infra_adoption_ratchet.sh`. Row format: `pattern-id | ERE | path | ceiling | owner leaf`. Only code lines count, and a `///` line is a code line to the engine.
- **The rehoming destinations exist.** These were checked at authoring:
  - `docs/prds/warm-lane-pool-cow-seeding.md` §9.5
  - `docs/reify-stdlib-reference.md` §13
  - `docs/decisions/` (precedents `2522-type-as-value-conforms.md`, `883-shared-domain-types.md`)
  - `docs/prds/v0_3/kinematic-constraints-completion.md` §5
  - `docs/prds/v0_3/trajectory-input-shaping.md`
  - `docs/reify-implementation-architecture.md` §3.5
  - `docs/prds/freshness-4-variant.md`
  - each finding-cited PRD (`precision-nominal-representation-guarantee.md`, `declared-intent-consumption-accounting.md`, persistent-naming-v2)
- **The pointer targets the seats verified exist**, among them: `mesher::map_geometry_error`; `git-rerere-guard.sh::cmd_arm`; `reify_eval::cache::CacheStore::mark_pending`; `SolveResult::into_ranked_pass_through`; `RankedCandidate::unique`; `EngineSession::resolve_source`; `reify_compiler::module_dag::compile_project_with_entry_source`; `ptodo::Kind::severity`; `reify_kernel_openvdb::init::ensure_initialized`; and the `REIFY_JC_INDEXER_CMD` → `INDEXER_CMD` seam. Each leaf re-verifies its own targets at the post-move paths.

## 5. Design (bare B; G5)

G5 heuristic: the changes are comment and doc edits across many crates, one table-driven test, and one helper relocation. Behaviour is unchanged except in ε₁ (the test set) and ε₂ (the helper's crate). No overlay load-bearing seam is touched. The existing gates are the boundary tests: PLINECITE, the adoption ratchet, `cargo test`, and the PTODO fingerprint ratchet, which a removed `ptodo:allow` line or a re-cited TODO must keep green. Approach **B**.

## 6. Resolved design decisions

- **D1 — The burn-down target is S24 ο #8822's regenerated ledger, partitioned by path.** A leaf's done-state is "no `plinecite-baseline.txt` row whose path is in this leaf's path set, and `reify-audit --pattern PLINECITE` exits 0". The leaf prunes the ledger itself, in the same diff, with the shrink-only generator, and never runs `--admit-new`. The generator never adds rows, so a leaf cannot grow the ledger by mistake. A path set that PLINECITE does not scan (crates/reify-audit, docs, `.manifest`/`.txt`, Python docstrings) states its measured before/after count in the commit, by the program's counting rule.
- **D2 — Pointer forms.** Rust: `crate::module::Item` or `Type::method` (`path.rs::symbol` when the crate path is ambiguous). Shell: `script.sh::function`, `# Block X`, a `# BEGIN <marker>`, or a quoted assignment such as `` `LANE_LOCK=` ``. Python: `module.py::symbol`. PRDs and docs: path plus `§N` or the heading text. Tests: the test name.
  - A line number survives only behind `plinecite:allow — <reason>`, and only for a quoted literal a test asserts on that sits on a **comment** line. PLINECITE scans comment lines only, so a code-line literal (for example `test_gui_vitest_rpc_hardening.sh`'s assert label `'verify.sh:328'`) needs no marker.
  - Dated incident evidence is moved to the commit message, never allowed.
  - **External code** is cited by its pinned package and symbol (`jcodemunch-mcp@1.108.54 server.py::<fn>`; dark-factory `orchestrator/…/git_ops.py::<fn>`). The cite carries no line, and a `plinecite:allow` is used only where no symbol exists (OQ-5).
- **D3 — What leaves the code, and where it goes.**
  - Measurements, timings, corpus counts, review-round and amendment narratives, "do not simplify" defences, rejected alternatives, plan-step and landing status ("has not landed yet", "T10 pending"), task histories, and lists of the tests that pin a property all leave the code.
  - The order of preference for where they go:
    1. Rationale a future editor needs goes to the PRD or docs section it already cites, or that owns the subject, leaving a one-line pointer.
    2. A durable decision with no home gets a `docs/decisions/` record (ε₆) or a `docs/notes/` page.
    3. Pure history is quoted verbatim in the leaf's commit message under a `Rehomed:` trailer keyed by `file::symbol`.
  - Task records are not edited by implementing leaves.
  - A function keeps a short contract, meaning what it guarantees and what it requires. It never keeps "why we did not do X".
  - A rule restated per site (reify-expr's "3 MiB" stack budget, 17 copies) is stated once at its anchor (`MAX_RECURSION_DEPTH`), and the per-site copies are deleted.
- **D4 — The exclusion rule** (the program's binding rule).
  - (a) A leaf never rewrites, rehomes or deletes prose that another stream's leaf rewrites, deletes, moves or rehomes (§8). It depends on that leaf.
  - (b) After the owning leaf has landed, a ledger cite that survives inside owned prose may be de-numbered by a **pointer-only** edit: the `file:line` token becomes a symbol, and no other word changes. This keeps D1's done-state reachable without re-touching the owner's prose (OQ-4).
  - (c) If the owning leaf is cancelled, the prose falls back to the S23 leaf whose path set contains it, and the leaf's commit names the cancelled id.
- **D5 — Dangling pointers.**
  - **Untracked-plan pointers** (`plan.json`, "plan design decision #N", "reuse-item #N", "plan step-N") are removed. A load-bearing rationale is inlined as one line or pointed at a PRD section. A rationale that cannot be recovered is never invented: the code states the contract only (reify-gcode's uppercase-only Klipper matching).
  - **`esc-NNNN-N` pointers** become the task id `#NNNN` plus the one-line rationale.
  - **Retired-crate paths** (`crates/reify-types/…`) become the current symbol.
  - **A cancelled or done task cited as a blocker** is re-cited to the live blocker in the TODO convention's canonical form, or the sentence is deleted. A changed TODO or `ptodo:allow` line must keep the PTODO fingerprint ratchet green; the baseline row is updated in the same diff as the ratchet's rules require.
  - A mention of `plan.json` as a concept, for example in reify-audit's tests of dark-factory artefacts, is not a pointer and stays.
- **D6 — Header block indexes are dropped.** Infra test headers that list their blocks (audit 1 of 20, pool 6 of 16, gc missing H) are deleted. The in-place `# Block X` headers are the map (fk-cb27e6ba131b's proposal).
- **D7 — Filed mechanical tasks.**
  - **#8531** (fk-9e30579d2c52, OCCT lib.rs) is **adopted as δ₃**. Pass 2 adds `#8531 ← S11 ι #8684, #8822` and appends the D2 pointer form.
  - **#8542** (fk-70d435aa0170, gui engine.rs) is **adopted as ζ₃**, re-scoped. Pass 2 adds the §10 edges, because today it has none and would collide with five S25 leaves and S1 ζ.
  - **#8316** (gui reifyGrammarCorpus cites plus `reify.grammar`) is **adopted as ζ₄**, with `#8316 ← S18 π #9096, #8822`. All three adopted tasks have `dependencies: []` today, so pass 2 wires their edges before anything else.
  - **#8540** (fk-8c68164d0151) **stays as ruled**: held behind S18 ζ #9112, verify.sh half moot, sibling-script cites its own. S23 excludes them (§12 OQ-3 (a)). `lib_test_semaphore.sh`'s two `cargo-test-occt-gated.sh` cites, which #8540 does not name, are α₂'s.
  - **#8073** stays, depending on #8540.
  - **#8537** (fk-91aaee6b8fde) is done; nothing remains.
  - **#8305** owns every `.ri` file. S23 excludes `.ri` rows, including the stdlib `materials_fea.ri` regression.
  - **#7682** owns the four `test_verify_env_ambient_isolation.sh:59-85` cites.
- **D8 — DiagnosticCode lands early, with ratchet rows** (ε₁; OQ-1).
  - About fifteen pending leaves add variants: S4 λ; S6 θ/ν/φ; S8 α1/β2/δ2/ζ/η/κ; S9 μ; S15 α/ι/κ; S16 μ. Burning the enum down last would mean burning down their new prose too. So ε₁ has no stream prerequisite.
  - It adds two rows to `tests/infra/adoption-ratchet.manifest` (S19's format; D18-style), naming them in its done-criteria:
    - `diagnostic-code-origin-pointer` (`^ *///.*Origin:` in `crates/reify-core/src/diagnostics.rs`, ceiling 0)
    - `diagnostic-code-per-variant-derive-test` (the ERE matching the deleted per-variant test fn names, ceiling 0)
  - This is INV candidate 5 (extract-with-ratchet) applied to the table-driven test. It is not a prose lint of S23's own. One-line docs for later variants are then a review convention, recorded in the enum's doc header.
- **D9 — The DiagnosticCode doc contract.**
  - Each variant gets one `///` line giving its meaning and severity. `Origin:` lines go, because the emitter is `git grep DiagnosticCode::<X>`. The 9 "Minting rationale" paragraphs go; the enum header says once that adding a variant is non-breaking. The 166 PRD-mnemonic lines go to the PRDs they name.
  - **Tests:** one round-trip test iterates `DiagnosticCode::iter()` and asserts distinctness of `code_str`, serde PascalCase round-trip and severity mapping. It replaces the 136 per-variant derive tests and keeps the 15 behavioural ones.
  - **Coordination:** S12 δ #9028 adds `strum::IntoStaticStr` to the same derive line. The two leaves serialise by file lock; whichever lands second merges the derive list.
- **D10 — One home per invariant (Leo S12 R7) inside code comments.**
  - A comment never restates an invariant's status or enforcement. It cites the id and its registry (`INV-BUILD-1 (docs/invariants.md)`).
  - S23 flips no registry status cell. At main none of its false-claim corrections touches a cell.
  - The one registry prose edit (η: the closing paragraph's "`reset_dispatch_tallies()` bundling" precedent, which dangles after S1 θ) keeps `reify-audit --pattern PREGISTRY` green once #8821 lands.
- **D11 — Docs-only leaves.**
  - η edits only `docs/` and `docs/invariants.md` prose. It runs as an ordinary leaf: the merge queue's full gate is the accepted cost, and the lead may instead land it as a hook-gated direct docs commit.
  - ε₆ and γ₁ also edit docs, together with code.
- **D12 — No new detector.** Python docstrings and `.manifest`/`.txt` cites are fixed where a leaf's path set contains them, and are not linted (cross-reference to S24: CodeView docstring coverage).

## 7. Out of scope

- `scripts/verify.sh`, all of it: S18 ζ #9112 makes it a 2-line shim, and α #9095 renders `--help` from data. The ~70 `verify.sh:N` cites outside verify.sh are in scope (α₄), retargeted at `scripts/reify_verify/` symbols after ζ.
- Burning down `docs/**`, `*.md` and `tests/prd-gate/**` line anchors. They stay under the overlay's dated-anchor rule (S24 R1). That includes `check-diagnostic-truthfulness.md`'s `diagnostics.rs:~4354-4408`.
- Every `.ri` cite (#8305).
- fk-a26634db4007's `--help` severity table; fk-3ac9bc99b963's backend-grammar IPC (S26 θ #9069); fk-7fbe91c20e82's signature half (§12 OQ-2 (b): a details note plus `x_finding_part` on #6903).
- Building or changing PLINECITE, PREGISTRY or the adoption-ratchet engine (S24, S19).
- Flipping registry status cells. INV-EVAL-4, INV-EVAL-5 and INV-FEA-2 are named in S24 ξ #8821's description ("correct every unowned row (EVAL-4, EVAL-5, FEA-2, …)"), so they are #8821's (§12 OQ-6); pass 2 appends S23's measurements to #8821's details.
- The CLAUDE.md "Native deps" bullets and Pointers row (S21 α #8634 / γ #8654 make them current).

## 8. Cross-PRD relationship and exclusion table (G4)

### 8.1 Seams

| artefact | owner | S23's use |
|---|---|---|
| PLINECITE lane, ledger `crates/reify-audit/plinecite-baseline.txt`, `plinecite-baseline-gen` (shrink-only default, `--rename`) | **S24 ο #8822** (program G4) | burn-down target; every burn-down leaf ← #8822 (ε₁ excepted, D1/§4) |
| docs/invariants.md Pointer column + PREGISTRY | **S24 ξ #8821** | η's prose edit keeps PREGISTRY green; S23 flips no cell |
| docs/invariants.md status cells touched by the false-claim corrections | **S23** (program G4) | measured: none touched at main (§2.4, D10); INV-EVAL-4/5 and INV-FEA-2 are S24 ξ #8821's (§12 OQ-6) |
| `tests/infra/adoption-ratchet.manifest` row format | **S19** | ε₁ adds two rows in today's format |
| `crates/reify-audit/ptodo-baseline.txt` fingerprint format | **S24** | ε₂, β₂ edit a TODO / `ptodo:allow` line; baseline updated in the same diff |
| `docs/decisions/` record for the quantity-slot rule | **S23 ε₆** | new home; conformance/mod.rs (after S9 ο #8745) points at it |

### 8.2 Exclusion table (binding; built from the program's S23 cross-reference lines, the FILED table and the six measurement seats)

Each row: file (current path; it may move) | prose S23 does **not** re-touch | owning leaf. The S23 leaf whose path set contains the file depends on the owner (§10).

**Infra and scripts**

| file | excluded prose | owner |
|---|---|---|
| `scripts/verify.sh` | everything | S18 ζ #9112, α #9095 |
| `tests/infra/plan_capture_lib.sh` | every verify.sh line cite | S18 θ #9105 |
| `tests/infra/test_nan_safe_ordering_guard_wired.sh` | hD–hH prose incl. the elastic_static / degenerate_shell / shell_assembly cites | S18 θ2 #9106 |
| `tests/infra/test_compute_trampoline_registration_wired.sh` | Part B cites (moved to `test_verify_gui_feature_test_pass.sh`) | S18 ν #9111 |
| `tests/infra/test_verify_semaphore_e2e.sh` | tree-sitter-generate.sh line cites | S18 λ #9109 |
| `scripts/verify-pipeline-guard.sh` | measurement prose; clauses 3/4a and the extractor | S18 κ #9100; ζ #9112 |
| `scripts/affected-crates-lib.sh` | `reify_is_inert_path` and its prose | S18 ζ #9112 |
| `tests/infra/test_slot_timeout_marker.sh` | Sections D–G (39 cite lines) | S19 η #8694 |
| `tests/infra/test_harness_kloc_cap.sh` and the 35 `crates/**` headers citing it | C1/C2 contract prose and pointers (2 of the 35 carry cites: reify-compiler `registry_seed_result_types.rs`, reify-eval `units_length_closure_guard.rs`; θ₁, θ₂ ← #8700) | S19 θ #8700 |
| `scripts/warm-lane-gc.sh` header (Always-reclaim note) | `seed-warm-lane.sh:1243/:1525/:136/:323/:1139` | #8540, #8073 |
| `scripts/warm-lane-gc-sweep.sh` | `warm-lane-disk-guard.sh:130-138` | #8540 |
| `scripts/with-jcodemunch-serve.sh` | `run-gui-dev.sh:165-173` | #8540 |
| `scripts/lib_test_semaphore.sh` | `verify.sh:161-165` | #8540 (its two `cargo-test-occt-gated.sh` cites are α₂'s, §12 OQ-3) |
| gc and gc-sweep `_usage` heredocs | Greek-letter aliases | S20 L7 #8541 (S23 owns the comment aliases, per S20 §6/§7) |
| `scripts/seed-warm-lane.sh` | LANE-LOCK RELEASE CONTRACT block and FD-9 lock prose; the three "must stay a LITERAL" notes | S20 L1 #8624; L8 #8670 |
| `tests/infra/test_seed_warm_lane.sh` (→ μ₂ satellites) | lane-lock / FD-9 prose; the `Q11_FLAG` note | S20 L1 #8624; L8 #8670 |
| `tests/infra/test_warm_lane_audit.sh` | producer byte-layout comments | S20 L5 #8709 |
| `scripts/warm-lane-gc.sh` | "stale gc-consumer citations" | S20 L3 #8641 |
| `test_run_all_ambient_isolation.sh`, `test_content_skip*`, `test_admission_knob_parity*`, `test_verify_release_delta_skip.sh` | `test_verify_env_ambient_isolation.sh:59-85` cites | #7682 |

**Analysis and solver crates**

| file | excluded prose | owner |
|---|---|---|
| reify-eval `engine_compute.rs` | "per the NOTE at line 407" and the mirror NOTE | S5 ε #8994 |
| reify-eval `engine_compute.rs` | `plan.json` re-export comment | S5 γ #8991 |
| reify-solver-elastic `assembly/global.rs` | `plan.json` in the `AssemblyMode` test | elastic-static-solver-performance ξ #8375 |
| reify-solver-elastic `form_find.rs`, `lib.rs` | `AnisoFormFindError` "separate enum" rationale (both copies) | S14 η #8628 |
| reify-solver-elastic `shell_assembly.rs` | KAPPA relocation (S23 edits only the surviving doc line) | S14 γ #8618 |
| reify-solver-elastic `mpc.rs` | T10/T11 status prose; struct-literal docs | S14 θ #8629 |
| reify-solver-elastic `qoi.rs` | release-behaviour, ~100 MB and TDD-step prose; the test move | S14 μ #8640 |
| reify-solver-elastic `volume_refine.rs` | `map_geometry_error` doc incl. `mesher.rs:535-544` | S10 β₁ #8935 |
| reify-solver-elastic `shell_result.rs` | header status and ShellStress pointer | #8528 (done) |
| reify-stdlib `analysis.rs` | `REGISTRY_FREE_TYPE_ID` const docs | S15 δ #8686 |
| reify-stdlib `fea.rs` | `grids_equal` doc (dead reify-types cite) | S15 ξ #8717 |
| reify-stdlib `dynamics/eval.rs` | `snapshot_for_sample` binding comment | S6 σ #8847 / λ #8839 |
| reify-stdlib `dynamics/eval.rs` | positions/compliance guard comment | S6 ρ #8845 |
| reify-stdlib `dynamics/eval.rs` | `compliance_cell_f64` INV-AD-4 declaration | angle-dimension-completion ι (kept) |
| reify-stdlib trampolines (modal, dynamics, trajectory) | "other half lives in reify-eval" docs | S5 λ #9002, μ #9003 |
| reify-mesh-morph `lib.rs` | "Task #5007 … no MorphOutcome bucket" section; `eligible`/`morph` docs | S16 α #9085; #8527 |
| reify-eval `compute_targets/elastic_static.rs` | PERF NOTE blocks | S16 λ #9118 |
| reify-eval `compute_targets/elastic_static.rs` | `solve_cantilever_fea` "10 args" allow | S16 ι #9094 |
| reify-eval `compute_targets/elastic_static.rs` | module-doc Determinism contract | elastic-static-solver-performance β #8374 / ξ #8375 |
| reify-eval `compute_targets/elastic_static.rs` | `DEFAULT_*` / `extract_shell_route_params` default cites | S15 β #8679 |
| reify-eval `compute_targets/elastic_static.rs` | "not available in this build" diagnostic | S10 β₁ #8935 |
| reify-eval `compute_targets/shell_gui_mesh_data` | its G-allow | S5 ζ #8996 |
| reify-eval `modal_ops.rs` | module doc | S16 ν #9091 |
| reify-eval `modal_ops.rs` | ModalResult "(6 fields" docstrings | S15 ζ #8693 |
| reify-analysis trajectory ops | `trajectory_ops` / `worst_case_residual_fraction` docs | S16 δ #9087 |
| reify-analysis | `options.rs` morph pointers | #8527 |
| `docs/prds/v0_3/compute-node-contract.md` §2/§4 | all | S5 ρ #9009 |

**Kinematics**

| file | excluded prose | owner |
|---|---|---|
| reify-stdlib `joints.rs` | `eval_joints` "zero column placeholder" (#2670) and the `JOINT_KINDS` doc | S6 ι #8836 (const deleted by ψ₁ #8852) |
| reify-stdlib `joints.rs` | `per_joint_jacobian_local` justifications (cylindrical, planar, spherical arms) | S6 ζ #8833 |
| reify-stdlib `joints.rs` | `DRIVING_JOINT_KINDS` "future task" doc | S6 ψ₁ #8852 |
| `loop_closure_solver.rs` | `LoopClosureReport`, `SINGLE_LOOP_RESIDUAL_COUNT`, `solve_loop_closure_with_diagnostics` docs | S6 ξ #8842 |
| `loop_closure_solver.rs` | module-doc `per_joint_jacobian_local` sentence | S6 ζ #8833 |
| `loop_closure_solver.rs` | `mechanism_loop_closure_chains` tests | S6 κ #8837 |
| `snapshot.rs` | `value_for`, `wrap_midpoint_for_joint`, `wrap_jointvalue_for_joint` docs; the unbound-planar test doc | S6 μ #8840 |
| `snapshot.rs` | warm-start guard comment | #8545, then S6 π #8844 |
| `snapshot.rs` | centre-of-mass group (incl. its two cites) | S6 σ₀ #8846; S8 α2 #8889 |
| `snapshot.rs` | bind group | S6 λ #8839 |
| `loop_closure.rs` | `extract_loop_closure_chains` docs; `strip_world_sentinel` pointer | S6 κ #8837 |
| `loop_closure.rs` | `value_for_joint` "widened from", `jointvalue_from_bound_value`, module-doc #2670 paragraph | S6 μ #8840 |
| `loop_closure.rs` | `per_joint_jacobian_local` bullet and tests | S6 ζ #8833 |
| `loop_closure.rs` | `LoopClosureSolverInputs` tuple defence | S6 ν #8841 |
| `mechanism.rs` | `append_body` closing-branch path prose and the two path-shape tests | S6 κ #8837 |
| `mechanism.rs` | duplicate-solid / world-parented error construction | S6 θ #8835 |
| `docs/reify-stdlib-reference.md` §13, `crates/reify-mcp/src/tools/chunks/kinematics.md` | the per-kind matrix rewrite | S6 ω₁ #8855 |

**Constraints, geometry, kernels**

| file | excluded prose | owner |
|---|---|---|
| reify-eval `dispatcher.rs` | `long_chain_threshold_from_env` doc and its two cites | S1 α #9036 |
| reify-eval `warm_pool.rs` | `from_env_or_default` rationale | S1 α #9036 |
| reify-eval `warm_pool.rs` | `PendingWarmSeedsGuard` mention | S4 δ #8862 |
| reify-eval `warm_pool.rs` | `retrieve` / `donate_preserving_lru` docs | S13 η #8753 (done) |
| reify-kernel-manifold `kernel.rs` | Send+Sync test and macro comments | S10 δ₂ #8960 |
| reify-kernel-manifold `kernel.rs` | `manifold_from_reify_mesh` MODE-cache rationale | S10 ζ₂ #8971 |
| reify-eval `structural_classifier.rs` | `content_hash` formula restatement | S3 η #8677 |
| reify-eval `engine_constraints.rs` | `:842-854` cite and GD&T P3 prose | S12 η #9026 |
| reify-eval `engine_constraints.rs` | DFM P1 prose | S12 θ #9027 |
| reify-eval `engine_constraints.rs` | supersession P2 and file-wide prose share | S12 ν #9031 |
| reify-eval `tolerance_combine.rs` | `conforms_to_trait` / `conforms_to_output` docs | S12 μ #9018 |
| reify-kernel-gmsh `kernel_real.rs` | `GeometryKernel` impl prose | meshing-service ε #8288 |
| reify-kernel-gmsh `init.rs` | tet-readback and session-preamble prose | #7636; meshing-service β #8285 |
| reify-kernel-occt `lib.rs` | prose for code S11 deletes (warm start, flat history accessors, the 21 "flat groups of" comments) | S11 ε #8682, δ #8676 |
| reify-constraints `solver.rs` | n/a: moved, not rewritten (S23 edits at the `solver/` paths) | S14 ξ #8653, ο #8656, π #8658 (prerequisites) |

**Language pipeline**

| file | excluded prose | owner |
|---|---|---|
| reify-core `diagnostics.rs` | the `detect_scope_coupling` test comment, if ε₁'s test consolidation keeps that test (the `ScopeCoupling` `/// Origin:` line is ε₁'s: every Origin line is; pass 2 notes #8868 and adds diagnostics.rs to its files) | S4 κ #8868 |
| reify-core `diagnostics.rs` | `Diagnostic` struct doctest block | S12 β #9024 |
| reify-eval `engine_build.rs` | `reset_per_build_state` doc and destructure | S1 ε #9043 |
| reify-eval `engine_build.rs` | `reset_dispatch_tallies` and its doc | S1 θ #9046 |
| reify-eval `engine_build.rs` / `realization_content.rs` | prose that moves with, or is rewritten with, the probe / executor / volume-mesh / seeding / demand code | S2 δ, β₁, β₂, γ, ι₁–ι₃ (ids at S2's filing; S2 §5.10 "comments that state the old shape go with their code") |
| reify-ir `geometry.rs` | `reify_ir::geometry::tests::…` comment cites | S10 α₂ #8931 |
| reify-ir `geometry.rs` | GeometryQuery reply-shape docs | S10 η₁ #8974, η₆ #8979 |
| reify-eval `engine_eval.rs` | the five predicate mirrors and their "no dependency" prose | S14 ν #8645 |
| reify-eval `engine_eval.rs` | `Engine::cell_eval_ctx` doc and the coexistence paragraph | S4 τ #8872 |
| reify-eval `engine_eval.rs` | provenance prose | S4 ε #8863, γ #8861, ζ #8867 |
| reify-eval `engine_eval.rs` | the `detect_scope_coupling` mentions (moved to `detectors.rs`) | S4 κ #8868 |
| reify-eval `cell_eval_ctx.rs` | all prose | S4 η #8864, θ #8866, τ #8872 |
| reify-eval `edit_param` | provenance prose | S4 ε #8863 |
| reify-eval `lib.rs` | Engine field docs, incl. `morph_source`, the `realization_cache.rs` / `types.rs` cites, `tolerance_wiring` | S1 λ #9050 |
| reify-eval `lib.rs` | `eval_ctx_with_meta` doc | S4 τ #8872 |
| reify-expr `lib.rs` | the new modules' purpose sentences | S8 τ #8911 |
| reify-expr `dual_eval.rs` | the 21 `lib.rs:NNN` pointers | S8 ε #8896 |
| reify-compiler `compute_targets/multi_case.rs` | "mirrors" prose | S8 λ #8909 |
| reify-builtins rows / `builtin_arg_slots` | per-builtin rationale (→ row `basis` column) | S8 #6944 |
| reify-stdlib `geometry::diagnose` | ruling/escalation prose | S8 β1 #8890 |
| reify-compiler `expr.rs` | ladder precedence comment | S8 #6014 |
| reify-compiler `entity.rs` `compile_entity` | "Two-pass" doc | S9 β #8731 |
| reify-compiler forall-connect | module doc and Info strings | S9 ε #8734 |
| reify-compiler conformance | module doc → pointer | S9 η #8736 |
| reify-compiler `PreRegisterOutput` | set-to-guard prose table | S9 θ #8738 |
| reify-compiler `conformance_accepts` | divergence prose | S9 ι #8739 |
| reify-compiler `lib.rs` / `name_env.rs` | install-site lists and "why a thread-local" essays | S7 μ #8643, ν #8650 |

**reify-audit, front ends, GUI, docs**

| file | excluded prose | owner |
|---|---|---|
| reify-audit `pdiag.rs` | the "38" doc count | S24 ζ #8812 |
| reify-lsp `analysis.rs` | `AnalysisContext` Send note "(server.rs:268-284)" | S27 ι #8774 |
| reify-lsp `analysis.rs` | `from_parsed` body prose incl. "outside this task's file scope" | S27 γ #8778; S1 θ #9046 |
| reify-lsp `analysis.rs` | `set_capture_undef_causes` call and field doc | S1 ζ #9044 |
| reify-lsp `diagnostics/eval_guard.rs` | predicate derivation prose and the module rationale (incl. "outside this task's file scope") | S27 β #8761 |
| reify-lsp `diagnostics.rs` | `ALL_PRODUCTION_COMPUTE_TARGETS` test list and doc | S5 (ε/ζ/ι row) |
| reify-lsp `diagnostics.rs` | `violated_messages` prose | S12 γ #9025 |
| gui `engine.rs` | `from_engine` set-capture comment | S1 ζ #9044 / γ₁ #9038 |
| gui `engine.rs` | `load_file` "engine.rs:30-44" | S25 ζ #8942 |
| gui `engine.rs` | `build_gui_state` surface-mapping comment | S25 η #8943 (recheck after) |
| gui `engine.rs` | `emit_auto_resolve_if_any` doc block | S25 λ #8947 |
| `gui/src/stores/unitLadder.ts` | all grammar/floor prose | S26 θ #9069 |
| `gui/src/panels/PropertyEditor.tsx` | "5 mm" lenience comment | S26 θ #9069 |
| `gui/src/__tests__/reifyGrammarCorpus.test.ts` | `examples/*.ri:N` corpus cites (ζ₄ is #8316 itself) | #8316 |
| `CLAUDE.md` | "Native deps" bullets and the Pointers row | S21 α #8634, γ #8654 |
| `docs/prds/v0_3/engine-integration-norm.md` §3.6 | retirement marker | S3 ζ #8699 |

## 9. Decomposition plan

Labels only; task ids at decompose. Every leaf:
- carries `metadata.x_finding_run = "review-all-reify-20261010"` and `metadata.source = "review-all"`;
- cites INV-DOC-1 where it burns ledger rows;
- applies D1–D5 within its path set, excluding §8;
- runs `plinecite-baseline-gen --write` (shrink-only) in its final diff.

A leaf that moves a file under PLINECITE scope runs `--rename` in the same diff. A leaf that edits a `tests/infra` member keeps its `run-all-classification.manifest` row. **Priority:** a leaf takes the highest severity among its keys (medium or low); residual sweeps are low. No leaf is high, so none feeds gate #8612.

### α — infra and scripts

- **α₁ — jcodemunch scripts and tests: symbol cites and the external-package pointer form** (low). Keys: **fk-4186fb4f7289**.
  - Paths: `scripts/jcodemunch-index-reify.sh`, `scripts/lib_jcodemunch_pin.sh`, `tests/infra/test_jcodemunch_index_reify.sh`, `tests/infra/test_jcodemunch_index_units.sh`, and `scripts/with-jcodemunch-serve.sh`'s three external cites.
  - Rewrite `jcodemunch-index-reify.sh:496-503` to the `REIFY_JC_INDEXER_CMD`/`INDEXER_CMD` seam.
  - Correct "enumerates the four sites" to point at `lib_jcodemunch_pin.sh` as the single definition site.
  - Apply D2's external form to the pinned-package cites; rehome the 4 esc pointers.
  - **Signal:** no ledger row under those paths, and the units test's "four sites" sentence is gone. Prereqs: #8822; #8540 (its files include `with-jcodemunch-serve.sh`).
- **α₂ — Seed and gc script prose: rehome measurements and amendments; symbol cites; Greek-letter comment aliases** (medium). Keys: **fk-5f3dcd2ae885**.
  - Paths: `scripts/seed-warm-lane.sh`, `scripts/warm-lane-gc.sh`, `scripts/warm-lane-gc-sweep.sh`, `scripts/lib_test_semaphore.sh` (its two `cargo-test-occt-gated.sh` cites only; §12 OQ-3), `scripts/git-rerere-guard.sh` (target only).
  - In seed-warm-lane.sh:
    - move the measurements and amendment narratives to the commit;
    - keep §9.5 of `warm-lane-pool-cow-seeding.md` as the rationale home;
    - shrink the `_close_inherited_fds` block to its contract;
    - rewrite `warm-lane-gc.sh:648-649` → `_reseed_lane_pinned`'s pipe consumer, and `git-rerere-guard.sh:735-758` → `cmd_arm`.
  - In gc and sweep: the 38 Greek-letter comment lines, the dark-factory `git_ops.py` cites (D2 external form), and the reviewer prose.
  - **Signal:** no ledger row under the four scripts outside #8540's excluded lines; the single-line token `0.249s` (seed-warm-lane.sh, count 1 at `9bdef68093`) is gone. `git grep -cE '^\s*#.*[α-ω]' scripts/warm-lane-gc*.sh` outside `_usage` is 0. Seed comment share is recorded in the commit.
  - Prereqs: S20 L1 #8624, L2 #8635, L3 #8641, L6 #8668, L7 #8541, L8 #8670; S18 ε #9101 (adds the `--hold` arm to lib_test_semaphore.sh); #8540; #8073; #8822.
- **α₃ — Warm-lane infra tests: symbol and Block cites, header indexes dropped, esc pointers rehomed** (medium). Keys: **fk-465b5eec51e2**, and the header-index part of fk-cb27e6ba131b (`x_finding_part`).
  - Paths: the `test_seed_warm_lane*` satellites (after μ₂), `test_warm_lane_pool*`, `test_refresh_warm_base.sh`, `test_warm_lane_audit.sh`, `test_warm_lane_gc*.sh`, and the two `seed-warm-lane.sh:N` cites in `test_git_rerere_guard.sh` (so no `seed-warm-lane.sh:N` cite is left anywhere under `tests/infra`).
  - Every `seed-warm-lane.sh:N`, `warm-lane-gc.sh:N`, `refresh-warm-base.sh:N` and self-cite becomes a symbol, assignment or Block id. The `git-rerere-guard.sh` cites become symbols. Header block indexes go (D6). esc pointers go (D5), including the assert label in the audit test.
  - **Signal:** no ledger row under those paths; `git grep -E 'seed-warm-lane\.sh:[0-9]' -- tests/infra` is empty; no `# Blocks:`-style header index remains.
  - Prereqs: S19 μ₂ #8719, μ #8716, ξ #8723, ν #8721; S20 L1 #8624, L2 #8635, L4 #8647, L5 #8709, L6 #8668, L8 #8670; #8822.
- **α₄ — Infra tests that cite verify.sh: retarget at `scripts/reify_verify/` symbols** (low). Keys: **fk-cb27e6ba131b**.
  - Paths: `test_plan_capture_lib.sh`, `test_nextest_absent_lib.sh`, `nextest_absent_lib.sh`, `test_verify_semaphore_wiring.sh`, the `test_tree_sitter_pipeline*` satellites, the `test_verify_scope*` partitions, the `test_verify_semaphore_e2e*` partitions, and every other `tests/infra` and `scripts` file whose remaining cites point into verify.sh. That includes `verify-pipeline-infra-tests.txt`, `release-scope-lib.sh`, `lib_portable.sh`, `run-all-ambient-vars.manifest` and `test_reify_audit_ptodo.sh`'s `verify.sh:1049`.
  - Each `verify.sh:N` becomes the planner or executor symbol in `scripts/reify_verify/` (S18 PRD §4 D1 module table), or a `--print-gates` step name. The `:NNN` self-cites become test or block names.
  - **Signal:** comment lines citing verify.sh are gone: `git grep -cE '^\s*#.*verify\.sh:[0-9]' -- tests/infra scripts` is 0 (68 at `9bdef68093`). Code-line string literals such as `test_gui_vitest_rpc_hardening.sh`'s assert label `'verify.sh:328'` stay as they are and need no `plinecite:allow`, because PLINECITE scans comment lines only. No ledger row remains under α₄'s paths.
  - Prereqs: S18 ζ #9112, θ #9105, η2 #9104, λ #9109, ξ #9113, ο #9114; S19 ο #8725; #7682; #8822. This leaf waits on the S18 cutover cascade.
- **α₅ — Infra and scripts long tail** (low). Keys: none (INV-DOC-1 burn-down).
  - Paths: every `tests/infra/**` and `scripts/**` path with a ledger row that α₁–α₄ do not own. That includes:
    - Part A of `test_compute_trampoline_registration_wired.sh` and `check-compute-trampoline-registration.sh` (cites into files S5 κ₁ moves)
    - `test_slot_timeout_marker.sh`'s residue
    - `test_reify_audit_ptodo.sh` (non-verify cites)
    - `reify-audit-freshness.sh`
    - `flake-density-report.py`, including its docstring cites (D12)
    - `test_run_all_pool_lock_host_global.sh`, `test_cpu_load_governance.sh`, `test_sandbox_cache_writability_seam.sh` (dark-factory `roles.py`, D2 external form)
    - `lib_git_env_scrub.sh`
    - `affected-crates-lib.sh` (the cancelled #9056 cite, if it survives ζ)
  - **Signal:** no ledger row under `tests/infra/` or `scripts/` beyond α₁–α₄'s paths and #8540/#7682's excluded lines.
  - Prereqs: S1 γ₂ #9039; S5 κ₁ #8999; S18 ν #9111, ζ #9112; S19 η #8694, λ #8712, δ #8689, κ #8707; S19 Ω #8729 (σ #8728 partitions `test_with_jcodemunch_serve.sh`, π #8726 `test_test_helpers.sh`, ρ #8727 `test_jobserver_balancer.sh`, γ #8685 `lib_slot_acquire.sh`); S24 ε #8811, ι #8815, μ #8818; #8822; α₁–α₄ (path-set disjointness).

### β — analysis and solver crates

- **β₁ — reify-solver-elastic src and tests: plan.json pointers, symbol cites, reviewer prose, the `two_tet_fan_mesh` volume fix** (medium). Keys: **fk-25c46b3f4c72**, **fk-e4b87a3c5f90**.
  - Paths: `crates/reify-solver-elastic/{src,tests}/**`.
  - Remove the plan.json pointers: eigensolve ×2, buckling_kernel ×3, lib ×2, form_find, form_find_free, prestress_stability, result, shell_assembly's KAPPA line. Inline the load-bearing rationale or point at the PRD.
  - Move eigensolve's task-4055 timing table and "478 tests pass" to the commit.
  - Fix `error_estimator.rs`'s "Both tets have volume 1/6" (tet1 is 1/3), and then the now-stale discrepancy clause in `tests/qoi.rs`.
  - Rewrite the remaining cites: constitutive → `trait ElasticMaterial`'s Poisson constraint in `materials_fea.ri`; interpolation → `element_stiffness_generic_with_d_global`; volume_refine "~line 199" → `refine_with_size_field_validated`; result, joint_stiffness, shell_benchmarks → `shell_element_stiffness_degenerate_ans`; plus the dirichlet test names, etc.
  - **Signal:** `git grep -c 'plan\.json' -- crates/reify-solver-elastic` is 0; "Both tets have volume 1/6" is absent; no ledger row under the crate.
  - Prereqs: S14 α #8614, β #8616, γ #8618, η #8628, θ #8629, ι #8630, λ #8637, μ #8640, ρ #8660 (close; covers δ #8619, ε #8621, ζ #8626); S16 ο #9121; S10 β₁ #8935; elastic-static-solver-performance #8375, #8380, #8381; #8822.
- **β₂ — Eval and analysis trampolines: elastic_static and modal_ops drift, engine_compute residue** (low). Keys: **fk-b0a7b8818be4**, and fk-25c46b3f4c72's engine_compute part (`x_finding_part`).
  - Paths: the post-move homes of `engine_compute.rs`, `compute_targets/elastic_static.rs` and `modal_ops.rs` (reify-analysis families after S5 μ/ν, directories after S16 θ/ν).
  - Correct the module doc's mesh claim against `synthetic_grid_counts`, and its sibling in modal_ops (the `/// … nz=6 → 28 nodes` test doc).
  - Rewrite the 9 `element_von_mises_anisotropic` mentions to the surviving function or delete them.
  - The `ptodo:allow — permanent design note` TODO: its premise is stale (`ComputeFn`/`ComputeOutcome` live in reify-compute-contract). Delete it or re-cite a live task (D5), keeping the PTODO ratchet green.
  - engine_compute's "a claim this comment made until task 5311" and "An earlier draft of this list" go to the commit. Re-point modal_ops's G-allow `mod.rs:140` cites. Remove the residual review and suggestion prose.
  - **Signal:** the single-line tokens `6 tets per hex = 36 tets` (1 at `9bdef68093`), `nz=6 → 28 nodes` (1) and `element_von_mises_anisotropic` (9) are absent under `crates/`; no ledger row under the moved paths.
  - Prereqs: S5 γ #8991, ε #8994, μ #9003, ν #9004; S4 γ #8861; S1 κ₁ #9048; S16 θ #9093, ι #9094, λ #9118, ν #9091, ξ #9120; S15 ζ #8693, β #8679; #8374; #8375; #8822.
- **β₃ — stdlib FEA and dynamics, shell-extract medial, mesh-morph module doc** (low). Keys: **fk-632e4850d7d0**.
  - Paths: the `fea/*` partitions (after S15 ξ), the `dynamics/*` partitions (after S15 ο), `crates/reify-shell-extract/src/{medial,mid_surface,partition}.rs`, `crates/reify-stdlib/src/sweep.rs`, `crates/reify-mesh-morph/src/{lib,quality}.rs`.
  - Move `min_max_stress`'s two-call trade-off and `envelope_tensor_projection`'s two-pass defence to the FEA PRD section they cite, leaving a pointer.
  - medial.rs "~:617" → `compute_medial_mask`'s band filter; "line 470/475" → symbols. Move `MedialOptions`' derivation restatements to their PRD pointer.
  - The mesh-morph module doc's PRD-task-history layout becomes a contract.
  - Remove the shell-extract and sweep plan.json pointers. Keep `compliance_cell_f64`'s INV-AD-4 block (§2.4).
  - **Signal:** the bare tokens `~:617` and `line 47[05]` are absent from medial.rs (4 at `9bdef68093`; PLINECITE cannot see bare cites); no ledger row under those paths; `plan\.json` is absent from them.
  - Prereqs: S15 ξ #8717, ο #8718, μ #8711, ν #8714, δ #8686; S6 ρ #8845, σ #8847, λ #8839, μ #8840; S8 ω #8914 (β1 #8890 and δ2 #8895 edit stdlib `geometry.rs` / `sweep.rs`); S16 α #9085; #8527; #8822.

### γ — kinematics and gcode

- **γ₁ — Kinematics residue: loop-closure, solver, snapshot, joints and mechanism prose** (medium). Keys: **fk-67ca81ff9067**, **fk-22068b67e813**.
  - Paths: the post-ψ₁/ψ₂ files of `joints`, `loop_closure`, `loop_closure_solver` (+ `newton.rs`), `snapshot` (+ `snapshot_queries.rs`), and `mechanism.rs`.
  - What S6 leaves: the "γ widening (PRD KCC-γ)" and "KCC-γ step-12" / "task 2678 step-8" narration on surviving functions; `loop_closure.rs`'s module-doc "future task 2585" and the five `reify_constraints::loop_closure` pointers (claimed by S6 §9 but named by no S6 leaf; see the cross-reference); the 7186 narration on `loop_residual_jacobian_by_joint`; surviving stale cites (joints `orientation.rs`/`geometry.rs`/`joints.rs:67-76`, loop_closure `joints.rs:785`/`loop_closure.rs:87`, snapshot test `snapshot.rs:726`).
  - In mechanism.rs:
    - remove the 4 plan.json pointers, the 1.3 m measurement and the rejected-alternatives text;
    - rehome the residual-conjugation argument into `docs/reify-stdlib-reference.md` §13 next to the rigid-tie argument already there, after ω₁ rewrites §13;
    - correct "#2538 will reconcile" to the current behaviour;
    - rewrite the stale self-cites ("module-level doc lines 13-16", "lines 435-446").
  - Per-kind capability pointers target `JointKind` and S6's chunk (§2.4).
  - **Signal:** `plan\.json`, `future task 2585` and `reify_constraints::loop_closure` are absent from γ₁'s own files (joints, loop_closure, loop_closure_solver, snapshot, mechanism and their split children; 11 at `9bdef68093`; sweep.rs's `plan.json` is β₃'s); `KCC-γ` narration is gone from surviving functions; no ledger row under those paths.
  - Prereqs: S6 ψ₁ #8852, ψ₂ #8853, κ #8837, θ #8835, ω₁ #8855; #8545; #8822.
- **γ₂ — reify-gcode: drop the colliding plan-decision numbers; inline contracts** (low). Keys: **fk-679840f73698**.
  - Paths: `crates/reify-gcode/src/{ast,klipper,marlin}.rs`, `tests/parse_klipper.rs`.
  - Remove every "plan design decision #N", "plan reuse-item #N", "design decision #N", "plan step-N", the wrapped "See plan design / decision #7" (klipper.rs) and "decision #6 in plan analysis" (tests/parse_klipper.rs).
  - Keep or inline the one-line rationale where it is recoverable: the '=' sentinel and first-'=' split are inline; partial-success is out of scope at v0.3; the raw-string KV params reason comes from commit `fef266e3c4` ("order matters for round-trip Display; raw strings keep the parser dialect-agnostic").
  - Uppercase-only Klipper matching is stated as a contract with no invented reason.
  - **Signal:** `git grep -E 'plan design decision|plan reuse-item|design decision #|plan step-|decision #[0-9]+ in plan|plan design$' -- crates/reify-gcode` is empty (17 at `9bdef68093`).
  - Prereqs: none (no stream touches reify-gcode). It may dispatch any time after the batch is committed.

### δ — constraints, geometry, kernels

- **δ₁ — reify-constraints solver tree and cpsat: history and re-measure prose rehomed, symbol cites** (medium). Keys: **fk-e90cf61da43a**.
  - Paths: `crates/reify-constraints/src/{solver/**,trial.rs,cpsat.rs}`.
  - Move "History (task #4700 → #4710)" and "RE-MEASURED (task #5711 step-7)" to the commit, keeping the rule as a contract.
  - Rewrite the cpsat cites: `constraint.rs:349` → `SolveResult::into_ranked_pass_through`; `constraint.rs:564` → the default `ConstraintSolver::solve_ranked`; `ranked.rs:83-85` → `RankedCandidate::unique`.
  - Rewrite `registry.rs:453/:789` → `SolverRegistry::solve_inner`'s sub-problem spread.
  - **Signal:** no ledger row under those paths; the two history markers are absent.
  - Prereqs: S14 ν #8645, ξ #8653, ο #8656, π #8658; #8822.
- **δ₂ — `SolverRegistry::solve_inner`: rehome the 150-line block and the review/history prose (prose only)** (medium). Keys: **fk-7fbe91c20e82** (the prose half; the refactor half is `x_finding_part` on #6903, §12 OQ-2).
  - Paths: `crates/reify-constraints/src/registry.rs`.
  - The block over the `ResolutionProblem` literal: the "ORDERS OF MAGNITUDE" measurement and the "When task #5720 landed" / "LAYER 2" history go to the commit. The drift-guard rule stays as one line plus a pointer to its PRD. The rest of `solve_inner`'s review-round, step and amend references go the same way.
  - No code change. The named outcome struct, the solve-mode enum and the `ObjectiveConsumption` branching are #6903's (pass 2 details note).
  - **Signal:** the single-line marker `ORDERS OF MAGNITUDE` is absent from `registry.rs` (1 at `9bdef68093`); `solve_inner`'s comment share is recorded in the commit (261/446 before).
  - Prereqs: S14 ν #8645, ξ #8653; #6677; #8822.
- **δ₃ = #8531 (adopted) — reify-kernel-occt lib.rs: symbol cites** (low). Keys: **fk-9e30579d2c52** (already stamped).
  - Pass 2 adds `#8531 ← S11 ι #8684, #8822` and a note: the cites move with S11's split (`extract_length_f64` → `op_args.rs` in γ1 #8652; the execute arms → `execute.rs`/`history.rs` in ι), so pointers use bare symbols (`draft_faces`, `required_length_value(s)`, the `GeometryOp::Fillet` / `Draft` / `InterpCurve` arms), never `lib.rs`.
  - **Signal:** no ledger row for `crates/reify-kernel-occt/src/lib.rs` and the files S11 splits it into (8 rows for lib.rs at `9bdef68093`). The crate's `register.rs` / `tessellate_options.rs` rows are θ₄'s.
- **δ₄ — Kernel crates: manifold, gmsh and openvdb cites into OCCT and sibling crates** (low). Keys: none (`x_finding_part` fk-e90cf61da43a).
  - Paths: `crates/reify-kernel-manifold/src/kernel.rs`, `crates/reify-kernel-gmsh/src/{init,kernel_real}.rs`, `crates/reify-kernel-openvdb/src/kernel_real.rs`.
  - The OCCT `lib.rs:456-466/460-461/516-523/677-710` cites → `ShapeStore` symbols after S11 ε. The others → their symbols: `mocks.rs:889` → `FailingMockGeometryKernel`; `kernel_registry.rs:329-353`; "PRD line 70" → the persistent-naming-v2 section heading; gmsh "line ~120" → `GmshKernel::mesh_to_volume`'s leading `ffi::clear()`; openvdb `geometry.rs:121-128` → `GeometryHandle::repr`.
  - **Signal:** no ledger row under those paths.
  - Prereqs: S11 ε #8682; S10 δ₂ #8960, ζ₂ #8971, ε₅ #8969, α₃ #8932; #7636; meshing-service β #8285, ε #8288; #8822.
- **δ₅ — reify-eval dispatch and classifier prose: dispatcher, warm_pool, structural_classifier, sweep_classifier, relate_solve, topology_attribute_propagation** (medium). Keys: **fk-d0569c316d46**.
  - Paths: those six reify-eval files.
  - Correct the dispatcher module doc's "(4 variants)" BFS bound to `ReprKind`'s 5, and repoint its retired `reify_types::ReprKind` cites to `reify_ir::ReprKind` in the same edit. Rewrite the test-region `warm_pool.rs` / `tolerance_promise.rs` cites.
  - `classify_cell`: keep a short contract plus a pointer to the mesh-morphing / precision PRD section. Move "verified 2026-06-27", "Residual risk … accepted", "The divergence is deliberate and is pinned by" to the commit.
  - Rewrite `graph.rs:599-605` → `EvaluationGraph::from_templates`; `guards.rs:297,667,700` → the three `compile_*_guard` fns.
  - Rewrite the eight "PRD line 33/34" and warm_pool "arch §4.3 line 538/539" cites → section headings.
  - relate_solve `solve_scopes` / `verify_static_scope`: "Measured, not assumed, by" and "Do NOT fix the extra level" become contract plus pointer.
  - topology_attribute_propagation's 8 "PRD line N" cites → persistent-naming-v2 headings.
  - **Signal:** the single-line tokens ``ReprKind`] (4`` (pattern ``ReprKind`\]? \(4``, 2 at `9bdef68093`) and `reify_types::ReprKind` (3) are absent from dispatcher.rs; no ledger row under the six files; `classify_cell` doc-line count recorded in the commit.
  - Prereqs: S1 α #9036; S4 δ #8862; S10 ι₆ #8998, θ₂ #8981, β₁ #8935; S3 η #8677; S6 γ #8830; S2's leaves editing sweep_classifier / relate_solve (S2-β₂, S2-η; ids at S2's filing, else HELD with `x_seam_wait`); #8822.
- **δ₆ — Check-surface remainder: tolerance_combine, representation_within, output_spec, and two engine_constraints cites** (low). Keys: none (`x_finding_part` fk-d0569c316d46, fk-e90cf61da43a).
  - Paths: the post-S12-λ files and `engine_constraints.rs`'s two surviving cites: `lib.rs:163` → `reify_constraints::SimpleConstraintChecker::check`; `engine_constraints.rs:42-62` → the RepresentationWithin ordering invariant doc at its new home.
  - Rehome the prose of `unenforced_representation_bound_diagnostic` (84/21) and `compute_representation_bounds` (75/21) to `precision-nominal-representation-guarantee.md`, leaving pointers.
  - "Task #6170 retired it" goes to the commit.
  - **Signal:** no ledger row under those paths.
  - Prereqs: S12 λ #9019, ε2 #9020, μ #9018, ν #9031; #8822.

### ε — language pipeline

- **ε₁ — DiagnosticCode: one line per variant, one strum round-trip test, ratchet rows** (medium). Keys: **fk-15e94643506d**.
  - Path: `crates/reify-core/src/diagnostics.rs` (+ the two ratchet rows in `tests/infra/adoption-ratchet.manifest`).
  - Applies D8/D9. Fix the five `reify-types` pointers, including `DiagnosticInfo`'s "lives in reify-types".
  - ε₁ owns **every** `Origin:` line, the `ScopeCoupling` one included (critic M2: an exclusion would keep its own ratchet row red). Pass 2 notes #8868 that its diagnostics.rs half becomes a no-op once ε₁ lands, and adds diagnostics.rs to #8868's files so a lock serialises them. Exclusion: the `Diagnostic` doctest block (S12 β #9024).
  - **Signal:**
    - `cargo test -p reify-core` passes with exactly one table-driven derive test and the 15 behavioural tests;
    - `python3 tests/infra/adoption_ratchet.py` reports both new rows at 0;
    - `git grep -cE '^ *///.*Origin:' crates/reify-core/src/diagnostics.rs` is 0 (204 at `9bdef68093`);
    - no ledger row for the file.
  - Prereqs: none (D8; OQ-1). File lock serialises it against S12 δ #9028 and the variant-adding leaves.
- **ε₂ — HexWedgeMeshOutcome beside p2_substitution_diagnostic; re-cite to #4746** (low). Keys: **fk-2e1e82c1d1c6**.
  - Move `HexWedgeMeshOutcome`, `hex_wedge_mesh_diagnostic` and their tests from reify-core `diagnostics.rs` to `p2_substitution_diagnostic`'s module after S2-β₂ moves it (`volume_mesh_assembly.rs`, S2 draft §5.8).
  - Re-cite the dead-code note as `TODO(#4746)` (drop #2947/#2991). The PTODO baseline is updated in the same diff.
  - Pass 2 appends a note to #4746 naming the new path.
  - **Signal:** `git grep -n 'hex_wedge_mesh_diagnostic' -- crates/reify-core` is empty; `git grep -n 'task #2947' -- crates/` is empty; `cargo test -p reify-eval` runs the moved tests.
  - Prereqs: ε₁ (same file); the S2 leaf labelled β₂ in the S2 draft ("S2-β₂", id at S2's filing, else HELD with `x_seam_wait`).
- **ε₂ (fallback):** if S2-β₂ does not move `p2_substitution_diagnostic`, the target is its current home in `engine_build.rs`'s successor.
- **ε₃ — reify-ir geometry: the five false comments, PRD-line cites, symbol cites** (medium). Keys: **fk-e2730bcb8111**.
  - Paths: the post-S10-α₃ `crates/reify-ir/src/geometry/{op,query,naming}.rs` and `geometry_id.rs`.
  - Descriptor doc: "all 49" becomes "every GeometryOp variant (the completeness test pins the count)"; delete the "unverified copy until L2" section; the table is authoritative.
  - ParentRole: "defined by `GEOMETRY_OP_DESCRIPTORS`".
  - `content_hash_bytes`: remove the stale `#[allow(dead_code)]` and its note.
  - `lookup_result_face`: drop only the "exercised only by tests" clause (#4263 is still pending).
  - `capability_kind`'s G-allow on done #3623: drop or re-cite.
  - "PRD line NN" → persistent-naming-v2 / structural-analysis-shells headings; `register.rs:58` / `kernel.rs:353` → symbols.
  - **Signal:** "all 49" and "unverified copy" are absent from reify-ir src; no ledger row under `crates/reify-ir/src/geometry*`.
  - Prereqs: S10 α₁ #8930, α₂ #8931, α₃ #8932, η₁ #8974, η₆ #8979, ω #9000 (S10 δ₃ #8961 and S10 θ₁ #8980, θ₂ #8981, θ₃ #8982, θ₄ #8983 also edit `geometry/op.rs` / `geometry_id.rs`); #8822.
- **ε₄ — engine_build residue: INV-BUILD pointers; reviewer and measurement prose** (medium). Keys: **fk-eb5a9e37eb9f**.
  - Paths: what remains of `crates/reify-eval/src/engine_build.rs` and its S2 children's residue not rewritten by S2.
  - Replace both "INV-BUILD-1 … still `proposed` / future work" claims (rustdoc and the "Release-mode reliance (reviewer_comprehensive #1, θ amend pass)" block) with "INV-BUILD-1, INV-BUILD-3 (docs/invariants.md)" (D10).
  - Remove the 48 reviewer, amendment and suggestion lines that survive S2.
  - `reset_per_build_state`'s "`elastic_result.rs` (:617/641/674)" → symbols, if S1 ε left any.
  - **Signal:** "INV-BUILD-1, still" is absent under `crates/reify-eval/src`; no ledger row under the engine_build paths.
  - Prereqs: S1 ε #9043, θ #9046; S2-δ and S2's close leaf (ids at S2's filing, else HELD with `x_seam_wait`); #8822.
- **ε₅ — reify-eval lib.rs, results.rs and engine_eval residue** (medium). Keys: **fk-a401e42a7b49**, **fk-420be0954017**.
  - Paths: `crates/reify-eval/src/{lib,results}.rs` and what engine_eval.rs (P8) and the S4 ν children keep, plus `detectors.rs` for the mis-stitch.
  - lib.rs:
    - the `:1` reify-types pointer → `reify_ir::value::SampledField`;
    - the `cell_commit` / `detectors` mod comments ("no external callers yet") become one-line purposes;
    - the `:60-64` / `:103-108` shim comments are reworded as public-API comments (S5 D12; no import migration);
    - the `results.rs` "`engine_eval.rs ~L494-518`" cite after μ.
  - engine_eval residue:
    - positional coordinates ("Arch §9.1 lines 868–877" family, `engine_eval.rs:609-614`, "~line 5270", "L3119", "(`:472`)") → symbols or headings;
    - the "falsehood this comment carried until amendment round 2" and MEASURED narratives go to the commit;
    - fix the `nondriving_joint_compile_spans` / `detect_error_map_diagnostics` mis-stitch at its post-κ home, a no-op if κ already split the docs correctly.
  - **Signal:** the single-line tokens `external callers yet` (1 at `9bdef68093`; the phrase wraps) and `reify-types` are absent from reify-eval `lib.rs`; `detect_error_map_diagnostics`'s doc opens at its own summary; no ledger row under those paths.
  - Prereqs: S1 λ #9050, μ #9051; S4 ν #8860, ι #8865, κ #8868, τ #8872; S14 ν #8645; S5 γ #8991; #8822.
- **ε₆ — ty.rs: the quantity-slot rule moves to a decisions record; ty.rs keeps a short `//!`** (low). Keys: **fk-863955f49c19**.
  - Create `docs/decisions/<originating-task>-quantity-slot-conformance.md`, following the `2522-…` precedent: rule text plus the ruling's provenance, with no measurement prose.
  - Replace ty.rs's 305-line `//!` with a short description of `Type` and one pointer.
  - Repoint the ~10 inbound pointers to the decision record: conformance/mod.rs at its post-S9-ο partitions, units.rs, and the assert-message string, which is a literal; update it only if no test asserts on it.
  - Edit `dimensioned-construction-strictness.md` §12's "single normative home" bullet to name the record.
  - Rewrite ty.rs's `geometry.rs:567` / `conformance.rs:521-531` / "lines 80–88" cites → symbols. Fix the reify-types path.
  - **Signal:** "single normative home" is absent from `crates/reify-core/src/ty.rs`; ty.rs's `//!` line count is recorded in the commit; no ledger row for ty.rs; the decision record exists and every inbound "quantity-slot convention" pointer names it.
  - Prereqs: S9 ο #8745; S7 λ #8638 (file lock only, no edge needed); #8822.
- **ε₇ — reify-expr: the stack-budget rule stated once; pin lists and self-cites gone** (low). Keys: **fk-5b6284e261af**.
  - Paths: `crates/reify-expr/src/**` after S8 τ's split.
  - State the `#[inline(never)]` stack-budget rule once at `MAX_RECURSION_DEPTH`, and delete the per-helper paragraphs (17 × "3 MiB").
  - Drop `eval_worst_case_dispatch`'s seven-test pin list and the other pin lists.
  - Rewrite the cross-file `lib.rs:NNN` self-cites → symbols. "suggestion 2" goes to the commit. Leave τ's one-sentence module purposes alone.
  - **Signal:** `git grep -c '3 MiB' -- crates/reify-expr/src` is 1; no ledger row under `crates/reify-expr/src/`.
  - Prereqs: S8 τ #8911; #8822.

### ζ — reify-audit, front ends, GUI

- **ζ₁ — reify-audit: correct the ten false statements; rehome measurement and rejected-alternative prose** (medium). Keys: **fk-a26634db4007**.
  - Paths: the post-S24 homes of lib.rs's crate doc (`lib.rs`), `Pattern` / `Severity` / `AuditContext` (`model.rs`), `RealGitOps` (`git_ops.rs`), `ptodo/*`, `pdoccover/*`, `p5_phantom_done/*`, `pdiag.rs`, and `bin/reify-audit.rs`'s "all three detectors" usage lines (the tenth claim).
  - PTODO severity is written once, as a pointer to `ptodo::Kind::severity` (Untracked, BareIgnore, Orphaned and GAllowOrphaned are High).
  - Drop the detector count; the naming convention says what the variants actually are.
  - Correct Medium's scope, the `AuditContext` claim (after ε moves `open_tasks_db` to `task_rows`) and the `RealGitOps` test claim; remove the "stubs" header.
  - Rehome measurement and rejected-alternative prose to the reify-audit PRDs' § anchors. Production cites into other crates become symbols.
  - PLINECITE does not scan this crate (D1): the commit records before and after cite counts by the program rule.
  - **Signal:** `git grep -nE 'three detectors:|all three detectors|slice-1 integration suite|all Medium severity|stubs — filled in by the TDD|never owns a connection' -- crates/reify-audit/src` is empty (7 at `9bdef68093`; every arm is a single-line token).
  - Prereqs: S24 α #8806, β #8807, γ1 #8808, δ #8810, ε #8811, ζ #8812, κ #8816, λ #8817, μ #8818, ν #8819.
- **ζ₂ — reify-lsp and reify-cli front-end prose** (medium). Keys: **fk-14195f0a11d2**.
  - Paths: reify-lsp `diagnostics.rs` (+ `diagnostics/tests.rs`), `goto_def.rs`, `server.rs`, `references/*`, residual `eval_guard.rs` / `analysis.rs`, and reify-cli `main.rs` successors.
  - `cache.rs:482-513` ×2 → `reify_eval::cache::CacheStore::mark_pending` / `mark_pending_with_cause`.
  - Task-number and measurement prose leaves goto_def, server, references and main successors. The reporting and throttle prose left in eval_guard after β gets a docs/notes pointer if durable.
  - **Signal:** `cache.rs:482-513` is absent; no ledger row under `crates/reify-lsp/src/` or `crates/reify-cli/src/`.
  - Prereqs: S27 β #8761, γ #8778, ε #8759, ζ #8771, η #8772, ι #8774, κ #8760, μ #8769, ψ #8757, ω #8770, τ #8758, υ #8775, φ #8776; S12 γ #9025; S1 ζ #9044, θ #9046; #8822.
- **ζ₃ = #8542 (adopted, re-scoped) — gui engine.rs pointers and the two misplaced doc blocks** (low). Keys: **fk-70d435aa0170** (already stamped).
  - Pass 2 re-scopes the description to what survives §8 (the root-file pointers; `build_values`' doc at its `engine/projection.rs` home; `build_template_node`'s doc at `engine/entity_tree.rs`) and adds the edges.
  - **Signal:** `build_values` and `build_template_node` each carry their own doc; no ledger row under `gui/src-tauri/src/engine*`.
  - Prereqs: S25 γ #8939, ε #8941, ζ #8942, η #8943, λ #8947; S1 ζ #9044, θ #9046; #8822.
- **ζ₄ = #8316 (adopted) — reifyGrammarCorpus corpus cites + `reify.grammar`** (low). Keys: none.
  - Pass 2 adds `#8316 ← S18 π #9096, #8822` and widens it to the file's 4 `crates/`/`tests/` cites.
  - **Signal:** the live token `examples/[^ ]+\.ri:[0-9]` is absent from `gui/src/__tests__/reifyGrammarCorpus.test.ts` (111 at `9bdef68093`), and no ledger row remains for it. The ledger holds no `gui/` row before #8822 regenerates it, so ledger absence alone would pass vacuously. Pass 2 wires #8316's edges before anything else, because today it has `dependencies: []`.
- **ζ₅ — GUI residual: gui-ts and gui-rs cites; unitLadder display-helper residue** (medium). Keys: **fk-3ac9bc99b963** (structural half on S26 θ #9069; see the cross-reference).
  - Paths: `gui/src/**` and `gui/src-tauri/src/**` minus ζ₃/ζ₄.
  - In unitLadder.ts, after θ: `normalizeUnitLabel`'s task-history "WHY" block and the header's "(task #5199)".
  - `engine_tests.rs`'s 26 cites at their post-S25 test homes; `write_tools.rs`, `commands_tests.rs`, `types.rs`, `App.test.tsx`, `Editor.test.tsx`, `bridge.ts`, `autoViewGenerator.ts`.
  - **Signal:** comment-line cites are gone from the GUI trees: `git grep -cE '^\s*(//|\*|/\*).*[A-Za-z0-9_-]\.(rs|ts|tsx|ri|sh|py|toml|yaml|yml):[0-9]' -- gui/src gui/src-tauri/src` is 0 (254 at `9bdef68093`, ζ₄'s included; ζ₅ follows ζ₄). No ledger row remains under `gui/`.
  - Prereqs: S26 θ #9069, ω #9083; S25 ω #8956; S1 ω #9052; S6 ω₂ #8856 (λ #8839 migrates `engine_tests` ×23 + `commands_tests` ×1 without locking the gui files); S16 ω #9122 (α #9085 edits `gui/src/types.ts`); ζ₃, ζ₄; #8822. The close-leaf list is re-derived mechanically at pass 2 (§9 θ preamble).

### η — docs

- **η — Docs that name retired mechanisms: architecture §3.5, the freshness PRD, the invariants closing paragraph** (low). Keys: none (program cross-reference lines from S3 and S1).
  - After S3 ζ #8699 deletes `propagate_freshness_only` and `freshness_walk.rs`: `docs/reify-implementation-architecture.md` §3.5's "freshness propagation" mode, and `docs/prds/freshness-4-variant.md`'s mechanism references, name `dirty::dirty_cone` (S3 §5.4) or are marked as-authored, per the overlay's never-re-anchor rule for dated records.
  - After S1 θ #9046: docs/invariants.md's closing paragraph stops citing `reset_dispatch_tallies()` as a precedent, keeping `reify-audit --pattern PREGISTRY` green (D10).
  - Docs-only (D11).
  - **Signal:** `git grep -nE 'propagate_freshness_only|freshness_walk' -- docs/reify-implementation-architecture.md` is empty; `reset_dispatch_tallies` is absent from docs/invariants.md; PREGISTRY exits 0.
  - Prereqs: S3 ζ #8699; S1 θ #9046; S24 ξ #8821.

### θ — residual ledger sweeps (pointer-only, low)

Each θ leaf burns the remaining ledger rows under its crates. They are mostly test-file cites (962 occurrences in `crates/*/tests`), plus src rows no finding leaf owns. The dangling untracked-plan pointers in those paths are also cleared (D5). Each depends on the **close leaf** of every program stream that edits its crates (D4's "after the movers"; §12 OQ-7 (a)), on the S23 finding leaves whose paths it borders (path-set disjointness), and on #8822.

**The close-leaf lists below are a floor, not the final list.** At pass 2 they are derived mechanically for θ₁–θ₄ and ζ₅: take the union of crates and paths in every program stream's filed leaf `metadata.files` plus its PRD's leaf Files lists. Any stream that touches a θ leaf's paths gets its close leaf added as a prerequisite. The edges written here are the ones the critic and the measurement seats proved.

- **θ₁ — reify-compiler** (src, tests; `.ri` excluded per D7). Prereqs:
  - S3 μ #8710, S6 ω₂ #8856, S7 ω #8665, S8 ω #8914, S9 π #8746, S10 ω #9000, S12 ω #9032, S27 Ω #8779; S19 θ #8700
  - ε₆; #8822
- **θ₂ — reify-eval residual** (src outside β₂/δ₅/δ₆/ε₂–ε₅, all of `tests/`), **reify-eval-fea-tests, and `crates/reify-analysis/**`** (S5 κ₁/λ/μ move `compute_targets/{buckling,mod,multi_case,shell_solve}`, `trajectory_ops` and `dynamics_ops` there; β₂ keeps only its three families). `.rs` rows only: `.ri` rows such as `tests/fixtures/undef_boundary_representative.ri`'s are #8305's. Prereqs:
  - S1 ω #9052, S2's close, S3 μ #8710, S4 ω #8878, S5 σ #9010, S6 ω₂ #8856, S8 ω #8914, S10 ω #9000, S12 ω #9032, S13 ι #8754, S14 ρ #8660, S15 σ #8724, S16 ω #9122
  - S19 θ #8700
  - β₂, δ₅, δ₆, ε₄, ε₅; #8822
- **θ₃ — Language and tooling crates residual:** reify-expr tests, reify-core (less ε₁/ε₆ files), reify-ir (less ε₃), reify-builtins, reify-syntax, reify-ast, reify-doc, reify-doc-build, reify-test-support, reify-cli tests, reify-lsp tests, reify-compute-contract, reify-mcp. Prereqs:
  - S1 ω #9052, S5 σ #9010, S7 ω #8665, S8 ω #8914, S9 π #8746, S10 ω #9000, S12 ω #9032, S15 σ #8724, S26 ω #9083, S27 Ω #8779
  - ε₁, ε₃, ε₆, ε₇, ζ₂; #8822
- **θ₄ — Analysis and geometry crates residual:** reify-stdlib (less β₃/γ₁), reify-shell-extract (less β₃), reify-mesh-morph src residue (`elasticity.rs`, `laplacian.rs`, …) and tests, reify-constraints `dual_jacobian.rs` and tests, the kernel crates' src residue (manifold `lib`/`queries`/`register`, gmsh `ffi`/`fill_metrics`/`lib`/`register`/`volume_mesh_options`, openvdb `ffi`/`ingest`/`kernel`/`*_options`/`register`, OCCT `register.rs`/`tessellate_options.rs`) and tests, reify-kernel-fidget, reify-kernel-conformance, and the `build.rs` files of reify-kernel-conformance and reify-mesh-morph. Prereqs:
  - S5 σ #9010, S6 ω₂ #8856, S8 ω #8914, S10 ω #9000, S11 ω #8688, S14 ρ #8660, S15 σ #8724, S16 ω #9122, S21 ω #8663
  - β₃, γ₁, δ₁, δ₂, δ₃, δ₄; #8822

**Signal** (each θ): no ledger row under its crates. Checks are restricted to `.rs` rows, since `.ri` rows are #8305's (D7).

### ω — close

- **ω — PRD close: id backfill, terminal stamp, AS-AUTHORED freeze, INV-DOC-1 ledger remainder** (low). Keys: none.
  - Per the overlay's decompose-close obligations: stamp `SHIPPED` (or the honest terminal token) with the landed leaf ids, the freeze paragraph and the LIVE/AS-AUTHORED map (§6 stays LIVE as the pointer-form convention), and the matching manifest header.
  - Record the ledger's final row count. The expected count is 0 outside `.ri` (#8305) and #8540's excluded lines, with every surviving row named.
  - Docs-only.
  - **Signal:** the committed header.
  - Prereqs: every other leaf (a cancelled sibling counts as satisfied).

**DAG summary.**
- γ₂ and ε₁ are free.
- ε₂ follows ε₁ and S2.
- Every other leaf follows #8822 plus its §8 owners.
- θ₁–θ₄ follow the finding leaves bordering their crates.
- ω follows all.
- 34 leaves: 31 new, 3 adopted (#8531 δ₃, #8542 ζ₃, #8316 ζ₄).

**Key-to-leaf map** (each key exactly once; `x_finding_part` noted for cross-leaf shares):

| key | leaf |
|---|---|
| fk-4186fb4f7289 | α₁ |
| fk-5f3dcd2ae885 | α₂ |
| fk-465b5eec51e2 | α₃ |
| fk-cb27e6ba131b | α₄ (part on α₃) |
| fk-91aaee6b8fde | closed by #8537 (§11) |
| fk-8c68164d0151 | #8540 (§11) |
| fk-25c46b3f4c72 | β₁ (part on β₂) |
| fk-e4b87a3c5f90 | β₁ |
| fk-b0a7b8818be4 | β₂ |
| fk-632e4850d7d0 | β₃ |
| fk-67ca81ff9067 | γ₁ |
| fk-22068b67e813 | γ₁ |
| fk-679840f73698 | γ₂ |
| fk-e90cf61da43a | δ₁ (parts on δ₄, δ₆) |
| fk-7fbe91c20e82 | δ₂ |
| fk-9e30579d2c52 | δ₃ = #8531 |
| fk-d0569c316d46 | δ₅ (part on δ₆) |
| fk-15e94643506d | ε₁ |
| fk-2e1e82c1d1c6 | ε₂ |
| fk-e2730bcb8111 | ε₃ |
| fk-eb5a9e37eb9f | ε₄ |
| fk-a401e42a7b49 | ε₅ |
| fk-420be0954017 | ε₅ |
| fk-863955f49c19 | ε₆ |
| fk-5b6284e261af | ε₇ |
| fk-a26634db4007 | ζ₁ |
| fk-14195f0a11d2 | ζ₂ |
| fk-70d435aa0170 | ζ₃ = #8542 |
| fk-3ac9bc99b963 | ζ₅ (structural half S26 θ #9069) |

**G7 advisory walk** (`docs/legibility/design-invariants.md`):
- **No-lockstep-duplication** is the reason for D3's state-once rule (reify-expr stack budget) and D10 (invariant status lives only in the registry).
- **INV-SF-5 `placeholders-owned-and-loud`** governs D5's re-cite of cancelled blockers (ε₂) and the `ptodo:allow` removal (β₂).
- **"Assert behaviour, not source prose"** is why ε₁'s ratchet counts constructs (`Origin:` lines, test fn names) rather than prose wording, and why no leaf adds a comment-content test.
- No hit needs a waiver.

## 10. Held leaves and external dependencies

- **No leaf waits on a later-wave stream.** S23 is the last wave.
- **S2 (wave 3) is in session and unfiled.** ε₂, ε₄, δ₅ and θ₂ name S2 leaves whose ids arrive at S2's filing, before S23's pass 2. If S2 has not filed by then, those four are filed with `metadata.x_seam_wait = {stream: "S2", contract: "<the leaf named here>"}` and excluded from S23's `commit_planning`.
- **Leaves behind the S18 cutover cascade:** α₄ waits on S18 ζ #9112, which is pinned and held. It is not an `x_seam_wait`: the id exists.
- **Non-program prerequisites:**
  - #8822 (S24 ο) for every burn-down leaf except ε₁ and γ₂
  - #6677 (δ₂; §12 OQ-2 removed #6903)
  - #7636, #8285, #8288 (δ₄)
  - #8375, #8374 (β₁, β₂)
  - #8527 (β₃), #8545 (γ₁), #7682 (α₄), #8540 / #8073 (α₂), #4746 note (ε₂)

## 11. Keys not given a new leaf

- **fk-91aaee6b8fde:** closed by #8537 (done, merge `7def1f6e97`). Re-measured: `run_all.sh` and `test_harness_kloc_cap.sh` carry 0 file:line cites; the finding's named verify.sh cites in `test_compute_trampoline_registration_wired.sh` are gone. Its remaining crate cites were outside the finding and go to α₅ / S18 ν #9111.
- **fk-8c68164d0151:** filed as #8540, held behind S18 ζ #9112 by the program's ruling. Its verify.sh half is moot after ζ / α #9095, and its sibling-script cites stay #8540's (§8). Referenced, not re-filed (§12 OQ-3 ruled (a)).
- **Partial non-adoption, recorded for the record:**
  - fk-a26634db4007's "`Kind::severity` per pattern in `--help`" (not adopted by S24; ζ₁ points the docs at the code instead);
  - fk-420be0954017's compat-re-export retirement (collides with S5 D12; ε₅ rewords only);
  - fk-3ac9bc99b963's backend-grammar proposal (S26 θ #9069 owns it);
  - fk-eb5a9e37eb9f's `reset_dispatch_tallies` deletion (S1 θ #9046 delivers it).

## 12. Resolved pass-1 questions (lead rulings, 2026-10-15) and the critic fold

Rulings (successor lead; the full record is the run scratch `prd-results/S23.md`):

- **OQ-1 → (a).** ε₁ lands early with no stream prerequisite. It ships the ratchet rows `Origin:` = 0 and per-variant derive tests = 0 (INV candidate 5). Whichever of ε₁ and S12 δ #9028 lands second merges the derive list.
- **OQ-2 → (b).** δ₂ is prose-only and depends on S14 ν/ξ and #6677. Pass 2 adds the named-outcome / solve-mode / `ObjectiveConsumption` refactor to #6903 as a details note, with `x_finding_part` fk-7fbe91c20e82. δ₂ keeps the key for the prose half.
- **OQ-3 → (a).** #8540's sibling half stays #8540's, held behind S18 ζ #9112. `lib_test_semaphore.sh`'s two `cargo-test-occt-gated.sh` cites go to α₂, and α₂ ← S18 ε #9101.
- **OQ-4 → (a).** Pointer-only token edits (file:line → symbol, no rewording) inside another stream's prose are allowed once the owner has landed. The leaf names the landed owner in its commit (D4(b)).
- **OQ-5 → (a).** Cites into external code take the form `<package>@<version> module.py::symbol`, with no line number (D2).
- **OQ-6 → neither option.** INV-EVAL-4, INV-EVAL-5 and INV-FEA-2 are already named in S24 ξ #8821's description, so they are #8821's. Pass 2 appends S23's measurements to #8821's details:
  - INV-EVAL-4: the divergence gate and the corpus sweep exist;
  - INV-EVAL-5: #4952's interim test runs in the corpus sweep;
  - INV-FEA-2: #5079's `catch_unwind` boundary landed.

  Merge-gate membership is still unverified. No new task is filed, and η flips nothing.
- **OQ-7 → (a).** The θ₁–θ₄ / ζ₅ close-leaf lists are derived mechanically at pass 2 (§9 θ preamble). The edges the critic proved are added now (θ₄, β₃ ← S8 ω #8914; ζ₅ ← S1 ω #9052, S6 ω₂ #8856, S16 ω #9122).
- **OQ-8 → (a).** #8305 owns every `.ri` file. S23's ledger checks are restricted to `.rs` rows.

Pass-2 directions carried here:
- Wire the adopted tasks' edges **before anything else**, because #8531, #8542 and #8316 have `dependencies: []` today and could dispatch ahead of #8822 and their owners:
  - #8531 ← S11 ι #8684, #8822;
  - #8542 ← S25 #8939 #8941 #8942 #8943 #8947, S1 #9044 #9046, #8822, with its description re-scoped;
  - #8316 ← S18 π #9096, #8822.
- Append the #4746 note (ε₂) and the #8868 note (ε₁ owns the Origin line), and add `crates/reify-core/src/diagnostics.rs` to #8868's files.
- Add `x_finding_part` fk-3ac9bc99b963 on S26 θ #9069.
- S2's ids, if S2 has filed: wire ε₂, ε₄, δ₅ and θ₂ to them. Otherwise file those four HELD with `x_seam_wait` (§10).

Critic fold (opus/high critic, 2026-10-15; 2 blockers, 7 majors, minors, all folded):
- Every phrase check is re-anchored on a single-line token measured at main.
- β₃ checks the bare tokens; ζ₄ and ζ₅ check live cite tokens.
- γ₂'s pattern is widened.
- Partition coverage is closed by widening θ₂, θ₃ and θ₄, and δ₃'s check is narrowed to lib.rs.
- ε₁ owns every `Origin:` line.
- New edges: ε₃ ← S10 ω; β₁ ← S14 ρ, S16 ο, #8380, #8381; α₅ ← S19 Ω; θ₁/θ₂ ← S19 θ #8700; α₁ ← #8540 as a real edge.
- θ₂'s check is `.rs`-only, and γ₁'s check is scoped to its own files.
- ζ₁ takes the tenth claim; δ₅ fixes the `reify_types::ReprKind` cite; β₂ takes modal_ops's "nz=6 → 28 nodes".
- α₄ drops the unneeded `plinecite:allow`.
