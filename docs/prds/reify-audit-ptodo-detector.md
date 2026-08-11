# PRD: reify-audit PTODO detector — TODO-tracking-invariant enforcement

**Status:** active — F-infra (audit cadence + tracking infrastructure), version-agnostic.
**Date:** 2026-06-11. Phase C of the 2026-06-11 two-track audit (Leo-ratified).
**Approach:** B + contract (citation grammar + violation taxonomy are normative; see §8/§9).

## 1. Goal

Make the TODO-tracking invariant durably enforced in the reify repo: a deterministic
**PTODO** pattern in `crates/reify-audit` sweeps tracked files for debt markers,
validates their task citations against the task DB, and reports violations — wired
into the `/audit` default sweep and a `tests/infra` baseline-ratchet check, warn-first,
ratcheting to a hard gate once the repo is green.

**The invariant (Leo, 2026-06-11, codified in dark-factory
`skills/review-briefing/SKILL.md` --validate checks 5/6, commits 24edb2cbf7 +
55c8229d44):** every real TODO — `TODO`/`FIXME`/`HACK` comment markers, Rust
`todo!()`/`unimplemented!()` stubs, and blocker-citing `#[ignore]` reasons — must be
tracked by a specific **non-terminal** task whose brief names resolving that TODO as a
completion condition. Cite ≠ tracked; a TODO whose tracking task is done/cancelled
while the marker still applies is **orphaned**.

## 2. Background — audit evidence (premise)

The 2026-06-11 audit (artifacts: `/tmp/reify-todo-audit/`; 51 agents; 129 graph
findings + 10 todo areas + 8 critic gaps) found, of 83 real/actionable TODO records:
**48 untracked, 17 orphaned-done, 2 orphaned-cancelled, 4 misattributed, only 12
genuinely tracked**. 10 of 22 blocker-citing `#[ignore]`s cited terminal/landed
blockers (~45% rot). Dominant rot modes:

- **Prose triggers that fire silently** — "when PRD X lands"; X lands, nobody retargets.
- **Phantom-tracking prose** — "tracked separately / as a follow-up task / in project
  memory", verifiably false.
- **Greek-letter & PRD-relative citations** — "task δ/ζ", "task-5": unresolvable or
  ambiguous (Greek letters collide across PRDs).
- **Subsumption-chain rot** — cancelled task's subsumer shipped only half.

The backlog was zeroed/owned on 2026-06-11 via tasks **4535–4552** (4551 =
perf-backlog registry, 4552 = hygiene backlog). This PRD is about *keeping* it green —
the detector is the enforcement mechanism, not the cleanup.

## 3. Consumers (G1)

All consumers exist today; no orphan-producer risk:

1. **`/audit` skill default sweep** (`.claude/skills/audit/SKILL.md`, tracked in-repo)
   — PTODO joins the no-`--pattern` default detector set (P1/P2/P5 + PTODO) at
   advisory severity (§6.5).
2. **`tests/infra` CI** — new `tests/infra/test_reify_audit_ptodo.sh`, auto-discovered
   by `run_all.sh`, baseline-ratchet semantics (§6.6).
3. **dark-factory review-briefing `--validate` checks 5/6** (cross-project, process
   layer) — PTODO is the mechanical reify-side answer to the prose invariant.
4. **Every dispatched implementer** — markers they leave must cite live tasks; the
   infra check is the feedback loop that catches a fresh untracked `TODO:` at verify
   time.

Not an in-engine seam — the engine-integration-norm §3 catalogue does not apply (dev
tooling, same class as P1/P2/P5).

## 4. Substrate (G3 — all verified 2026-06-11)

| Capability | Evidence |
|---|---|
| Pattern registry + dispatch | `crates/reify-audit/src/lib.rs:84-133` (`Pattern` enum, per-module `check(ctx) -> Vec<Finding>`); CLI dispatch `bin/reify-audit.rs:590-627` |
| `--pattern` token parser | `bin/reify-audit.rs:249-270` (hand-rolled, comma-separated; add `PTODO` token) |
| rusqlite (bundled) | `crates/reify-audit/Cargo.toml:20` — already a direct dep (`runs.db` access) |
| Task DB | `.taskmaster/tasks/tasks.db` — sqlite, `tasks(tag, id, title, …, status)`, PK `(tag,id)`; read-only URI open verified live. **Untracked** → absent in task worktrees (drives §6.7 degradation) |
| GitOps subprocess seam | `lib.rs:442-513` (`RealGitOps::run`); PTODO adds an `ls_files()` method on the same seam |
| `#[ignore]` extraction | `crates/reify-test-support/src/ignore_hygiene.rs` pub fns (`check_ignore_reasons`, `walk_test_rs_files`, …) — Task-1622 tool, reuse not duplicate |
| Freshness guard | `scripts/reify-audit-freshness.sh` + `scripts/reify-audit-predone-wrapper.sh` — binary-level, PTODO rides it automatically |
| infra harness | `tests/infra/run_all.sh` auto-discovers `test_*.sh` |

No novel grammar — G3 grammar gate N/A (no `.ri` syntax introduced).

## 5. Sketch of approach

A new `ptodo.rs` detector module in `crates/reify-audit`, selected via `--pattern
PTODO` and included in the default sweep. Two lanes sharing one finding stream:

- **Structural lane** (no task DB): sweep tracked code files for the marker
  vocabulary; parse citations against the canonical grammar (§8); emit
  `untracked` / `malformed-cite` / `phantom-tracking` / `bare-ignore` findings.
  Runs everywhere, including worktrees.
- **Liveness lane** (task DB): resolve cited ids against
  `.taskmaster/tasks/tasks.db` (read-only); emit `orphaned` (terminal status) /
  `parked-on-anchor` (cite resolves to a non-terminal task with
  `metadata.do_not_complete == true`) / `unknown-id` findings. Degrades
  fail-soft when the DB is absent (§6.7).

Plus a narrow **inverse lane** (§6.3): non-terminal tasks whose `metadata.files`
entries name git-deleted paths.

## 6. Resolved design decisions

### 6.1 Policy: trigger-conditioned perf TODOs — **plain non-debt comments, no anchor cite**

Leo's invariant is universal; an annotation form (`TODO(perf, until: <trigger>)`)
would create a sanctioned untracked class whose trigger conditions are mechanically
unverifiable — exactly the "prose triggers fire silently" rot mode the audit found
dominant. **A TODO must cite a specific, actionable, non-terminal task — a
permanently-deferred catch-all anchor is not one.**

Speculative perf notes about currently-correct code (conditional on an unfired scale
trigger, with no consumer today) are documented as **plain non-debt comments** prefixed
with "Perf note:" or "Scaling note:", not as TODO/FIXME/HACK citations. The §8.1
detector regex `\b(TODO|FIXME|HACK)\b\s*[(:]` does not match these prefixes, so they
are invisible to the detector and require no citable task. The detector has **zero**
perf special-casing.

(Prior anchor history — **DO NOT cite**, all terminal: v1 4551 done 2026-06-12;
v2 4590 done 2026-06-13; v3 4592 done; v4 4593 cancelled 2026-06-17 — markers retired
per `docs/prds/reify-audit-ptodo-perf-anchor-retirement.md`. Each anchor closed when
its markers were reworded to plain comments; the anchor pattern itself is now retired.)

### 6.2 Policy: softer vocabularies — **core vocabulary now, expansion gated on FP review (task θ)**

V1 vocabulary: `TODO`/`FIXME`/`HACK` (marker-form only, §8.1), `todo!()` /
`unimplemented!()`, `#[ignore]` reasons. The softer vocabularies ("not yet
implemented" 51 hits incl. 4 production STUB_MSGs, "for now" 42, "placeholder" 973,
"stub" 1472, "XXX" 232, "workaround" 68) are dominated by legitimate technical usage;
enforcing them unreviewed would replicate the alert-fatigue failure that task 4115's
NO-decision exists to prevent (P2 live-corpus review returned ~all false positives;
P5 ~96% benign). Task θ is an ASSESS leaf mirroring the 4075/4076/4141 FP-review
methodology: measure each candidate vocabulary's live FP rate, then extend the
detector for those that clear, and record a NO (in this PRD, amendment commit) for
those that don't. The 4 production STUB_MSG sites are θ's first candidates.

**Resolved by §14 (θ, #4560, 2026-06-15): NO for all six vocabularies — see §14.**

### 6.3 Policy: inverse invariant — **in scope, narrowed to structured evidence**

Non-terminal tasks citing dead code locations (the audit found 9+ tasks citing the
deleted `reify-types` crate) are detected via the **structured** field only: for each
non-terminal task, each `metadata.files` path absent from the tracked-file set is
checked against git history — if the path **previously existed** (`git log -1 --
<path>` non-empty) it is reported as `task-cites-deleted-path` (with the path and
last-touching commit); if it never existed it is presumed to-be-created and passes.
Prose-path scanning of task descriptions is **out of scope** (FP-prone — historical
mentions, planned files, partial paths). Advisory severity; own leaf (ζ).

### 6.4 Citation grammar — **canonical `#NNNN`, strict from day one, one migration sweep**

Canonical forms (normative spec in §8): `TODO(#NNNN):` for comment markers; `#NNNN`
inside the `todo!`/`unimplemented!` message string or a comment on the same line or
the line directly above; `#NNNN` inside the `#[ignore = "..."]` reason string.
**Banned** (→ `malformed-cite`): Greek-letter cites, PRD-relative cites ("task-5",
"task δ/ζ") — both verifiably rot. Legacy forms ("task NNNN", "task_NNNN") are **not**
recognized by the detector; task δ migrates all existing valid cites to canonical form
in one reviewable sweep. Strict-only keeps the detector trivially auditable and makes
the convention self-teaching (one form to learn). Top-level task ids only (subtasks
deprecated repo-wide).

### 6.5 Wiring + severity — **default sweep at Medium (advisory), exit-neutral**

PTODO joins the default sweep immediately. reify-audit's exit code is the
High-severity count, so Medium findings are visible (JSON + summary) but exit-neutral
— warn-first by construction. This does **not** conflict with task 4115's NO-decision
(`reify-audit-p1-jcodemunch-substrate.md` §10): that decision quarantines
**jcodemunch-dependent, FP-unvalidated** detectors (P-DEAD/P-UNTESTED/P-LAYER); PTODO
is deterministic (grep + sqlite, no LLM, no jcodemunch, no MCP) and its violation
model was validated by hand-triage of all 83 live records on 2026-06-11. Task ε adds a
note to that PRD's §10 recording the distinction.

**Vocabulary coherence with P2:** P2's Family-1 stub vocabulary
(`p2_consumer_stub.rs:43-92`) recognizes `TODO(task_N)` but not the canonical
`TODO(#N)` — after δ's migration, P2 would silently stop seeing cited TODOs added in
done-task commits. ε extends P2 Family 1 with the canonical form (one substring
pattern + test).

**Implementer-facing surface:** ε adds a short "TODO citation convention" section to
`CLAUDE.md` — the contract every dispatched agent reads.

### 6.6 Infra check — **baseline-ratchet from day one**

`tests/infra/test_reify_audit_ptodo.sh` compares the detector's violation set against
a committed baseline of **fingerprints** (`path :: kind :: normalized marker text` —
no line numbers; they drift). Any violation not in the baseline fails the check
immediately — a fresh untracked `TODO:` is red at verify time from the moment ε lands,
even while grandfathered violations are being burned down. Baseline is shrink-only
(ratchet-above-baseline oracle pattern, Leo-ratified jun11 on 4521). After δ the
baseline should be ≈ empty.

**Sequencing rule for a new lane — re-seed in the same diff (2026-08-07, task #6087).**
Widening marker recognition necessarily discovers pre-existing debt, so the lane's own
landing diff would otherwise be red at verify. The rule: regenerate
`ptodo-baseline.txt` with `ptodo-baseline-gen` **in the same commit as the lane**, after
hand-inspecting every seeded line, rather than landing the lane behind an opt-in flag.
Rationale: (1) this is what the shrink-only ratchet is *for* — a seeded entry can only
be burned down, never grown; (2) no opt-in flag mechanism exists to reuse
(`REIFY_PTODO_TASKS_DB` is the only env var the detector reads in production; everything
else `REIFY_AUDIT_*`/`REIFY_PTODO_*` is a test seam), so a flag is new machinery that
would leave the lane dark indefinitely; (3) decisively, the baseline is read only by the
ratchet test and the generator — `reify-audit` itself never consults it — so seeding
keeps the gate green while `--pattern PTODO` still REPORTS the findings. An opt-in flag
would suppress the report too, defeating the point of adding the lane. Generate the seed
with the task DB present: a `Cited` line yields an `orphaned` fingerprint when the DB is
reachable and none when it degrades (§6.7), so the DB-present set is a superset of the
degraded one and the `comm -23 live baseline` subset oracle stays empty in both the main
checkout and a task worktree. **Every seeded line must be hand-inspected before commit**
— a false positive seeded here is permanent by design, and worse, teaches later readers
that the cite is real debt.

**Vacuity floor on the live set (2026-08-11, task #6127, esc-6087-3).** The ratchet's
oracle is `comm -23 <live> <baseline>` — subset-of — and the empty set is a subset of
everything. A generator run that emitted **zero** fingerprints therefore satisfies it
trivially, and the check reports green having asserted nothing. That is not hypothetical:
a stale or reverted `ptodo-baseline-gen` produces exactly that, and mtime is a weak oracle
against it because the freshness guard's epoch only tracks commits under
`crates/reify-audit/` (`scripts/reify-audit-freshness.sh`, SCOPE LIMITATION). The check
therefore runs **two** assertions in order: a non-emptiness floor on the live set, then
the subset check. Measured basis at the time of writing: **4** structural fingerprints in
the degraded no-task-DB mode the assertion runs under, against **5** committed baseline
entries — the floor is a structural "did the detector report anything at all" oracle, not
a tuned threshold. The floor is **gated on the committed baseline being non-empty**, so it
retires itself automatically when the shrink-only ratchet drains the baseline to empty,
the end state this section already anticipates ("After δ the baseline should be ≈ empty");
nobody has to remember to delete an assertion. **One partition is deliberately uncovered:**
if burn-down ever leaves a baseline holding only *liveness-derived* kinds (`orphaned` and
friends, which §6.7 drops when the task DB is absent), the live set can legitimately be 0
while the baseline is non-empty and the floor would false-RED. Classifying fingerprint
kinds in the shell test would duplicate detector knowledge this section requires to live
only in `ptodo-baseline-gen`, so that case is instead named as the second hypothesis in the
floor's own diagnostic; the remedy is to remove the floor together with the last structural
baseline entry.

### 6.7 Degradation contract — **fail-soft, mirroring the 4109 jcodemunch contract**

`.taskmaster/` is untracked → the task DB is absent in task worktrees, where the infra
check runs during verify. When the DB is missing/unreadable: the liveness lane (and
inverse lane) skip with a one-line stderr breadcrumb (`reify-audit: tasks.db
unreachable at '…' — PTODO liveness degraded; structural checks still run`); the
structural lane runs in full; exit semantics unchanged; **never** exit 125 for DB
absence (125 stays reserved for arg/IO misconfiguration). The implementer-facing gate
(no new untracked markers) is structural and therefore works everywhere; orphan
detection (liveness) runs wherever the DB exists — the main checkout, where the
`/audit` sweep runs. DB path: `REIFY_PTODO_TASKS_DB` env override, default
`<repo-root>/.taskmaster/tasks/tasks.db`; rows filtered to `tag='master'`.

### 6.8 Allowlist — **path-prefix + inline escape; `.md` excluded entirely**

Swept files: tracked files with extensions `.rs .ri .sh .py .ts .tsx .js` (~1900 of
2462 tracked). **Markdown is excluded**: PRD docs legitimately use Greek-letter task
labels by authoring convention (banning them there would fight `/prd` itself), and the
130 `**State:** TODO` taxonomy lines in `docs/architecture-audit/` are descriptive.
This makes the brief's suggested `State: TODO` → `GAP-OPEN` rename unnecessary —
**declined** (see §11). Path-prefix allowlist (in detector source, with rationale
comments): `crates/reify-audit/` (the tool's own pattern strings and fixtures),
`crates/reify-test-support/src/ignore_hygiene.rs` + its tests (same). Inline escape
hatch: a line containing `ptodo:allow` is skipped — greppable, reviewable, for
legitimate pattern-string sites outside allowlisted paths.

## 7. Pre-conditions for activating

None hard. The 4535–4552 zeroing batch is **not** a dependency: δ's migration re-cites
whatever is current at execution time, and the baseline absorbs any residue. (Three
zeroing tasks are in-progress; comment-line merge conflicts with δ are trivial and the
orchestrator's file locks serialize them.)

## 8. Contract — citation grammar + violation taxonomy (normative)

### 8.1 Marker recognition (structural lane)

- Comment markers: regex `\b(TODO|FIXME|HACK)\b\s*[(:]` — token must be followed by
  `(` or `:`; bare prose mentions ("the extractor's TODO") do not fire. Applies to all
  swept file types.
- Rust stubs: `todo!(` / `unimplemented!(` macro invocations (`.rs` only).
- Ignore attributes: trimmed line starts with `#[ignore` (`.rs` only). Doc-comment
  prose mentioning `#[ignore]` does not fire.
- **Allow-attribute rationales (lane δ-A, task #6087, `.rs` only):** a trimmed line
  starting `#[allow(` whose bracketed lint list contains a whole `dead_code` token
  (so `#[allow(dead_code, unused_variables)]` fires and `#[allow(dead_codex)]` does
  not) AND whose trailing `//` comment carries **deferral prose** (below). Matching is
  against the RATIONALE — the comment body — never the whole line, so the `dead_code`
  token inside the attribute itself can never be read as prose. Doc-comment prose
  merely mentioning `` `#[allow(dead_code)]` `` does not fire: the `///`/`//!` guard
  runs before the `#[allow(` search, exactly as for `#[ignore]` above (the live
  motivating line is `crates/reify-core/src/diagnostics.rs:4046`). A bare
  `#[allow(dead_code)]` with no rationale is not a marker, and a `/* … */` trailing
  comment is not recognised (unmeasured in the live corpus, so unpinned by evidence).
  The rationale must be on the **same line**: a rationale on the PRECEDING line — the
  shape the stub-macro lane's above-line lookback handles for `// #NNNN` \ `todo!()` —
  is out of scope for δ-A v1. That is an evidence-backed decision, not an oversight:
  measured over the live corpus (task #6087) the preceding-line population is **2 sites**
  (`crates/reify-stdlib/src/loads.rs:71`, `crates/reify-stdlib/src/supports.rs:76`) and
  **both are `///` doc comments** describing the item — the same class the `///`/`//!`
  guard already excludes on the same-line form, for the same reason. The plain-`//`
  preceding-line form has **zero** occurrences, so the lookback would buy no signal while
  widening the anchor to doc-comment prose. Revisit only if that count moves.
  Precedence: this lane sits AFTER comment markers, so a line carrying both the
  attribute and a real `TODO(...)` stays owned by the marker lane — at most one
  finding per line, which the §6.6 fingerprint machinery assumes.

**Deferral prose (δ-A only; `DEFERRAL_PROSE` = `pending`, `deferred to`, `not yet`,
`blocked on`, `awaiting`).** A separate const from the `#[ignore]` γ policy's
`BLOCKER_PROSE`, deliberately excluding that set's `once ` / `until `: those are safe
against a short extracted `#[ignore]` reason but explode against a whole comment ("run
once manually"). Keeping them separate also keeps the γ reason policy byte-identical.
Matching carries **three per-occurrence false-positive guards**, each derived from a
measured class (§16) and each pinned by a verbatim negative test — a disqualified
occurrence is skipped and scanning continues, so a line that both names an identifier
and states a real deferral still matches on the latter:

1. **Case-sensitive, lowercase-only** (no `to_lowercase()`, unlike `has_blocker_prose`;
   mirrors its existing `RED:` precedent). `Pending` is the `NodeCache` freshness enum
   VARIANT and prose about it is not a deferral. Kills six of the seven originally
   pinned sites: `crates/reify-eval/src/cache.rs:977`, `:1055`, `:1418`, `:3917`,
   `:4156` and `crates/reify-eval/src/engine_demand.rs:110`.
2. **Delimiter guard** — a needle immediately preceded or followed by `"` or a backtick
   is a quoted state name / code span. Kills the seventh, `gui/src-tauri/src/types.rs:1010`
   (`/// "pending"` …`), which is lowercase and survives guard 1.
3. **Identifier context** — a needle flanked by an ASCII word byte (the module-shared
   `is_word_byte`, not a re-spelled copy) is inside an IDENTIFIER
   (`mark_pending_with_cause`, `mark_pruned_pending`;
   `crates/reify-eval/src/cache.rs:968`, `:3651`). The guard covers the whole identifier
   family, not just its `snake_case` half: `-` disqualifies on **either** side (a
   hyphenated compound — "the pending-queue path" — names a thing rather than deferring
   work), and `.` / `:` disqualify on the **left only** (member/path qualification —
   `self.pending`, `NodeCache::pending`). The `.`/`:` asymmetry is deliberate and pinned
   by test: a TRAILING `.`/`:` is ordinary punctuation, and disqualifying it would
   silently kill the two most natural ways to write a real deferral ("wiring is
   pending.", "pending: the morph rewrite"). Widening guard 3 to this full family
   changed **nothing** in the live δ-A population (14 findings / 5 fingerprints before
   and after) — it is prospective FP control for rationales a future author would write
   without any thought of debt, which would otherwise land as High `untracked`.

FP control here is load-bearing, not cosmetic: every false-positive line measured over
the live corpus cites an already-`done` task, so a spurious match does not merely add
noise — it resolves through the unchanged liveness lane to a **High `orphaned`** finding
and hard-fails the merge gate. Do not "simplify" this to a lowercased `contains` sweep;
that reintroduces guard 1's class wholesale and walks back into the §14 regime.

### 8.2 Citation resolution

A marker is **cited** iff a `#NNNN` token (`#` + 1–5 digits) appears: (comment
markers) inside the `TODO(...)` parens or after the `TODO:` on the same line; (stubs)
inside the macro's message string, or in a comment on the same line or the line
directly above; (ignores) inside the reason string. Multiple cites: all are validated;
one live cite suffices for tracking.

### 8.3 Violation taxonomy (finding `kind` values)

| Kind | Trigger | Lane |
|---|---|---|
| `untracked` | marker with no citation, excluding `#[ignore]` reasons with no blocker-prose (see below); includes a δ-A allow-rationale that defers with no cite | structural |
| `malformed-cite` | Greek-letter or PRD-relative cite ("task-5", "task δ"), or legacy form ("task NNNN") | structural |
| `phantom-tracking` | prose claims: "tracked separately", "tracked as a follow-up", "tracked in project memory", "follow-up task will" (case-insensitive) without a cite | structural |
| `bare-ignore` | `#[ignore]` with no reason string | structural |
| `unknown-id` | cite parses but id not in the task DB | liveness |
| `orphaned` | cited task status ∈ {done, cancelled} — reported with cited id + status | liveness |
| `task-cites-deleted-path` | non-terminal task `metadata.files` path absent from tracked set but present in git history | inverse |
| `parked-on-anchor` | cited task is non-terminal but `metadata.do_not_complete == true` (a permanently-parked / never-completing anchor) and no other cite on the marker is genuinely live | liveness |

**`#[ignore]` reason policy:** reasons containing a cite → liveness-checked; reasons
matching blocker-prose (`pending|not yet|RED:|until |once |blocked`) without a cite →
`untracked`; operational reasons (e.g. "requires OCCT", "probe: run manually",
"timing/benchmark out of CI") without blocker-prose → pass without a cite. The
Task-1622 tool (`reify-test-support`) keeps format-level checks; PTODO owns
citation-liveness — γ wires the split using the existing pub extraction fns.

**Lane δ-A adds NO new kind (task #6087).** The `#[allow(dead_code)]` rationale lane
reuses the existing taxonomy end-to-end, with the SAME three-way split as the comment-marker
lane: a canonically-cited rationale emits nothing of its own — it hands the extracted ids to
the **unchanged** liveness lane, which resolves them to `orphaned` / `unknown-id` /
`parked-on-anchor` exactly as for any other marker; a rationale carrying a legacy/Greek cite
is `malformed-cite` (structural, Medium per §8.4); an uncited deferral rationale is
`untracked` (structural, High). The `malformed-cite` branch is normative, not incidental:
the trigger in the table above is defined lane-independently, and the live corpus contains
the legacy form on this anchor (`// production wiring deferred to task 4050 …`,
`crates/reify-eval/src/engine_build.rs:2199/2278/2292`). Collapsing it into `untracked`
would report an imprecise cite at hard-gate severity where §8.4 rates it advisory.

*Known divergence:* the `#[ignore]` γ lane has no `malformed-cite` branch — its reason
policy is cite-first-then-blocker-prose and is byte-frozen (changing it would reclassify
existing `#[ignore]` findings and perturb the §6.6 baseline). That is recorded here as a
deliberate asymmetry so it is not silently inherited by the next lane; aligning γ is a
separate change, not a consequence of #6087.

Consequently the
§6.6 fingerprint grammar (`path :: kind :: normalized text`), the ratchet test's
`VALID_KINDS`, `ptodo-baseline-gen`'s filter and the §8.4 severity mapping are all
untouched by the lane; the diff is confined to two pure recognizers plus one `scan_file`
precedence arm.

### 8.4 Severity + exit

As of task η (#4559, 2026-06-15) `untracked` / `orphaned` / `bare-ignore` emit
**High** (hard gate: `reify-audit` exits non-zero, exit code = High count; the
`tests/infra` PTODO check hard-fails verify). `unknown-id` stays **Medium** (a
DB-sync artifact must not hard-fail verify); `task-cites-deleted-path` stays
advisory; `malformed-cite` / `phantom-tracking` stay **Medium**.

`parked-on-anchor` emits **Medium** (advisory, exit-neutral): a `do_not_complete`
anchor is non-terminal but never resolves the cited debt; surface it ("parked, not
promised") without hard-failing. Keyed on the structured `metadata.do_not_complete`
flag — NOT bare `deferred` (genuine paused/human-owned deferred tasks like #4577/#4642
would be false positives) and NOT `do_not_dispatch` (#4642 is human-owned and will
complete). See §15 for the full design-decision record.

## 9. Boundary-test sketch

Fixture-driven, both directions across the detector↔repo and detector↔DB seams
(in-memory sqlite + temp git fixture, same pattern as `tests/p2.rs` /
`real_git_ops.rs`):

| # | Scenario | Pre | Post |
|---|---|---|---|
| 1 | Untracked marker | fixture file with `// TODO: wire this` | one `untracked` finding, path + kind |
| 2 | Canonical live cite | `// TODO(#42): …`, DB has 42=pending | no finding |
| 3 | Orphaned cite | `// TODO(#42): …`, DB has 42=done | `orphaned` with id 42 + status `done` |
| 4 | Greek/PRD-relative | `// TODO(task δ): …` | `malformed-cite` |
| 5 | Phantom prose | `// tracked as a follow-up task` | `phantom-tracking` |
| 6 | Stub macro, comment-above cite | `// #42` then `todo!()` , 42 pending | no finding |
| 7 | Ignore blocker-prose, no cite | `#[ignore = "pending fillet binding"]` | `untracked` |
| 8 | Ignore operational reason | `#[ignore = "requires OCCT"]` | no finding |
| 9 | DB absent | unset/missing tasks.db | breadcrumb on stderr; scenarios 1/4/5/7 still found; 2/3 silent |
| 10 | Allowlist + escape | marker in allowlisted path; `ptodo:allow` line elsewhere | no findings |
| 11 | Inverse: deleted path | non-terminal task metadata.files names a path deleted in git history | `task-cites-deleted-path` |
| 12 | Inverse: to-be-created path | metadata.files names a never-existed path | no finding |
| 13 | Infra ratchet | introduce a fresh untracked `TODO:` in a tracked file | `test_reify_audit_ptodo.sh` exits non-zero |
| 14 | Parked-on-anchor cite | `// TODO(#42): perf`, DB has 42=deferred + `{"do_not_complete":true}` | one `parked-on-anchor` Medium finding, summary carries `#42`, `deferred`, `do_not_complete` |
| 15 | Deferred without flag (FP guard a) | `// TODO(#42):`, DB has 42=deferred, NULL metadata | no finding |
| 15b | do_not_dispatch-only (FP guard b) | `// TODO(#42):`, DB has 42=deferred + `{"do_not_dispatch":true}` | no finding |
| 16 | One genuinely-live co-cite (§8.2 preservation) | marker cites #42 (deferred+do_not_complete) AND #43 (pending) | no finding |

## 10. Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §10 (task 4115 record) | amends | default-sweep membership policy note | **this PRD** (ε) | queued |
| Task 4593 (perf-backlog anchor v4) — **retired/cancelled** | n/a | trigger-conditioned-TODO citation rule (§6.1 amended) | `docs/prds/reify-audit-ptodo-perf-anchor-retirement.md` | 4593 cancelled 2026-06-17; all markers reworded to plain "Perf note:" / "Scaling note:" comments; anchor pattern retired — see §6.1 |
| Task-1622 ignore-hygiene tool (`reify-test-support`) | consumes/extends | pub extraction fns; format-vs-liveness split (§8.3) | **this PRD** (γ) | queued |
| `/audit` skill (`.claude/skills/audit/`) | consumed-by | default sweep + severity routing docs | **this PRD** (ε) | queued |
| dark-factory `skills/review-briefing/SKILL.md` checks 5/6 | parallel (process layer) | invariant prose, cross-project | dark-factory (Leo; branch `docs/review-briefing-todo-invariant`, not yet on df main) | informational — no code seam, no dep |

No contested-ownership pairs touched (overlay §G4 list is engine-side).

## 11. Out of scope

- **`State: TODO` → `GAP-OPEN` rename in `docs/architecture-audit/`** — declined;
  Markdown is excluded from the sweep (§6.8), so the 130 taxonomy lines generate no
  noise. Revisit only if θ brings `.md` into scope.
- **Prose-path scanning of task descriptions** (inverse lane stays `metadata.files`-only, §6.3).
- **Softer vocabularies** — θ's FP review (§14) returned NO for all six; none cleared. Vocabulary stays `.rs/.ri/.sh/.py/.ts/.tsx/.js` comment markers + `todo!()`/`unimplemented!()` + `#[ignore]` (§6.2, resolved by §14 (θ)).
- **Cross-project dependency-edge auditing** (dark_factory:NNNN liveness) — different
  data source (foreign task DBs); a future detector if the need is demonstrated.
- **Auditing terminal tasks / done-provenance** — the audit critic's Track-A gap;
  belongs to the existing P5 family, not PTODO.
- **Orchestrator/dark-factory changes** — none required.

## 12. Decomposition plan

Labels are PRD-relative; ids assigned at decompose. All signals CLI-observable.

- **α — PTODO structural lane + CLI wiring** (`crates/reify-audit`: `ptodo.rs`,
  `lib.rs` Pattern variant, `bin` token + dispatch; GitOps `ls_files()`).
  Marker recognition (§8.1), citation grammar (§8.2), kinds `untracked` /
  `malformed-cite` / `phantom-tracking` / `bare-ignore`, allowlist + `ptodo:allow`
  (§6.8). *Intermediate* — unlocks β/γ/δ. Signal: `reify-audit --pattern PTODO` on a
  committed fixture tree emits exactly the expected findings (scenarios 1/4/5/10);
  on the live repo it emits the current inventory.
- **β — liveness lane** (dep α). Read-only sqlite open of tasks.db (§6.7 path +
  degradation contract), kinds `orphaned` / `unknown-id`. *Intermediate* — unlocks
  δ/ζ. Signal: a fixture cite of a done task is reported with cited id + status
  (scenario 3); with the DB absent, the stderr breadcrumb appears and structural
  findings still emit (scenario 9).
- **γ — `#[ignore]` lane** (dep α, β). Reuse `reify-test-support` pub extraction fns;
  blocker-prose vs operational-reason policy (§8.3); reconcile the format/liveness
  split with the Task-1622 test. *Intermediate* — unlocks δ. Signal: scenarios 7/8
  pass on fixtures; the 10-rotted-ignores class (terminal-blocker cite) is detected.
- **δ — migration sweep + baseline** (dep α, β, γ). Rewrite existing valid cites to
  canonical form across the repo, clean prose-mention FPs, finalize allowlist entries,
  commit the (≈ empty) fingerprint baseline. *Intermediate* — unlocks ε. Signal:
  `reify-audit --pattern PTODO` on main reports zero violations above the committed
  baseline; the marker-rewrite diff is reviewable in one commit.
- **ε — integration gate: default sweep + infra check + convention docs** (dep δ;
  **critical**). Add PTODO to the default sweep at Medium; `tests/infra/
  test_reify_audit_ptodo.sh` (baseline-ratchet, §6.6) wired into `run_all.sh`;
  `CLAUDE.md` convention section; P2 Family-1 canonical-form extension (§6.5);
  §10 note in `reify-audit-p1-jcodemunch-substrate.md`. **Leaf (gate).** Signal:
  introducing an untracked `TODO:` in a tracked file flips the infra check red
  (scenario 13); the no-`--pattern` sweep lists PTODO findings; CLAUDE.md documents
  the convention.
- **ζ — inverse lane** (dep β). `task-cites-deleted-path` per §6.3. **Leaf.** Signal:
  a non-terminal fixture task whose `metadata.files` names a git-deleted path is
  reported with the path + last-touching commit (scenarios 11/12).
- **η — ratchet to hard gate** (dep ε). Flip `untracked`/`orphaned`/`bare-ignore` to
  High (§8.4); infra check fails hard accordingly. Dispatch condition (checked at
  dispatch, not a dep edge): PTODO reports **zero** violations on main — if not,
  fix cites first or bounce. **Leaf.** Signal: a violation makes `reify-audit` exit
  non-zero and verify fail. **Landed 2026-06-15 (task #4559).**
- **θ — vocabulary-expansion ASSESS** (dep ε). FP-review of softer vocabularies
  (§6.2: STUB_MSG idiom, "for now", "placeholder", "stub", "XXX", "workaround")
  mirroring 4075/4076/4141 methodology; extend the vocabulary for those that clear;
  record NO-decisions as an amendment commit to this PRD (the 4115 pattern). **Leaf.**
  Signal: the decision record committed to this PRD + (if any cleared) new vocabulary
  live in `--pattern PTODO` with fixtures.
- **ι — parked-on-anchor liveness guard** (dep β, ε). Detect cites resolving to a
  non-terminal `do_not_complete` task (§8.3/§8.4); advisory Medium. **Leaf.** Dispatch
  condition: zero live parked-on-anchor on main. Signal: scenarios 14/15/16 pass; live
  repo reports zero above baseline. **Landed 2026-06-17 (task #4644).**

Dependency DAG: α → β → {δ, ζ}; α → γ (also γ ← β); {β, γ, δ} → ε; ε → {η, θ}; {β, ε} → ι.

## 13. Open questions (tactical)

1. **Extension list breadth** — `.toml`/`.yml`/`.yaml` comments carry occasional
   TODOs. Suggested resolution: add them in α if the fixture sweep shows signal;
   otherwise θ reassesses. Decide during α. **Resolved by §14 (θ): DECLINE — `.toml`/`.yml`/`.yaml` carry 0 TODO/FIXME/HACK markers (1 raw "todo" substring total); swept set stays `.rs .ri .sh .py .ts .tsx .js`.**
2. **`unknown-id` grace for freshly-filed tasks** — a cite written in the same
   commit-window as its task filing could race DB sync. Suggested resolution: none
   needed (the DB write is synchronous via fused-memory); revisit only if ε's soak
   shows false `unknown-id`s. Decide during ε soak.
3. **Fingerprint normalization details** (whitespace folding, marker-text truncation
   length). Decide during ε.

## 14. Assessment 2026-06-15 (task θ, #4560): softer-vocabulary expansion — NO for all six

**DECISION: NO — no softer vocabulary is added to the PTODO marker set. The core vocabulary
(`TODO`/`FIXME`/`HACK` + `todo!()`/`unimplemented!()` + `#[ignore]`) is unchanged.**

Task θ applied the 4075/4076/4141 FP-review methodology over the 2044 tracked swept-extension
files (`.rs .ri .sh .py .ts .tsx .js`, excluding `crates/reify-audit/`) via `git grep`,
measuring each candidate vocabulary's occurrence count and live FP rate. FP = a hit that is
legitimate technical usage, NOT untracked debt that should cite a task.

**Evidence table (measured 2026-06-15):**

| Vocabulary            | Occ / Files | Measured FP rate | Dominant benign class |
|-----------------------|-------------|------------------|-----------------------|
| `"XXX"`               | 84 / 18     | ~100%            | `mktemp …XXXXXX` shell template placeholders (a libc idiom; the X's are replaced by random chars at runtime — not a debt marker) |
| `"placeholder"`       | 864 / 212   | ~100%            | Compiler/type-system domain vocabulary (`Type::TypeParam("__auto_…")` placeholders, `StructureTypeId(0)` ephemeral placeholders, "scalar placeholder"); UI/HTML `<input placeholder=…>` text in GUI tests |
| `"stub"`              | 1391 / 224  | ~100%            | "stub mode" is a first-class architectural concept (OCCT/OpenVDB-absent build mode); stub kernels, test stubs, `stubs.rs`, `p2_consumer_stub.rs` |
| `"not yet implemented"` | 46 / 26   | ~89%             | Descriptive doc comments (`"…is not yet implemented"`), user-facing diagnostic message strings (`type_resolution.rs:1394`), and test assertions that a message does **NOT** contain "not yet implemented" (flagging those would be perverse). Only ~4–5 genuine production stubs (the 4 `STUB_MSG` sites + `solver.rs` `debug_assert`) |
| `"for now"`           | 26 / 23     | high             | Descriptive comments documenting deliberate (often permanent) current design choices ("use Real for now", "omitted for now") |
| `"workaround"`        | 31 / 23     | high             | Comments documenting existing/resolved workarounds; many already citing tasks/escalations (esc-3851-32, #3117, task 3184) |

A deterministic substring marker cannot separate the few true positives from the dominating
legitimate usage without dragging in 40+ benign hits — exactly the alert-fatigue failure
(P2 ~all-FP; P5 ~96% benign) that §6.2 exists to prevent.

**Note on the 4 production `STUB_MSG` sites:** The `STUB_MSG` const in
`crates/reify-kernel-manifold/src/kernel.rs:46`,
`crates/reify-kernel-openvdb/src/kernel.rs:23` (cites legacy "task 2645"),
`crates/reify-mesh-morph/src/lib.rs:168` (cites PRD-relative "tasks #5–#9"),
and `crates/reify-constraints/src/solver.rs:577` (cites Greek "task ε") are genuine
untracked debt, but live inside string literals — no vocabulary substring can target them
without the 40+ FPs measured for "not yet implemented". The correct enforcement path is a
canonical `// TODO(#NNNN):` adjacent comment (the existing §6.4 / `CLAUDE.md` convention).
These production files are **not migrated in this task** (each needs a real owning live
task id; that's a separate concern). This record documents the disposition.

**§13-Q1 reassessments (resolved here):**

- **Extension list breadth (Q1):** `.toml`/`.yml`/`.yaml` carry **0** TODO/FIXME/HACK
  markers (1 raw "todo" substring total across the repo). Adding them would catch nothing.
  DECLINE confirmed — swept set stays `.rs .ri .sh .py .ts .tsx .js` (§6.8/§12 stand).
- **`.md` sweep:** Bringing `.md` into scope would flag ~90 TODO/FIXME/HACK markers +
  ~45 `State: TODO` taxonomy lines + pervasive Greek task-labels (the PRD authoring
  convention) as untracked/malformed-cite FPs — fighting `/prd` itself, exactly what
  §6.8/§11 already declined. DECLINE confirmed.

**In-code guard:** `ASSESSED_REJECTED_VOCAB` (a documented `&[&str]` const) +
`softer_vocabularies_remain_unrecognised` (a unit test iterating that const and asserting
each vocabulary yields empty `scan_file` results) live in the `#[cfg(test)]` module of
`crates/reify-audit/src/ptodo.rs`. A future contributor who adds one of these vocabularies
as a recognised marker will see that test fail, prompting them to revisit this evidence and
update this §14 record before proceeding.

**Outcome.** Every candidate vocabulary reaches state (b): committed NO-decision with
measured FP evidence. No vocabulary cleared, so state (a) (live coverage + fixtures) applies
to none. The detector vocabulary is unchanged; the committed ptodo baseline and freshness
guard remain valid.

**Revisit condition.** If a future audit pass finds a substantial volume of genuine
untracked debt in one of these vocabulary forms that could not be tracked via the existing
`TODO(#NNNN):` convention, reopen with a fresh live-corpus sample and update this table.
The in-code guard (`ASSESSED_REJECTED_VOCAB`) must be updated alongside any vocabulary
addition, with a new dated row in this table.

> **Amended, not reversed — see §16 (2026-08-07, task #6087).** The revisit condition
> above was exercised once: deferral vocabulary was admitted inside an **anchored
> conjunction** (an `#[allow(dead_code)]` attribute), never as a standalone marker. The
> NO-decision on every vocabulary in the table stands unchanged, and so do
> `ASSESSED_REJECTED_VOCAB` and its guard test. §16 carries the required fresh
> live-corpus sample, including a **negative** result for a second candidate lane that
> was measured and rejected.

## 15. Design decisions 2026-06-17 (task ι, #4644): parked-on-anchor liveness guard

### 15.1 The anchor-laundering loophole

Before this task, `is_terminal_status()` was true only for {done, cancelled}. A
permanently-parked task carrying `metadata.do_not_complete == true` was classified as
live — so a TODO citing it passed the hard gate as genuinely tracked, silently laundering
open debt through a "live but never-completing" anchor. This task is the recurrence guard:
it detects cites to such tasks and emits an advisory `parked-on-anchor` finding.

### 15.2 Signal decision: key on `metadata.do_not_complete == true`

**DECISION: Key the signal on the structured `metadata.do_not_complete == true` flag, NOT
on bare `status == 'deferred'` and NOT on `do_not_dispatch`.**

Evidence captured at loophole-discovery / decompose (2026-06-17):

| Task | Status | do_not_complete | do_not_dispatch | Verdict |
|------|--------|-----------------|-----------------|---------|
| #4593 | deferred | true | — | Exploited anchor; caught by the guard |
| #4592 | done | true | — | Terminal → moot (orphaned classification applies) |
| #4577 | deferred | false/absent | — | Genuine paused design task; resumes → FP if caught by bare-deferred |
| #4642 | deferred | false/absent | true | Human-owned, will complete → FP if caught by bare-deferred or do_not_dispatch |

Zero false positives with the `do_not_complete` signal. `do_not_complete` is the structural
generalization — matched by flag, not by literal id — so a future v5 anchor is caught
automatically.

**CRITICAL CONSISTENCY NOTE:** #4593 has since been retired/cancelled by the sibling task
(#4643, landed 2026-06-17, §6.1/§10). This guard is a pure RECURRENCE GUARD: there are
zero live `parked-on-anchor` findings on main today by design. A future author who introduces
a new `do_not_complete` anchor and cites it from a TODO will see this finding surface.

### 15.3 Why Medium (advisory, exit-neutral)

A `do_not_complete` anchor is non-terminal but never resolves the cited debt. Parked perf
notes are a deliberate, accepted backlog ("parked, not promised"), NOT broken work — they
must not hard-fail verify. Medium keeps the exit code = High count unchanged. This is
distinct from `orphaned` (High — the cited task is dead/broken) and shares Medium with
`unknown-id` (a DB-sync artifact). The `parked-on-anchor` finding lives in the liveness lane
so it degrades fail-soft (§6.7) — silent in worktrees when the task DB is absent — and the
structural lane is unaffected.

### 15.4 Dispatch condition and baseline

The dispatch condition (checked at dispatch, NOT a dep edge — mirrors η #4559): zero live
`parked-on-anchor` findings on main at land. The `ptodo-baseline.txt` is empty (0 bytes) and
stays empty — no grandfathering of residual #4593 cites (they were retired by the sibling
before this task landed). *(Historical as of 2026-08-07: still true of task ι's own lane —
zero `parked-on-anchor` entries were ever grandfathered — but the file is no longer 0 bytes.
Task #6087 re-seeded it for the δ-A lane; see §6.6 and §16.)*

### 15.5 Coordination with the sibling (§6.1/§10 anchor retirement, task #4643)

Task #4643 retired the historical exploited anchor: removed all `// TODO(#4593):` cite
markers from the codebase and updated §6.1/§10 of this PRD to record the retirement.
Task ι (#4644) adds the structural guard so a future v5 anchor is caught automatically.
The two tasks are disjoint (no shared file writes) and were coordinated by landing #4643
first, then #4644. Coordination is now complete.

### 15.6 Revisit condition

If a future audit finds a flag-less `deferred` task used as a never-completing anchor and
cited by TODOs, extend the signal to a documented allowlist or to bare-deferred-with-review;
update this §15 record and add a guard test. Do NOT silently widen the signal without updating
the evidence table (§15.2) and test coverage (scenarios 14/15/16).

## 16. Assessment 2026-08-07 (task #6087): two anchored deferral lanes — δ-A ADOPTED, δ-B NOT ADOPTED

**Premise.** The citation grammar (§8.2) is sound; the *marker* set (§8.1) was the blind
spot. Deferred work is routinely recorded in this codebase without any `TODO`/`FIXME`/
`todo!()`/`#[ignore]` token, so §8.1 could not see it. Two candidate lanes were measured
over the live corpus; **one was adopted and one was rejected**, and both results are
recorded here because §14's revisit condition demands the evidence, not just the outcome.

**Enumeration (2026-08-07, `git ls-files` + the detector's own predicates):** 3829 tracked
files → 2941 pass `is_swept_ext` and fail `is_allowlisted` → **1727 `.rs`**. Both lanes were
scoped to `.rs` (the corpus where the idiom lives), keeping `.sh`/`.py`/`.ts`/`.ri` blast
radius at zero.

### Row 1 — ADOPTED: δ-A, the `#[allow(dead_code)]` rationale anchor

| Measure | Value |
|---|---|
| Candidates (`#[allow(…dead_code…)]` + trailing `//` rationale) | 68 |
| Needle-bearing after guards 1–3 | 14 |
| Split | 1 canonically cited + 3 legacy-cited + 10 uncited |
| Measured false positives | **0** (all 14 hand-inspected) |
| Baseline seeded | 5 fingerprints |

The 54 non-firing candidates are the dominant benign class — rationales that EXPLAIN
rather than defer ("used by some, but not all, test binaries that include this module";
"Phase-1 scaffold; consumed in later phases") — and they stay correctly silent. The one
cited hit is `crates/reify-eval/src/engine_build.rs:12891` (`// production wiring pending
task #4744 …`), whose cite is `done` → **High `orphaned`**: the user-observable signal this
task exists to produce. Three hits carry the legacy `task 4050` cite form → **Medium
`malformed-cite`** (§8.3). The remaining 10 are genuine unmarked deferrals → **High
`untracked`**.

*Why 14 findings seed only 5 fingerprints:* `fingerprint()` is line-number-erased
(§6.6) and the generator dedupes through a `BTreeSet`, so repeated rationales ("production
wiring deferred to task 4050" ×3, "T12 layer-B seam …" ×8) collapse to one entry each. The
14 findings are still all reported by `reify-audit`; only the ratchet's grandfather list
is deduped.

*Guard 3's status:* it killed nothing in the live δ-A population (14 needle-bearing before
and after) because the identifier-class sites carry no allow-attribute and so are not δ-A
candidates at all. The same holds for the amended, wider guard 3 (`-` on either side,
`.`/`:` on the left) re-measured over the same corpus: still 14 findings / 5 fingerprints.
It is a **forward guard**, adopted because the class is real and cheap
to exclude, and pinned by a synthetic negative fixture
(`tests/fixtures/ptodo/scenario14_allow_dead_code_deferral.rs`, six rationales, **zero**
expected findings — an over-fire guard rather than a smoke test).

### Row 2 — NOT ADOPTED: δ-B, the bare cited-deferral anchor

A second lane (`.rs` comment line ∧ canonical `#NNNN` ∧ deferral prose ∧ no marker token)
was implemented and measured, because §8.1's allow-attribute rule provably cannot reach
`crates/reify-core/src/diagnostics.rs:3991` (a `///` above a `#[derive]`+`pub enum`, no
attribute anywhere). It was **rejected on measurement**:

| Measure | Value |
|---|---|
| Live hits | 25 |
| False positives | **12 (48%)** — every one citing a `done` task |
| Class (a): needle inside an identifier | 5 (`mark_pending_with_cause`, `mark_pruned_pending`, `mark_pending`) |
| Class (b): `#N` is a PRD-relative index, not a task id | 6 ("PRD task #10", "downstream PRD task #12", "PRD invariant #2") |
| Class (c): stale prose | 1 (`temp_dirs.rs:1242`) |
| Underlying class-(b) idiom repo-wide | **337 occurrences / 64 files** (`(PRD\|invariant\|§[0-9]+)[^#]{0,30}#[0-9]{1,3}\b` over `crates/**/*.rs` + `gui/src-tauri/src/**/*.rs`, excluding `crates/reify-audit/`) |

Class (a) is killed by guard 3. Class (b) is **not fixable by any prose guard**: the defect
is in cite recognition, not marker recognition. `CLAUDE.md`'s convention already says a
PRD-relative index should resolve to `malformed-cite`, but the §8.2 grammar only catches
forms like `task-5`, not `PRD task #10`. Since δ-B keys on exactly "a canonical `#NNNN` in
an ordinary comment", its exposure to a 337-line idiom is **structural, not incidental** —
48% is a floor, observed only where deferral prose happened to co-occur.

Because all 12 cite terminal tasks, adopting δ-B would have seeded 12 known-wrong High
`orphaned` entries into a **shrink-only** baseline: permanent by design, and actively
misleading to later readers. **Disposition:** deferred to a follow-up task (ticket
`tkt_0RS6DPESK4EKM5FEP64H08PCEH`, spawned from #6087), blocked on a §8.2 cite-grammar fix.

### The claim this evidence supports — and the one it does not

**Do not read this as "anchoring ⇒ low FP."** That general claim is exactly what the
measurement falsified. The true, narrower statement:

> Anchoring on the **attribute** is low-FP (68 → 14 → 0 measured FP). Anchoring on a bare
> **cite** is not (25 → 12 FP, 48%).

Both lanes were "anchored conjunctions"; only one had acceptable economics, and the
difference was measured rather than argued. This is the §14 methodology applied honestly
to a case where it produced one yes and one no.

### Relationship to §14 — an amendment, not a reversal

- §14's NO-decision on all six vocabularies **stands**. `ASSESSED_REJECTED_VOCAB` and
  `softer_vocabularies_remain_unrecognised` are **unchanged and still green**: the guard
  feeds `// this uses {vocab} in a comment`, which carries no `#[allow(dead_code)]`
  anchor, so δ-A cannot reach it. If that test ever goes red, the **lane** is over-broad —
  fix the lane, never the guard.
- The δ-A needle `not yet` overlaps §14's rejected `"not yet implemented"`. It is admitted
  **only inside the anchored conjunction**, never as a standalone marker. §14 measured
  *unanchored substring sweeps* over 2044 files (`placeholder` 864 occ, `stub` 1391 occ,
  89–100% FP); δ-A is a 68-line population. Different populations, different economics.
- The needles are not new vocabulary in any case: `BLOCKER_PROSE` already carried
  `pending`/`not yet`/`blocked` for the γ `#[ignore]` policy. δ-A applies them in a new
  anchored context, via a separate const so that policy stays byte-identical.

### Scope note — the signal delivered here is reduced by ruling, not by omission

The originally-scoped user-observable signal named three sites. `engine_build.rs:12891`
(`orphaned`, cite #4744 `done`) **is delivered by this task**.
`crates/reify-core/src/diagnostics.rs:3991` and `:4046` (cite #2947 `cancelled`) are
reachable only by δ-B and therefore **move to the follow-up**; they are not a miss, and
they are not silently dropped. Recorded so a later reader does not re-derive the gap.

### Revisit condition

Re-open δ-B once §8.2 can classify a PRD-relative `#N` as `malformed-cite` rather than a
task cite; re-measure the 25-hit population before adopting anything. Any further widening
of §8.1 must arrive with the same shape of evidence: a fresh live-corpus enumeration, a
hand-inspected FP count, and a dated row here — including a row when the answer is no.
