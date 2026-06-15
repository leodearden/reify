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
  `unknown-id` findings. Degrades fail-soft when the DB is absent (§6.7).

Plus a narrow **inverse lane** (§6.3): non-terminal tasks whose `metadata.files`
entries name git-deleted paths.

## 6. Resolved design decisions

### 6.1 Policy: trigger-conditioned perf TODOs — **strict must-cite, no annotation form**

Leo's invariant is universal; an annotation form (`TODO(perf, until: <trigger>)`)
would create a sanctioned untracked class whose trigger conditions are mechanically
unverifiable — exactly the "prose triggers fire silently" rot mode the audit found
dominant. Task **4593** (perf-backlog anchor v4) is the standing citable owner: it owns
being citable, periodically re-checking triggers, and graduating items. Perf TODOs
write `TODO(#4593): <trigger prose>`; the trigger prose stays human-readable, the
tracking is machine-checkable. The detector has **zero** perf special-casing.
(Prior owners — **DO NOT cite**, all terminal: v1 4551 done 2026-06-12 (263502544d);
v2 4590 done 2026-06-13 (5a725c832805); v3 4592 done. Each anchor was retargeted forward
as it closed; 4593 is the v4 holding anchor and its brief defers to this PRD for the rule.
**Lifecycle invariant:** the citable anchor must remain non-terminal for as long as any
perf marker cites it. 4593 is **deferred + DO-NOT-DISPATCH**, so it never advances to
done/cancelled — a permanently non-terminal standing owner. This is what ends the v1→v4
churn, where each prior anchor closed and re-orphaned its surviving cites. Should 4593 ever
need to close, a v5 anchor must absorb the markers and be retargeted before 4593 goes terminal.)

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

### 8.2 Citation resolution

A marker is **cited** iff a `#NNNN` token (`#` + 1–5 digits) appears: (comment
markers) inside the `TODO(...)` parens or after the `TODO:` on the same line; (stubs)
inside the macro's message string, or in a comment on the same line or the line
directly above; (ignores) inside the reason string. Multiple cites: all are validated;
one live cite suffices for tracking.

### 8.3 Violation taxonomy (finding `kind` values)

| Kind | Trigger | Lane |
|---|---|---|
| `untracked` | marker with no citation, excluding `#[ignore]` reasons with no blocker-prose (see below) | structural |
| `malformed-cite` | Greek-letter or PRD-relative cite ("task-5", "task δ"), or legacy form ("task NNNN") | structural |
| `phantom-tracking` | prose claims: "tracked separately", "tracked as a follow-up", "tracked in project memory", "follow-up task will" (case-insensitive) without a cite | structural |
| `bare-ignore` | `#[ignore]` with no reason string | structural |
| `unknown-id` | cite parses but id not in the task DB | liveness |
| `orphaned` | cited task status ∈ {done, cancelled} — reported with cited id + status | liveness |
| `task-cites-deleted-path` | non-terminal task `metadata.files` path absent from tracked set but present in git history | inverse |

**`#[ignore]` reason policy:** reasons containing a cite → liveness-checked; reasons
matching blocker-prose (`pending|not yet|RED:|until |once |blocked`) without a cite →
`untracked`; operational reasons (e.g. "requires OCCT", "probe: run manually",
"timing/benchmark out of CI") without blocker-prose → pass without a cite. The
Task-1622 tool (`reify-test-support`) keeps format-level checks; PTODO owns
citation-liveness — γ wires the split using the existing pub extraction fns.

### 8.4 Severity + exit

All PTODO kinds emit at **Medium** until task η flips `untracked` / `orphaned` /
`bare-ignore` to **High** (hard gate: non-zero exit). η is dispatch-gated on PTODO
reporting zero violations on main. `unknown-id` stays Medium even post-η (a DB-sync
artifact shouldn't hard-fail verify); `task-cites-deleted-path` stays advisory.

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

## 10. Cross-PRD relationship (G4)

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §10 (task 4115 record) | amends | default-sweep membership policy note | **this PRD** (ε) | queued |
| Task 4593 (perf-backlog anchor v4) | this PRD defines policy; 4593 aligns | trigger-conditioned-TODO citation rule (§6.1) | **this PRD**; 4593 brief defers to it | prior owners (DO NOT cite, all terminal): 4551 done, 4590 done, 4592 done; 4593 (deferred + DO-NOT-DISPATCH) is the v4 standing anchor |
| Task-1622 ignore-hygiene tool (`reify-test-support`) | consumes/extends | pub extraction fns; format-vs-liveness split (§8.3) | **this PRD** (γ) | queued |
| `/audit` skill (`.claude/skills/audit/`) | consumed-by | default sweep + severity routing docs | **this PRD** (ε) | queued |
| dark-factory `skills/review-briefing/SKILL.md` checks 5/6 | parallel (process layer) | invariant prose, cross-project | dark-factory (Leo; branch `docs/review-briefing-todo-invariant`, not yet on df main) | informational — no code seam, no dep |

No contested-ownership pairs touched (overlay §G4 list is engine-side).

## 11. Out of scope

- **`State: TODO` → `GAP-OPEN` rename in `docs/architecture-audit/`** — declined;
  Markdown is excluded from the sweep (§6.8), so the 130 taxonomy lines generate no
  noise. Revisit only if θ brings `.md` into scope.
- **Prose-path scanning of task descriptions** (inverse lane stays `metadata.files`-only, §6.3).
- **Softer vocabularies** until θ's FP review clears them (§6.2).
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
  non-zero and verify fail.
- **θ — vocabulary-expansion ASSESS** (dep ε). FP-review of softer vocabularies
  (§6.2: STUB_MSG idiom, "for now", "placeholder", "stub", "XXX", "workaround")
  mirroring 4075/4076/4141 methodology; extend the vocabulary for those that clear;
  record NO-decisions as an amendment commit to this PRD (the 4115 pattern). **Leaf.**
  Signal: the decision record committed to this PRD + (if any cleared) new vocabulary
  live in `--pattern PTODO` with fixtures.

Dependency DAG: α → β → {δ, ζ}; α → γ (also γ ← β); {β, γ, δ} → ε; ε → {η, θ}.

## 13. Open questions (tactical)

1. **Extension list breadth** — `.toml`/`.yml`/`.yaml` comments carry occasional
   TODOs. Suggested resolution: add them in α if the fixture sweep shows signal;
   otherwise θ reassesses. Decide during α.
2. **`unknown-id` grace for freshly-filed tasks** — a cite written in the same
   commit-window as its task filing could race DB sync. Suggested resolution: none
   needed (the DB write is synchronous via fused-memory); revisit only if ε's soak
   shows false `unknown-id`s. Decide during ε soak.
3. **Fingerprint normalization details** (whitespace folding, marker-text truncation
   length). Decide during ε.
