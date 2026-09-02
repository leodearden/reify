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

> **As-built correction (2026-08-27, esc-6088-2; see §8.4).** Of that last clause, only
> the **baseline-ratchet** half is enforced against this repo. The severity "hard gate"
> (task η) exists in the CLI's exit code but has no real-tree consumer — every
> exit-code assertion is hermetic. Read "hard gate" below as *High severity*, which
> routes `/audit`, not as *verify fails*.

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

**Renamed ≠ deleted (2026-08-12, task #5654).** An absent path whose last-touching
commit **renamed** it, to a target still tracked at HEAD, is reported as
`task-cites-renamed-path` carrying the old path, the new path, and the commit — a cite
a consumer can repoint without re-running any git archaeology. Mechanism: `git show -M
--name-status --format= <sha>` on the commit `git log -1` already resolved (one bounded
single-commit diff, not a history walk), matching the `R`-status line whose old side is
the cited path. `-M` is explicit so a user/global `diff.renames=false` cannot silently
disable detection. Everything else stays `task-cites-deleted-path`, and the fall-back is
total: no matching `R` line, a **merge** commit (`git show` defaults to `--cc` and prints
no diff for a merge, so a rename landed directly in a merge is not detected), any git
error, or a rename target that is itself no longer tracked → the deleted kind. A git
failure can therefore only ever cause a MISSED reclassification, never a false renamed
finding. Copies (`C` status) are not resolved — only `-M` is passed. See §17.

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
(ratchet-above-baseline oracle pattern, Leo-ratified jun11 on 4521) — **a convention
enforced by nothing**: the implemented oracle is subset-of, and no assertion anywhere
requires the baseline to shrink, or a baseline entry to still be live. Adding that second
assertion was considered and **declined** on measurement — **§18** (2026-08-28, task
#6859), the single home for that ruling. After δ the baseline should be ≈ empty —
**an aspiration with no mechanism**: measured unchanged from the 2026-08-07 seed through
2026-08-28 (**§18**).

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

**Amendment — "keeps the gate green" means the RATCHET, and there was only ever one gate
(2026-08-27, esc-6088-2 ruling; task 6088 cancelled as vacuous).** Rationale (3) above is
correct as written about the ratchet: the baseline is read only by the ratchet test and
the generator, `reify-audit` never consults it, and seeding therefore keeps the ratchet
green while `--pattern PTODO` still REPORTS the findings. What it leaves implicit is the
other half — η's exit-code gate (§8.4) was **already** green-by-absence, independently of
any seeding, because no verify step ever runs the exit-code check against the real tree.
So the seeding decision did not trade away a second layer of enforcement; there was no
second layer to trade. That also explains why re-seeding 11 High findings onto main in
the same diff turned nothing red — the outcome this paragraph predicted, for one more
reason than it stated. Full record: **§8.4**.

**Vacuity floor on generator-emitted SCAN EVIDENCE (2026-08-11, task #6127, esc-6087-3;
rebased off the live finding count by task #6241).** *This paragraph is the single home for
the floor's rationale — `test_reify_audit_ptodo.sh` and
`test_reify_audit_ptodo_ratchet_vacuity.sh` point here rather than restating it.*

The ratchet's oracle is `comm -23 <live> <baseline>` — subset-of — and the empty set is a
subset of everything. A generator run that emitted **zero** fingerprints therefore satisfies
it trivially, and the check reports green having asserted nothing. That is not hypothetical:
a stale or reverted `ptodo-baseline-gen` produces exactly that, and mtime is a weak oracle
against it because the freshness guard's epoch only tracks commits under
`crates/reify-audit/` (`scripts/reify-audit-freshness.sh`, SCOPE LIMITATION). The check
therefore runs **two** assertions in order: a floor proving the detector RAN, then the
subset check.

The floor keys on evidence of the RUN, not on what the run FOUND. `ptodo-baseline-gen`
emits, on **stderr**, exactly one machine-readable line per run — unconditionally, on the
normal exit path, including when stdout is empty:

```text
@@PTODO_SCAN@@ files_scanned=<N> markers_examined=<M>
```

Both counters come from `ptodo::check_with_stats`, accumulated **inside the single existing
sweep** (a second walk would be a second derivation — the very drift this section exists to
prevent). `files_scanned` counts tracked paths that survived `is_swept_ext && !is_allowlisted`
and were read successfully; `markers_examined` counts `scan_file`-classified marker lines
across exactly those files. The line never goes to stdout, which is the baseline stream: a
leak there would corrupt `ptodo-baseline.txt` on the next regen.

Two rules make that grammar a contract rather than a shape, and both consumers implement
them. **Multiplicity: exactly one line per run.** The Rust contract test
(`crates/reify-audit/tests/ptodo_baseline.rs`) is the strict consumer and asserts it; the
shell floor is the tolerant one and reads only the first match (`grep -m1`), because its job
is to prove the sweep ran, not to police the emitter. **Extensibility: the field list is OPEN
for additive extension.** `files_scanned` and `markers_examined` are REQUIRED and must parse
as integers; any further `key=value` token is IGNORED by both consumers. So a later counter
can be appended without a lockstep edit to either gate, while a missing or unparseable
required field still fails loud in both. That obliges both to match by **whole token** —
split on whitespace first, then anchor the key (`strip_prefix` in Rust; `tr`-split plus
`^files_scanned=` in the shell). A substring match instead reads any token whose name merely
*ends with* a required key (`skipped_files_scanned=0`), turning the very extension this rule
blesses into a hard RED; each side therefore pins the adversarial name rather than a generic
extra field (`parse_scan_line_ignores_unrecognised_tokens`, and fixture (vi) in
`tests/infra/test_reify_audit_ptodo.sh`).

The floor passes iff that line is present with `files_scanned >= 1`. That oracle is
**structural, not tuned, and debt-independent**: a repository cannot have zero swept tracked
files, so no amount of burning the debt down can make it fire — while a binary predating this
contract emits no such line at all, so a stale or reverted generator goes RED on *evidence*
rather than on the weak mtime heuristic. It is also the only shape that separates the two
states a live-count floor conflates: "detector ran, tree is clean" (must be GREEN) and
"detector did not run" (must be RED). A malformed count falls to the firing branch
deliberately — loud over silent-disarm.

**Self-disarm and its DB-dependent kind list: RETIRED by task #6241.** The floor used to
key on the live finding count, which forced a compensating self-disarm in the shell: it
subtracted baseline entries whose fingerprint `kind` was DB-dependent (`orphaned`,
`unknown-id`, `g-allow-orphaned`, `g-allow-unknown-id`, `parked-on-anchor`,
`task-cites-deleted-path` — §6.7 drops those in the no-task-DB mode scenario (a) runs under)
and disarmed once the structural remainder hit 0. That existed for one reason: without it,
the burn-down commit this shrink-only ratchet exists to produce — the one that fixes the
last structural markers and shrinks them out of `ptodo-baseline.txt`, leaving a non-empty
baseline against a legitimately empty live set — would hard-RED (#6127 review). It was the
reachable mitigation for a floor coupled to the debt level.

Both the disarm and the kind list are now gone. The floor does not read
`ptodo-baseline.txt` at all, so a burn-down commit cannot false-RED it by construction, and
no detector-kind knowledge remains in bash — which restores the "derivation lives only in
`ptodo-baseline-gen`" invariant above **in full**. The surviving helper reads two fields off
a line the generator emitted; it derives nothing.

**Cross-file contract.** The floor prints `@@RATCHET_VACUITY_FIRED@@` as the first line of
its diagnostic, in the same idiom as the `@@HARDGATE_*_PASSED@@` sentinels; the wiring
meta-test greps that token. Grepping the English text instead is wrong in both directions —
a short anchor was observed matching the assert *descriptions* that `assert()` echoes into
the same stream, and a longer sentence merely trades that false match for a false RED on the
next rewording. A **second** token now flows the other way, generator → shell:
`@@PTODO_SCAN@@` (grammar above). The wiring meta-test pins BOTH directions of the floor —
it fires without scan evidence, and stays silent (exit 0, `0 failed`, token absent) with it —
because a one-directional wiring test cannot tell a live floor from one wired to a constant
failure.

**Residual limitation: RESOLVED by task #6241.** The #6127 floor keyed on evidence the
detector *found* something rather than on evidence it *ran*, and the two coincide only while
structural debt remains. What landed: `ptodo::check_with_stats` returns a `ScanStats`
counted inside the one existing sweep, `ptodo-baseline-gen` emits it as the `@@PTODO_SCAN@@`
stderr line every run, and the shell floor asserts on `files_scanned >= 1` — decoupling the
check from the debt level entirely and removing the kind list from the shell.

One residual genuinely remains: scan evidence proves the sweep ran and enumerated files, not
that every downstream lane produced *correct* findings. `files_scanned >= 1` would still be
satisfied by a detector that walked the tree and misclassified everything. That property is
covered elsewhere and deliberately not folded in here — by the hermetic scenarios (b)–(g) in
`test_reify_audit_ptodo.sh`, which drive known fixtures through the real binary and assert
its classifications and exit codes, and by the Rust integration tests in
`crates/reify-audit/tests/`.

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

- **Bare cited-deferral comments (lane δ-B, task #6103, `.rs` only):** a trimmed line
  starting `//` — so `//`, `///` and `//!` alike — that carries BOTH a canonical `#NNNN`
  cite (§8.2) AND deferral prose, and is not a `// G-allow:` marker body. The **cite is the
  anchor**: δ-A has an attribute to anchor on and can therefore afford to report the uncited
  case as `untracked`, whereas δ-B has nothing but the comment itself, so an uncited deferral
  comment is not a candidate at all and the lane emits **no structural kind** (§8.3). Both
  predicates match the WHOLE line, which is safe here in a way it is not for δ-A, because on
  a δ-B line the entire line already IS comment text. Precedence: this lane sits **last**,
  after the phantom lane, so every earlier lane keeps every line it owned and the
  at-most-one-finding-per-line property the §6.6 fingerprint machinery assumes still holds; a
  `// G-allow:` line is delegated to its own lane, which runs an independent pass and would
  otherwise double-report the same line under two kinds. δ-B reuses δ-A's `DEFERRAL_PROSE`
  and its three FP guards unchanged — those guards are what make the lane viable at all
  (§16 Row 2, 2026-08-29).

**Deferral prose (δ-A and δ-B; `DEFERRAL_PROSE` = `pending`, `deferred to`, `not yet`,
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

**The PRD-relative register is NOT a cite (task #6103).** A `#N` whose left context places it
in a PRD-local register does not name a task, so it is invisible to canonical-cite
recognition. Three measured families, all governed by one shared digit bound:

1. **Glued PRD-artifact namespace** — `§<section>#N` (`§7#5`; the section number is scanned
   back over digits and dots), or an uppercase artifact abbreviation with a left word
   boundary: `OQ#N`, `DD#N`, `Q#N`, `T#N`.
2. **Spaced PRD-local noun**, exactly one space to the left — `invariant(s)`, `row(s)`,
   `boundary`, `open-question`, `design_decision`, and bare `decision` only when `design`
   immediately qualifies it. "Exactly one space" is the conservative reading: a wider
   separator rule would classify MORE cites as PRD-relative, and every such classification
   *suppresses* a cite, so the narrow form is the fail-safe direction.
3. **`task(s) #N`**, exactly one space to the left of a `task`/`tasks` token.

**Only family 3 is a `malformed-cite` trigger (§8.3) — the two halves of the grammar are
deliberately ASYMMETRIC (2026-09-02 review correction, task #6103).** "This `#N` cannot
anchor tracking" and "this `#N` is a botched citation" are different claims, and only
`task(s) #N` spells a citation *attempt*. A marker whose text merely cross-references a
document (`// TODO: revisit §7#5 handling`, `// FIXME: fix invariant #2 first`) never claimed
to be tracked at all, so it stays `untracked` — High, hard gate. Reporting it as
`malformed-cite` instead would silently DEMOTE genuinely untracked debt to Medium/advisory
purely because its prose names a PRD section or row number, and §6.6's `live ⊆ baseline`
ratchet is as blind to a demotion as it is to a lost finding. Family 3 keeps the second half
for the reason it was added: a marker line whose only cite is `task #10` HAS lost a canonical
anchor it tried to state, and collapsing that into `untracked` would over-report an author
who cited imprecisely at hard-gate severity. Both dispositions are vacuous on the live corpus
today — no marker-lane line in any swept extension carries a 1–2-digit `#N` in any of the
three registers (re-swept 2026-09-02) — so the split is pinned hermetically, by
`malformed_cite_prd_relative` and `marker_with_prd_reference_only_is_untracked`.

**The `N ≤ 99` bound governs all three families** — applied once, as a single early return,
not per family. It is a property of the PRD-relative *register* (a document-local index is
small), not of the `task` noun, so a fourth family added later inherits it instead of having
to remember it. It keys on DIGIT COUNT rather than on a `PRD` left-context window, because a
window fails in both directions: a long path can push `PRD` outside any sane window
(`crates/reify-core/src/diagnostics.rs:3304`), while `task #333 per PRD §Slice B`
(`crates/reify-compiler/src/stdlib_loader.rs:257`) would have a symmetric window kill a
GENUINE cite. The bound is safe **by construction** against the legacy short task ids: every
three-digit-and-up id falls outside it, so no `#NNN` cite can be suppressed (the corpus's
legacy ids include `#333`, `#479`, `#630`).

**The sub-100 id space is real, and the loss there is an ACCEPTED, bounded one (2026-09-02
review correction, task #6103).** The "three-digit-and-up" argument above covers only ids
≥ 100; ids 1–100 all exist in the task DB, so the bound genuinely overlaps live id space. It
is bounded on both ends. (i) Every one of ids 1–100 is `done` — measured over the full range
on 2026-09-02 via `get_statuses` — so a cite to one could only ever have resolved to an
`orphaned` finding against a terminal id, never to a live-task finding. (ii) The range is
CLOSED: allocation runs monotonically from 1 upward and the live head is past 6100, so no
future task can be issued an id inside the bound; the accepted loss cannot grow. (iii) No
such cite exists today — every 1–2-digit `#N` in the covered registers in tracked `.rs` is
genuinely PRD-relative. The alternative — making the bound resolution-aware for family 3, so
a sub-100 id that resolves to a real DB row is read as a cite — was considered and rejected:
it drags a task-DB lookup into a pure recogniser whose whole contract is that liveness
belongs to the separate β lane (and, under §6.7's no-DB degradation, would make the *grammar*
worktree-dependent). The 99/100 step itself is pinned in both directions by
`prd_relative_cite_positives` / `prd_relative_cite_negatives`. Re-measured per-family maxima
(2026-08-30):
family 1 **11** (`PRD T#11`), family 2 **18** (`boundary #18`), family 3 one- and two-digit
throughout (max **27**) — every one comfortably inside the bound, so the bound costs no
recall.

An **unbounded** family is fail-dangerous, in the one direction §6.6's ratchet cannot see.
Exactly one tracked line repo-wide puts a real task id in family-2 register:
`crates/reify-eval/tests/engine_eval_commit_migration.rs:1490`, `invariant #5238`, where
#5238 is a genuine task (`done`). Unbounded, that terminal cite is either **downgraded** from
a High `orphaned` hard-gate finding to the Medium advisory `malformed-cite` (marker lane) or
**erased** outright (δ-B is cite-anchored, so with no canonical cite there is no candidate at
all) — purely on which noun precedes the `#`. §6.6's ratchet asserts only `live ⊆ baseline`,
which catches a GAINED finding and never a LOST one, so nothing downstream would have
reported it. Evidence: §16 Row 2's 2026-08-30 line.

Consulted **per-occurrence, never per-line**: six live lines carry both idioms at once, so a
per-line verdict would either lose a real cite or resurrect a PRD-relative one. The G-allow
lane's own owner-cite rule (c) is deliberately left byte-unchanged — it has its own exemption
grammar and its own `g-allow-orphaned` baseline exposure. Evidence: §16 Row 2.

**Registers considered and left OUT (2026-08-31).** The non-PRD `#N` idioms `edge #N`, `site
#N`, `suggestion #N` and `Gap #N` are unambiguous non-task references and could be added, but
they are not PRD-relative and they are not what this fix is for. Repo-wide occurrence counts
over tracked `.rs` — `edge` 81, `suggestion` 75, `site` 30, `Gap` 25 — none of which reaches
any lane: no member of the δ-B population and no marker-lane line carries one, so admitting
them would change no finding today. Adding a family is therefore governed by the §14/§16 rule
that applies to every widening — a fresh live-corpus enumeration, a hand-inspected FP count
and a dated §16 row — not by a one-line edit to the recogniser. `crates/reify-audit/src/
ptodo.rs`'s `prd_relative_cite` rustdoc points here rather than restating it.

### 8.3 Violation taxonomy (finding `kind` values)

| Kind | Trigger | Lane |
|---|---|---|
| `untracked` | marker with no citation, excluding `#[ignore]` reasons with no blocker-prose (see below); includes a δ-A allow-rationale that defers with no cite | structural |
| `malformed-cite` | Greek-letter or PRD-relative cite ("task-5", "task δ", "task #5"), or legacy form ("task NNNN") — the `#N` spelling only in the `task(s) #N` register, never a bare `§7#5` / `invariant #2` document reference (§8.2) | structural |
| `phantom-tracking` | prose claims: "tracked separately", "tracked as a follow-up", "tracked in project memory", "follow-up task will" (case-insensitive) without a cite | structural |
| `bare-ignore` | `#[ignore]` with no reason string | structural |
| `unknown-id` | cite parses but id not in the task DB | liveness |
| `orphaned` | cited task status ∈ {done, cancelled} — reported with cited id + status | liveness |
| `task-cites-deleted-path` | non-terminal task `metadata.files` path absent from tracked set but present in git history | inverse |
| `task-cites-renamed-path` | non-terminal task `metadata.files` path absent from tracked set, whose last-touching commit renamed it to a path still tracked at HEAD — reported with both paths + the commit | inverse |
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

**Lane δ-B adds NO new kind either — and no structural kind at all (task #6103).** Because
δ-B is cite-anchored (§8.1) it hands its extracted ids straight to the **unchanged** liveness
lane and emits nothing of its own: there is no δ-B `untracked` and no δ-B `malformed-cite`.
`VALID_KINDS`, the §6.6 fingerprint grammar, `ptodo-baseline-gen`'s filter and the §8.4
severity map are all untouched. One consequence must be named because it bites the ratchet:
with no task DB the lane contributes **nothing** (§6.7 drops the liveness kinds), so a
generator run in a task worktree is silent about δ-B while the same run in the main checkout
is not. That is exactly why §6.6's "seed the baseline with the task DB present" rule is
load-bearing for δ-B in a way it was not for δ-A — a δ-B lane seeded from a worktree run
looks green locally and goes red on `main`.

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

**Correction — the exit code is not the real-tree gate; the §6.6 ratchet is
(2026-08-27, esc-6088-2 ruling; task 6088 cancelled as vacuous).** *This paragraph is
the single home for the correction — §6.6 and §12's η entry point here rather than
restating it.* The severity mapping above is accurate, and so is "exit code = High
count" (`bin/reify-audit.rs::high_severity_exit_code`, a raw `Severity::High` count
clamped to 254, with **no** baseline suppression). The parenthetical's second half —
"the `tests/infra` PTODO check hard-fails verify" — was never wired to this repo.
`tests/infra/test_reify_audit_ptodo.sh` makes eleven `--project-root` invocations, and
exactly **one** targets the real repo: `ptodo-baseline-gen --project-root "$REPO_ROOT"`
(§6.6 scenario a), which asserts `comm -23 <live> <baseline>` is empty. The other ten
all target hermetic fixture repos built in `mktemp -d` — nine assert an **exit code**
(scenarios c–f, at `:706 :726 :823 :842 :927 :948 :1038 :1055 :1086`), and the tenth
(scenario b, `:624`) asserts hermetic ratchet behaviour. Every one of those nine expects
0 or 1, so even on fixtures the "exit code = High **count**" arithmetic is never
asserted above 1.

So on the real tree the enforcement mechanism is the **fingerprint ratchet**, and since
`ptodo::fingerprint` is `{path} :: {kind} :: {text}` (severity plays no part) the ratchet
is **severity-blind**: it blocks any NEW fingerprint of any kind at any severity, and it
never observes the High count at all. η's exit-code gate has no real-tree consumer.

Consequences, recorded so they are not re-derived:

- **A non-zero PTODO exit on main is the steady state, not an alarm.** Measured on main
  2026-08-27: **65 findings, 11 High, exit code 11** — 10 `untracked` + 1 `orphaned`
  (High), 3 `malformed-cite`, 51 `task-cites-deleted-path`. No gate observes any of it.
  (That ζ kind breakdown predates §17: after #5654 the same population splits between
  `task-cites-deleted-path` and `task-cites-renamed-path`. The two are mutually
  exclusive per cited path and both Medium, so the 51 total and the exit code are
  unchanged — only the kind labels move.)
- **What the ratchet actually reaches is narrower than "all findings".**
  `ptodo-baseline-gen` filters to path-keyed source-marker findings
  (`is_swept_ext(&f.task_id) && !is_g_allow_finding(f)`), so of those 65 only **14** are
  fingerprinted, and they collapse to the 5 committed baseline lines because
  fingerprints drop line numbers — the 8 identical `#[allow(dead_code)] // T12 layer-B
  seam …` markers in `engine_build.rs` are 8 findings but 1 fingerprint. The 51 ζ
  inverse-lane findings are keyed by **task id**, not a swept path, so they fall outside
  the ratchet; being Medium they are also exit-neutral. That lane is therefore gated by
  nothing at all — deliberate for an advisory lane (§6.3), but it means "the ratchet is
  the gate" bounds the ratchet to the source-marker lanes only.
- **Severity is not decorative** — it still routes the `/audit` skill (High →
  `escalate_info`, Medium → deferred follow-up task, `.claude/skills/audit/SKILL.md`).
  It is decorative only for the verify gate.
- **Why this was invisible for two months.** η's dispatch condition was "PTODO reports
  zero violations on main", which held when it landed 2026-06-15; a gate that is green
  because it never runs is indistinguishable from one that is green because the tree is
  clean. Re-seeding the baseline for lane δ-A (2026-08-07, #6087) put 11 High findings
  on main without turning anything red, which is what made the gap legible.
- **This is a record, not a change request.** Whether to wire the exit code to the real
  tree — or to delete η's exit-code framing as superseded by the ratchet — is an open
  design question, deliberately not decided here.

`parked-on-anchor` emits **Medium** (advisory, exit-neutral): a `do_not_complete`
anchor is non-terminal but never resolves the cited debt; surface it ("parked, not
promised") without hard-failing. Keyed on the structured `metadata.do_not_complete`
flag — NOT bare `deferred` (genuine paused/human-owned deferred tasks like #4577/#4642
would be false positives) and NOT `do_not_dispatch` (#4642 is human-owned and will
complete). See §15 for the full design-decision record.

`task-cites-renamed-path` emits **Medium** (advisory, exit-neutral), exactly like the
`task-cites-deleted-path` it refines: `reify-audit`'s exit code is the High count, and
the inverse lane must never hard-fail verify — a stale-but-repointable citation is a
cleanup prompt, not a blocker. The two kinds are mutually exclusive by construction (a
cited path either resolves to a rename target still tracked at HEAD, or it does not), so
adding the kind changes no exit class and no finding count. See §17.

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
| 17 | Inverse: renamed path, target tracked | metadata.files names a path whose last-touching commit renamed it to a path still tracked at HEAD | one `task-cites-renamed-path` Medium finding naming BOTH paths + the sha; no `task-cites-deleted-path` |
| 18 | Inverse: renamed path, target itself absent | same, but the rename target is not tracked either (renamed again / later deleted) | `task-cites-deleted-path` (never advertise a target that is itself gone) |
| 19 | Inverse: genuine delete (regression pin) | metadata.files names a deleted path, no rename target resolvable (also the merge-commit and git-error shapes) | `task-cites-deleted-path`, unchanged — and carrying no `File` evidence ref |
| 20 | δ-B over-fire guard (committed fixture) | `tests/fixtures/ptodo/scenario17_delta_b_cited_deferral.rs` — identifier-class, PRD-relative, cite-free-deferral, G-allow and benign-explanatory lines, with **every** cite it carries seeded TERMINAL | **no findings** |

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
- **ζ — inverse lane** (dep β). `task-cites-deleted-path` + `task-cites-renamed-path`
  per §6.3. **Leaf.** Signal: a non-terminal fixture task whose `metadata.files` names
  a git-deleted path is reported with the path + last-touching commit; one whose cited
  path was RENAMED to a still-tracked target is reported with both paths + the renaming
  commit (scenarios 11/12/17/18/19). Renamed-vs-deleted landed 2026-08-12 (task #5654,
  §17) — a within-lane refinement, not a new leaf.
- **η — ratchet to hard gate** (dep ε). Flip `untracked`/`orphaned`/`bare-ignore` to
  High (§8.4); infra check fails hard accordingly. Dispatch condition (checked at
  dispatch, not a dep edge): PTODO reports **zero** violations on main — if not,
  fix cites first or bounce. **Leaf.** Signal: a violation makes `reify-audit` exit
  non-zero and verify fail. **Landed 2026-06-15 (task #4559).**
  **Signal correction (2026-08-27, esc-6088-2 ruling; task 6088 cancelled as vacuous):**
  only the first half of that signal was ever wired. A violation does make `reify-audit`
  exit non-zero, but nothing makes **verify** fail on the real tree — the exit-code
  assertions live entirely on hermetic fixtures, and the real-tree gate is the §6.6
  fingerprint ratchet, which is severity-blind. The dispatch condition ("PTODO reports
  zero violations on main") held at landing, so a gate that never ran was
  indistinguishable from one that ran and passed. See **§8.4**.
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

## 16. Assessment 2026-08-07 (task #6087): two anchored deferral lanes — δ-A ADOPTED, δ-B NOT ADOPTED (δ-B superseded 2026-08-29 — see Row 2)

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

### Row 2 — δ-B, the bare cited-deferral anchor: NOT ADOPTED 2026-08-07 → **ADOPTED 2026-08-29** (task #6103)

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
misleading to later readers. **Disposition (2026-08-07):** deferred to a follow-up task
(ticket `tkt_0RS6DPESK4EKM5FEP64H08PCEH`, spawned from #6087), blocked on a §8.2
cite-grammar fix. **Superseded 2026-08-29 — see the next block.**

#### Disposition 2026-08-29 (task #6103): **ADOPTED** — the §8.2 blocker is closed, FP rate re-measured at 0%

The rejection above was conditional on exactly one defect, and that defect is now fixed. §8.2
recognises the PRD-relative register (`prd_relative_cite`, three measured families), so a
class-(b) `#N` no longer resolves to a task cite at all; and class (a) is killed by δ-A's
word-boundary guard 3, which the 25-hit enumeration above predates and which the rejection
row itself already named as the fix for that class. The lane landed unchanged in shape (a
four-way conjunction of pre-existing predicates, §8.1) — what moved was the cite grammar
underneath it, not the lane's own economics.

| Measure | 2026-08-07 (rejection) | 2026-08-29 (re-measurement, #6103) |
|---|---|---|
| Measured false positives | **12 / 25 = 48%** | **0** |
| Class (a): needle inside an identifier | 5 | **0** — killed by δ-A guard 3 |
| Class (b): `#N` is a PRD-relative index | 6 | **0** — killed by §8.2 `prd_relative_cite` |
| Class (c): stale prose | 1 | **0 remaining** — 2 found, both truth-corrected at source |
| Underlying class-(b) idiom repo-wide | 337 occ / 64 files | **341 occ / 72 files** |
| `ptodo-baseline.txt` fingerprints | 5 | **11** (seeded in the same commit, §6.6) |

Three things about this row are worth stating plainly, because a later reader could otherwise
over-read the `0`:

- **Classes (a) and (b) are suppressed by grammar, and verified so, not assumed.** The five
  class-(a) lines still exist verbatim in `crates/reify-eval/src/cache.rs`
  (`mark_pending_with_cause` ×3, `mark_pruned_pending`, `pending_cause`) and none appears in
  the live output. The class-(b) idiom was re-swept (same predicate and pathspec as the
  2026-08-07 row) and has **grown** to 341 occurrences over 72 tracked `.rs` files, so the
  structural exposure §16 warned about is larger now than at rejection time — the grammar fix
  is load-bearing, not a formality.
- **Class (c) was fixed at the source, not suppressed.** The re-measurement found **two**
  stale-prose lines (`crates/reify-eval/src/detectors.rs`, "deferred to task μ, #5062", which
  the referenced doc comment itself calls "obviated, not deferred"; and
  `crates/reify-test-support/src/temp_dirs.rs`, a present-tense "had not yet landed on `main`"
  narrative that its own next clause contradicts). Each stated something its own code
  falsifies, so both were **truth-corrected** — fixing a falsehood, not dodging a detector.
  One further line (`crates/reify-eval/tests/node_traits_boundary.rs`, "obsolete as written
  rather than merely pending") is a genuine non-deferral and took the sanctioned
  `// ptodo:allow` escape (§6.8). Nothing was suppressed by widening a guard to fit.
- **The seeded baseline is hand-inspected and known-clean.** All 6 new fingerprints are
  genuine deferrals citing terminal tasks — `diagnostics.rs` ×2 (#2947 `cancelled`),
  `elastic_result.rs` (#3787 `done`), `elastic_static.rs` ×2 (#4092 `done`),
  `engine_build.rs` (#3437 `done`) — with statuses cross-checked independently of the
  detector's own DB read. That matters because §6.6 makes a seeded false positive permanent
  by design; the 12 known-wrong entries the 2026-08-07 ruling refused to seed are exactly the
  entries that do not exist here.

*The FP economics that made this a `no` in August are therefore not merely tolerated — they
are gone.* Anchoring on a bare cite is still not intrinsically low-FP (the falsified claim in
the next block stands); it became acceptable only once the cite grammar stopped mis-reading a
337→341-line PRD-relative idiom as task citations.

**2026-08-30 — post-review correction (task #6103).** The §8.2 `N ≤ 99` bound was hoisted to
govern **all three** families; it had been spelled only inside family 3, leaving families 1
and 2 unbounded. The motivating counterexample is the one tracked line repo-wide that puts a
real task id in family-2 register — `invariant #5238` (#5238 `done`) at line 1490 of
`crates/reify-eval/tests/engine_eval_commit_migration.rs` — where an unbounded family
downgraded a High `orphaned` finding to Medium `malformed-cite` on a marker line, or erased
it outright in the cite-anchored δ-B lane, purely on which noun preceded the `#`. Zero recall
cost: the re-measured per-family maxima are 11 / 18 / 27, all inside the bound. The live
fingerprint set was verified **unchanged at 11** by an exact-set diff in BOTH directions —
deliberately not the one-way `comm -23` subset oracle, which by construction cannot see a
LOST finding and is exactly what let this defect through — so the §6.6 baseline is untouched
and no re-seed was needed.

**Measurement method for the digit bound (re-run 2026-08-31).** The bound is decided by one
enumeration over tracked `.rs`, and both predicates are recorded here so a later reader
re-runs the same thing rather than a plausible variant:

| Predicate | Hits | Reading |
|---|---|---|
| `git grep -nE '\btasks? #[0-9]{4}\b' -- '*.rs'` | 2042 | genuine four-digit task cites |
| `git grep -nE '\btasks? #[0-9]{1,2}\b' -- '*.rs'` | 303 | one/two-digit PRD-relative cites |
| `git grep -nEi '(§[0-9.]*\|\b(OQ\|DD\|Q\|T))#[0-9]{3,}' -- '*.rs' ':!crates/reify-audit/*'` | 0 | family 1 has no live three-or-more-digit exposure |

Two caveats a re-run must carry, both learned by hitting them. (1) The third predicate needs
the `':!crates/reify-audit/*'` exclusion: without it the detector's own synthetic four-digit
controls (`§7#4553`, `T#4553`, …, in `prd_relative_cite_negatives`) match their own sweep and
it returns 5, not 0. (2) The counts drift with the corpus — they were 2039 / 307 on
2026-08-30 — so they are evidence for a *split of two orders of magnitude*, not figures to
assert. The only genuine sub-four-digit ids are the legacy #333, #479 and #630, all ≥ 100,
which is what the `N ≤ 99` bound turns on.

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

### Scope note — the deferred half of the signal is now DELIVERED (2026-08-29, task #6103)

The originally-scoped user-observable signal named three sites. `engine_build.rs:12891`
(`orphaned`, cite #4744 `done`) **was delivered by #6087**.
`crates/reify-core/src/diagnostics.rs:3991` and `:4046` (cite #2947 `cancelled`) are
reachable only by δ-B and therefore **moved to the follow-up** under the 2026-08-07 ruling;
they were not a miss and were not silently dropped.

**Both are now delivered by #6103**, at their drifted current lines `:4279`
(``/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring``) and
`:4334` (``/// `dispatch_volume_mesh` (blocked on task #2947).  The future dispatcher will``),
each reported as a High `orphaned` finding and seeded into `ptodo-baseline.txt`. The line drift
since 2026-08-07 is immaterial to the gate: `fingerprint()` is line-number-erased by §6.6
design, which is precisely why that erasure exists. The originally-scoped signal is complete;
nothing from it remains outstanding.

### Revisit condition — δ-B half DISCHARGED 2026-08-29 (task #6103)

The δ-B trigger is spent and must not be re-armed. Its precondition — "§8.2 can classify a
PRD-relative `#N` as `malformed-cite` rather than a task cite" — is met, the 25-hit
population was re-measured *before* adopting anything, and the result is the dated
disposition block above.

The **standing** half survives unchanged and governs every future lane: any further widening
of §8.1 must arrive with the same shape of evidence — a fresh live-corpus enumeration, a
hand-inspected FP count, and a dated row here, including a row when the answer is no.

## 17. Amendment 2026-08-12 (task #5654): inverse lane distinguishes renamed from deleted

**DECISION: the ζ inverse lane reports a renamed-not-deleted `metadata.files` citation as
its own kind, `task-cites-renamed-path`, carrying old path + new path + commit. A class
fix, chosen over another instance sweep of the live backlog.**

**Measured evidence.**

1. **The instance backlog has a half-life in hours.** Re-measured 2026-07-28 at HEAD
   `3e54addf4a`: 350 non-terminal master tasks, 301 `metadata.files` paths absent from the
   tracked set, **12** live `task-cites-deleted-path` findings — with **zero** overlap
   against the 24 enumerated one day earlier. A sweep fixes the 12 it can see and is stale
   before it lands; only a detector change survives the churn.
2. **Half the findings were renames, not deletions.** 6 of those 12 were exact `R100`
   renames landed by #5477, i.e. the file is still in the tree under a new name and the
   citation is repointable — reported to the reader as "deleted" with no pointer to where
   it went. 2 renamed paths accounted for 6 citing tasks, which is also the argument for
   the per-run memo on the added `git show`.
3. **The mechanism resolves the real case.** `git show -M --name-status --format=
   60be72d922` prints two status lines, the second of which is
   `R100<TAB>crates/reify-compiler/tests/geometry_chunk_smoke.rs<TAB>crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs`
   (the first is an unrelated `A` line — which is why the parse scans every line rather
   than only the first), and `git log -1 -- <old path>` returns that same sha — so the two
   seam calls compose on the commit the lane already resolved, with no history walk.
4. **Every degenerate input measured collapses to the unchanged deleted kind.** A genuine
   delete prints only `D<TAB><path>` lines; a merge commit prints **0** lines (`git show`
   defaults to `--cc`); a bogus sha exits non-zero with `fatal: bad object`, which
   `RealGitOps::run` turns into `Err` and `run_or_warn` into `None`.

**Outcome.** `GitOps::rename_target_for_path(path, sha)` shells `git show -M
--name-status --format= <sha>` through the existing `run_or_warn` (no new
`Command::new("git")` call site, so `git_env.rs`'s sanitization and sweep-status inventory
are inherited unchanged), and `resolve_inverse` emits `task-cites-renamed-path` (Medium,
§8.4) when — and only when — an `R` line's old side is the cited path AND the target is
still present in the tracked set, per the same `path_present_in_tracked` helper the cited
path is tested with. Findings stay keyed on the numeric task id, so inverse findings
remain outside `ptodo-baseline.txt` (both `ptodo-baseline-gen` and the ratchet test filter
on `is_swept_ext(task_id)`) and no ratchet, `VALID_KINDS`, or generator change was needed.
Scenarios 17/18/19 (§9) pin the split; the real-git seam has its own temp-repo test.

**Known limits, recorded honestly.** (a) A rename landed **directly in a merge commit** is
not detected — `git show` prints no diff for a merge — and degrades to
`task-cites-deleted-path`. (b) **Copies** (`C` status) are not resolved, because only `-M`
is passed. Both are misses, never mislabels.

**Revisit condition.** Revisit if a live sweep shows a material share of absent-path
findings whose rename landed inside a merge commit (the fix would be `-m --first-parent`
on the `git show`, at the cost of a wider diff per lookup), or if `C`-status copies show
up in practice. A further inverse kind must arrive with the same shape of evidence: a
dated live-corpus measurement, the fail-safe argument for why git failure cannot
manufacture it, and a row here.

## 18. Assessment 2026-08-28 (task #6859): baseline liveness assertion — NO

**DECISION: NO.** The §6.6 ratchet oracle stays `comm -23 <live> <baseline>` (subset-of).
No second `comm -13 <live> <baseline>` (baseline ⊆ live) assertion is added — in neither
the full set-equality form nor the structural-kinds-only variant (c) below. This section
is the **single home** for that ruling; §6.6 carries a pointer, not a restatement.

**The question.** The ratchet asserts only that the live violation set is a *subset* of
the committed baseline. Nothing asserts the converse, and two consequences follow — both
real: (1) there is **no drain forcing function** — a grandfathered entry may sit in
`ptodo-baseline.txt` forever at zero cost; (2) a grandfathered fingerprint is a
**re-entry permit** — since fingerprints erase line numbers (§6.6), the same marker text
may be re-introduced *anywhere in the same file* without the gate noticing. Should the
ratchet also assert `baseline ⊆ live`?

### Measurements (2026-08-28, this branch tip — re-measured, not copied from analysis)

| Measure | Value |
|---|---|
| Degraded live set (`env -u REIFY_PTODO_TASKS_DB` — the mode the gate actually runs in) | **4** fingerprints: `untracked` ×3, `malformed-cite` ×1 |
| Committed `crates/reify-audit/ptodo-baseline.txt` | **5** fingerprints |
| `comm -13 <live> <baseline>` (baseline entries NOT live) | **exactly 1** — the `orphaned` entry (`engine_build.rs`, cite #4744 `done`), a DB-dependent liveness-lane kind that §6.7 drops in the no-task-DB mode |
| `comm -23 <live> <baseline>` (the implemented oracle) | **empty** — green |
| Scan evidence, same run | `@@PTODO_SCAN@@ files_scanned=3069 markers_examined=42` |
| Fingerprint **multiplicity** in the tree | `T12 layer-B seam …` **×8**; `deferred to task 4050` ×3; `GHR-ζ` ×1; `RBD-ε RNEA` ×1; `pending task #4744` ×1 |
| **Churn** since 2026-06-01, the three baseline-bearing files | `engine_build.rs` **351**; `significance_filter.rs` 13; `joints.rs` 17 commits |
| Baseline history | seeded `96961ab605` (2026-08-07), amended `48dbd973a3` (2026-08-09) — **never shrunk** in 21 days |
| DB-present regeneration | exceeded **5 minutes** without completing (the ζ inverse lane walks git history per finding). Measured at analysis time and deliberately not re-run: regenerating the baseline is **not** a fast local action |

### 1. The premise is false — set-equality is not a drain forcing function

Consequence (1) above is real, but the proposed mechanism does not address it. Set-equality
constrains the **baseline** to track the **live set**; it places *zero* pressure on the live
set to shrink. A tree in which all five entries stay live forever satisfies set-equality
forever. Recording this correction is the most valuable output of this assessment: it stops
a future reader from re-proposing set-equality as a drain mechanism. **The drain mechanism
is doing the work** — fixing the markers — not guarding it.

### 2. The re-entry permit is real, but the mechanism's reach is anti-correlated with the risk

`ptodo-baseline-gen` dedupes through a `BTreeSet`, so a fingerprint leaves the live set only
when its population in the file reaches **zero**. Crossing multiplicity with churn:

- `engine_build.rs :: untracked :: T12 layer-B seam …` — **8 copies**, in a file at ~351
  commits/quarter, where copy-paste re-introduction of byte-identical rationale text is a
  **demonstrated mechanism**, not a hypothesis: `1812b5cce9` added 4 (2026-05-30) and the
  later `c7bd324106` added 4 more the same day. This is where the permit is *maximally*
  exercisable — and set-equality is **inert** here: it needs all 8 copies gone.
- `significance_filter.rs` and `joints.rs` — **1 copy each**, 13 and 17 commits. Set-equality
  is fully effective, but the exposure is minimal.

The proposal is strongest exactly where the risk is smallest and weakest exactly where the
risk is largest. Bounding re-entry would require ratcheting the **count**, not set
membership — see the alternatives below.

### 3. The permit is the price of line-number erasure, which §6.6 chose deliberately

§6.6 erases line numbers because "they drift". A grandfather list of **texts** rather than
**sites** is precisely what makes re-entry free. Set-equality does not buy back what
line-number erasure gave away; it only detects the **last** removal. That is not
proportionate to a kind-partition, a new generator machine-contract, and a new false-RED
surface on the hottest file in the crate.

### 4. Constraint (a) is confirmed by measurement — and it bites twice

The committed baseline is generated **with** the task DB and therefore carries liveness-lane
kinds (§6.7) that a degraded structural-only run cannot reproduce. Every context the gate
actually runs in — task worktrees and the `_merge-verify` lane — lacks `.taskmaster/`.

1. **On the assertion.** A naive set-equality assert REDs *today*, in every no-DB context,
   on that one `orphaned` line. Not argued — measured.
2. **On the remediation path.** The natural fix ("just regenerate") is *unavailable in a
   task worktree*: regenerating in degraded mode drops the `orphaned` line, yielding a
   4-line baseline that then REDs the **subset** direction wherever the DB *is* present. A
   correct regen also takes >5 minutes. So the cost lands on third parties — authors of the
   ~4 commits/day into `engine_build.rs` who have never heard of `ptodo-baseline.txt` — with
   no cheap remedy available to them.

### 5. Variant (c) — structural-kinds-only — is implementable, but disproportionate

Restricting the converse assertion to the structural kinds is **measured green today**: all
4 structural baseline entries are live in degraded mode. But per constraint (b) the kind
partition must **not** live in bash. Task #6241 removed exactly that list and restored the
"derivation lives only in `ptodo-baseline-gen`" invariant **in full** (§6.6). Re-establishing
it correctly costs: a generator-emitted machine token or emit-mode, Rust unit tests for the
partition, a shell parse, and a two-directional wiring meta-test — roughly the size of the
#6241 change — to close a hole whose *effective* reach, per §18.2, is **2 cold-file
fingerprints**.

### 6. For the record — set-equality would NOT recreate the #6127 false-RED

State this so a future reader does not re-litigate constraint (b) on the wrong ground. The
retired #6127 floor keyed on the live finding **count** and fired when it hit 0, which is
why a burn-down commit false-RED it. Set-equality compares two **sets**, and a burn-down
that shrinks the baseline in the same diff leaves both sides equal (empty ⊆ empty). The bite
of constraint (b) is the **kind list**, not the false-RED — which is why §18.5, not (b),
carries the decision.

### Alternatives considered and rejected

| Alternative | Why rejected |
|---|---|
| (a) Full set-equality (`comm -13` unconditionally) | REDs today in every no-DB context (§18.4), and does not do the job it is named for (§18.1). |
| (b) Structural-kinds-only (variant (c)) | Implementable and green today, but re-introduces a kind partition that #6241 deliberately removed; ~#6241-sized cost for 2 cold-file fingerprints (§18.5). |
| (c) DB-conditional set-equality — assert only when the task DB is reachable | Needs no kind list at all, which is its appeal. But it would be **dark everywhere the gate actually runs**: both task worktrees and the `_merge-verify` lane lack `.taskmaster/`. A guard that never executes in the gate is not a guard. |
| (d) Count-ratcheting the baseline (`live_count <= baseline_count`) | The **only** shape that actually bounds re-entry, and named here so a future YES starts from it rather than from set-equality. Rejected on cost: it changes the baseline format and re-couples the gate to line-level churn in the hottest file in the crate — strictly worse on friction than the permit it closes. |

### What this ruling does NOT claim

It does not claim the two consequences are acceptable in general — only that **this**
mechanism does not buy them down at a proportionate price. They stand as **known
limitations** of §6.6, now stated in print rather than implied away.

### Revisit condition

Re-open when the cost/benefit inverts — measurably, either:

- **no baseline fingerprint has multiplicity > 1** (at which point set-equality's reach
  becomes total and its false-RED surface is one commit per drain); or
- **a baseline fingerprint is ever observed re-entering after its population reached zero**
  (the permit exercised in fact rather than in principle).

Re-measure the multiplicity and churn table before adopting anything, per the §16 evidence
standard — including a dated row here when the answer is again no.

### Mechanical pin

Prose-only guidance in *this* PRD has a measured track record of failing: §12's η signal was
wrong for two months (esc-6088-2), and §6.6's "keeps the gate green" needed an amendment for
the same reason. §6.6's surviving "shrink-only" phrasing is exactly what invites the naive
`comm -13` edit — the edit this assessment declines. The ruling is therefore pinned by
`tests/infra/test_reify_audit_ptodo_ratchet_superset.sh`, which asserts both directions: a
committed-baseline entry absent from the live set must **not** red the ratchet, and a live
fingerprint absent from the baseline must **still** red it (the second direction exists so
the guard cannot degenerate into a constant-true after the oracle it pins is disarmed).
