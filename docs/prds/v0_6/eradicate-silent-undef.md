# Eradicate silent undef: provenance backstop, error-exit unification, diagnostic hygiene

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-24
**Provenance:** 2026-07-24 silent-undef/placeholder eradication investigation
(spawned from language-review session a0d342d4; probe census #5360, #5345,
#5385, #5386, #5197, #5390). Key scope/approach decisions in §6 were made by
Leo in that investigation and are recorded here, not re-opened.
**Normative substrate:** `docs/legibility/design-invariants.md` — this PRD
implements **INV-SF-1** (`undef-has-provenance`), **INV-SF-2**
(`error-severity-exits-nonzero`) and **INV-SF-6** (`diagnostics-carry-codes`).
(Doc landing via task 5395 / merge request mr-bc4e684b; if not yet on main,
read it from `origin/task/5395`.)

## 1. Goal

Three user-observable guarantees, one per leg:

- **Leg A (INV-SF-1).** No design ever evaluates to a silent undef. Every
  root undef cell either carries a recorded `UndefCause` that the CLI/GUI/LSP
  can name, or trips a loud backstop diagnostic (`W_UNDEF_UNEXPLAINED`)
  telling the user the engine lost provenance — converting every future
  cause-coverage gap from silent to loud. `reify check` joins eval/GUI/LSP in
  reporting undef causes; a new opt-in `--deny undef` makes undef outputs fail
  the command.
- **Leg B (INV-SF-2).** Any Error-severity diagnostic on any channel makes
  `reify check` exit nonzero — same rule `cmd_eval`/`cmd_build` already apply
  — with a hard gate NOW plus an enumerated, ratcheted burn-down allowlist
  (target: zero entries) for the known expected-on-healthy-path Error
  emissions. The per-code bolt-ons are deleted once subsumed.
- **Leg C (INV-SF-6).** Every *new* Warning/Error diagnostic must carry a
  `DiagnosticCode`, enforced mechanically (reify-audit `PDIAG` pattern with a
  per-file baseline ratchet over the ~362 existing ctor sites / 67 coded);
  existing sites migrate opportunistically. A severity policy doc states what
  earns Error vs Warning vs Info.

## 2. Background (probe-anchored, 2026-07-24)

All probes re-verified this session against the 2026-07-22 debug binary and
current main (`0d70ef1d5b`):

- `reify eval` on `docs/prds/v0_6/fixtures/silent_undef_generate_geometry.ri`
  prints `P.holes = [undef, undef, undef, undef]` with **no note** and exit 0;
  `reify check` on the same file prints `All constraints satisfied.` (green
  check, zero undef information). Two stacked gaps: (a) the undef-notes loop
  in `cmd_eval` (`crates/reify-cli/src/main.rs`, ~1640) `continue`s past
  cells whose `trace_undef_causes` result is empty — the silent class is
  exactly what gets skipped; (b) a `List` whose *elements* are undef is not
  itself `is_undef()`, so such cells never even enter the notes census.
- `reify eval` on `fixtures/silent_undef_unbound_param.ri` prints
  `note: P.d is undef (because: P.w unbound)` — the tracer half works where a
  cause was recorded. `reify check` on the same file reports nothing: only
  eval (`main.rs:1570/1588`), the GUI, and the LSP
  (`crates/reify-lsp/src/analysis.rs:106`) call
  `set_capture_undef_causes(true)`; `cmd_check` never does.
- `UndefCause` (`crates/reify-ir/src/value.rs:3855`) has five variants
  (Unbound, AwaitingSolve, SolveFailed, OpContractFailed, UserUndef) against
  ~1,350 `Value::Undef` construction sites. Known unrecorded-root classes:
  kernel-query failure (#5197), instance member-resolution failure through
  nested subs (#5360), lambda/collection geometry eval (#5385).
- `check_no_stale_undef` (`crates/reify-eval/src/invariants.rs:106` + the
  `Engine` wrapper at :268) already computes exactly the backstop predicate —
  undef cell, all deps determined, no recorded origin — but is wired only
  into the debug-gate corpus harness
  (`crates/reify-eval/tests/no_stale_undef_invariant_gate.rs`).
- `cmd_check` decides exit from constraint outcomes alone (`finish_check`,
  `main.rs:2719`) plus two bolt-ons: `GdtIllegalModifier` code match
  (`main.rs:~672`) and `E_DFM_` message-prefix match (`main.rs:~688`,
  `dfm_has_error_diagnostic`). The bolt-on comment itself names the co-resident
  code-less healthy-path Error this PRD's allowlist must carry at seed time:
  FEA "no registered compute trampoline" (`engine_compute.rs:654`,
  `engine_admin.rs:1519`, `engine_eval.rs:8301`). #5386's repro (non-pub
  structure: `error:` text printed, exit 0) is one instance of the general
  gap. `cmd_eval`/`cmd_build` already gate on `Severity::Error`
  (`main.rs:1661` region; `build_is_success` `main.rs:2286`, task 4458).
- reify-audit's per-pattern module layout (`crates/reify-audit/src/ptodo.rs`
  etc.) plus the `tests/infra/test_reify_audit_ptodo.sh` hard-gate precedent
  is the enforcement substrate Leg C mirrors.

## 3. Sketch of approach

### Leg A — undef-provenance backstop

1. **Census helper** (`reify-ir`): `Value::contains_undef()` — true for
   `Value::Undef` and for any collection value transitively containing one.
   Both the backstop and every undef-notes loop use this one predicate
   (no lock-step duplication).
2. **Engine backstop** (`reify-eval`): generalize `check_no_stale_undef` from
   debug-harness-only to a production post-eval pass, active whenever
   `capture_undef_causes` is on. For every *root* undef-carrying cell (deps
   all determined, no recorded origin, empty `trace_undef_causes`) emit
   `W_UNDEF_UNEXPLAINED` (Warning, coded, naming the cell) into the engine
   diagnostics stream — eval, check, build, GUI and LSP all surface it with
   zero per-surface work. Healthy designs never see it: undefs with recorded
   causes (unbound params etc.) are not unexplained; the backstop fires only
   on provenance gaps, i.e. engine bugs.
3. **CLI loud-path** (`reify-cli`): flip the empty-cause `continue` in
   `cmd_eval`'s undef-notes loop to print
   `note: <cell> is undef (cause unrecorded — see W_UNDEF_UNEXPLAINED)`, and
   widen the notes census from `is_undef()` to `contains_undef()`.
4. **check joins** (`reify-cli`): `cmd_check` (both the no-purpose and
   `--purpose` paths) enables cause capture and, after `finish_check`, prints
   the same undef notes for **root** undef-carrying cells (deduped, stderr);
   `--explain-undef` (already on eval) is accepted by check and widens to all
   cells.
5. **New causes for known gaps** (additive provenance only — the functional
   fixes stay with their owning tasks):
   `UndefCause::MemberResolutionFailed { path, span }` recorded where
   instance-level member projection materializes Undef (#5360's silent seam,
   `unfold.rs`/`engine_eval.rs`), and
   `UndefCause::CollectionEvalFailed { op, detail, span }` recorded (via the
   existing reify-expr undef-cause sink) where lambda/collection eval yields
   undef elements (#5385's seam). `UndefCause::KernelQueryFailed` and the
   `record_op_contract_failures` kernel-query skip (the misattribution fix,
   `engine_eval.rs:~3300`) are **owned by #5197** (its items i–iii; its dep
   #5211 is done) — not duplicated here.
6. **`--deny undef`** (Leo-approved): opt-in flag on eval and check; the
   command exits nonzero when its *default undef census* is non-empty
   (eval: printed-value cells; check: root undef-carrying cells). Sits
   alongside `--strict` (indeterminate promotion), composes with it.

### Leg B — error-severity exit unification (decided: hard gate now + burn-down allowlist)

1. **Gate**: after `finish_check`, escalate to exit FAILURE when any
   diagnostic has `Severity::Error` and is not matched by
   `CHECK_ERROR_EXIT_ALLOWLIST` — converging check on the same severity rule
   as `build_is_success`/`cmd_eval` (shared helper, not a third copy).
2. **Allowlist**: in-code, enumerated, each entry = matcher
   (`DiagnosticCode`, or message-prefix *only* for code-less legacy
   emissions) + disposition (demote to Warning | recode | fix the path) +
   PTODO-grammar cite of the live burn-down task. A ratchet unit test pins
   the exact entry set — additions are loud diffs; the PTODO gate keeps every
   cite live. Seeded from a corpus run over `examples/` + the test corpus;
   the known seed entry is the kernel-less FEA trampoline Error.
3. **Bolt-on deletion**: the `GdtIllegalModifier` and `E_DFM_` escalations
   are deleted in the same change (both match Error-severity diagnostics, so
   the general gate subsumes them; behavior stays byte-identical for those
   classes).
4. **Burn-down to zero**: a follow-up task clears every seeded entry per its
   disposition. The trampoline disposition (demote/recode per the INV-SF-2
   corollary — kernel-less check is a healthy path, so Error severity is
   wrong there) is **already owned by #5311** (filed 2026-07-20 against
   cmd_check's own documented deferral); the seed entry cites it, and the
   burn-down task depends on it. End state: empty allowlist pinned by the
   ratchet; the mechanism remains as the guard against re-additions.

### Leg C — diagnostic hygiene

1. **PDIAG detector** (reify-audit): new pattern module scanning tracked Rust
   source for `Diagnostic::error/warning` construction sites with no attached
   `DiagnosticCode`; per-file baseline manifest committed in-repo (ratchet:
   count may only decrease; a new/increased code-less site fails
   `reify-audit --pattern PDIAG` and the new tests/infra step). Escape:
   trailing `// pdiag:allow — reason` (mirrors `ptodo:allow`). Migration of
   the existing backlog is opportunistic (per-file, alongside other work) —
   not big-bang.
2. **Severity policy** (`docs/notes/diagnostic-severity-policy.md`): what
   earns Error (never expected on a healthy path — the INV-SF-2 corollary),
   Warning (actionable degradation), Info (debug tier, per #5196's layering);
   the PDIAG failure message cites this doc.

## 4. Pre-conditions

- `docs/legibility/design-invariants.md` on main (task 5395, in merge queue)
  — prose substrate only; no task here reads it at runtime.
- #5211 (OCCT null-shape hardening) — **done**, so #5197 is dispatchable;
  this PRD's Leg A deliberately excludes #5197's scope rather than depending
  on it (see §5).
- No novel grammar: fixtures use existing syntax (verified parsing + running
  this session); no new substrate beyond this PRD's own mechanisms. G3 clean.

## 5. Cross-PRD / cross-task relationship (G4)

| Other work | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #5197 kernel-query undef causes | adjacent | `UndefCause::KernelQueryFailed` + `record_op_contract_failures` kernel-query skip (items i–iii) | **#5197** | pending, dep #5211 done; backstop α is correct before and after it lands (misattributed cells carry a non-empty — wrong — cause until then; the never-overwrite half of INV-SF-1 is #5197's) |
| #5360 nested-sub member reads | adjacent | functional fix (chains resolve) | **#5360** | pending-high; this PRD's γ is additive provenance at the same seam; γ's e2e is disjunctive (value determined OR cause named) so it stays green in either landing order; file locks serialize the worktrees |
| #5385 generate-of-geometry | adjacent | functional fix (constructs work) | **#5385** | pending-high; same disjunctive-test pattern for δ |
| #5386 non-pub structure exit-0 | this PRD generalizes | check Error-severity exit gate | **this PRD (ε)** for the exit rule; **#5386** keeps the compile-time visibility diagnostic | blocked; decompose adds edge 5386 → depends-on → ε so its remaining scope shrinks to the visibility E\_ code |
| #5390 design-health manifest | consumes | undef reason strings (`format_undef_cause`, `undef_tracer.rs`) | **this PRD** owns the reason-string vocabulary (incl. new variants); #5390 consumes | pending; no hard edge — it starts with today's strings by its own text |
| #5196 diagnostic noise floor | adjacent | severity tiering (Info = debug tier) | **#5196** for the persistent-naming de-noise; **this PRD (η)** for the written policy | in-progress; η's policy doc must agree with its landed severity choices |
| #5311 trampoline Error demotion | this PRD consumes | the trampoline diagnostic's severity/recode disposition (`main.rs` cmd_check doc-comment deferral) | **#5311** | pending-low; ε's seed allowlist entry cites it; ζ depends on it |
| Sibling PRD: consumption accounting (INV-SF-3/4) | sibling | none shared beyond the invariants doc; its new compile errors are genuine failures and correctly fail ε's gate | n/a | authored in parallel from the same investigation |
| Sibling PRD: placeholder-type ratchet (INV-SF-5) | sibling | its new diagnostics fall under η's codes-mandatory ratchet automatically | n/a | authored in parallel |

No contested-ownership pair from the audit catalogue is touched; no new
in-engine seam is introduced (all mechanisms live on existing CLI/diagnostic
paths — the §3.N engine-seam catalogue does not apply).

## 6. Resolved design decisions

Decisions marked **(Leo)** were made in the 2026-07-24 investigation and are
recorded, not re-opened.

1. **(Leo)** Leg B is a **hard gate now** with a burn-down allowlist — not
   triage-first. Every allowlist entry carries a disposition and a live task
   cite; the list ratchets to zero.
2. **(Leo)** `--deny undef` is **opt-in** (not default), on eval and check,
   alongside `--strict`.
3. **(Leo)** Leg C (INV-SF-6) is folded into this PRD; migration of existing
   code-less sites is opportunistic, enforcement is for new sites.
4. Backstop lives in the **engine** (generalized `check_no_stale_undef`),
   not per-CLI-command — one implementation serves eval/check/build/GUI/LSP.
   `W_UNDEF_UNEXPLAINED` is **Warning** severity: it reports a reify
   provenance bug, not a user error, and Error would make every future
   coverage gap fail builds under Leg B (too aggressive for a backstop whose
   whole point is graceful loudness).
5. The undef census is **`contains_undef`** (transitive through collections),
   not `is_undef` — the generate-of-geometry probe shows element-wise undef
   evading the cell-level check. One shared helper; every consumer (backstop,
   eval notes, check notes, `--deny`) uses it.
6. γ/δ are **additive provenance with disjunctive e2e tests** ("value
   determined OR cause recorded+named — never silent undef"), making them
   robust to landing order against #5360/#5385, which own the functional
   fixes. No hard dependency edges to those tasks (they must not be delayed
   by diagnostic work); worktree file locks serialize the shared files.
7. Allowlist matcher supports message-prefix **only** for legacy code-less
   emissions; any *new* entry must be code-matched (and Leg C makes new
   code-less emissions unrepresentable going forward).
8. PDIAG ratchet granularity is **per-file counts** in a committed baseline
   manifest (robust to line churn, still localizes regressions), mirroring
   the throughput-sentinel pattern; `pdiag:allow` is the per-site escape.
9. `reify check`'s default undef-notes census is **root cells only**
   (deduped) — bounded noise on par with #5196's direction; `--explain-undef`
   widens. Eval's default census stays printed-values (unchanged semantics,
   widened to contains-undef).

## 7. Contract (compact B+H)

- **Backstop predicate** (α): fires for cell `c` iff
  `value(c).contains_undef()` ∧ every dependency of `c` is determined ∧
  `trace_undef_causes(c)` is empty, evaluated post-eval while
  `capture_undef_causes` is on. Emission: one `W_UNDEF_UNEXPLAINED` Warning
  per root cell, message names the cell id; carried in the ordinary engine
  diagnostics stream. The debug-gate harness keeps its existing
  strict/zero-tolerance use of the same function.
- **Reason strings** (γ/δ + #5390 seam): `format_undef_cause` remains the
  single formatter; new variants extend it. Strings are stable, human-readable
  clauses of the form `<subject> <verb phrase>` (e.g. `member resolution
  failed for self.m.relay`, `collection eval of generate yielded undef
  elements`); #5390 embeds them verbatim in its `reason` field.
- **Allowlist entry** (ε): `{ matcher: Code(DiagnosticCode) |
  MessagePrefix(&'static str) /* legacy code-less only */, disposition:
  Demote | Recode | FixPath, cite: "#NNNN" }` in a single `const` table in
  `main.rs`; ratchet test asserts the exact table contents; every entry's
  TODO cite is PTODO-live. Gate rule: `Severity::Error` diagnostic ∧ no
  allowlist match ⇒ exit FAILURE, applied identically on check's no-purpose
  and `--purpose` paths via one shared helper also used by eval/build.
- **PDIAG baseline** (η): committed manifest mapping
  `crate-relative-file → code-less-ctor-site count`; detector fails on any
  file whose live count exceeds its baseline entry (or that is absent with a
  nonzero count); a fixing diff shrinks the baseline in the same commit.

## 8. Boundary-test sketch

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| 1 | eval of generate-of-geometry fixture | #5385 unfixed (undef elements) | stderr carries an unexplained-undef note naming `P.holes` (backstop) OR, post-δ, the `CollectionEvalFailed` cause; never silence |
| 2 | same fixture, post-#5385 functional fix | elements determined | no note, no backstop warning (predicate can't fire) |
| 3 | `reify check` on unbound-param fixture | — | `note: P.d is undef (because: P.w unbound)` on stderr; exit unchanged |
| 4 | `reify eval --deny undef` on unbound-param fixture | — | exit ≠ 0; without the flag exit 0 (byte-identical output otherwise) |
| 5 | `reify check` on #5386 repro (non-pub structure) | ε landed | exit 1 (Error-severity gate), regardless of #5386's compile-time fix landing |
| 6 | `reify check` on a kernel-less FEA module | allowlist seeded | exit 0 (trampoline entry matched); after ζ: still exit 0 but the emission is Warning/recoded, allowlist empty |
| 7 | GD&T / DFM error modules | ε landed, bolt-ons deleted | exit codes byte-identical to today (general gate subsumes) |
| 8 | new `Diagnostic::error(...)` without a code added to any crate | η landed | `reify-audit --pattern PDIAG` fails; tests/infra step RED |
| 9 | 5360-shaped nested-sub read | any landing order of γ/#5360 | `Parent.echo` determined OR note names the unresolvable member path; never silent |

## 9. Decomposition plan

Greek labels; real ids at decompose. "Signal" = user-observable signal (G2).
All tasks `task_kind=normal`. New gate-resident tests must extend **existing**
test binaries where possible; any new `crates/*/tests/*.rs` binary bumps the
THROUGHPUT-COUNTS sentinel (`docs/notes/verify-scope-throughput.md`) in the
same diff; η's new `tests/infra/test_*.sh` registers its
`run-all-classification.manifest` bucket row in the same diff (overlay
drift-guard rule).

| Label | Title | Crates | Signal | Prereqs |
|---|---|---|---|---|
| α | Unexplained-undef backstop: `contains_undef` census, engine `W_UNDEF_UNEXPLAINED`, loud CLI notes | reify-ir, reify-core, reify-eval, reify-cli | `reify eval` on `silent_undef_generate_geometry.ri` emits the backstop warning + note naming `P.holes` (today: silence) | — |
| β | `reify check` reports undef causes (capture + root-cell notes + `--explain-undef`) | reify-cli | `reify check` on `silent_undef_unbound_param.ri` prints the unbound-cause note (today: nothing) | α |
| γ | `UndefCause::MemberResolutionFailed` recorded at instance member-projection seam | reify-ir, reify-eval | disjunctive e2e on the #5360 repro: value determined OR note names `self.m.relay`; never silent | — |
| δ | `UndefCause::CollectionEvalFailed` recorded at lambda/collection-eval seam | reify-ir, reify-expr, reify-eval | disjunctive e2e on the #5385 repro: elements determined OR note names the collection cell + op; never silent | — |
| ε | Gate `reify check` exit on Error severity; seed burn-down allowlist; ratchet test; delete GD&T/DFM bolt-ons | reify-cli | #5386 repro exits 1; kernel-less FEA fixture still exits 0 via a cited allowlist entry; ratchet test pins the table. G7 waiver: `error-severity-exits-nonzero` — temporary enumerated exemptions are the Leo-decided burn-down mechanism (2026-07-24), ratchet+PTODO-guarded, cleared by ζ | — |
| ζ | Burn the check allowlist to zero (clear all seeded entries per their dispositions; trampoline demotion itself is #5311's) | reify-cli | ratchet test pins an **empty** allowlist; kernel-less FEA `check` emits a coded Warning, not `error:` text | ε, #5311 |
| η | reify-audit PDIAG codes-mandatory detector + baseline ratchet + infra gate + severity policy doc | reify-audit, tests/infra, docs | `reify-audit --pattern PDIAG` runs; a code-less `Diagnostic::error` addition turns the new infra step RED; failure message cites `docs/notes/diagnostic-severity-policy.md` | — |
| ι | `--deny undef` on eval and check | reify-cli | boundary-test row 4: exit flips with the flag, output otherwise byte-identical | α, β |

Out-of-batch edges wired at decompose: `#5386 depends-on ε`; `ζ depends-on #5311`.
Intermediates with consumers: α → β, ι (and every surface that renders
diagnostics); ε → ζ. All eight tasks carry their own CLI-observable signal.

## 10. Out of scope

- #5197's entire scope (KernelQueryFailed, geometry-eval capture, the
  `record_op_contract_failures` kernel-query skip, resolve-handle warning).
- Functional fixes for #5360 (nested member chains) and #5385 (geometry in
  lambdas, `union_all` over a list) — those tasks own making the constructs
  *work*; this PRD owns "if it stays undef, it says why".
- INV-SF-3/4 (consumption accounting) and INV-SF-5 (placeholder ratchet) —
  sibling PRDs from the same investigation.
- Big-bang migration of the ~295 existing code-less diagnostic sites.
- GUI/LSP presentation changes (they inherit the new causes and the backstop
  through existing surfaces for free).
- Default-on strictness (`--deny undef` stays opt-in; revisit only with
  usage evidence).

## 11. Open questions (tactical)

1. **Allowlist seed extent.** The corpus run in ε may surface healthy-path
   Error emissions beyond the trampoline. Each becomes a cited entry with a
   disposition; if one needs deep path work, ζ files a follow-up and re-cites
   rather than blocking. Decide entry-by-entry during ε/ζ.
2. **`--deny undef` and `--explain-undef` interplay.** Decided: deny binds to
   the *default* census regardless of `--explain-undef`. If users want
   deny-over-widened-census, add `--deny undef=all` later — decide on demand.
3. **PDIAG multiline-ctor detection.** Whether a simple paired-grep suffices
   for `Diagnostic::error(...)` chains split across lines, or the detector
   needs a small brace-matching scan — decide in η against the real corpus.
4. **#5390 structured reasons.** Whether the manifest eventually wants the
   `UndefCause` enum serialized (kind + fields) instead of formatted strings
   — decide in #5390 when it lands; the formatter seam here is stable either
   way.
