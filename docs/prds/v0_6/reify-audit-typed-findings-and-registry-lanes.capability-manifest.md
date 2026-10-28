# Capability manifest — `reify-audit-typed-findings-and-registry-lanes` (review-all stream S24)

PRD: `docs/prds/v0_6/reify-audit-typed-findings-and-registry-lanes.md`. Machine-readable twin: `reify-audit-typed-findings-and-registry-lanes.capability-manifest.yaml` (validated by dark-factory `shared/src/shared/capability_manifest.py::load_capability_manifest`; 18 blocks).
Drafted in author mode, pass 1 (2026-10-13). Bindings were measured at main `e217c0f30b` and re-checked at `a79726885d`. Every mechanical `delivered_check` was run at `a79726885d`. All are non-vacuous (each `present` check has no match yet, and each `absent` check matches today) except γ2's `legacy-task-id-mirror-deleted`, which is vacuous by design: it guards a construct that γ1 creates. An adversarial critic pass (31 findings) is folded in; the C-ids below refer to it. The decompose pass re-walks these bindings; nothing below is a D3 workflow result.

Evidence vocabulary: `grep:<file> <symbol>` (present on main) · `run:` (a command executed for this manifest) · `record:` (the review record / a measurement seat at `e217c0f30b`) · `producer:<leaf|task-N>` (upstream in the DAG).

---

## α — Partition `lib.rs` *(intermediate → β, γ1, δ, ζ)*

| Capability | Evidence | Verdict |
|---|---|---|
| the 4-way partition and its couplings exist | record `h14_measurements` row `crates/reify-audit/src/lib.rs` (model / git_ops / jcodemunch_ops / source predicates); the model partition's only symptom is the `pub use` shim for 26 test files + 4 bins | PASS |
| new modules can import downward only (INV-ARCH-1) | `run:` `scripts/module-scc-baseline.json` has no `reify-audit` row (M15) | PASS |
| #6935 and #7098 land first (R3, R5) | `producer:task-6935`, `producer:task-7098` upstream (`git log main..task/6935`: 18 commits; `main..task/7098`: 9 commits; both unmerged at `a79726885d`) | PASS |
| two lib.rs predicates have a typed home | `grep:crates/reify-audit/src/lib.rs fn is_symbol_suppressed(symbol: &ChangedSymbol)` / `fn wholly_unlocatable_count(symbols: &[ChangedSymbol])` → `jcodemunch_ops.rs` | PASS |

## β — AuditContext builder; `--since` family deleted *(leaf; fk-0b1d530f0264, high)*

| Capability | Evidence | Verdict |
|---|---|---|
| 142 struct literals at main (146 after #6935) exist to rewrite | `run:` `git grep -c 'AuditContext {' -- crates/reify-audit/src crates/reify-audit/tests` = 8 + 134 at `e217c0f30b`; 146 on `task/6935` | PASS |
| no detector narrows on `window` | `grep:crates/reify-audit/src/p2_consumer_stub.rs ctx.window.is_none()` is the sole reader (M6) | PASS |
| the guard would panic once `window` is gone (signal premise) | `grep:crates/reify-audit/src/p2_consumer_stub.rs debug_assert!(false` behind `task_metadata.len() > SWEEP_BACKLOG_WARN_THRESHOLD` | PASS |
| the skill's default argv carries `--since` | `grep:.claude/skills/audit/references/modes.md --since <14d-ago` (§1, §4); §3 "Window sweep mode" | PASS |
| other `--since` callers | `grep:.claude/skills/hotspot-survey/project.md --since <window>`; `grep:scripts/reify-audit-predone-wrapper.sh Window sweep from ISO date`; dark-factory: none (`run:` `git -C dark-factory grep -- --since` hits only a calibration jsonl) | PASS |
| `--since` becomes an unknown argument (rejection, B6) | the bin's `parse_args` errors on unknown flags with exit 125 (`grep:crates/reify-audit/src/bin/reify-audit.rs ExitCode::from(ERROR_EXIT)` after `parse_args` Err) | PASS |

## γ1 — Typed Finding, in-crate *(intermediate → γ2, δ, ζ, η; in-crate half of fk-33f9891ad16c, high)*

| Capability | Evidence | Verdict |
|---|---|---|
| three production summary parsers to delete | `grep:crates/reify-audit/src/ptodo.rs fn fingerprint` / `fn is_g_allow_finding` / `fn g_allow_finding_line` (M1) | PASS |
| a latent line-0 path is made unrepresentable (B4 premise) | `grep:crates/reify-audit/src/ptodo.rs parsed.unwrap_or(0)` after a `debug_assert!`; every current G-allow summary carries `: line N:` (latent, M1) | PASS |
| a byte-identical summary is decomposable (B1) | each `<kind>: [line N: ]<text>` producer is the inverse of `ptodo::fingerprint`'s decomposition; unprefixed producers render `Bare` (D3) | PASS |
| the fingerprint has no committed rows to regenerate (B2) | `run:` `wc -c crates/reify-audit/ptodo-baseline.txt` = 0 (M4) | PASS |
| a typed kind exists to extend | `grep:crates/reify-audit/src/ptodo.rs enum Kind` (structural lane, `Kind::as_str`, `Kind::severity`) | PASS |
| kinds in a lower module avoid a model↔detector cycle (C4) | `run:` `scripts/assert-module-scc.py` sees only `crate::<module>` edges, so a facade hides a cycle; `crate::kind` imports nothing above `scan_util` | PASS |
| fingerprint parity is not vacuous (C20) | B2 runs over `tests/ptodo.rs`'s scenario set with its inline DB fixtures and asserts every `PtodoKind::all()` member occurs; the live DB-absent set on main is empty | PASS |
| `Pattern` moves into `crate::kind` (fold check N2) | `grep:crates/reify-audit/src/lib.rs pub enum Pattern` beside `Finding`; `kind` must not import `model` | PASS |
| run-level findings need a third subject (D2) | `grep:crates/reify-audit/src/lib.rs Run-level form, \`task_id\` empty` (#8441) | PASS |

## γ2 — Finding wire contract *(leaf; fk-33f9891ad16c, high)*

| Capability | Evidence | Verdict |
|---|---|---|
| the JSON readers to migrate | `grep:scripts/pprdstatus-escalate.py finding["task_id"]`; `grep:scripts/docs-truth-sweep.py finding["task_id"]`; `grep:tests/infra/stub_escalation_mcp.py "task_id": path`; `grep:tests/infra/test_reify_audit_pdoccover.py undocumented-name: lonely_op` (M5) | PASS |
| the skill scrapes kind/line from summary | `grep:.claude/skills/audit/references/severity-routing.md violation taxonomy kind from \`finding.summary\`` | PASS |
| dedupe/state keys survive (D2) | `docs-truth-sweep.py` keys on `(pattern, path)`; the skill's `index.json` stores `parent_task_id` = today's `task_id` string = `Subject::key()` | PASS |
| dark-factory reads no field | `run:` read of `fused-memory/src/fused_memory/middleware/pre_done_hook.py`: `returncode == 0` gate plus a stderr echo only | PASS |
| parity test reads the §0 columns | `grep:crates/reify-audit/tests/skill_registration_parity.rs PATTERN_VALUES_COLUMN` | PASS |

## δ — Fallible GitOps *(leaf; fk-c45888cf175b)*

| Capability | Evidence | Verdict |
|---|---|---|
| 12 questions, 3 fallible | `grep:crates/reify-audit/src/lib.rs fn try_log_grep` / `fn try_is_gitignored` / `fn try_path_tracked_on`; 15 trait methods (M7) | PASS |
| 3 silent-clean ls_files consumers (signal premise) | `grep:crates/reify-audit/src/pdssentinel.rs ctx.git.ls_files()`, `pcite.rs`, `pprdstatus.rs::tracked_prds` pass the Vec straight to filters | PASS |
| the unexamined-finding precedent exists | `grep:crates/reify-audit/src/bin/reify-audit.rs enum JcodemunchUse` (`unexamined` builders, #8441) | PASS |
| P5's High path uses infallible calls | `grep:crates/reify-audit/src/p5_phantom_done.rs task_referencing_commits(` / `all_files_tracked_on_main(` / `path_tracked_on(MAIN_BASE` inside `check_one`; `task_referencing_commits` is a `try_…().unwrap_or_default()` wrapper that δ deletes (C3) | PASS |
| PLINECITE and the baseline generators call `ls_files` (C27) | `run:` `git show task/6935:crates/reify-audit/src/bin/plinecite-baseline-gen.rs` builds an AuditContext | PASS |
| *(gap-fill 2026-10-16, §K9-D8 B)* `ptodo-baseline-gen --help` exits 0 with empty stdout, so `ptodo-baseline-gen --help > crates/reify-audit/ptodo-baseline.txt` empties the merge-gate ledger and reports success; `pdiag-baseline-gen` already exits 2 — new check `ptodo-gen-help-exits-2` (test `ptodo_baseline_gen_help_exits_2_with_empty_stdout`) | `grep:crates/reify-audit/src/bin/ptodo-baseline-gen.rs "-h" \| "--help"` branch ends in `return;`; `grep:crates/reify-audit/src/bin/pdiag-baseline-gen.rs std::process::exit(2)` in its help branch | PASS |

## ε — tasks.db handle *(leaf; fk-bdc845122da0; subsumes #8516)*

| Capability | Evidence | Verdict |
|---|---|---|
| DB access lives in ptodo | `grep:crates/reify-audit/src/ptodo.rs pub fn tasks_db_path` (REIFY_PTODO_TASKS_DB), `pub fn open_tasks_db`; callers `pdcheck::check`, `bin::enforce_tasks_db_opens` | PASS |
| an opens-but-not-a-DB file must stay refused (C2) | `grep:crates/reify-audit/tests/require_tasks_db_cli.rs fn a_file_that_opens_but_is_not_a_tasks_db_is_refused`; `open_tasks_db` has no schema probe, so D10 adds one | PASS |
| the whole `@@PTODO_SCAN@@` line keeps its sources (C2, fold check N3) | `grep:crates/reify-audit/src/bin/ptodo-baseline-gen.rs files_scanned={} markers_examined={} tasks_db={}`; `grep:tests/infra/test_reify_audit_ptodo.sh files_scanned`; `ptodo::check_with_stats` keeps returning `ScanStats` | PASS |
| PDCHECK refused by `--require-tasks-db` today (B8 premise) | `grep:crates/reify-audit/src/bin/reify-audit.rs this run set does not include PTODO` | PASS |
| exit 255 is the DB-absent code | `grep:crates/reify-audit/src/bin/reify-audit.rs TASKS_DB_ABSENT_EXIT` | PASS |
| cite grammar peer import | `grep:crates/reify-audit/src/pprdstatus.rs use crate::ptodo::canonical_cite_occurrences` | PASS |
| *(gap-fill 2026-10-16, §K9-D9 B — supersedes the schema-probe rows above)* statuses can be read through the task API | `grep:crates/reify-audit/src/fused_memory_client.rs pub struct FusedMemoryClient` (`get_task`, `get_tasks`; ε adds `get_statuses`, one call per run); checks `tasks-db-handle-in-task-rows` → `status-source-handle-in-task-rows` (`pub (enum\|struct) StatusSource` in task_rows.rs) and `schema-probed-open` → `no-direct-tasks-db-read` (`REIFY_PTODO_TASKS_DB\|fn open_tasks_db\|fn tasks_db_path` absent in crates/reify-audit/src; 12 hits at main c7bbfe2519) | PASS |

## ε₂ (#9266) — runs.db handle *(leaf; fk-923f53493f57, review-reify-20261015; gap-fill 2026-10-16 §K9-D8 B)*

| Capability | Evidence | Verdict |
|---|---|---|
| runs.db is opened read-write against the CWD | `grep:crates/reify-audit/src/bin/reify-audit.rs rusqlite::Connection::open(&args.runs_db)` (creates a missing file); default `"data/orchestrator/runs.db"` not joined to `--project-root` | PASS |
| one bad input yields N per-task Mediums (signal premise) | `grep:crates/reify-audit/src/p5_phantom_done.rs runs.db unreadable — cannot corroborate merged provenance for task` in `check_one`'s Err arm; pinned by `tests/p5.rs fn runs_db_unreadable_emits_medium_breadcrumb` | PASS |
| a read-only, fallback-aware open exists to copy | `grep:crates/reify-audit/src/jcodemunch_index.rs rusqlite::Connection::open_with_flags` (read-only, first-query probe) | PASS |
| the degradation rule to mirror | `producer:ε` (status-source handle with `Unavailable` / `DbUse`) | PASS |
| delivered checks fail at main | `run:` at c7bbfe2519: `pub (enum\|struct) RunsDb` in task_rows.rs → 0; `Connection::open\(&args\.runs_db\)` → 1; `cannot corroborate merged provenance for task` → 1 | PASS |

## ζ — source_shape *(leaf; fk-ec468bfa673a; subsumes #7066)*

| Capability | Evidence | Verdict |
|---|---|---|
| four divergent cfg(test) recognisers | `grep:` `pdiag.rs fn is_cfg_test_attr`, `pdoccover.rs fn is_cfg_test_attr`, `p2_consumer_stub.rs fn is_test_cfg_attr`, `jcodemunch_client.rs contains("cfg(test)")` (M9) | PASS |
| three G-allow grammars | `grep:` `ptodo.rs fn g_allow_marker_body`, `jcodemunch_client.rs fn extract_g_allow`, `scripts/audit-orphan-producers.sh G_ALLOW_RE` | PASS |
| the positional rule diverges too (C21, #7066 A(2)) | `grep:scripts/audit-orphan-producers.sh G_ALLOW_RE.search(lines[idx - 1])` | PASS |
| a literal-aware code view exists to lift | `grep:crates/reify-audit/src/pdoccover.rs fn code_view` / `fn blank_literals` | PASS |
| the script test can run under `/usr/bin/python3` | the script's embedded Python is stdlib-only; the new test is stdlib `unittest` (CLAUDE.md "Infra tests") | PASS |

## η — P5 rescue steps *(leaf; fk-20ce222ff90c)*

| Capability | Evidence | Verdict |
|---|---|---|
| duplicated rescues and literals | `record:` `check_one` 1124–1533 at `e217c0f30b`: 12 returns, 9 `Finding {`, 5 `EvidenceRef::RunsDb`; `task_referencing_commits` ×2, `all_files_tracked_on_main` ×2 (M10) | PASS |
| a directory-aware predicate exists to adopt | `grep:crates/reify-audit/src/p5_phantom_done.rs fn covers_path`; `grep:crates/reify-audit/src/task_rows.rs fn path_present_in_tracked` | PASS |

## θ — Exit saturation *(leaf; fk-31eb23b6b93f)*

| Capability | Evidence | Verdict |
|---|---|---|
| the collision is real | `grep:crates/reify-audit/src/bin/reify-audit.rs count.min(254) as u8` beside `ERROR_EXIT: u8 = 125` | PASS |
| existing exit asserts survive | fixtures in `tests/infra/test_reify_audit_ptodo.sh` (c)–(g) assert `-eq 1`; `pre_done_hook.py` gates on 0 only (M5, M11) | PASS |

## ι — Pre-done posture *(leaf; fk-ba93c26e42d1, low, weakened)*

| Capability | Evidence | Verdict |
|---|---|---|
| the detector reads the ambient env | `grep:crates/reify-audit/src/p5_phantom_done.rs fn pre_done_refusal_severity` (`std::env::var("REIFY_AUDIT_PREDONE_WARN_ONLY")`) | PASS |
| the freshness knob is a different axis (skeptic) | `grep:scripts/reify-audit-freshness.sh REIFY_AUDIT_FRESHNESS_STRICT` decides whether a stale binary runs at all | PASS |

## κ / λ / μ / π — Partitions *(intermediates → ν; π → leaf; h14)*

| Capability | Evidence | Verdict |
|---|---|---|
| ratchet globs survive the partitions (fold check N4) | `grep:tests/infra/adoption_ratchet.py root.glob(row.path_glob)` (pathlib); S24 rows use `crates/reify-audit/src/**/*.rs` | PASS |
| measured partitions with no symptom | record `h14_measurements` rows for `ptodo.rs` (6), `pdoccover.rs` (5), `p5_phantom_done.rs` (6), `tests/cli.rs` (5), `tests/p5.rs` (5), all `stays_whole: false` | PASS |
| no reach-back once kinds are typed (C23) | `producer:γ1` (kinds in `crate::kind`); λ moves #7988's allow-marker trio down into `registry.rs` | PASS |
| μ's children sit under `src/p5_phantom_done/` (C27) | `scripts/assert-module-scc.py::top_level_module` files a nested path under its first segment | PASS |
| their prerequisite moves are upstream | `producer:ε` (cite grammar, DB), `producer:ζ` (marker grammar, code view), `producer:η`/`ι` (P5) | PASS |

## ν — Tests through `check()` *(leaf; fk-422bbd7986a6)*

| Capability | Evidence | Verdict |
|---|---|---|
| 22 test-only pub items, 26 detector-internal G-allow markers | `record:` measurement seat at `e217c0f30b` (M12) | PASS |
| a fixture shape to generalise | `grep:crates/reify-audit/src/pdiag.rs pub mod test_support` (`Fixture::{write, track_only, run, keys}`) | PASS |
| a test-only wrapper to delete | `grep:crates/reify-audit/src/ptodo.rs pub fn resolve_liveness` (its G-allow names `tests/ptodo.rs` as sole caller) | PASS |

## ξ — PREGISTRY *(leaf, HELD until S4/S1/S10/S25 file — ruling R4; fk-d668027f44f2; INV candidate 1 / INV-META-1's registry-status clause)*

| Capability | Evidence | Verdict |
|---|---|---|
| PLINECITE is a TOKENS member before ξ (C1) | `run:` `git show task/6935:crates/reify-audit/src/bin/reify-audit.rs` DETECTORS row `pattern_flag::PLINECITE`; ξ adds its census row | PASS |
| the library can read the token set (C6) | `grep:crates/reify-audit/src/pattern_flag.rs pub const TOKENS`; DETECTORS stays in the bin | PASS |
| the liveness lane has a runner (C11) | `grep:scripts/docs-truth-sweep.py token=` (PPRDSTATUS, PCITE members); ξ adds PREGISTRY | PASS |
| a stale binary cannot skip the docs-commit gate (C10) | the PTODO precedent needed `REIFY_PTODO_RATCHET_REQUIRED=1` via `scripts/verify.sh::selected_infra_leaf_env`; ξ's wrapper refuses on any non-zero guard result instead | PASS |
| only INV-ARCH-1 has an `**Enforcement**:` paragraph (C14) | `run:` `grep -n '^\*\*Enforcement\*\*' docs/legibility/design-invariants.md` → one line | PASS |
| the drift the checker must settle (signal premise) | `record:` K2 seat at `e217c0f30b`: 8 of 13 `proposed` rows resolve; INV-FEA-1's named symbol absent while `register_production_compute_fns` exists; census mismatches PPRDSTATUS / PCITE / PDCHECK / P2 / P5 / PDROP / PVAC / PTYPE (M13) | PASS |
| no reify reader parses either registry; dark-factory parses headings | `run:` `git grep docs/invariants.md` over scripts/tests/hooks/crates: no parser; dark-factory `scripts/legibility/invariants.py::parse_headings` (headings only) | PASS |
| a docs edit can trigger an infra test without a verify.sh edit | `grep:scripts/verify.sh select_infra_tests` exact-path match over `CHANGED_FILES_RAW`; existing `docs/notes/*.md` keys in `scripts/verify-pipeline-infra-tests.txt` | PASS |
| item resolution has a home | `producer:ζ` (`source_shape::declares_item`) | PASS |
| liveness classification has a home | `producer:ε` (`task_rows`); `grep:crates/reify-audit/src/task_rows.rs is_terminal_status` | PASS |
| the census can be derived from one registry | `grep:crates/reify-audit/src/bin/reify-audit.rs const DETECTORS` with a unit test asserting one row per `pattern_flag` token | PASS |

## ο — PLINECITE gate *(leaf; INV candidate 3 / INV-DOC-1; subsumes #8509, #7302)*

| Capability | Evidence | Verdict |
|---|---|---|
| the cite grammar widens beyond `.rs`/`.ri` (C8) | `run:` `git show task/6935:crates/reify-audit/src/plinecite.rs`: `if extension != b".rs" && extension != b".ri"`; 336 `.sh/.py/.ts/.toml/.yaml`-target comment cites under scripts/tests/infra/hooks | PASS |
| `.ri` outside `crates/` stays out (C9) | tests/prd-gate and docs `.ri` land through docs commits that never select the PLINECITE test | PASS |
| the detector, ledger and `Ledger<R>` exist upstream | `producer:task-6935` (`git show task/6935:crates/reify-audit/src/plinecite.rs`, `…/plinecite-baseline.txt`, commit `c3df401b6d` `ratchet_ledger::Ledger<R>`) | PASS |
| one scope function to widen | `plinecite::in_scope` (decision record `docs/prds/reify-audit-stale-line-cite-detector.md` §5 on `task/6935`) | PASS |
| per-language comment leaders | `producer:ζ` (`source_shape::CodeView`) | PASS |
| the one-directional ledger precedent | `grep:tests/infra/cited-test-path-baseline.manifest` (a shrinking grandfather list, live ⊆ baseline) | PASS |

## ω — PRD close *(leaf; no key)*

| Capability | Evidence | Verdict |
|---|---|---|
| terminal vocabulary | `.claude/skills/prd/project.md` "PRD terminal status — closed vocabulary" | PASS |

---

## Gap-fill amendments (2026-10-16)

- §K9-D9 B (`plans/gap-fill-deliberation-reify-20261016.md` §K9-D9): ε #8811's two tasks.db checks are rewritten (three writes): `tasks-db-handle-in-task-rows` → `status-source-handle-in-task-rows`, `schema-probed-open` → `no-direct-tasks-db-read`.
- §K9-D8 B (§K9-D8): new leaf ε₂ #9266 (runs.db handle) — 19 blocks now; δ #8810 gains `ptodo-gen-help-exits-2`.
