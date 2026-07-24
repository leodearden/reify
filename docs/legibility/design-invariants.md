# Design invariants

A gate checklist, not an essay. This is reify's adoption of the
dark-factory `docs/legibility/design-invariants.md` convention: `/prd`
decompose's G7 gate and `/review` phase 2 Read this path at run time and
walk every task/finding against each invariant's checkable question. It is
the single normative copy — do not restate invariant text elsewhere; cite
slugs. Stable slug ids are load-bearing (G7 waivers reference them).
Numeric aliases INV-SF-* are prose convenience only.

This first population is the **silent-failure family** (INV-SF-1..6),
from the 2026-07-24 silent-undef/placeholder eradication investigation
(dogfood session a0d342d4 → investigation session, probe evidence in the
task census: #5360, #5345, #5385, #5386, #5197, #5390). Kleene 3-valued
logic and undef-as-value are deliberate design; these invariants target
**silence**, not undef. Conservative degradation (Indeterminate, skip,
refuse) stays correct — but must always leave an observable trace. Other
invariant families may be appended later; keep slugs stable.

## INV-SF-1 `undef-has-provenance`

**Rule**: Every *root* undef cell (one whose undef-ness is not inherited
from an undef dependency) carries a recorded `UndefCause`. An unexplained
root undef is itself a defect and surfaces as a diagnostic, never as
silence. Recorded causes are never overwritten with a wrong generic cause.

**Checkable design question(s)**: Can any code path this feature adds
leave a cell Undef without recording why? If a future coverage gap slips
through, does a backstop pass make it visible (a "cell X is undef for an
unrecorded reason" diagnostic), or does it vanish? Does any re-derivation
pass risk overwriting a true recorded cause with a guessed one?

**Evidence**: ~1,350 `Value::Undef` sites in reify-eval/reify-expr vs
~40 cause-recording sites over 5 variants; `trace_undef_causes` returns
empty for unrecorded roots and `cmd_eval` skips empty-cause cells
(reify-cli `main.rs`, undef-notes loop); `reify check` never enables
cause capture; kernel-less re-eval misattributes every geometry undef as
`OpContractViolation` (#5197); #5360 (chained sub reads), #5385
(`generate` of geometry), #5345 (inline geometry query) all silent.

**House pattern**: the undef-self-describing tracer + `UndefCause` origins
map (PRD `docs/prds/v0_6/undef-self-describing.md`); the
`check_no_stale_undef` causeless-staleness checker
(`crates/reify-eval/src/invariants.rs`) — the backstop shape, currently
wired only into the debug-gate corpus harness.

## INV-SF-2 `error-severity-exits-nonzero`

**Rule**: Any Error-severity diagnostic emitted on any channel (compile,
eval, constraint, kernel, build) makes the CLI command exit nonzero. No
per-code bolt-on escalation lists. Corollary (severity hygiene): a
diagnostic *expected* on a healthy path is by definition not
Error-severity — demote or recode it; never exempt it from the gate.

**Checkable design question(s)**: Can this feature print `error:` while
the process exits 0 on any command? Does it add a special-case escalation
for one code instead of relying on the severity gate? Does it emit
Error-severity output on a path that a healthy design can hit (kernel
absent, capability not installed)?

**Evidence**: `reify check` decides exit from constraint outcomes alone
(`finish_check`); eval-phase errors are printed and discarded; escalation
is per-code bolt-on (`GdtIllegalModifier`, `E_DFM_` message-prefix match);
#5386 (non-pub structure error text, exit 0); relate operand-type errors
exit 0.

**House pattern**: `cmd_eval`'s and `cmd_build`'s Severity::Error exit
gates (task 4458) — check must converge on the same rule.

## INV-SF-3 `declared-intent-consumed-or-diagnosed`

**Rule**: Every declaration expressing design intent — a constraint, a
`relate` block, an objective, a DFM rule — is either consumed by a
solve/verify pass this run, or generates a diagnostic naming why not. A
declaration structurally incapable of ever being consumed is a
compile-time error, not a silent no-op.

**Checkable design question(s)**: Enumerate the paths where this feature
drops declared work (filters it out, returns an empty solution, skips on
a missing precondition) — does each emit a diagnostic naming what was
dropped and why? If a user writes the declaration in a place where it can
never take effect, do they find out at compile time?

**Evidence**: `relate` on a scope with no `at auto` sub returns
`RelateSolution::default()` — relations neither solved nor verified
(`crates/reify-eval/src/relate_solve.rs`, zero-auto early return); Bool
autos fall back to the continuous DimensionalSolver because
`SolverRegistry::production()` leaves the Logical slot None while
`cpsat.rs` sits unregistered (`crates/reify-constraints/src/registry.rs`);
minimize over Bool autos builds zero objective components and never
solves; DFM rules with unrealized handles are "silently skipped".

**House pattern**: the #5014 collateral-observability diagnostic (names a
whole unresolved cluster when a merged solve is skipped); the relate
redundant-remainder verify pass — the machinery to verify-instead-of-drop
already exists in `solve_relate_scope`.

## INV-SF-4 `indeterminate-attributable-transient`

**Rule**: `Indeterminate` is reserved for "not measurable in this run,
for a stated runtime reason" (no kernel, below resolution, no
measurement) and carries that reason. A constraint that would be
Indeterminate in every possible run (structurally unresolvable operand)
violates INV-SF-3 and is a compile error.

**Checkable design question(s)**: For each Indeterminate outcome this
feature can produce: what runtime condition clears it, and where is that
reason surfaced? Is there any input for which the constraint is
*permanently* indeterminate — and if so, why is that not a compile error?

**Evidence**: ad-hoc `@`-selector `frame_align` constraints are
permanently INDETERMINATE (inert) under a green check; non-strict check
reports "No constraints violated (N indeterminate)" and exits 0.

**House pattern**: the conservative-refusal discipline in
`engine_constraints.rs` ("degrades to Indeterminate and can NEVER produce
a false Violated") — keep the never-false-Violated half; add the
attributable-reason half. `reify check --strict` promotes indeterminate
to failure.

## INV-SF-5 `placeholders-owned-and-loud`

**Rule**: Every placeholder in tracked source — placeholder-typed public
signature (`Real`/`String`/`Length` standing in for a richer type),
placeholder function body, sentinel default — cites a live, non-terminal
task per the PTODO grammar. Blanket escapes that name no owner (the
"awaiting future type-system PRD" pattern) are banned: "no task yet owns
the retarget" must be impossible. Where a placeholder type can silently
accept a wrong argument, it must additionally be loud: a non-matchable
marker/opaque type, or an eval-time misuse diagnostic — statically silent
AND dynamically silent is never acceptable.

**Checkable design question(s)**: Does this feature introduce a public
signature typed with a stand-in (`Real` for a handle, `String` for a
selector) — and if so, which live task owns the retarget, and what
happens today when a caller passes a plausible-but-wrong value? Does any
function body return a value it knows is wrong (typecheck-only body), and
what guarantees the runtime intercept shadows it?

**Evidence**: `flexure_compliance(joint: Length)` — a bare `5mm`
silently overload-matches and yields a sentinel-default record
(`crates/reify-compiler/stdlib/flexures.ri`, joint-type placeholder
block: "no task yet owns" the retarget); mechanism/joint-id `Real`
placeholders across trajectory.ri/dynamics.ri escaped the PTODO gate via
blanket allows; option_recovery.ri/result.ri bodies return incorrect
values, shadowed only by the reify-expr intercept.

**House pattern**: `docs/notes/stdlib-real-placeholder-audit.md` — the
six-bucket census whose task-owned buckets all got fixed (#3111, #3115,
#3116); the `W_FlexureNonJointArg` eval-time misuse warning (task 4547);
the PTODO detector (`docs/prds/reify-audit-ptodo-detector.md` §8) as the
enforcement substrate.

## INV-SF-6 `diagnostics-carry-codes`

**Rule**: Every emitted Warning/Error carries a `DiagnosticCode`.
Code-less diagnostics cannot be gated, filtered, counted, or de-noised
systematically, and force message-substring hacks downstream.

**Checkable design question(s)**: Does this feature emit any diagnostic
without a code? Does any consumer it adds match on message text where a
code should exist?

**Evidence**: 362 `Diagnostic::error/warning` ctor sites in reify-eval,
67 with codes; the CLI's `E_DFM_` message-prefix escalation exists only
because co-resident Error diagnostics are code-less.

**House pattern**: `DiagnosticCode` registry + typed-code test assertions
(tasks 2255, 3416 flipped substring tests to code identity).

## Census seam

Reify's confusion-codebook entries
(`docs/legibility/confusion-codebook.yaml`) MAY carry
`invariant_violated: <slug>`; the slug vocabulary is this doc. A slug
violated repeatedly across census batches is an enforcement gap: file a
guard task.
