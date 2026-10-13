# PRD — reify-audit: typed Finding, shared source-shape recognisers, fallible seams, registry-status and line-cite lanes

**Status:** active — authored and decomposed 2026-10-13 (review-all program `review-all-reify-20261010`, stream **S24**; leaves #8806–#8823, ξ #8821 held). Version-agnostic factory infrastructure; lives under `v0_6/` per the program's shared convention.

**Class:** dev-process infra/tooling (no `.ri` language surface — the docs-truth gate does not fire; the D3 substrate workflow has no `.ri`/grammar/semantic premise to probe here, so the manifest's grep bindings are verified directly).
**Approach:** B+H (G5). The mechanism count is well above 8, and the crate's output contract is consumed by the `/audit` skill, five scripts, the merge-gate infra tests and dark-factory's pre-done hook. That contract is the findings JSON, the exit code and the PTODO fingerprint. §5 is the contract, §6 the two-way boundary-test sketch.

**Code anchors** verified against main `e217c0f30b` (2026-10-12) by four measurement seats and re-checked at `a79726885d` (2026-10-13). Between the two, only `docs/invariants.md` and `docs/legibility/design-invariants.md` moved: the INV-ARCH-1 row and family landed. In-flight branches read at the same time: `task/6935` (PLINECITE, 18 commits) and `task/7098` (PTODO lane δ-C, 9 commits). An adversarial critic pass (31 findings, every decisive one reproduced by the lead) is folded in. Main moves fast — cite-by-symbol; re-locate lines at implementation time.

Program contract: `plans/review-all-program-reify-20261012.md` (stream row S24; ruling "S24 reify-audit: typed Finding, shared recognisers, registry-status lane"; rulings "Invariant candidates", "Contradictions", "Weakened findings"). Finding record: `plans/review-all-reify-20261010.json`. Quality definition: dark-factory `docs/code-quality.md` (heuristics cited by name). Session rulings R1–R3 (Leo, 2026-10-13): §11.

---

## 1. Goal and consumers (G1)

`reify-audit` is the factory's detector CLI. The merge gate runs its PTODO, PDIAG and PDOCCOVER ratchets, the `/audit` skill routes its findings into tasks and escalations, and dark-factory's pre-done hook refuses a done-flip on its exit code. Today:

- Its findings carry their kind and line only as text inside `summary`. Three production functions, the skill's routing templates and five test helpers parse that text back out.
- `task_id` holds a task id, a path or the empty string, depending on the detector.
- `--since` scopes nothing and silently disables P2's backlog guard.
- Nine of GitOps' twelve questions answer "git failed" with an empty value, so three detectors report clean on a failed enumeration.
- Task-DB access and DB degradation are PTODO-private, so PDCHECK's degradation cannot be guarded.
- Four cfg(test) recognisers, three G-allow grammars and two test-path rules disagree.
- P5's sweep spells two rescues twice.
- The exit code reuses 125 for both "infra error" and "125 High findings".
- Nothing checks that an invariant registry row's status matches its enforcement, and nothing gates a new `file:line` cite in code prose.

After this PRD:

- An agent adding a PTODO kind, rewording a summary or making PDIAG report a line edits a typed `Kind` in `crate::kind` and a `detail` string. The PTODO baseline fingerprint, the G-allow sort key and the skill's follow-up titles read fields, never `summary`.
- `reify-audit --since …` is an unknown argument (exit 125). `/audit` with no flags sweeps the whole corpus, which is what it already did in fact.
- A detector that enumerates the tree cannot run on a failed `git ls-files`. The dispatcher records it as unexamined with one High finding per such detector.
- `reify-audit --pattern PDCHECK --require-tasks-db` is accepted and exits 255 when tasks.db is absent or is not a tasks DB, exactly as PTODO's lanes do.
- One `source_shape` module answers "is this region test-only / opted out / a G-allow marker?" for every detector and for the orphan-producer script's grammar. An adoption ratchet keeps the private copies at zero.
- Exactly 125 High findings exit 124 (saturated). Exit 125 always means an infra error, and the skill's §3.1 disambiguation is deleted.
- `reify-audit --pattern PREGISTRY` reds on three kinds of drift: an `enforced` registry row whose enforcement pointer does not resolve, a `proposed` row whose pointer does resolve, and a census row that disagrees with the bin's detector tokens. Its infra test gates every merge and every direct commit that edits either registry, and its liveness lane runs in the weekly docs-truth sweep (INV-META-1's registry-status clause enforced; invariant candidate 1).
- `reify-audit --pattern PLINECITE` gates every merge. A new `<file>.<ext>:<line>` cite in tracked code comments reds, while the grandfathered ledger burns down through S23 without redding. A file move keeps its ledger rows through `plinecite-baseline-gen --rename` (invariant candidate 3, ruling R1).

| Mechanism | Consumer |
|---|---|
| `crate::kind` per-detector kind enums; `Finding` with `Kind` / `Subject` / `detail`; rendered `summary()`; field-derived `fingerprint()` / `sort_key()` (§5.1) | `ptodo-baseline-gen` + `tests/infra/test_reify_audit_ptodo.sh` (merge-gate ratchet); `.claude/skills/audit/references/severity-routing.md` titles and dedupe keys; `scripts/pprdstatus-escalate.py`; `scripts/docs-truth-sweep.py`; `tests/infra/stub_escalation_mcp.py`; `tests/infra/test_reify_audit_pdoccover.py`; the bin's `print_summary` |
| `AuditContext::new` + builder (§5.2) | the five bins (`reify-audit` and four `*-baseline-gen`, PLINECITE's included), every detector's tests (146 struct literals after #6935) |
| fallible `GitOps` + `TrackedFiles` (§5.3) | the eight `ls_files` detectors via the dispatcher (PLINECITE included); P5's sweep and pre-done corroboration; the four baseline generators |
| `Detector` row declarations + `DetectorOutput` + dispatcher-level unexamined/degraded outcomes (§5.4) | `reify-audit` runs from `/audit`, the predone wrapper, the docs-truth sweep; `--require-tasks-db` callers |
| `task_rows::TasksDb` handle (schema-probed); `cite_grammar` module | PTODO, PDCHECK, PPRDSTATUS, PREGISTRY's liveness lane; `ptodo-baseline-gen`'s `@@PTODO_SCAN@@ … tasks_db=` token |
| `source_shape` module (§5.6) | PDIAG, P2, PDOCCOVER, PTODO, `jcodemunch_client` suppression, PREGISTRY's item resolver, PLINECITE's per-language comment leaders, `scripts/audit-orphan-producers.sh` (shared grammar and positional corpus) |
| P5 rescue-step list + one coverage predicate | the `/audit` sweep's P5 Highs (the escalation path) |
| saturated exit code (§5.5) | dark-factory `fused-memory/src/fused_memory/middleware/pre_done_hook.py` (0 vs non-zero only), `/audit` skill §3, `tests/infra/test_reify_audit_{ptodo,pdiag}.sh`, `scripts/smoke-predone-hook.sh` |
| `PreDonePosture` in `AuditContext` | `--pre-done` runs from the dark-factory hook (the effective posture is printed once per run) |
| `test_support::RepoFixture` | every working-tree detector's integration tests |
| PREGISTRY lane + registry row grammar (§5.7) | `tests/infra/test_reify_audit_pregistry.sh` (merge gate and the docs-only pre-commit); `scripts/docs-truth-sweep.py` (weekly, DB-present liveness); every session that adds or flips a registry row (S3, S4, S1, S5, S7, S10, S12, S15, S23, S25 and the pending tasks in M18) |
| PLINECITE merge-time ratchet + `--rename` (§5.8) | `tests/infra/test_reify_audit_plinecite.sh` (merge gate); S23's burn-down; every stream that moves a file under PLINECITE scope |

## 2. Findings owned (verbatim keys; statements in the record)

| key | short | anchor | sev | verdict | closed by |
|---|---|---|---|---|---|
| fk-33f9891ad16c | f285 | `crates/reify-audit/src/lib.rs::Finding` | high | confirmed | γ1 + γ2 (key on γ2) |
| fk-0b1d530f0264 | f286 | `crates/reify-audit/src/lib.rs::AuditContext` | high | confirmed | β |
| fk-ec468bfa673a | f287 | `slug:reify-audit-source-shape-recognisers` | medium | confirmed | ζ |
| fk-c45888cf175b | f288 | `crates/reify-audit/src/lib.rs::GitOps` | medium | confirmed | δ |
| fk-bdc845122da0 | f290 | `crates/reify-audit/src/ptodo.rs::open_tasks_db` | medium | confirmed | ε |
| fk-20ce222ff90c | f289 | `crates/reify-audit/src/p5_phantom_done.rs::check_one` | medium | confirmed | η |
| fk-31eb23b6b93f | f292 | `crates/reify-audit/src/bin/reify-audit.rs::high_severity_exit_code` | medium | confirmed | θ |
| fk-422bbd7986a6 | f293 | `crates/reify-audit/src/ptodo.rs::resolve_liveness` | medium | confirmed | ν |
| fk-ba93c26e42d1 | f291 | `crates/reify-audit/src/p5_phantom_done.rs::pre_done_refusal_severity` | low | weakened | ι |
| fk-d668027f44f2 | f316 | `slug:invariant-registry-status-not-derived-from-enforcement` | medium | confirmed | ξ (INV candidate 1) |

The stream's heuristic-14 measurements carry no finding key. Their partitions and the couplings each must keep are in the record's `h14_measurements` rows:

| file | partitions | leaf |
|---|---|---|
| `src/lib.rs` | 4 | α |
| `src/ptodo.rs` | 6 | κ |
| `src/pdoccover.rs` | 5 | λ |
| `src/p5_phantom_done.rs` | 6 | μ |
| `tests/ptodo.rs` | 4 | ν |
| `tests/pdoccover.rs` | 4 | ν |
| `tests/cli.rs` | 5 | π |
| `tests/p5.rs` | 5 | π |

Invariant candidate 3 has no finding key of this stream (its keys are S23's) and lands in ο. `jcodemunch_client.rs` also measured `stays_whole: false`; only its recogniser partition is in scope (ζ; the rest is §9).

## 3. Measured premises (G3 / G6)

Each premise was re-measured at `e217c0f30b`, not inherited from the review.

- **M1 — the summary parsers.** Three production parsers read `summary`:
  - `ptodo::fingerprint`: `split_once(':')`, then a hand strip of `line <digits>: `;
  - `ptodo::is_g_allow_finding`: `starts_with("g-allow-")`;
  - `ptodo::g_allow_finding_line`: `split_once(": line ")`, a `debug_assert!`, then `unwrap_or(0)`. Every current G-allow summary carries `: line N:`, so the release-mode line-0 fallback is **latent**: it fires only for a summary written without that segment.

  On the test side, the parsers are `tests/pcite.rs::{category, name}`, `tests/pdoccover.rs::{finding_name, finding_category}` (plus `category_name_pairs`), and an inline closure in `tests/p2.rs`.
- **M2 — what `task_id` holds.** It carries a task id for P1, P2, P5, PDCHECK and PTODO's two inverse kinds; a path for PTODO's structural, liveness and g-allow lanes, PDSSENTINEL, PDIAG, PDOCCOVER, PCITE and PPRDSTATUS; and the empty string for two different things. One is the run-level P1/P5 unexamined findings (since #8441). The other is the per-symbol PDEAD/PLAYER/PUNTESTED findings, which are located at a `file:line` (`pdead_dead_code.rs`: "Empty task_id is intentional"). `pdcheck::resolve_delivered_check_paths` and `ptodo::resolve_inverse` each re-parse `task_id` as `i64` to sort. **The ruled `Subject` = `Task | Path` needs a `Run` form, and the per-symbol detectors are `Path`** (D2).
- **M3 — the kind vocabulary.**

  | detector | kinds |
  |---|---|
  | PTODO | 11 kinds in 4 lanes, plus #7098's new lane when it lands. Only the structural four are the typed `ptodo::Kind`; the rest are bare literals. |
  | PDIAG | 4 |
  | PDOCCOVER | 7 |
  | PDCHECK | 2 (`pdcheck::Verdict::kind`) |
  | PCITE | 2 |
  | PPRDSTATUS | 2 |
  | PDSSENTINEL | 1 |
  | PLINECITE (#6935) | 3 (`line-cite`, `allow-missing-reason`, `stale-baseline-entry`) |
  | P1, P2, P5, PDEAD, PLAYER, PUNTESTED | unprefixed prose |

  P5's five sub-patterns are already distinct `Pattern` variants. `Pattern` serialises as its Rust variant name, unrenamed. Summary shapes vary:
  - `<kind>: [line N: ]<text>` (PTODO, PDSSENTINEL);
  - `<kind>: <path> line N: …` (PPRDSTATUS `cite-status-contradiction`);
  - `<kind>: <cite> — <path>:<line> …` (PLINECITE);
  - per-file site lists (PDIAG `pdiag-ratchet`);
  - `{category}: {name} {detail}` (PDOCCOVER, PCITE);
  - free prose with an embedded `file:line` (P5 H2, PDEAD);
  - a `[warn-only] ` / `[advisory — …] ` prefix (P5 pre-done).

  Only PCITE emits `EvidenceRef::FileLine`.
- **M4 — the PTODO baseline is empty.** `crates/reify-audit/ptodo-baseline.txt` is 0 bytes (since `3910accacf`), so a field-derived fingerprint needs no regeneration. The live DB-absent set on main is empty too, so a parity check over the live tree alone would be vacuous; B2 uses the PTODO fixture corpus. Readers of the format:
  - `tests/infra/test_reify_audit_ptodo.sh` (`comm` in both directions);
  - `tests/ptodo_baseline.rs` (`splitn(3, " :: ")`, which also reads the `tasks_db=` token);
  - the stub literals in `tests/infra/test_reify_audit_ptodo_ratchet_superset.sh`.
- **M5 — wire consumers.** Findings are a bare JSON array written last on **stderr**, plus a human summary on stdout; every Rust field name crosses the wire verbatim. The JSON readers:
  - `scripts/pprdstatus-escalate.py` reads `finding["task_id"]`, `["summary"]` and `["pattern"]`;
  - `scripts/docs-truth-sweep.py` keys silence on `(pattern, task_id)`;
  - `scripts/reify_audit_findings.py` is shape-agnostic;
  - `tests/infra/stub_escalation_mcp.py` *synthesises* the wire shape;
  - `tests/infra/test_reify_audit_pdoccover.py` asserts on summary substrings;
  - `crates/reify-audit/tests/skill_registration_parity.rs` reads the §0 table's columns over four skill surfaces: the SKILL.md frontmatter, SKILL.md `## Modes`, modes.md `## §4 ` and cli-invocation.md `## §2 `;
  - the `/audit` skill references.

  The skill's `data/audit-runs/index.json` uses today's `task_id` string as a dedupe key. **No dark-factory code reads any JSON field:** `pre_done_hook.py` reads only `returncode` (0 allows the flip, non-zero blocks) and echoes 2,000 chars of stderr.
- **M6 — `--since` is the skill's default argv.** `.claude/skills/audit/references/modes.md` passes `--since <14d-ago>` in §1 (the default sweep) and §4; §3 is the window mode. `.claude/skills/hotspot-survey/project.md` Phase 0 runs `--pattern P1,P2,P5 --since <window>`. The only reader of `ctx.window` is P2's guard: `target_task_id.is_none() && window.is_none() && task_metadata.len() > 50` → `eprintln!` + `debug_assert!(false)`. Its KNOWN LIMITATION comment records that the loader never narrows. Deleting `window` without deleting the guard therefore makes every untargeted sweep panic in a debug build. P2's actual work on a default sweep is unchanged: it was always the whole corpus. `producer_branch` has no reader. dark-factory has no `--since` caller.
- **M7 — GitOps.** The trait's 15 methods ask 12 questions; only `log_grep`, `is_gitignored` and `path_tracked_on` have `try_` twins. After #6935 there are 8 production `ls_files()` callers, and each handles an empty result its own way:

  | caller | on an empty result |
  |---|---|
  | PDOCCOVER | raises `census-empty` High |
  | PDIAG | raises `pdiag-census-empty` High, only when its baseline has rows |
  | PTODO | exposes `ScanStats.files_scanned` |
  | PDSSENTINEL, PCITE, PPRDSTATUS | report clean |
  | PDCHECK | reads it as "every path absent" |
  | PLINECITE | applies its own scope policy |

  `p5_phantom_done::check_one` reaches git infallibly on its High path: `task_referencing_commits` (itself a `try_task_referencing_commits(..).unwrap_or_default()` wrapper) ×2, `all_files_tracked_on_main` ×2 and `path_tracked_on` ×1.
- **M8 — the tasks.db seam.** `ptodo::tasks_db_path` (which honours `REIFY_PTODO_TASKS_DB`) and `ptodo::open_tasks_db` are called from `ptodo::check_with_stats`, `pdcheck::check` and the bin. `open_tasks_db` is `open_with_flags(READ_ONLY)` with no schema probe: an existing file that is not a tasks DB opens fine and fails at prepare time. `tests/require_tasks_db_cli.rs::a_file_that_opens_but_is_not_a_tasks_db_is_refused` pins the refusal. `check_with_stats` sets `TasksDbMode::Present` only after its resolve lanes succeed. `ptodo-baseline-gen` emits `@@PTODO_SCAN@@ … tasks_db=<absent|present>`, and the merge-gate ratchet's DB-absent floor asserts that token. Only PTODO is `Check::TasksDbBacked`, so `--pattern PDCHECK --require-tasks-db` exits 125.
- **M9 — the recognisers.**
  - cfg(test) attribute recognisers, four semantics: `pdiag::is_cfg_test_attr`, `p2_consumer_stub::is_test_cfg_attr`, `pdoccover::is_cfg_test_attr`, and `jcodemunch_client::extract_suppression`'s `contains("cfg(test)")`. A fifth, the script's `CFG_TEST_RE`, belongs to #8369.
  - Test-path rules: `lib.rs::is_test_path` (it also matches `.spec.`) and `pdiag::is_swept_path` gate 4.
  - `allow(dead_code)` readers: whole-token in `ptodo`, substring in `extract_suppression`.
  - G-allow grammars, three:
    - `ptodo::g_allow_marker_body`: line-start, trims both ends;
    - `jcodemunch_client::extract_g_allow`: trims the start only, walks the contiguous attribute and comment block above a declaration;
    - `audit-orphan-producers.sh` `G_ALLOW_RE.search(lines[idx-1])`: matches anywhere, preceding line only, so a marker above `#[inline]` is invisible to it (#7066 sub-finding A(2)).
  - Region trackers: PDIAG's literal-unaware `brace_delta` and PDOCCOVER's literal-blanking `code_view`.
  - `scan_util.rs` exports five `pub(crate)` word/marker helpers.
- **M10 — P5 `check_one`.**
  - Size: 410 lines with 12 `return`s, 9 `Finding {` literals (7 Low), a tail `build_high_finding`, and 5 identical `EvidenceRef::RunsDb { "events", "task_id={} AND event_type=task_completed" }` literals.
  - The sibling-commit rescue and the deliverable-presence rescue each appear in both arms.
  - Coverage predicates, three with two semantics: `files_missing_from` (exact, used by the sweep) against `covers_path` and `task_rows::path_present_in_tracked` (both slash- and directory-aware).
- **M11 — the exit code.** `high_severity_exit_code = count.min(254)`, `ERROR_EXIT = 125` (12 return sites), `TASKS_DB_ABSENT_EXIT = 255`. Both doc comments are false about 125. The infra exit-code asserts use fixtures that produce 0 or exactly 1 High, and their retry lists treat 125 as transient infra.
- **M12 — test reach.** 22 `pub` detector items have no `src` caller outside their module. Of 71 line-start `// G-allow:` markers in `src`, 66 cite tests: 40 are test-support fixtures and mocks (`lib.rs` 25, `pdiag.rs` 15), which legitimately stay, and 26 expose detector internals to a separate test crate. #6935 adds one more, `plinecite::in_scope`. `pdiag::test_support::Fixture` (feature `test-support`) already drives `check` over a real tempdir.
- **M13 — the registries.**
  - `docs/invariants.md` has 25 rows: 13 `proposed`, 10 `enforced(…)`, 2 `partial:`. Its Enforcement column is free prose with no machine pointer.
  - 8 of the 13 `proposed` rows name a mechanism that exists. INV-FEA-1 names `Engine::new_production`, which does not exist; `register_production_compute_fns` implements it. INV-EVAL-2 names `CellEvalCtx`; the real symbol is `cell_eval_ctx`.
  - INV-GEO-4's and INV-COMP-2's text contradicts the code, and a resolver cannot see that. INV-META-1 has two clauses ("Tasks cite the INV they serve; every INV names its enforcement mechanism"); a registry checker can enforce only the second.
  - The design-invariants census table (`| Surface | Gate | Status |`) is out of step with `pattern_flag::TOKENS`, the library's token list that a bin test holds equal to `DETECTORS`:
    - PPRDSTATUS is marked chartered but ships.
    - PCITE, PDCHECK, P1/P2/P5's task-corpus surfaces and (after #6935) PLINECITE have no row.
    - PDROP, PVAC and PTYPE are named but are not tokens.
    - Two rows are `convention-only … | unenforced`.
    - It also holds a non-audit gate (`module_scc_ratchet`). #7098's branch adds a second PTODO-gated row.
  - Only INV-ARCH-1 has a `**Enforcement**:` paragraph; INV-PD-1/2 state enforcement inline, in the present tense, through patterns that do not exist.
  - No reify file parses either registry. dark-factory `scripts/legibility/invariants.py::parse_headings` parses the `## INV-… \`slug\`` **headings**, so the heading grammar is a cross-repo contract.
- **M14 — the line-cite lane exists, unlanded (#6935).** PLINECITE is a syntactic ban on comment-leading `<stem>.(rs|ri):<digit>` cites. `cite_at` accepts only `.rs`/`.ri` targets, and the measured `.sh`/`.py`/`.ts`/`.toml`/`.yaml`-target comment cites under scripts/tests/infra/hooks number 336, against 43 for `.rs`/`.ri`. Its scope (`plinecite::in_scope`) is `crates/reify-compiler/stdlib/**.ri` plus `crates/<crate>/src/**.rs`, minus `crates/reify-audit/`. #6935 ships:
  - the ledger `crates/reify-audit/plinecite-baseline.txt` (744 rows of `<path> :: <cite>`);
  - the generic `ratchet_ledger::Ledger<R>`;
  - `plinecite-baseline-gen`;
  - a decision record `docs/prds/reify-audit-stale-line-cite-detector.md`.

  It is Medium and report-only, and #8509 owns its merge-time gate. Its 21 files include `lib.rs`, `pattern_flag.rs`, the bin, `pdoccover*.rs`, `tests/cli.rs`, `tests/p5.rs` and the skill references.
- **M15 — `crates/reify-audit` has no module SCC** (`scripts/module-scc-baseline.json` has no reify-audit row). `scripts/assert-module-scc.py` sees only `crate::<module>` edges: an import through a `pub use` facade (`crate::{Finding}`) adds none, so a cycle can hide behind α's facade. Every new module must import downward in fact, not just through the checker's eye.
- **M16 — #7098 is in flight on `task/7098`.** It touches the same files as R3's case: 9 commits, `ptodo.rs` +581/-96, a new `ptodo_promise.rs`, a new lane and kind, `lib.rs`, `tests/infra/test_reify_audit_ptodo.sh` and the census.
- **M17 — the adoption-ratchet engine exists.** S19 α #8678 landed `tests/infra/adoption-ratchet.manifest`, evaluated by `tests/infra/adoption_ratchet.py` and gated by `test_infra_adoption_ratchet.sh`. A row is `pattern-id | ERE | path glob | ceiling | owner leaf`. Lines starting with `#` never count, so a Rust `#[…]` attribute line cannot be counted there.
- **M18 — pending tasks that will write registry rows:** #6701 (INV-DR family + row), #5101 (flips INV-GUI-3), #8720 (S15 π: INV-PD-2 Enforcement text + census row), #8657 (S7 ξ: INV-ARCH-1), #7102 / #7085 / #5414 (PVAC / PDROP / PTYPE tokens), #7098 (census row). None knows the pointer grammar.

## 4. Resolved design decisions

- **D1 — one kind module; Pattern derived from Kind.**
  - Every per-detector kind enum lives in one lower module, `crate::kind`, which imports nothing above `scan_util`. Its contents: `PtodoKind` (all PTODO kinds live at γ1, #7098's included), `PdiagKind`, `PdocCoverKind`, `PdeliveredCheckKind`, `PciteKind`, `PprdStatusKind`, `PlineCiteKind` (and `PregistryKind`, added by ξ), plus the crate-level `Kind` enum with one variant per `Pattern`; a single-kind pattern's variant is unit-like.
  - `Pattern` itself (serde names unchanged) moves into `crate::kind` in γ1, with `model` re-exporting it, so `kind` never imports `model`.
  - `model` (the `Finding` home) and every detector import `kind` downward, and `kind` imports no detector. This closes the model↔detector cycle that a kind enum inside each detector would create and the facade would hide (M15).
  - `Kind::pattern()` is total, so `Finding.pattern` is set by construction (heuristic 11, SPOT: the pattern and the summary prefix are two hand-kept copies of one fact today).
  - `Kind::token()` is the stable kebab string. It is today's summary prefix where one exists, and a new token for the unprefixed patterns (`producer-orphan`, `task-unexamined`, `consumer-stub`, `phantom-done`, `dead-code`, `layer-violation`, `untested`, and P5's sub-pattern names).
  - `Pattern`'s serde names and the CLI tokens are unchanged, so routing on `pattern` keeps working.
- **D2 — `Subject` has three forms: `Task(TaskId) | Path(String) | Run`.**
  - `Run` is only for findings about the run itself: #8441's run-level unexamined findings and the dispatcher's new ones.
  - PDEAD, PLAYER and PUNTESTED become `Path` with locus `Line`: each finding is about one symbol at one place (heuristic 1, informative names; the empty string today carries two meanings). Their dedupe key changes from `""` to the path, a recorded change for three opt-in Low patterns.
  - `Subject::key()` returns the string today's `task_id` holds for every other detector, so the skill's `index.json` keys and `docs-truth-sweep.json`'s `{pattern, path}` state stay valid.
  - `Subject::sort_key()` orders numeric task ids numerically and replaces the two `i64` re-parses.
- **D3 — `detail` and the rendered summary.** `detail` is the text after the kind prefix and after an optional leading `line N: ` segment, exactly what `ptodo::fingerprint` extracts today. Line-independence is required only of the fingerprinted kinds (all PTODO kinds); the other kinds may carry a location inside `detail` (PPRDSTATUS `cite-status-contradiction`, PLINECITE, PDIAG site lists, P5 H2, PDEAD).
  - `summary()` renders by the kind's declared `Layout`: `Prefixed{line: bool}` (`"{token}: [line {n}: ]{detail}"`) or `Bare` (`detail`). The P5 pre-done `[warn-only] ` / `[advisory — …] ` prefixes are part of `detail`.
  - The output is **byte-identical to today's summary for every finding** (parity contract, §6 B1). That keeps the human text, every summary-substring test assertion and the 2,000-char hook echo unchanged.
  - `fingerprint()` is `"{key} :: {token} :: {fold_whitespace(detail)}"`, byte-identical to today's for every PTODO finding (B2).
- **D4 — located findings are typed.** Every `Kind` declares its `Locus` (`Line`, `Path`, `Task`, `Run`) in `crate::kind`. Findings are built only through `Finding::at_line(kind, severity, path, line, symbol, detail)`, `::at_path`, `::for_task` and `::run_level`.
  - `at_line` makes `EvidenceRef::FileLine` the first evidence entry and sets `Subject::Path`.
  - Each constructor `assert!`s (release included) that the kind's locus matches: a programming error caught in every build, never a silent default (heuristic 10, clear invariants uniformly enforced).
  - `Finding::line()` reads the FileLine.
  - Switching from `File` to `FileLine` as primary evidence: PTODO's located kinds, PDSSENTINEL, PPRDSTATUS's `cite-status-contradiction`, PDEAD, PLAYER and PUNTESTED.
- **D5 — the wire changes once, expand then contract.** γ1 adds `kind` (serialised as `Kind::token()`, a plain string), `subject` (`{"task":"8612"}` / `{"path":"…"}` / `"run"`), `subject_key` (= `Subject::key()`, a derived string so no reader re-implements the mapping) and `detail`. It keeps a legacy `task_id` (= `subject_key`) beside them, so every out-of-crate reader keeps working. γ2 migrates every reader (M5) and deletes `task_id`. Within one PRD there are two high leaves, each green on landing.
- **D6 — `--since`, `TimeWindow`, `window`, `producer_branch` and P2's backlog guard are deleted together (ruled).** The guard's premise, "callers MUST pre-narrow", has no caller that satisfies it, and the ruling makes whole-corpus sweeps the contract. Keeping the guard would make every default sweep a debug-build panic (M6).
  - The skill's default and §4 argv drop `--since` and the 14-day pre-flight, and §3 is deleted. The remaining sections keep their numbers, so §4 stays §4 for `skill_registration_parity.rs`.
  - The artefact's `scope.window` becomes `scope.corpus: "all"`.
  - `hotspot-survey`'s Phase 0 drops `--since`.
- **D7 — AuditContext is built, not spelled.** `AuditContext::new(project_root, conn, git, jcodemunch)` plus consuming `with_tasks`, `targeting` and `at_time` (β). `with_tasks_db` comes with ε and `with_pre_done_posture` with ι. Fields become private and detectors use accessors (heuristic 9, deep modules with narrow interfaces; heuristic 5, minimum data access scope).
- **D8 — every GitOps question is fallible.** Each trait method returns `Result<T, GitError>`, with `GitError { command, stderr }` structured (heuristic 12). The fail-safe default methods and the infallible P5 wrappers (`task_referencing_commits`, `all_files_tracked_on_main`) are deleted, and each call site states its own policy. `ls_files()` returns `Result<TrackedFiles, EnumerationError>`, and `TrackedFiles` cannot be built from a failure or an empty listing (`EnumerationError::{Git(GitError), Empty}`). `MockGitOps` and every other implementor follow; `MockGitOps::fail(method)` injects failures.
- **D9 — the dispatcher owns degenerate inputs.** Each `Detector` row declares its check shape and `reads_tasks_db`, beside the existing `jcodemunch` use:
  - `Check::Corpus(fn(&AuditContext) -> DetectorOutput)`, or
  - `Check::Tree(fn(&AuditContext, &TrackedFiles) -> DetectorOutput)`.

  The bin enumerates once per run, only when a selected row is `Tree`. Rules, in order: (1) an empty task corpus skips a `refuses_empty_task_corpus` row, as today; (2) a failed enumeration replaces every remaining `Tree` row's run with one run-level High `Kind::Unexamined{pattern}`, the cause text in `detail` (#8441's unexamined precedent). A `Tree` detector cannot be called without a successful enumeration (heuristic 7, calls carry what they need).

  The four emptiness policies over `ls_files` are deleted, because an empty enumeration can no longer reach a detector. They are PDSSENTINEL's, PCITE's and PPRDSTATUS's silent clean, and PDCHECK's reading of an empty set as "every path absent".

  Three things stay, because they are not policies over a failed enumeration. PDOCCOVER's zero-registries vacuity and PDIAG's zero-sites-against-a-non-empty-baseline vacuity are floors about the detector's own corpus. PTODO's `ScanStats` (`files_scanned`, `markers_examined`) is run evidence. `ptodo::check_with_stats` keeps returning it beside its `DetectorOutput`, and `ptodo-baseline-gen` keeps calling it directly.

  The baseline generators call the same `Tree` functions with their own `TrackedFiles` and exit non-zero on an `EnumerationError`.
- **D10 — one schema-probed tasks.db handle; degradation is computed in one place.** `task_rows::TasksDb::open(project_root)` honours `REIFY_PTODO_TASKS_DB`, opens read-only and runs a schema probe (`SELECT 1 FROM tasks LIMIT 1`). It returns `Open{path, conn}`, or `Unavailable{path, cause}` on an absent file, an open error or a probe failure. The bin opens it once.
  - `DetectorOutput { findings, tasks_db: DbUse }`, where `DbUse` is `NotRead | Read | Failed`. A DB-reading detector reports a mid-run query failure as `Failed`; nothing else about degradation is detector-private.
  - The dispatcher derives `tasks_db_degraded = row.reads_tasks_db && (ctx.tasks_db().is_unavailable() || output.tasks_db == Failed)`.
  - `--require-tasks-db` guards any run set containing a `reads_tasks_db` row, so PDCHECK qualifies. `ptodo-baseline-gen` keeps emitting `tasks_db=<absent|present>`, now derived from the same rule, so the whole `@@PTODO_SCAN@@ files_scanned=… markers_examined=… tasks_db=…` line is byte-unchanged.
  - `ptodo::TasksDbMode` and `Check::TasksDbBacked` are deleted, and PDCHECK's private stderr line gives way to the dispatcher's uniform breadcrumb.
  - The §8.2 cite grammar moves to `crate::cite_grammar`, which PTODO and PPRDSTATUS import downward.
- **D11 — one source-shape vocabulary with named policies.** `crate::source_shape` lives beside `scan_util` and imports only it:
  - `CodeView`: comment and literal masking, with a per-extension comment leader (`//` for `.rs`, `.ri`, `.ts`, `.tsx`; `#` for `.sh`, `.py`, `.toml`, `.yaml`).
  - `CfgGate::classify(attr) -> CfgGate`: `Test`, `AnyWithTest`, `AllWithTest`, `TestFeature`, `NotTest` or `Other`.
  - Named `TestRegionPolicy` consts, and `test_regions(content, policy, BraceMode::{LiteralAware, LineGranular})`.
  - `allow_dead_code_attr` (whole-token).
  - One G-allow grammar: `g_allow_body` for the line form, and `g_allow_marker_for(decl_lines)` for the positional walk-up policy through contiguous attribute and comment lines, triple-slash included. #7066's sub-findings A(1)–A(3) are pinned.
  - `is_test_path`: one rule, the union of today's two.
  - `declares_item(content, path_item)` for PREGISTRY.

  A detector that needs a different policy names it as a parameter and never copies the recogniser. Each adoption runs that detector over the live tree before and after and records the finding delta in the commit. A delta is either justified (it fixes a false positive or negative the finding names) or the policy is chosen to preserve today's output. `scripts/audit-orphan-producers.sh` adopts the same line grammar *and* positional rule, pinned by one shared fixture corpus that a Rust test and a Python infra test both read. Its `CFG_TEST_RE` stays #8369's.
- **D12 — P5's sweep is a list of rescue steps.** Each rescue is one `fn(&Claim, &CheckCtx) -> Option<Verdict>`. The merged/no-event arm and the git-diff leg compose ordered lists from the same step functions, so each rescue is written once. One `verdict_finding` renderer and one `runs_db_completion_evidence(task_id)` build every Finding. A single `covers(declared, landed)` predicate (trailing-slash and directory aware) replaces `files_missing_from` and `covers_path`, and `task_rows::path_present_in_tracked` delegates to it. The sweep's trailing-slash false positive closes, with the live delta recorded.
- **D13 — the exit code saturates at 124.** `high_severity_exit_code = count.min(124)`:

  | code | meaning |
  |---|---|
  | 0 | clean |
  | 1–123 | the exact High count |
  | 124 | ≥ 124 (the JSON array is authoritative) |
  | 125 | always an infra error |
  | 255 | tasks.db unusable under `--require-tasks-db` |

  This is the minimal injective channel. Every existing exit-code assert keeps passing (fixtures produce 0 or 1 High), dark-factory's hook is unaffected, and skill §3.1's 125 disambiguation is deleted rather than rewritten.
- **D14 — the pre-done posture is carried in AuditContext** (the ruled "one pre-done posture carried in AuditContext", read at weakened weight: the skeptic showed the two knobs gate different axes).
  - `PreDonePosture::{Refuse, WarnOnly}` is resolved once by the bin from `REIFY_AUDIT_PREDONE_WARN_ONLY`, and P5 reads `ctx.pre_done_posture()`.
  - Every `--pre-done` run prints one stderr line, `reify-audit: pre-done posture: refusal=<refuse|warn-only>`.
  - `scripts/reify-audit-freshness.sh` prints its own `freshness=<strict|fail-open>` line once per run. Binary freshness is decided before the binary runs, so it cannot live in the binary's context.
  - The deploy script's probe messages name both knobs. Both effective values are visible once per run, which is the finding's actual hazard. `hooks/main-gate-lib.sh::gate_switch_on` is not adopted: it answers enforce/bypass for hooks, not a severity.
- **D15 — the registry pointer grammar** (§5.7). Each `docs/invariants.md` row gains a `Pointer` cell of `;`-separated typed pointers to the thing that **fails when the invariant is violated**. The mechanism prose stays in Enforcement.
  - Pointer forms: `test:<.rs path>[::<fn>]` or `test:tests/infra/<test_*.sh>` (a discoverable wrapper, never a bare `.py`, per the CLAUDE.md wrapper trap), `item:<path>::<Item>[.<method>]` (the program's `path::Type.method` convention), `script:<path>`, `audit:<token>`, `planned:#NNNN` and `pending-owner:#NNNN`.
  - The census `Gate` column takes the same grammar plus the literal `convention` for `unenforced` rows.
  - Where a design-invariants entry states enforcement, ξ lifts it into a `**Enforcement**:` paragraph in the same grammar. Headings are untouched (M13).
  - PREGISTRY's structural lane gates the merge DB-absent, PTODO-style. ξ waits until every owning stream has filed, so every `pending-owner:` cites a real owning leaf (ruling R4). Its liveness lane (`planned:` / `pending-owner:` cites against tasks.db) runs as a member of the weekly docs-truth sweep and reports Medium, so `pending-owner:` exceptions really do expire (ruling R2).
  - Content contradictions a resolver cannot see (GEO-4, COMP-2) are corrected by hand in the migration diff, or handed to the owning stream by cross-reference.
  - INV-META-1 becomes `partial: enforced(audit) for registry status, proposed for task INV-cites`: the checker enforces the second clause only.
- **D16 — the line-cite lane is PLINECITE (ruling R1).** ο depends on #6935. It subsumes #8509 (the merge-time gate) and #7302 (whose semantic path+range guard the #6935 decision record argues against).
  - Recognised cite targets widen from `.rs`/`.ri` to the checkable form's set: `.rs .ri .sh .py .ts .tsx .toml .yaml .yml`.
  - `plinecite::in_scope` widens to tracked code comments:
    - `crates/*/{src,tests,benches}/**.rs`, `build.rs` and `crates/**/*.ri`;
    - `scripts/**/*.{sh,py}` and `tests/infra/**/*.{sh,py}`;
    - `hooks/*`;
    - `gui/src/**/*.{ts,tsx}` and `gui/src-tauri/src/**/*.rs`.
  - **Excluded:** `crates/reify-audit/` (self-match, as #6935 decided); `docs/**`, `*.md`, `tests/prd-gate/**` and every other `.ri` outside `crates/`. Those land through hook-gated docs commits that never select the PLINECITE test, and R1 keeps docs under the overlay's dated-anchor rule.
  - Comment leaders come from `source_shape::CodeView`.
  - The gate is one-directional. A cite absent from the ledger is High (`line-cite`). A ledger row with no live cite is Medium (`stale-baseline-entry`, #6935's token) and is pruned by `plinecite-baseline-gen`, following the `tests/infra/cited-test-path-baseline.manifest` precedent. S23's burn-down therefore never reds a merge, while new debt always does.
  - `plinecite-baseline-gen --rename <old>=<new>` carries a moved file's rows. The gate's High message names it, so a wave-2/3 stream that moves files is not charged new debt.
  - INV-3 is promoted as **INV-DOC-1**.
- **D17 — S24 lands after both in-flight reify-audit branches (ruling R3, applied to #7098 by the same reasoning).** α depends on #6935 and #7098, so neither finished branch has to rebase over the lib.rs split and the Finding change. Every other leaf follows α. Priority inheritance lifts both low-priority tasks once high S24 leaves depend on them.
- **D18 — ratchets use the shared engine.** Where a ratchet is a line count of an ERE over a path glob, it is a row in `tests/infra/adoption-ratchet.manifest` at its post-extraction count, named in the leaf's done-criteria (INV candidate 5). The engine expands globs with `pathlib` (`*` does not cross `/`), so every S24 row uses `crates/reify-audit/src/**/*.rs` to keep matching after κ/λ/μ move files into subdirectories. A ratchet the engine cannot express is a Rust test in `crates/reify-audit/tests/`. That covers a `#[…]` attribute line (M17), a trait-shape property and region-scoped counts.

## 5. Contract (H)

### 5.1 Finding (`crate::model`, kinds in `crate::kind`)

```rust
// crate::kind — imports nothing above scan_util
pub enum Locus { Line, Path, Task, Run }
pub enum Layout { Prefixed { line: bool }, Bare }
pub enum PtodoKind { Untracked, MalformedCite, PhantomTracking, BareIgnore, Orphaned, ParkedOnAnchor, UnknownId,
                     GAllowOrphaned, GAllowUnknownId, TaskCitesDeletedPath, TaskCitesRenamedPath, /* + #7098's */ }
// … PdiagKind, PdocCoverKind, PdeliveredCheckKind, PciteKind, PprdStatusKind, PlineCiteKind
pub enum Kind { PTodo(PtodoKind), PDiag(PdiagKind), …, PDsSentinel, P1ProducerOrphan, P1TaskUnexamined,
                P2ConsumerStub, P5PhantomDone, …, Unexamined { pattern: Pattern } }
impl Kind { pub fn pattern(&self) -> Pattern; pub fn token(&self) -> &'static str;
            pub fn locus(&self) -> Locus; pub fn layout(&self) -> Layout; pub fn all() -> impl Iterator<Item = Kind>; }

// crate::model
pub enum Subject { Task(TaskId), Path(String), Run }   // serde: {"task":"8612"} | {"path":"…"} | "run"
pub struct Finding { /* private */ pattern, kind, severity, subject, detail, evidence }
impl Finding {
    pub fn at_line(kind, severity, path, line, symbol: Option<String>, detail) -> Self;  // assert!(kind.locus()==Line)
    pub fn at_path(kind, severity, path, detail) -> Self;
    pub fn for_task(kind, severity, task_id, detail) -> Self;
    pub fn run_level(kind, severity, detail) -> Self;
    pub fn with_evidence(self, e: EvidenceRef) -> Self;
    pub fn pattern(&self) / kind() / severity() / subject() / detail() / evidence() / line() -> Option<usize>;
    pub fn summary(&self) -> String;      // rendered; byte-identical to today's text (B1)
    pub fn fingerprint(&self) -> String;  // "{key} :: {token} :: {fold(detail)}" (B2)
    pub fn sort_key(&self) -> SortKey;    // (pattern, subject.sort_key(), line, token, detail)
}
```

JSON after γ2: `{pattern, kind, severity, subject, subject_key, summary, detail, evidence}`. `kind` is the token string, and `subject_key` is derived. `summary` stays on the wire as rendered text for humans and for the hook echo, and no reader parses it. Deserialisation (the CLI tests read the array back) resolves `kind` within `pattern` and rejects a mismatch.

### 5.2 AuditContext

`AuditContext::new(project_root, conn: &Connection, git: &dyn GitOps, jcodemunch: &dyn JCodemunchOps)`, with the builders `with_tasks(HashMap<String, TaskMetadata>)`, `targeting(task_id)`, `at_time(epoch)`, `with_tasks_db(&TasksDb)` (ε) and `with_pre_done_posture(PreDonePosture)` (ι). All fields are private, read through `project_root()`, `git()`, `jcodemunch()`, `conn()`, `task_metadata()`, `target_task_id()`, `now()`, `tasks_db()`, `pre_done_posture()` and `read_relative`. There is no `window` and no `producer_branch`.

### 5.3 GitOps

12 methods, each `-> Result<T, GitError>`. `ls_files(&self) -> Result<TrackedFiles, EnumerationError>`; `TrackedFiles` exposes `paths() -> &[String]`, `contains`, `covers(declared)`, and is non-empty by construction. `RealGitOps` maps a spawn error or a non-zero exit to `GitError{command, stderr}`, keeping #4800's transient-spawn resilience. `MockGitOps::fail(method)` injects a failure.

### 5.4 Detector row and dispatcher

```text
Detector { token, in_default_sweep, refuses_empty_task_corpus, check: Check, jcodemunch: JcodemunchUse, reads_tasks_db: bool }
Check::Corpus(fn(&AuditContext) -> DetectorOutput) | Check::Tree(fn(&AuditContext, &TrackedFiles) -> DetectorOutput)
DetectorOutput { findings: Vec<Finding>, tasks_db: DbUse }      // DbUse::{NotRead, Read, Failed}
DetectorRun { findings, unexamined: bool, tasks_db_degraded: bool }
```

Rule order:

1. A row that refuses an empty corpus is skipped on one (unchanged).
2. A `Tree` row runs only with `Ok(TrackedFiles)`. On `Err`, it yields exactly one `Finding::run_level(Kind::Unexamined{pattern}, High, <cause>)`.
3. `tasks_db_degraded` is computed by D10's single rule.
4. `--require-tasks-db` refuses with 125 when no selected row `reads_tasks_db`, and exits 255 when a selected row degraded.

`DETECTORS` stays in the bin. Library code that needs the token set (PREGISTRY's `audit:` resolver and census rule) reads `pattern_flag::TOKENS`, which the bin's existing unit test holds equal to `DETECTORS`.

### 5.5 Exit codes

| code | meaning |
|---|---|
| 0 | no High finding |
| 1–123 | exact High count |
| 124 | ≥ 124 High findings (count in the JSON) |
| 125 | infra/setup error; no findings array is written |
| 255 | `--require-tasks-db` and a DB-reading lane could not use tasks.db |

126–254 are never produced. A unit test pins counts 0, 1, 123, 124, 125, 254, 255 and 300.

### 5.6 source_shape

`CfgGate::classify(&str) -> CfgGate`, plus `pub const` policies named for the question they answer:

| policy | accepts | used by |
|---|---|---|
| `TEST_ONLY_STRICT` | exact `#[cfg(test)]` | PDOCCOVER |
| `TEST_OR_ANY_TEST` | adds `any(test, …)` | PDIAG |
| `TEST_OR_TEST_FEATURE` | adds `all(test, …)` and `feature = "test-*"` | P2 |

jcodemunch suppression adopts one of these, recorded by ζ with its delta.

The other functions:
- `CodeView::new(&str, ext)`: masked code per line plus a comment flag and comment text.
- `test_regions(&str, policy, BraceMode) -> Vec<Range<usize>>`.
- `allow_dead_code_attr(&str) -> Option<&str>`.
- `g_allow_body(&str) -> Option<&str>` and `g_allow_marker_for(&[&str]) -> Option<&str>`.
- `is_test_path(&str) -> bool`.
- `declares_item(content, "Name" | "Type.method") -> bool`: fn, struct, enum, trait, type, const, static, mod or `macro_rules!`; a method resolves only inside an `impl` of the named type.

### 5.7 Registry row grammar and PREGISTRY

`docs/invariants.md` columns become `| ID | Invariant | Enforcement | Pointer | Status | Owner (expected PRD) |`. A Pointer entry is one of `test:…`, `item:…`, `script:…` or `audit:<token>` (the **enforcement pointers**), or a `planned:#NNNN` / `pending-owner:#NNNN` cite. The Status grammar is unchanged, and the usage contract gains the pointer rule.

The census table keeps `| Surface | Gate | Status |`. Status is `shipped | chartered | unenforced`. A Gate cell is `;`-separated entries: an enforcement pointer for `shipped`, `audit:<token>; planned:#NNNN` for `chartered` (the token is not yet in `TOKENS`), and `convention` (optionally with `planned:`) for `unenforced`. A design-invariants `**Enforcement**:` paragraph whose only entry is `planned:#NNNN` is legal; the liveness lane alone checks it.

**Structural lane** (gate, DB-absent):

| rule | kind | severity |
|---|---|---|
| `enforced`/`partial` row with no enforcement pointer, or one that does not resolve | `pointer-unresolved` | High |
| `proposed` row whose enforcement pointer resolves, with no `pending-owner:` | `status-behind-enforcement` | High |
| malformed Pointer, Status or Gate cell; a `pending-owner:`/`planned:` that is not a well-formed `#NNNN` | `malformed-row` | High |
| census `shipped` row whose Gate pointer does not resolve; `chartered` row whose `audit:` token is in `TOKENS`; `unenforced` row whose Gate is not `convention` | `census-status-wrong` | High |
| a `TOKENS` member named by no census row (several rows may name one token) | `census-row-missing` | High |
| a design-invariants `**Enforcement**:` paragraph whose pointer does not resolve | `pointer-unresolved` | High |
| `proposed` row or `chartered` census row with no `planned:` | `proposed-unowned` | Medium |

**Liveness lane** (DB-present; a `scripts/docs-truth-sweep.py` member): `planned:` / `pending-owner:` cites whose task is terminal or unknown → `cite-terminal` / `cite-unknown`, Medium. It classifies through `task_rows` (`TasksDb`, `is_terminal_status`, a status lookup) and `cite_grammar`, and never imports `crate::ptodo`. A `pending-owner:` passes the structural lane on syntax alone, and its liveness makes it expire (R2).

Gate: `tests/infra/test_reify_audit_pregistry.py`, its `.sh` wrapper and a `run-all-classification.manifest` row. `scripts/verify-pipeline-infra-tests.txt` gains rows mapping `docs/invariants.md` and `docs/legibility/design-invariants.md` to the wrapper, so a docs-only direct commit runs it through the pre-commit's selective infra subset; `docs/notes/*.md` keys are the precedent, and no `scripts/verify.sh` edit is needed. **The wrapper never skips.** It refreshes the binary through `reify_audit_guard` rebuild and reds on any non-zero guard result, rather than taking `RATCHET_SKIP` (the hole PTODO closed only through `selected_infra_leaf_env`, a `verify.sh` edit this PRD may not make).

### 5.8 PLINECITE gate

`in_scope` and the cite-target set widen per D16, with comment leaders from `CodeView`. A ledger row is `<path> :: <cite>` with no line number, as #6935 defines it. `check()` against the committed ledger: a live cite not in the ledger is `line-cite` High; a ledger row with no live cite is `stale-baseline-entry` Medium. `plinecite-baseline-gen` prunes stale rows by default. `--admit-new` adds rows, and the commit must say why. `--rename <old>=<new>` rewrites the path column of a moved file's rows. The gate is `tests/infra/test_reify_audit_plinecite.py`, its `.sh` wrapper and a manifest row, run on every merge. The `plinecite:allow — <reason>` escape is unchanged.

## 6. Boundary-test sketch (both directions)

| # | scenario | preconditions | postconditions |
|---|---|---|---|
| B1 | summary parity | every detector's fixtures + the live tree, at γ1 | `finding.summary()` == the pre-γ1 summary string for every finding (recorded corpus diff empty) |
| B2 | fingerprint parity | `tests/ptodo.rs`'s scenario set, with its inline DB fixtures, plus the live tree | `Finding::fingerprint()` set == the deleted `ptodo::fingerprint` set over the same findings (recorded before deletion), and every `PtodoKind::all()` member occurs at least once (the check cannot be vacuous) |
| B3 | locus invariant | a `Line` kind passed to `at_path` | panics in both the debug and the release test profiles |
| B4 | sort from fields | two g-allow findings at lines 9 and 10 whose `detail` contains no `line N` text | sorted 9, 10 from `Finding::line()` (the latent line-0 path of M1 cannot occur) |
| B5 | wire expand/contract | `stub_escalation_mcp.py`, `pprdstatus-escalate.py`, `docs-truth-sweep.py` against the real binary | γ1: unchanged results with legacy `task_id`; γ2: same results reading `subject_key`, and no `task_id` key |
| B6 | `--since` gone | `reify-audit --since 2026-01-01` | exit 125, `unknown argument`; a debug-build default sweep over >50 tasks does not panic |
| B7 | enumeration failure | dispatcher unit test in the bin: `MockGitOps::fail(LsFiles)`, rows P1, PTODO, PCITE, PPRDSTATUS, non-empty corpus | one High `Unexamined` per tree row (3), P1 runs, exit 3 |
| B8 | PDCHECK DB guard | `--pattern PDCHECK --require-tasks-db` with `REIFY_PTODO_TASKS_DB` set to (a) a missing path, (b) an empty file | exit 255 in both; `a_file_that_opens_but_is_not_a_tasks_db_is_refused` still passes; with a real DB, exit 0 |
| B9 | recogniser policies | the shared attribute corpus (`#[cfg(test)]`, `any(test, x)`, `all(test, x)`, `feature = "test-fixtures"`, `not(test)`) through every policy const | each verdict table matches §5.6; each detector's live delta is recorded in ζ's commit |
| B10 | G-allow cross-language parity | shared corpus: marker lines, a marker above `#[inline]`, a `///` marker, a trailing marker | Rust `g_allow_body`/`g_allow_marker_for` and the script agree on every case |
| B11 | P5 rescue order | the existing `tests/p5.rs` corpus | identical verdicts to pre-η, except the trailing-slash fixture, which is now covered |
| B12 | exit saturation | 0/1/123/124/125/254/255/300 High findings | 0/1/123/124/124/124/124/124; infra errors 125 |
| B13 | posture visibility | `--pre-done` with and without `REIFY_AUDIT_PREDONE_WARN_ONLY=1` | one `pre-done posture:` stderr line naming the effective value; the refusal severity follows it |
| B14 | registry checker | hermetic 5-row registry: enforced→missing test; proposed→existing fn; enforced→resolving `item:…::Type.method`; proposed + `pending-owner:#1`; `test:` naming a bare `.py` under tests/infra | High, High, clean, clean, High (structural); the liveness lane flags `#1` when it is terminal |
| B15 | census derivation | a token no row names; a `chartered` row naming a shipped token; two rows naming PTODO; a `chartered` row `audit:PDROP; planned:#7085`; an `**Enforcement**: planned:#7085` paragraph | `census-row-missing`, `census-status-wrong`, clean, clean, clean (structural) |
| B16 | registry gate on a docs commit | stage a pointer-breaking edit to `docs/invariants.md`, run `verify.sh --scope staged --include-infra`, once with a fresh binary and once with a stale-mtime binary | the plan selects `test_reify_audit_pregistry.sh`; it reds in both runs (never skips green) |
| B17 | line-cite ratchet | add `# see foo.rs:12` to a scoped `.sh` and `// see foo.sh:3` to a scoped `.rs`; delete a ledgered cite; `--rename` a ledgered file | two High `line-cite`; one Medium `stale-baseline-entry`; after `--rename` the moved file adds no High |
| B18 | consumer side of the skill | `skill_registration_parity.rs` over the updated §0 table and the four invocation surfaces with PREGISTRY added | passes |

## 7. Cross-PRD / cross-stream relationship (G4)

| Other | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `.claude/skills/audit/` (`SKILL.md`, `references/{severity-routing,cli-invocation,modes,output-format}.md`) | produces | Finding fields, exit codes, modes, pattern registration | **S24** (program G4 row) | β, γ2, θ, ξ, ο edit them |
| `crates/reify-audit/ptodo-baseline.txt` fingerprint format | produces | `Finding::fingerprint` | **S24** | γ1 (byte-identical, B2) |
| `docs/invariants.md`, `docs/legibility/design-invariants.md` row format | produces | §5.7 grammar | **S24** | ξ migrates every row, including S3's landed INV-ARCH-1 row |
| status cells owned elsewhere (S4 EVAL-1/2/3, S1 FEA-1/BUILD-1, S10 GEO-1..4, S25 GUI-2; filed owners #5023 EVAL-6, #5101 GUI-3, #8657 ARCH-1) | consumes | `pending-owner:#NNNN` | the owning streams | ξ is held until S4/S1/S10/S25 record their leaf ids on it (R4); owners flip and delete their tag (R2) |
| pending registry writers (M18) | consumes | §5.7 grammar | those tasks | descriptions amended at decompose; PREGISTRY's messages name the usage contract |
| S23 prose rehoming | produces for | PLINECITE ledger (burn-down target), PREGISTRY (keeps cells green) | S24 builds; S23 burns down | ο and ξ upstream of S23 |
| every stream that moves a file under PLINECITE scope, or a symbol named by a registry pointer (S1–S4, S7–S10, S12, S25, S26 …) | consumes | `plinecite-baseline-gen --rename`; pointer edit in the same diff | the moving stream | cross-reference lines (result.md) |
| #6935 PLINECITE, #7098 PTODO δ-C (in flight) | consumes | `plinecite.rs`, `ratchet_ledger::Ledger<R>`, ledger; PTODO lane δ-C | those tasks | α depends on both (R3, D17); ο also on #6935 |
| S19 run-all harness | consumes | `run-all-classification.manifest` rows (today's format); `tests/infra/adoption-ratchet.manifest` rows (D18); the Python test policy; S19 λ's `tests/common/schema.rs` | S19 | ξ, ο and ζ add rows; ν rebases over λ (file locks serialise) |
| S18 verify.sh | consumes | none: gate registration uses `verify-pipeline-infra-tests.txt` rows; no `verify.sh` edit | S18 | conforms to the no-bash-edits amendment |
| dark-factory `pre_done_hook.py` | produces | exit 0 vs non-zero | S24 | unchanged (B12) |
| dark-factory `scripts/legibility/invariants.py::parse_headings` | produces | design-invariants.md heading grammar | S24 (must not change) | ξ touches cells and paragraphs only |
| `docs/prds/reify-audit-ptodo-detector.md` §8 | consumes | PTODO kind taxonomy (`crate::kind::PtodoKind`) | unchanged vocabulary | ε updates its module pointer; dark-factory `config_census_ignore.py` copies the tokens |

## 8. Decomposition plan

Task ids were assigned at decompose (pass 2, 2026-10-13). ξ (#8821) is a held leaf: deferred with `x_seam_wait`, outside this batch's `commit_planning`, released by the lead once S4, S1, S10 and S25 have recorded their owning leaf ids on it (ruling R4). Subsumed and cancelled with `x_subsumed_by`: #7066 and #8199 → ζ, #8516 → ε, #8200 → κ, #7988 → λ, #8365 → μ, #7740 → δ, #8509 and #7302 → ο. Every row's stamps follow the program's shared conventions (`metadata.x_finding_key`, `x_finding_run = review-all-reify-20261010`, `source = review-all`, priority = severity; an h14 leaf with no key is low). Three rules hold for every leaf:

- `python3 scripts/assert-module-scc.py check` stays green, and every new module imports downward in fact (M15).
- Every new `tests/infra` test is Python stdlib `unittest` with a `.sh` wrapper and its `run-all-classification.manifest` row in the same diff (overlay "Gate-test drift-guard registration").
- `metadata.files` at filing lists explicit files or `[]`, never a directory or glob.

Counts below are post-#6935/#7098 where those branches change them; re-measure at implementation time.

- **α (#8806) — Partition `lib.rs` into `model.rs`, `git_ops.rs` and `jcodemunch_ops.rs` behind a `pub use` facade** (low; no key; h14).
  - Pure move along the measured partition.
  - `is_symbol_suppressed` and `wholly_unlocatable_count` (they take `ChangedSymbol`) move into `jcodemunch_ops.rs`; `is_test_path` waits for ζ.
  - `reify_audit::Finding` etc. keep resolving for every test file and bin.

  Files: `crates/reify-audit/src/lib.rs`, `src/model.rs`, `src/git_ops.rs`, `src/jcodemunch_ops.rs`. **Signal:** `cargo test -p reify-audit` lists the same test-id set before and after (both recorded in the commit); `lib.rs` defines no `struct`/`enum`/`trait` beyond re-exports and the crate doc. Prereqs: **#6935, #7098** (R3, D17).
- **β (#8807) — `AuditContext::new` + builder; delete `--since`, `TimeWindow`, `window`, `producer_branch` and P2's backlog guard** (high; keys [fk-0b1d530f0264]). Implements D6, D7 and §5.2, with the builders that exist at β (`with_tasks`, `targeting`, `at_time`).
  - Rewrite the 146 struct literals.
  - Bin: delete the `--since` parse arm, the usage lines, the `--pre-done`+`--since` conflict and the window build.
  - Delete the guard, `tests/p2.rs::sweep_mode_with_window_scoping_does_not_panic` and the guard clauses of the remaining guard tests.
  - Rewrite each of the 8 `tests/cli.rs` `--since` cases to its real subject.
  - Skill: delete `modes.md` §3 without renumbering §4; drop the default/§4 argv and 14-day pre-flight; update the `output-format.md` scope object and `SKILL.md`'s trigger and mode rows.
  - Outside the skill: `hotspot-survey/project.md` Phase 0, the `scripts/reify-audit-predone-wrapper.sh` usage text, and one line in `docs/architecture-audit/f-infra-design.md` §10.

  Files: `crates/reify-audit/src/model.rs`, `src/bin/reify-audit.rs`, `src/bin/{ptodo,pdiag,pdoccover,plinecite}-baseline-gen.rs`, `src/p2_consumer_stub.rs`, the test files holding literals (enumerated at filing from `git grep -l 'AuditContext {'`), `.claude/skills/audit/SKILL.md`, `.claude/skills/audit/references/{modes,output-format,cli-invocation}.md`, `.claude/skills/hotspot-survey/project.md`, `scripts/reify-audit-predone-wrapper.sh`, `docs/architecture-audit/f-infra-design.md`. **Signal:** B6. **Ratchet (D18):** an `adoption-ratchet.manifest` row `auditcontext-struct-literal | AuditContext [{] | crates/reify-audit/**/*.rs | <post-β count, 1 (the constructor)> | β`. Prereqs: α.
- **γ1 (#8808) — Typed `Finding`: `crate::kind`, `Subject`, `detail`, located constructors; summary rendered byte-identically; fingerprint and sort keys from fields** (high; keys [] — the in-crate half of fk-33f9891ad16c, whose key closes on γ2). Implements D1–D5 and §5.1 for every producer, with the legacy `task_id` mirror kept on the wire.
  - Delete `ptodo::fingerprint`'s parser, `is_g_allow_finding`, `g_allow_finding_line` and the two `i64` re-parses.
  - `ptodo-baseline-gen` filters on `Subject::Path` and on kind.
  - Rewrite test assertions that read `task_id`; summary-substring assertions keep passing by B1.

  Files: `crates/reify-audit/src/{kind,model}.rs` plus every detector, bin and test file (`metadata.files` is `[]` at filing; the architect acquires the footprint). **Signal:** B1, B2, B3, B4. **Ratchet (D18):** a Rust test asserts that no production region in `crates/reify-audit/src` calls `.summary()` outside the renderer, serialisation and `print_summary` (today 3 parsers). An ERE cannot scope the production region, so this is not a manifest row. Prereqs: β.
- **γ2 (#8809) — Finding wire contract: every reader moves to `kind`/`subject_key`; the legacy `task_id` mirror is deleted** (high; keys [fk-33f9891ad16c]). Readers to migrate:
  - `scripts/pprdstatus-escalate.py` and `scripts/docs-truth-sweep.py` (silence key from `subject_key`; persisted state unchanged by D2), plus the `scripts/reify_audit_findings.py` docstring;
  - `tests/infra/stub_escalation_mcp.py`, `tests/infra/test_reify_audit_pdoccover.py` and the `tests/infra/test_smoke_predone_hook.sh` stub;
  - the test-side summary parsers;
  - `crates/reify-audit/tests/skill_registration_parity.rs`;
  - `severity-routing.md` §0 (a `subject` column), §1, §2 and §3: titles take `<kind>` from `finding.kind` and `<n>` from `finding.evidence[0].FileLine.line`, and the dedupe `symbol_or_path` reads FileLine;
  - the "kind is its summary prefix" and "task_id is the path" sentences in `modes.md`, `output-format.md` and `SKILL.md`, plus `SKILL.md`'s stale "5 fingerprints in ptodo-baseline.txt".

  Files: the above. **Signal:** B5 (γ2 half), B18; `/audit --pattern PTODO --format markdown` renders kind and subject columns. Prereqs: γ1.
- **δ (#8810) — Fallible GitOps, `TrackedFiles`, dispatcher-owned enumeration** (medium; keys [fk-c45888cf175b]; subsumes #7740). Implements D8, D9, §5.3 and §5.4.
  - Convert every caller, including P5's infallible High-path calls, which fail to a Low "uncorroborated: git failed" verdict as the pre-done gate already does, and delete the infallible wrappers.
  - Convert PLINECITE and the four baseline generators.
  - Delete the four emptiness policies over `ls_files`.
  - Converge PDIAG's two hand-rolled reads onto `read_relative` (#7740).

  Files: `crates/reify-audit/src/{git_ops,model,ptodo,pdssentinel,pcite,pprdstatus,pdcheck,pdiag,pdoccover,plinecite,p5_phantom_done}.rs`, `src/bin/reify-audit.rs`, `src/bin/{ptodo,pdiag,pdoccover,plinecite}-baseline-gen.rs`, `tests/real_git_ops.rs`, `tests/cli.rs`. **Signal:** B7; `reify-audit --pattern PCITE --project-root <a non-repo dir>` prints one High `unexamined` finding, not a clean `[]`. **Ratchet (D18):** a Rust test asserts that the `GitOps` trait declares no method with a default body (today 3) and no non-`Result` return type. Prereqs: γ1.
- **ε (#8811) — One schema-probed tasks.db handle (`task_rows::TasksDb`), `DetectorOutput.tasks_db`, and `cite_grammar`** (medium; keys [fk-bdc845122da0]; subsumes #8516). Implements D10.
  - Move `tasks_db_path`/`open_tasks_db` to `task_rows`, adding the schema probe.
  - Delete `TasksDbMode` and `Check::TasksDbBacked`; PDCHECK becomes a `reads_tasks_db` row.
  - `ptodo-baseline-gen` keeps the `tasks_db=` token.
  - Move the §8.2 cite-grammar partition, including #7098's `legacy_task_cite_occurrences`, to `src/cite_grammar.rs`, and update `docs/prds/reify-audit-ptodo-detector.md` §8's module pointer.

  Files: `crates/reify-audit/src/{task_rows,cite_grammar,ptodo,pdcheck,pprdstatus,model}.rs`, `src/bin/reify-audit.rs`, `src/bin/ptodo-baseline-gen.rs`, `tests/{require_tasks_db_cli,pdcheck,cli,ptodo_baseline,g_allow_repo_wide_hard_gate}.rs`, `tests/infra/test_reify_audit_ptodo.sh`, `docs/prds/reify-audit-ptodo-detector.md`. **Signal:** B8; `reify-audit --pattern PDCHECK` with no DB prints the dispatcher's degraded breadcrumb naming PDCHECK; the `@@PTODO_SCAN@@` line is byte-unchanged and the merge-gate ratchet's run-evidence floor (`files_scanned`) and DB-absent floor (`tasks_db=absent`) still pass. **Ratchet (D18):** a manifest row counting `crate::(ptodo|pdssentinel|pdiag|pdoccover|pdcheck|pcite|pprdstatus|plinecite)::` references in `crates/reify-audit/src/**/*.rs`; ceiling = its measured post-ε count (the three production peer imports this finding names are gone; scan_util's four in-file test references are not ε's). Prereqs: δ.
- **ζ (#8812) — `source_shape`: one recogniser vocabulary with named policies; jcodemunch suppression and the orphan script adopt it** (medium; keys [fk-ec468bfa673a]; subsumes #7066 and its planned follow-up #8199, which converges `tests/engine_seam_g_allow_cites_live.rs`'s private walk-up onto the shared positional rule).
  - Implements D11 and §5.6.
  - Moves `lib::is_test_path` here as the union rule (`pdiag::is_swept_path` gate 4 calls it), along with the G-allow line grammar and positional rule. The G-allow lane itself moves later, in κ.
  - #7066's divergences (triple-slash, positional, trailing-marker; its `crates/reify-stdlib/src/dynamics/mass_props.rs` case) are each pinned.
  - The shared corpus lives at `tests/infra/fixtures/g_allow_grammar.txt`, read by the source_shape unit test and by `tests/infra/test_audit_orphan_producers_g_allow.py` (+ `.sh`, manifest row).
  - The commit message records each detector's before/after live finding counts.
  - Fix pdiag's "38" doc count.

  Files: `crates/reify-audit/src/{source_shape,scan_util,pdiag,p2_consumer_stub,pdoccover,ptodo,jcodemunch_client,model,lib}.rs`, `crates/reify-audit/tests/g_allow.rs`, `crates/reify-audit/tests/engine_seam_g_allow_cites_live.rs`, `crates/reify-stdlib/src/dynamics/mass_props.rs`, `scripts/audit-orphan-producers.sh`, `tests/infra/fixtures/g_allow_grammar.txt`, `tests/infra/test_audit_orphan_producers_g_allow.{py,sh}`, `tests/infra/run-all-classification.manifest`. **Signal:** B9, B10. **Ratchet (D18):** manifest rows `private-cfg-test-recogniser | fn (is_cfg_test_attr|is_test_cfg_attr)[^a-z_] | crates/reify-audit/src/**/*.rs | 0 | ζ`, `cfg-test-substring | contains[(]"cfg[(]test[)]"[)] | crates/reify-audit/src/**/*.rs | 0 | ζ` and `g-allow-strip | strip_prefix[(]"G-allow:"[)] | crates/reify-audit/src/**/*.rs | 1 | ζ` (`source_shape.rs` only). Prereqs: γ1.
- **η (#8813) — P5 sweep corroboration as ordered rescue steps; one coverage predicate** (medium; keys [fk-20ce222ff90c]). Implements D12. The commit records the live-corpus P5 finding delta (trailing-slash entries). Files: `crates/reify-audit/src/{p5_phantom_done,task_rows}.rs`, `tests/p5.rs`. **Signal:** B11; `check_one` holds no `Finding {` literal and no `EvidenceRef::RunsDb` literal. **Ratchet (D18):** manifest rows `p5-runsdb-literal | EvidenceRef::RunsDb [{] | crates/reify-audit/src/**/*.rs | 1 | η` and `p5-sibling-rescue-call | try_task_referencing_commits[(] | crates/reify-audit/src/**/*.rs | <post-η count> | η`; both constructs occur only in P5 today (5 and 3). Prereqs: δ.
- **θ (#8814) — Injective exit-code channel: saturate at 124** (medium; keys [fk-31eb23b6b93f]). Implements D13 and §5.5.
  - Fix both doc comments.
  - Delete `cli-invocation.md` §3.1 and `SKILL.md`'s "Exit 125 is overloaded" paragraph.
  - Update `review/briefing.yaml`'s exit note, the `scripts/reify_audit_findings.py` docstring and `tests/cli.rs`'s §3.1 model.

  Files: `crates/reify-audit/src/bin/reify-audit.rs`, `tests/cli.rs`, `.claude/skills/audit/SKILL.md`, `.claude/skills/audit/references/{cli-invocation,output-format}.md`, `review/briefing.yaml`, `scripts/reify_audit_findings.py`. **Signal:** B12; a hermetic fixture with exactly 125 High findings exits 124 and its stderr array has 125 entries. Prereqs: γ2.
- **ι (#8815) — Pre-done posture carried in `AuditContext`; both knobs' effective values printed once per run** (low; keys [fk-ba93c26e42d1]). Implements D14 (adds `with_pre_done_posture`). Files: `crates/reify-audit/src/{model,p5_phantom_done}.rs`, `src/bin/reify-audit.rs`, `scripts/reify-audit-freshness.sh`, `scripts/deploy-reify-audit-predone-hook.sh`, `tests/cli.rs`, `tests/infra/test_reify_audit_freshness.sh`, `.claude/skills/audit/references/cli-invocation.md`. **Signal:** B13; `scripts/reify-audit-predone-wrapper.sh --task <id> --pre-done` prints both posture lines. Prereqs: η, β.
- **κ (#8816) — Partition `ptodo.rs`: `ptodo/g_allow.rs` (the G-allow lane), `ptodo/liveness.rs`, and `ptodo.rs` core + entry** (low; no key; h14; subsumes #8200). Follows the measured partition, re-measured after #7098, ε (cite grammar, DB access), ζ (marker grammar) and γ1 (kinds in `crate::kind`, so the children import `kind` and `cite_grammar` downward, never their parent). Files: `crates/reify-audit/src/ptodo.rs`, `src/ptodo/g_allow.rs`, `src/ptodo/liveness.rs`. **Signal:** identical test-id set before and after; `assert-module-scc.py check` green; each resulting file is under the 1,500-line soft ceiling or records its stays-whole measurement in the commit. Prereqs: ε, ζ.
- **λ (#8817) — Partition `pdoccover.rs`: `pdoccover/registry.rs`, `pdoccover/oracle.rs`, entry** (low; no key; h14; subsumes #7988). The code-view partition already moved to `source_shape` (ζ). #7988's hazard is kept: the allow-marker trio (`allow_marker_present`, `allow_marker_reason`, `ALLOW_TOKEN`) moves down into `registry.rs`, so the entry imports it and nothing reaches back. Files: `crates/reify-audit/src/pdoccover.rs`, `src/pdoccover/registry.rs`, `src/pdoccover/oracle.rs`. **Signal:** as κ. Prereqs: ζ.
- **μ (#8818) — Partition `p5_phantom_done.rs` along its 6-way partition** (low; no key; h14; subsumes #8365). The H1, H2, gitignore, corroboration and pre-done partitions become `src/p5_phantom_done/{tests_assert_empty,live_path,gitignore,corroboration,pre_done}.rs` under the entry module. Corroboration is the lower module that the sweep and pre-done import. The η manifest rows' globs keep matching. Files: `crates/reify-audit/src/p5_phantom_done.rs` and the five children. **Signal:** as κ. Prereqs: η, ι.
- **ν (#8819) — Integration tests through `check`/`check_with_stats` over a shared `RepoFixture`; test-only `pub` items close; `tests/ptodo.rs` and `tests/pdoccover.rs` partitioned** (medium; keys [fk-422bbd7986a6]).
  - Generalise `pdiag::test_support::Fixture` into `crate::test_support::RepoFixture` (feature `test-support`).
  - Rewrite `tests/ptodo.rs`, `tests/pdoccover.rs`, `tests/pcite.rs` and `tests/plinecite.rs` to drive `check` and assert on fields, partitioned along their measured partitions with shared helpers in `tests/common`.
  - Move pure-grammar tests into in-file `#[cfg(test)]` modules.
  - Delete `ptodo::resolve_liveness`.
  - Make the test-only items private or `pub(crate)`, `plinecite::in_scope` (widened by ο) included, and delete their detector-internal G-allow markers (26 + 1). The 40 fixture/mock markers stay.

  Files: `crates/reify-audit/tests/{ptodo,pdoccover,pcite,plinecite}.rs`, their partition files, `tests/common/mod.rs`, and the detector modules holding the items (`metadata.files` is `[]` at filing; the architect acquires the footprint). **Signal:** `cargo test -p reify-audit` passes with the detector scenario count unchanged or larger, recorded in the commit. **Ratchet (D18):** a manifest row `g-allow-for-test-crate | // G-allow:.*(separate crate|tests/[a-z_]+[.]rs) | crates/reify-audit/src/**/*.rs | <post-ν count> | ν`. The ERE covers 19 of the 26 detector-internal markers. The expected post-count is 1: `jcodemunch_client.rs`'s live-test seam, out of scope per §9. Prereqs: κ, λ, μ, ο.
- **π (#8820) — Partition `tests/cli.rs` and `tests/p5.rs`** (low; no key; h14). `tests/cli.rs` (5,403 lines) splits along its four measured modules: `tests/cli.rs` keeps the `cli` module, and `tests/cli_http_loader.rs`, `tests/cli_freshness_gate.rs` and `tests/cli_wire_identity.rs` take the other three, with the helpers in `tests/common`. `tests/p5.rs` splits along the five groups that mirror μ's source partition: `tests/p5_sweep.rs`, `tests/p5_pre_done.rs`, `tests/p5_tests_assert_empty.rs`, `tests/p5_live_path.rs` and `tests/p5_gitignore.rs`. Files: `crates/reify-audit/tests/cli.rs`, `crates/reify-audit/tests/p5.rs`, the partition files named above, `crates/reify-audit/tests/common/mod.rs`. **Signal:** identical test-id set before and after. Prereqs: ν, θ.
- **ξ (#8821) — PREGISTRY registry-status lane; registry row grammar; census derived from `pattern_flag::TOKENS`; INV-META-1's registry-status clause enforced** (medium; keys [fk-d668027f44f2]; INV candidate 1). Implements D15, §5.7 and ruling R2. The work:
  - **The lane:** `src/pregistry.rs` plus its DETECTORS row (opt-in, not in the default sweep), and a docs-truth-sweep member for the liveness lane (`scripts/docs-truth-sweep.py` and `docs/notes/docs-truth-sweep.md`).
  - **`docs/invariants.md`:** migrate every row to the Pointer column. Correct every row no other stream owns: EVAL-4, EVAL-5, FEA-2, FEA-3, GUI-1 and DIM-1 (status or pointer as the code shows; DIM-1 verified first), plus INV-COMP-2's text.
  - **Owned rows** get `pending-owner:#<id>` per the owner map recorded in the task description:

    | row | owner |
    |---|---|
    | EVAL-6 | #5023 |
    | GUI-3 | #5101 |
    | ARCH-1 (reify-compiler half) | #8657 |
    | EVAL-1/2/3 | S4 |
    | FEA-1, BUILD-1 | S1 |
    | GEO-1..4 | S10 |
    | GUI-2 | S25 |

    ξ is a **held leaf** (ruling R4). It is filed deferred with `metadata.x_seam_wait = {stream: "S4, S1, S10, S25", contract: "the filed leaf ids that own the status cells of INV-EVAL-1/2/3, INV-FEA-1/INV-BUILD-1, INV-GEO-1..4 and INV-GUI-2"}` and excluded from this batch's `commit_planning`. Each of those four sessions records its owning leaf ids in ξ's owner map. The lead releases ξ (`commit_planning`) once all four have filed. No row ever carries an interim owner cite.
  - **`docs/legibility/design-invariants.md`:** census rows naming the surfaces P1, P2, P5, PCITE and PDCHECK gate, plus a PREGISTRY row. It adds or migrates the **PLINECITE** row (`shipped`, `audit:PLINECITE`; a TOKENS member since #6935); ο may already have added it in today's format. PPRDSTATUS → `shipped`. PDROP, PVAC and PTYPE → `audit:PDROP; planned:#7085`, `audit:PVAC; planned:#7102`, `audit:PTYPE; planned:#5414`. The two convention rows → `convention`. INV-PD-1's inline enforcement is lifted into `**Enforcement**: planned:#7085`, and INV-PD-2's is left to #8720, whose description is amended to use the grammar. The usage-contract bullet states the pointer rule.
  - **Four-site lockstep (lead amendment definition):**
    - the INV-META-1 row: status → `partial: enforced(audit) for registry status, proposed for task INV-cites`, Pointer `audit:PREGISTRY; test:tests/infra/test_reify_audit_pregistry.sh`;
    - the design-invariants census row;
    - one `CLAUDE.md` Pointers row ("Invariant registry row grammar and status checker | `docs/invariants.md` usage contract; `reify-audit --pattern PREGISTRY`");
    - the program-doc pointer (lead).
  - **The gate** per §5.7, with the never-skip wrapper. PREGISTRY is registered on all four skill surfaces and in severity-routing §0/§2.

  Files: `crates/reify-audit/src/{pregistry,pattern_flag,source_shape,kind,model}.rs`, `src/bin/reify-audit.rs`, `crates/reify-audit/tests/pregistry.rs`, `docs/invariants.md`, `docs/legibility/design-invariants.md`, `CLAUDE.md`, `.claude/skills/audit/SKILL.md`, `.claude/skills/audit/references/{severity-routing,modes,cli-invocation}.md`, `scripts/docs-truth-sweep.py`, `docs/notes/docs-truth-sweep.md`, `tests/infra/test_reify_audit_pregistry.{py,sh}`, `tests/infra/run-all-classification.manifest`, `scripts/verify-pipeline-infra-tests.txt`. **Signal:** B14, B15, B16, B18; `reify-audit --pattern PREGISTRY` exits 0 on main after the migration. Prereqs: ε, ζ, γ2.
- **ο (#8822) — PLINECITE merge-time ratchet over tracked code prose; `--rename`; INV-DOC-1 promoted** (medium; keys []; invariant candidate 3; subsumes #8509 and #7302). Implements D16, §5.8 and ruling R1.
  - Widen `in_scope` and the cite-target set, and add the per-extension leaders.
  - Regenerate the ledger once with `--admit-new`; the commit records the row count by scope.
  - Add `--rename`, the gate test and its manifest row.
  - **Four-site lockstep** (in today's row format if ο lands before ξ; ξ then migrates the row, and whichever of ξ/ο lands first adds the PLINECITE census row):
    - a new `docs/invariants.md` row in today's five-column format, with the pointers carried inside the Enforcement cell until ξ moves them into the Pointer column: `| INV-DOC-1 | Tracked code comments cite code by symbol, never by <file>.<ext>:<line> | lint (PLINECITE ledger ratchet; audit:PLINECITE; test:tests/infra/test_reify_audit_plinecite.sh; grammar: docs/prds/reify-audit-stale-line-cite-detector.md §4) | enforced(lint ratchet) | reify-audit-typed-findings-and-registry-lanes + S23 |`;
    - the PLINECITE census row (added here if ξ has not landed; otherwise updated for the widened scope);
    - one `CLAUDE.md` Pointers row;
    - the program-doc pointer (lead).
  - Update the decision record's §5 for the scope and §7 for the gate.

  Files: `crates/reify-audit/src/plinecite.rs`, `crates/reify-audit/src/bin/plinecite-baseline-gen.rs`, `crates/reify-audit/plinecite-baseline.txt`, `crates/reify-audit/tests/plinecite.rs`, `tests/infra/test_reify_audit_plinecite.{py,sh}`, `tests/infra/run-all-classification.manifest`, `docs/invariants.md`, `docs/legibility/design-invariants.md`, `CLAUDE.md`, `docs/prds/reify-audit-stale-line-cite-detector.md`, `.claude/skills/audit/references/severity-routing.md`. **Signal:** B17; the merge gate runs the PLINECITE test; `reify-audit --pattern PLINECITE` on main exits 0 with the ledger as committed. Prereqs: **#6935**, ζ, γ2. ο is deliberately not ordered after ξ, so INV-DOC-1 does not wait for the held ξ.
- **ω (#8823) — PRD close: id backfill, terminal stamp, AS-AUTHORED freeze** (low; no key). Per the overlay's decompose-close obligations: stamp `SHIPPED` (or the honest terminal token) with the landed leaf ids, the freeze paragraph and the LIVE/AS-AUTHORED map. §5 stays LIVE because the crate's rustdoc defers to it. Apply the matching header to the capability manifest. Docs-only; `task_kind normal`, complexity simple. **Signal:** the committed header. Prereqs: every other leaf (a cancelled sibling counts as satisfied).

DAG, edge by edge:

- #6935, #7098 → α → β → γ1 → γ2 → θ
- γ1 → δ → ε
- δ → η → ι → μ
- γ1 → ζ
- ε, ζ → κ
- ζ → λ
- κ, λ, μ, ο → ν → π (θ → π also)
- ε, ζ, γ2 → ξ (held: released by the lead once S4, S1, S10 and S25 have filed their owning leaves)
- #6935, ζ, γ2 → ο (no ξ edge; ξ is held, R4)
- all → ω

High leaves (gate-8612 dependencies): β, γ1, γ2.

G7 (advisory walk, reify family):
- The umbrella principle `nothing-vacuous-and-unowned` is what δ closes (silent clean on a failed enumeration) and what ξ mechanises (the census obligation). ξ and ο add their own census rows in the same diff.
- INV-SF-5 `placeholders-owned-and-loud`: every `pending-owner:` and `planned:` cites a live task, and the liveness lane reports terminal cites.
- INV-ARCH-1: every new module imports downward in fact (M15).
- No waiver.

## 9. Out of scope

- `jcodemunch_client.rs`'s other four partitions (munch decode, wire decoders, enrichment, client), beyond the recogniser moves in ζ.
- `scripts/audit-orphan-producers.sh`'s `CFG_TEST_RE` (#8369) and `scripts/lib_rust_production_view.sh`'s lexer: a scripts-side home.
- Burning down the PLINECITE ledger or de-numbering any cite (S23, #8305, #8316, #8540, #8073). Linting `docs/**`, `*.md` or `tests/prd-gate/**` (ruling R1, D16).
- Generalising #8441's `P1TaskUnexamined`/`P5LivePathUnexamined` patterns into `Kind::Unexamined`. Their pattern tokens are routing keys; recorded as a candidate.
- Content-derived binary freshness (#6642). ξ's wrapper refuses a stale binary rather than relying on freshness being right.
- Building PVAC, PDROP or PTYPE (#7102, #7085, #5414): ξ only records their chartered state.
- A task-INV-cite checker (INV-META-1's first clause) and a `--window` design (ruled: a future window is designed deliberately).

## 10. Open questions (tactical)

1. **`Kind` tokens.** Answered by D1's token list; γ1 may add a token only for a kind D1 does not name.
2. **ν's grammar-test placement.** Each pure-grammar test goes either into its source module's in-file `#[cfg(test)]` or into the partition module after κ/λ/μ. Decide per test in ν.
3. **ξ, `item:` pointers into `gui/`.** These resolve through `declares_item` like any `.rs`; a `.ts` pointer is `test:`/`script:` path-only. Confirm in ξ that no row needs a TS symbol.

## 11. Rulings

**Program rulings applied** (2026-10-12, not relitigated):
- `--since`, `TimeWindow`, `producer_branch` and the skill's `--since` mode are deleted (β).
- Finding gets a typed kind, a required located EvidenceRef and a subject enum (γ1/γ2). D2 adds `Run` (M2).
- One recogniser module (ζ).
- Fallible GitOps (δ).
- The registry-status lane (ξ, INV candidate 1).
- One PCITE-family line-cite lane owned by S24, re-read by R1.
- The weakened f291 is kept at weakened weight (ι).

**Session rulings (Leo, 2026-10-13):**

- **R1 — INV-3's lane is PLINECITE, scoped to code prose.** S24 depends on #6935. ο subsumes #8509: the ledger becomes a merge-time ratchet (a new cite reds; burn-down never reds), and the scope widens to all tracked code comments. `docs/` and `*.md` stay under the overlay's dated-anchor rule, unlinted. INV-3 is worded for code prose (INV-DOC-1), and S23 burns the ledger down.
  - Rejected: also linting undated docs (~6.5k ledger rows); gating PLINECITE as built; extending PCITE as literally ruled (it would duplicate the in-flight detector).
- **R2 — The checker lands green by fixing unowned rows and tagging owned ones.** ξ migrates every row to the pointer format and corrects every row no other stream owns, covering text, status and census. A row whose status cell another stream owns carries `pending-owner:#NNNN`, which the checker accepts only while the cited task is live (the liveness lane), so the exceptions burn down as owners land.
  - Rejected: baselining all drift for S23; report-only until S23.
- **R3 — S24 is ordered after #6935.** α depends on #6935 (21 shared files), and priority inheritance lifts it once high S24 leaves depend on it. D17 applies the same reasoning to the in-flight #7098 (M16).
  - Rejected: S24 first, with #6935 rebasing; absorbing #6935 into S24.
- **R4 — ξ is a held leaf until the owning streams file.** S4 (INV-EVAL-1/2/3), S1 (INV-FEA-1, INV-BUILD-1), S10 (INV-GEO-1..4) and S25 (INV-GUI-2) have no filed leaves yet, so ξ is filed with `x_seam_wait` and excluded from this batch's `commit_planning`. Each of those sessions records its owning leaf ids on ξ, and the lead releases ξ once all four have. Every `pending-owner:` therefore cites a real owner. ο is not ordered after ξ.
  - Rejected: an interim cite of the trigger-chain gate #8612; an exemption list inside `pregistry.rs`.
- **R5 — α waits for #7098 as well as #6935** (D17; R3's reasoning applied to the second in-flight branch, confirmed).
