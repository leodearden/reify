# `auto:` Constraint-Seeding Gaps — Computed Defaults (C) and Nested Member Access (D)

Status: completion-residual contract for
`docs/prds/v0_3/auto-type-param-resolution-completion.md`. Authored 2026-06-14 in
an interactive `/prd` session, from the esc-4596-161 seeding-scope investigation
(`/home/leo/.claude/spawn-briefs/esc-4596-161-seeding-scope-findings.md`).
Bare-B for the one near-term leaf (a self-contained diagnostic); the two larger
gaps are **deferred** with explicit un-defer triggers, so no B+H design surface is
queued by this PRD. **Pending Leo approval before queueing any tasks** (hard stop —
see §9).

## §0 — Purpose and relationship to the parent

The parent completion PRD landed L1 monomorphization (α/4431), candidate-field
literal seeding (β/4433), BFS joint-recheck soundness (γ/4434), L3 value
population (δ/4435), reconciliation (ε/4436); its integration gate (ζ/4437) is
still pending behind two unowned capabilities surfaced at the gate:

- **4596** (pending) — member access on a `Type::TypeParam` receiver compiles to a
  `ValueRef` instead of a "member access not yet supported" poison
  (`crates/reify-compiler/src/expr.rs:3262`).
- **4599** (deferred) — seed the **parameterized template's own literal params**
  (`bore_radius`, `max_stack`) into the `ConstraintInput` ValueMap at **all three**
  seeding sites; folds Gaps A & B from the seeding-scope investigation.

The same investigation found **two more** gaps in the seeding substrate that 4599
explicitly does **not** fold because they are architecturally distinct (4599's
own description routes them "to a separate /prd (report Gaps C & D)"). This PRD is
that routing. It does **one** near-term thing and **records two deferrals**:

| Gap | Subject | This PRD's disposition |
|---|---|---|
| **C** | computed / derived / `let`-cell **non-literal** value-cell defaults (candidate **and** template side) | **defer** the full fix (needs a compile-time const-folder); **ship a near-term honesty diagnostic** so the silent precision loss is visible |
| **D** | **nested** member access (`seal.foo.bar`) — seeding is one level deep | **defer-as-future** (not reachable: needs 4596 **plus** an unbuilt nested-member feature) |

The honesty diagnostic is the only mechanism this PRD queues. Both core gaps stay
deferred with the un-defer triggers in §7.

## §1 — Goal and user-observable surface

### §1.1 — Near-term (queued) signal — Gap C honesty diagnostic

Today a constraint that reads a cell whose default is a **computed expression**
(e.g. `let clearance : Length = bore_radius - 0.5mm`) is **silently** skipped by
the literal-only seeder, so the constraint is `Indeterminate`, the candidate is
treated as feasible (the sound-but-imprecise monotonic direction), and `auto:`
falls back to a lex-first / ambiguous pick **with no indication the constraint was
not used**. The user cannot tell their constraint was ignored.

**CI-gate signal:** a fixture under `examples/auto/` whose `Bearing<T: Seal>` has a
constraint reading a computed-default cell, run through `reify check`, emits a new
**`W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`** warning (name final at impl) naming
the constraint and the computed cell whose default could not be reduced to a value
at compile time — instead of silence. The warning reaches every existing
diagnostic consumer (LSP hover, MCP `report_diagnostics`, CLI `reify check`) with
no per-consumer change, exactly like the existing `AutoTypeParam*` family.

This is a **CLI-output-difference / user-facing-diagnostic** signal (overlay G2
menu), not "a unit test passes against synthetic input."

### §1.2 — Deferred (not queued) would-be signals

Recorded so a future decompose has the leaf spec ready, and so this PRD is honest
about what is *not* reachable now:

- **Gap C full fix** — a constraint reading a computed default flips
  `Indeterminate → Satisfied/Violated` and `auto:` selects the unique survivor
  (the same signal shape as ζ/4437's `bearing_constraint_select`, but with a
  computed rather than literal threshold). **Reachable only once a const-folder
  exists** (§7.1).
- **Gap D** — a constraint reading a **two-hop** field
  (`constraint housing.seal.thickness < bore_radius`) drives selection. **Reachable
  only once 4596 lands and a nested-member-access feature is built** (§7.2). Today
  even the **one-hop** form is poison (`expr.rs:3262`; 4596 pending), so a passing
  fixture cannot be written — filing a leaf now would freeze a RED test on an
  unbuilt feature (the G6 anti-pattern in esc-3436-210).

## §2 — Background: the seeding substrate (verified on current `main`)

The only ValueMap-resolved constraint leaves are `CompiledExprKind::ValueRef` /
`CrossSubGeometryRef` (`reify-ir/src/expr.rs:903`); any leaf that resolves to
`Undef` makes the comparison `Undef` → `Satisfaction::Indeterminate`
(`reify-constraints/src/lib.rs:167-200`).

The sole seeding primitive is **`seed_candidate_value_map`**
(`crates/reify-compiler/src/auto_type_param.rs:933-949`). Its literal guard
(`:942-944`) extracts **only** `CompiledExprKind::Literal` defaults; its loop is
**one level deep** (`:941-947`, candidate top-level cells); its rustdoc
(`:902-914`) documents the deferral verbatim:

> Only direct literal constants are extracted. Nested member chains and computed
> defaults (e.g. expressions involving other cells) are deferred to PRD §14.3;
> `reify-compiler` cannot run the evaluator, so non-literal defaults cannot be
> reduced to a `Value` at this layer.

It is called at the **three** ConstraintInput-building sites (4599's scope):
`filter_feasible_candidates_seeded` `:845`, γ joint-recheck `:1513`, `dfs_search`
leaf `:2594`. The constraint→ref-cell map used to attribute a constraint to the
cells it reads is **`build_constraint_blame_map`** (`auto_type_param.rs:2288`).

- **Gap C anchor:** the `CompiledExprKind::Literal` guard at `:942-944` skips any
  cell whose default is a computed expression. Applies on **both** the candidate
  side (β/4433's primitive) and the template side (4599's reuse of the same
  primitive), so 4599 inherits the limitation.
- **Gap D anchor:** the one-level loop at `:941-947` never produces a nested key
  `(param_member, field, subfield)`, so a second hop is `Undef` even if it
  compiles. And it does not compile today: member access on a `Type::TypeParam`
  receiver falls through to `make_poison_literal("member access not yet
  supported")` at `expr.rs:3262` (4596 adds the one-hop branch; nothing adds the
  second hop).

## §3 — Scope

### §3.1 — Gap C — computed / non-literal value-cell defaults (full fix DEFERRED)

**Premise (G6) — valid but not worth a near-term full build.** The gap is real and
verified (§2). Closing it end-to-end requires reducing a non-literal default
expression to a `Value` at the `reify-compiler` layer — i.e. a **compile-time
const-folder / partial-evaluator** over `CompiledExprKind` (literals, arithmetic,
unit conversions, references to *other already-const-folded* cells). That is a
**new mechanism with its own risk profile** (numeric/unit correctness, reference
cycles, partiality), categorically different from "more seeding." Three facts make
a near-term full build the wrong call:

1. **No consumer demands it today.** The ζ/4437 demo fixtures use **literal**
   thresholds (`bore_radius : Length = 3mm`, `max_stack : Length = 10mm`) — covered
   by 4599. No queued task, fixture, or dogfood `.ri` needs a computed threshold.
2. **It is gated behind 4596 anyway** on the candidate side: `seal.thickness <
   clearance` cannot evaluate the `seal.thickness` half until 4596 lands, so even a
   const-folded `clearance` would not produce an end-to-end selection signal yet.
3. **The parent PRD already deferred it** (open question §14, item 3 / rustdoc
   "§14.3"): "seed one level … for v0.3; … a measured follow-up **if a real
   constraint needs it**." Same logic applies to const-folding.

**Disposition:** defer the full fix (§7.1 trigger); ship the honesty diagnostic
(§3.3, §6) so the loss is not silent.

### §3.2 — Gap D — nested member access (DEFERRED-AS-FUTURE, no queued work)

**Premise (G6) — not reachable today.** Two unbuilt layers sit under it:
(a) 4596 (pending) for the **first** hop on a `Type::TypeParam` cell, and (b) a
**nested**-member-access feature for the **second** hop plus a nested seeding key
the one-level seeder never produces. There is no fixture that can pass and no
mechanism to attach a leaf to. Filing one now would be a false-premise /
orphan-producer leaf (esc-3436-210 class). **Disposition:** record the deferral and
the un-defer trigger (§7.2); queue nothing.

### §3.3 — Near-term deliverable: the Gap C honesty diagnostic (the only queued mechanism)

A single self-contained leaf (§6): record, in the seeding pass, which cells were
**skipped because their default is non-literal**; cross-reference with
`build_constraint_blame_map` so that when a constraint reads such a cell, emit
`W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` naming the cell and the constraint. No
const-folder, no new `Type`, no grammar. Sequenced **after 4596 and 4599** so its
demonstrable signal is a real candidate-disambiguating constraint and the only
remaining skip-reason is the non-literal default (precision, not noise — §6.2).

### §3.4 — What this PRD does NOT add

- A const-folder / partial-evaluator (deferred — §7.1).
- Nested-member-access seeding or the nested-member compile feature (deferred —
  §7.2; the compile feature is not even this PRD's to own).
- Any change to 4596 (member-access node) or 4599 (literal sibling-param seeding) —
  they are separate, already-filed; this PRD **depends on** both for the diagnostic.
- Any change to the `Type` representation — that is the esc-4312 substrate PRD's
  deliverable (§8). Both gaps here are **value-level**, not type-level.
- Ambient-default (`default Material`) seeding — eval-time injection, collapses
  into the Gap C class per the investigation's non-gap analysis; no new class.

## §4 — Pre-conditions (G3 / G4)

| Pre-condition | Owner | Status (2026-06-14) | Gate phase |
|---|---|---|---|
| `seed_candidate_value_map` + the three call sites | β/4433 (done) + 4599 (deferred) | landed / deferred | substrate for the diagnostic |
| Member access on `Type::TypeParam` → `ValueRef` (one hop) | **4596** | **pending** | hard prereq for the diagnostic's *demonstrable* fixture (candidate side) |
| Parameterized-template literal-param seeding at all three sites | **4599** | **deferred** | precision prereq — makes the diagnostic fire only on genuinely non-literal skips |
| `build_constraint_blame_map` (constraint → ref cells) | landed (`auto_type_param.rs:2288`) | landed | substrate for the cross-reference |
| `AutoTypeParam*` diagnostic family + severity/format contract | landed (`crates/reify-core/src/diagnostics.rs`) | landed | substrate for the new `W_*` code |
| Compile-time const-folder / partial-evaluator | **none — does not exist** | absent | **why Gap C full fix is deferred** (§7.1) |
| Nested member access on resolved/TypeParam cells (2nd hop) | **none — does not exist** | absent | **why Gap D is deferred** (§7.2) |

**G3 grammar gate:** no novel syntax. The repros use `let x : Length = a - b`,
`param y = base * 2`, and member-access chains `seal.foo.bar`, all of which parse
today (the failure is semantic/compile, not a parse error). Re-confirm 0-ERROR on
the diagnostic fixture at decompose.

## §5 — Resolved design decisions

**(5.1) Gap C: ship a diagnostic now, defer the const-folder.** The honest v0.3
behaviour is *visible* imprecision, not silent imprecision. A const-folder is a
real mechanism with no current consumer; a diagnostic is reachable from existing
substrate (blame map + seeding pass) and turns a silent precision loss into a
user-facing warning. (Leo, 2026-06-14.)

**(5.2) Gap D: defer-as-future, queue nothing.** Not reachable; a leaf would freeze
a RED test on an unbuilt feature. Record the trigger instead.

**(5.3) The diagnostic is a Warning, not an Error.** The monotonic design keeps
selection **sound** (`Indeterminate` = feasible never picks an *infeasible*
candidate, only a less-disambiguated one). The condition is a precision loss, not
an unsound result — `W_*`, not `E_*`.

**(5.4) Sequence the diagnostic after 4596 + 4599.** 4599 ensures literal siblings
are seeded, so a remaining constraint-reads-skipped-cell condition is *genuinely*
the non-literal-default case (no false positives from unseeded literals). 4596
makes the candidate-side member access compile, so the demonstrable fixture is a
real selection constraint, not an artificial template-only one. Wire the leaf
`depends_on 4596, 4599` at decompose.

**(5.5) Value-level, not type-level.** Both gaps concern the **ValueMap** the
constraint checker reads, not the `Type` representation. The esc-4312 substrate
(type-args-at-type-level + associated-type projection) is orthogonal and owned
there; this PRD neither needs nor touches a new `Type` variant (§8).

## §6 — Contract: the Gap C honesty diagnostic (near-term leaf)

### §6.1 — Mechanism

1. Add a sibling to `seed_candidate_value_map` (or extend it via an out-param) that
   collects, per template (candidate **and** parameterized), the set of cell
   members whose `default_expr` is `Some(expr)` with `expr.kind` **not**
   `CompiledExprKind::Literal` — the cells the literal guard at `:942-944` silently
   skips. Keyed the same way as the seeds so it lines up with the ValueMap.
2. After the per-candidate / joint `ConstraintInput` is built and checked, use
   `build_constraint_blame_map` (`:2288`) to get each constraint's referenced ref
   cells. For any constraint whose blame set intersects the skipped-non-literal set,
   emit `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` naming the constraint, the cell,
   and (for the message) that its default is a computed expression not reducible at
   compile time.
3. Register the new `DiagnosticCode` variant
   (`AutoTypeParamConstraintUnevaluated`) in `crates/reify-core/src/diagnostics.rs`
   beside the existing `AutoTypeParam*` codes, with the standard severity/format
   contract, so it flows to every consumer unchanged.

### §6.2 — Invariants

1. **No false positives from unseeded literals.** Gated on 4599: every literal
   sibling is seeded, so the only blame→skip intersection is a genuinely
   non-literal default.
2. **No noise on the stub path.** The condition is checker-independent (a cell IS
   skipped regardless of checker), but to avoid firing where the real checker is
   not even consulted, the emit is scoped to the resolution path that the
   structure-sub-component caller drives with the real checker (mirrors the
   parent PRD's β/γ stub-no-op discipline). Tactical detail in §10.
3. **Sound-direction preserved.** The diagnostic changes no selection outcome — it
   only *reports*. A module with no computed-default-reading constraint is
   byte-identical to today.

### §6.3 — Observable signal (LEAF)

`examples/auto/bearing_computed_default_unevaluated.ri` (NEW) — a `Bearing<T:
Seal>` whose constraint reads a computed-default cell — run through `reify check`
emits `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` naming the cell and constraint; a
sibling fixture using a **literal** threshold emits **no** such warning (negative
control). Regression test asserts both.

## §7 — Deferral register (un-defer triggers + conditional decomposition sketch)

### §7.1 — Gap C full fix — compile-time const-folder

**Un-defer when** a real `.ri` / dogfood constraint needs a **computed** threshold
for `auto:` selection (the honesty diagnostic from §6 is the in-product tripwire
that will surface such demand), **or** a const-folder is built for another reason
and Gap C can ride it. **Re-run `/prd` author** at that point — a const-folder is a
G5 high-stakes mechanism (numeric/unit correctness, reference cycles, partiality)
deserving B+H, not a bare leaf.

**Conditional decomposition sketch (do not queue now):**
- *cf-α* — const-folder over `CompiledExprKind` (literals, arithmetic, unit
  conversion, references to already-folded sibling cells; partial — bail to
  "unevaluable" on anything outside the closed set, never guess). Crate:
  `reify-compiler`. Signal: a unit test folds `bore_radius - 0.5mm` → `2.5mm`;
  unevaluable expressions return a typed "not const" result, not a panic.
- *cf-β* — wire the folder into all three seeding sites so a folded default is
  seeded as a `Value`; the §6 diagnostic now fires only on *genuinely* unevaluable
  (e.g. eval-only) defaults. Signal: `bearing_computed_default_select.ri` flips
  `Indeterminate → Selected` (where today it warns).

### §7.2 — Gap D — nested member-access seeding

**Un-defer when** 4596 has landed **and** a nested-member-access feature on
resolved/TypeParam cells exists (or is co-scoped). Until both, Gap D has no
substrate to attach to. **Re-run `/prd`** then; the nested-member *compile* feature
is likely a separate PRD this one would *consume*, not own.

**Conditional decomposition sketch (do not queue now):**
- *nm-α* (likely a different PRD) — compile a second member hop on a
  resolved/TypeParam-derived receiver to a `ValueRef`.
- *nm-β* — extend the seeder to recurse one more level, producing the nested key
  `(param_member, field, subfield)` for fields that are themselves structures with
  literal defaults. Signal: `constraint housing.seal.thickness < bore_radius`
  drives selection.

## §8 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/auto-type-param-resolution-completion.md` (parent) | completes | the residual seeding gaps its §14 open-question 3 deferred | this PRD | this PRD's diagnostic + the §7 register close the open question's reporting half |
| **4596** (member-access-on-TypeParam node) | depends-on | the diagnostic's candidate-side fixture needs one-hop member access to compile | 4596 | **pending** — wire the §6 leaf `depends_on 4596` |
| **4599** (3-site literal sibling-param seeding) | depends-on | seeds literal siblings so the diagnostic fires only on non-literal skips | 4599 | **deferred** — wire the §6 leaf `depends_on 4599` |
| **β/4433** (candidate-field literal seeding) | shares primitive | `seed_candidate_value_map` literal guard is Gap C's anchor on the candidate side | 4433 (done) | do **not** re-open; cite |
| **ζ/4437** (integration gate) | independent | the §6 diagnostic could join ζ's `examples/auto/` set as a regression fixture, but does **not** block ζ | 4437 (pending) | optional fixture add at decompose |
| `docs/prds/type-args-and-assoc-type-projection.md` (esc-4312 **type-system substrate PRD**: type-args-at-type-level + associated-type projection) | **orthogonal, named seam** | **type-level** `Type::Applied`/`Type::Projection` variant (`reify-core/src/ty.rs:108`) + `::` projection (`type_resolution.rs:818-845`); this PRD is **value-level** ValueMap seeding (`auto_type_param.rs` + `expr.rs`) | esc-4312 PRD | **running in parallel** (draft on disk, at/near decompose) — see seam note below |

**Seam note — esc-4312 (the PRD Leo flagged as concurrently running; verified
against its on-disk draft, not just its brief).** Both PRDs descend from the same
`auto-type-param` family and both name auto-type-param resolution as a consumer, so
the seam is real — but the layers are **disjoint**:

- **No core edit clash; one benign shared file.** esc-4312's migration (its §5)
  touches `reify-core/src/ty.rs` (the `Type` enum) + ~11 **exhaustive `Type`-match**
  sites (`reify-eval/src/{engine_eval,lib}.rs`, `reify-compiler/src/{type_compat,type_resolution}.rs`,
  `reify-expr/src/lib.rs`, `conformance/checker.rs`) + grammar + `kinematic.ri` +
  `joint_signatures.rs`. **Its migration table does not include `auto_type_param.rs`**,
  which is this PRD's near-term core edit region (seeding/blame) — so the seeding
  mechanism is genuinely disjoint. The **one** shared file is
  `reify-core/src/diagnostics.rs`: esc-4312 appends `E_TYPE_ARG_ARITY` /
  `E_TYPE_ARG_BOUND` to the `DiagnosticCode` enum; this PRD appends
  `AutoTypeParamConstraintUnevaluated`. Distinct variants → at worst a trivial
  enum-append merge, not a semantic clash — and this PRD's leaf is gated behind
  4596/4599 (unlanded) so it cannot land concurrently anyway.
- **No capability overlap.** esc-4312 distinguishes `Coupling<Prismatic>` from
  `Coupling<Revolute>` and projects **associated types** (`P::MotionValue`, a
  *Type*, via `::`). This PRD seeds **runtime values** for constraints and, for
  Gap D, reads **value fields** (`seal.foo.bar`, a *Value*, via `.`). Type-level
  `::` projection ≠ value-level `.` member access — adjacent-sounding, different
  planes. esc-4312 explicitly scopes itself **not** to "fix auto-type-param," only
  to be designed so it *can* serve it; this PRD reciprocally does not touch the
  `Type` representation.
- **One forward-compat coupling, not a present clash.** If Gap C's const-folder
  (§7.1) or Gap D's nested seeding (§7.2) is ever un-deferred, the
  `Type::StructureRef` world it touches may by then have migrated to esc-4312's
  type-arg-carrying variant (esc-4312 warns its `StructureRef` migration fallout is
  large). That is a *future* coordination at un-defer time, recorded here so the
  re-run `/prd` checks esc-4312's landed shape first. The near-term diagnostic adds
  and matches **no** `Type` variant, so it is immune to that migration.

## §9 — Decomposition plan + hard stop

**Hard stop:** authoring + committing this PRD is done; **do NOT queue any tasks**
until Leo signs off (brief instruction). The plan below is what a *later* decompose
would file — recorded, not executed.

- **Queued (1 leaf):** the Gap C honesty diagnostic (§6). Crates:
  `crates/reify-compiler/src/auto_type_param.rs` (skip-set collection +
  blame-map cross-reference at the resolution path), `crates/reify-core/src/diagnostics.rs`
  (new `AutoTypeParamConstraintUnevaluated` code), `examples/auto/` (+ regression
  test). Deps: `4596`, `4599`. Observable signal: §6.3.
- **Not queued (deferred, §7):** Gap C const-folder (`cf-α`, `cf-β`) and Gap D
  nested seeding (`nm-α`, `nm-β`) — each re-enters `/prd` author on its un-defer
  trigger.
- **At decompose:** commit the capability manifest beside this PRD
  (`auto-type-param-constraint-seeding-gaps.capability-manifest.md`) binding the
  diagnostic leaf's signal to evidence (the new `W_*` code wired into the
  resolution path; the fixture parsing 0-ERROR; negative control). The deferred
  sketches carry no manifest bindings until un-deferred.

## §10 — Open questions (tactical; surfaced, not blocking)

1. **Stub-path scoping of the diagnostic.** §6.2 scopes the emit to the
   real-checker resolution path to avoid firing where the checker is not consulted.
   Alternative: fire checker-agnostically (the skip is a structural fact). Decide at
   impl — the real-checker scope is the conservative default. Confirm it does not
   suppress the warning in `reify check` headless contexts (which may default to the
   stub per parent PRD §14 item 4).
2. **Diagnostic granularity.** One warning per (constraint, skipped-cell) pair vs.
   one aggregated per declaration. Per-pair is more actionable; aggregate is less
   noisy on a template with many computed cells. Decide at impl.
3. **Final code name.** `W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED` vs.
   `…SEED_SKIPPED_NONLITERAL`. Pick the user-facing-clearest at impl; align with the
   `AutoTypeParam*` family naming.

End of PRD.
