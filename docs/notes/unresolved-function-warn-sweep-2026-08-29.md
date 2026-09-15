# `UnresolvedFunction` warn-sweep — enumerated violation list (task #5371)

**Measured** 2026-08-29 on branch `task/5371`, base `1d4417977673`;
**re-measured** 2026-09-14 against base `ed7f60c635` — same two dispositions,
no new violations, see "Re-measurement" below.
**Gate:** `crates/reify-compiler/tests/harness_compilation_surface/unresolved_function_corpus_sweep.rs`.
**Consumers:** #5997 (flips the Warning to an Error; names this sweep as a
precondition) and #6014 (registry ω; deletes the terminal first-arg fallback,
and seeds its family-by-family migration from this list).

This note is the durable half of the sweep. The test binary is the live half —
it re-derives the list on every run, so this file records *dispositions and
reasoning*, never a list that has to be kept in sync.

---

## What the sweep asks

`expr.rs`'s `NoUserFunctions` ladder ends in a terminal first-arg fallback: a
callee no ladder arm claims is typed as its first argument. Before #5371 that
fallback was **open-world** — a name that exists nowhere compiled with zero
diagnostics. `reify_compiler::is_known_builtin` closes the world; the sweep asks
whether any committed `.ri` file calls a name outside it.

Answer, after the dispositions below: **no**. That is #5997's precondition met.

---

## The compile path is the finding

The single most important thing this sweep established is not a callee list, it
is which compile entry point makes such a list *mean* anything.

A callee is reported unresolved when nothing **in scope** declares it. Any scope
the walker fails to seed therefore shows up as a corpus defect that is really a
harness defect. Measured, on this corpus:

| entry point | seeds stdlib prelude | follows user `import` | sites reported |
|---|---|---|---|
| `compile_project` (the plan's choice) | **no** | yes | **117** across 32 callees |
| `compile_with_stdlib` (`examples_smoke.rs`) | yes | **no** | not run — mirror-image hole |
| `compile_entry_with_stdlib_cfg` (`reify check`) | yes | yes | **4** across 2 callees |

The 113 phantoms `compile_project` produced were every stdlib `pub fn` and
`structure def` reached through the prelude rather than through an explicit
`import`: `SPEED_OF_LIGHT()` (a real `pub fn` at `stdlib/units.ri:185`),
`Frame3()`, `MassProperties()`, `PointLoad()`, `FixedSupport()`,
`ElasticOptions()`, `solve_elastic_static()`, `Steel_AISI_1045()`,
`TrajectorySample()`, `MotionTrajectory()`, `inverse_dynamics()`,
`Concentricity()`, `MarlinDialect()`, `map_or()`, `pointwise_max/min()`, and the
nine SI constants. **None of these is a real violation.** A sweep built on that
entry point would have produced a 117-line "violation list" whose every line was
a lie, and #6014 would have inherited it as migration input.

`compile_entry_with_stdlib_cfg` is what `crates/reify-cli/src/main.rs` calls for
`reify check`. Using it means the sweep answers the same question a user's
`reify check` answers — which is the question #5997 actually needs answered.

---

## Dispositions

Two real callees, four call sites. Both are **disposition (i)**: genuinely
eval-dispatchable, not yet family-registered, added to
`EVAL_DEFERRED_BUILTIN_NAMES` in `crates/reify-compiler/src/unresolved_function.rs`.

### `__flexure_compliance_get` — 1 site

| | |
|---|---|
| eval dispatch | `crates/reify-stdlib/src/flexures/diagnostics.rs:59` (`if name == "__flexure_compliance_get"`) |
| corpus sites | `crates/reify-compiler/stdlib/flexures.ri:238` — the body of `pub fn flexure_compliance(joint: Length) -> FlexureCompliance` |
| disposition | manifest, τ4 group |
| owning task | **#6006** (registry τ4 — FEA/flexures/stackup/dfm/tolerancing/supports/loads/tensegrity) |

An undeclared accessor intrinsic — the double underscore is the convention for
exactly that. It is inside τ4's family (the same `crates/reify-stdlib/src/flexures/`
surface as the 14 `prb_*` constructors τ4 already owns), but it is **not
enumerated in τ4's task text**, which lists the `prb_*` constructors and not this
accessor. The manifest comment says so, so τ4 discovers it rather than being
surprised by a stale entry after it thinks it is done.

### `RepresentationWithin` — 3 sites in the swept roots (11 corpus-wide)

| | |
|---|---|
| engine dispatch | `crates/reify-eval/src/tolerance_combine::match_representation_within_shape`; constructed at `crates/reify-eval/src/tolerance_scope.rs:221,592` |
| swept sites | `examples/fea_bracket_member_access.ri:29`, `examples/representation_within.ri:54`, `examples/tolerancing/gdt_pass_weave.ri:117` |
| unswept sites | 8 more under `tests/prd-gate/fixtures/` (see "Not swept" below) |
| disposition | manifest, own group |
| owning task | **none verified** — stated as such in the manifest |

A fully landed language feature, not a typo and not dead: #4198 measured the
achieved-deviation metric, #4199 promoted the verb from a tolerance-bound
extractor to a three-valued post-realization assertion, and #6167/#6170 added the
bound pre-pass and the export refusal — all `done`. It is specified by
`docs/prds/v0_6/precision-nominal-representation-guarantee.md`.

Two properties explain why no τ task picked it up, and both are worth carrying
forward:

1. **It appears only in CONSTRAINT position.** `constraint
   RepresentationWithin(subject, 1um)` yields no value cell anyone reads, so the
   fallback's mistyping of it has never been observable. Every τ task enumerates
   *value-returning* builtins.
2. **It is PascalCase**, unlike every other manifest entry. The engine matches
   the exact string; it must not be "normalised" to snake_case.

Because the manifest's contract is that every entry has a verified owning
registry task, this one is called out as the exception — in the manifest's own
module doc as well as at the entry — rather than filed under the nearest
plausible τ. Assigning it a row owner is follow-up ticket
`tkt_0RT1CF3Q06BNRBRGCS970CVB75`; #6014 needs it resolved before it can delete
the fallback.

---

## Not swept, and why

### `tests/prd-gate/fixtures/`

The plan called for sweeping this directory. It is **excluded**, for two
independent reasons — the first is a hard constraint, the second is a design one.

1. **A Rust walk of that directory is forbidden by design.**
   `tests/infra/test_verify_scope.sh`'s `PG-DRIFT-DIR` scenario reds on any
   tracked `*.rs` naming the fixtures *directory* with a string/format/glob
   terminator. `verify.sh`'s docs no-heavy-checks carve-out for that directory
   rests on "nothing globs it", which is what makes *adding* a fixture provably
   inert. A walker would make every newly added fixture a silently ungated Rust
   build input — reachable through a hook-gated docs commit on `main`, with no
   later gate to catch a bad edit. That scenario's own note is explicit: if the
   half fires, the fix is **not** to extend `_RUST_COUPLED_RI_FIXTURES` but to
   re-examine the carve-out. That is a cross-cutting infra decision and was not
   #5371's to make.
2. **The directory's contract says fixtures need not compile.**
   `tests/prd-gate/README.md`: *"fixtures here are not required to parse or to
   pass `reify check`: several are deliberately unparseable or deliberately
   failing … Nothing in the repo compiles this directory wholesale."* A
   zero-violations assertion over a drawer of deliberate negatives is the wrong
   shape — its allowlist would need re-curating on every new fixture, which is
   the opposite of a gate.

The signal is preserved here instead. Measured before the root was dropped, with
`compile_entry_with_stdlib_cfg`, **12 fixture sites across 3 callees**:

| callee | sites | disposition |
|---|---|---|
| `RepresentationWithin` | 8 — `pnrg_envelope_{cone,fillet_blend,loft,pipe,sphere,spline,sweep,torus}.ri`, `pnrg_cost_split_sphere.ri:50`, `gui_purpose_surface.ri:41` | covered by the manifest entry above; nothing further needed |
| `line` | 1 — `unknown_fn_silent_accept_baseline.ri:10` | **deliberate negative.** This fixture is #6014's pre-state baseline and carries the 5371 observation verbatim. Its header is now STALE and was NOT corrected here — see below. |
| `cube` | 1 — `driver_contract_geometry_test_indeterminate.ri:14` | **a real defect, left in place and filed.** See below. |

`cube` is the one name in this whole sweep that is neither a builtin nor a
deliberate negative. It appears in no compiler classification family, no
`eval_builtin` dispatch arm and no stdlib `.ri` declaration; the real primitive
is `box`. Its one call site matters more than a typo normally would, because it
**silently substitutes for the effect the fixture is supposed to measure**:

`driver_contract_geometry_test_indeterminate.ri` is the driver-contract PRD's
baseline B4, and its header states its intent as *"a geometry-dependent `@test`
is structurally Indeterminate under the kernel-free test runner"*. Its recorded
measurement is `INDETERMINATE ... undefined inputs: TestBlockVolume.body`. But
`body` is undefined because `cube` does not exist — not because no kernel is
present. Both causes produce the same Indeterminate, so the fixture reports green
either way, and leaf ε (which gives the runner a real BRep kernel) would find the
verdict unchanged and have no way to tell that its own premise was never
exercised.

**Not fixed here, deliberately.** Rewriting `cube(w)` to `box(w, w, w)` changes
what another PRD's probe measures, and doing that from outside its owning task is
how a probe silently starts measuring something else again. Filed instead as
follow-up ticket `tkt_0RT1CEJYCY0QCVNRN1PVQNV6M1`. Note that the sweep gate added
by this task does **not** cover the file (see the exclusion above), so the fix
needs doing deliberately — it will not be forced RED.

### The `line` fixture's header is stale, and this task did not fix it

`unknown_fn_silent_accept_baseline.ri`'s header claims the file *"passes `reify
check` exit 0 with ZERO diagnostics"*. #5371 makes the second half of that false:
the same source now emits exactly one `Severity::Warning` carrying
`DiagnosticCode::UnresolvedFunction`, and zero Errors.

Everything else in that header survives — **exit 0 is unchanged** (a Warning does
not move the exit code) and the typing is unchanged (the fallback still adopts
arg0) — so the pre-state the fixture exists to hold is intact, and the capability
manifest's own wording (`builtin-signature-registry.capability-manifest.md:35`,
"check exit 0 today, verified 2026-08-04") stays true and needs no edit. The
blast radius is exactly one sentence.

**Not corrected here: the file is outside this task's plan scope.** The plan's
fixture slot names a sibling path (`unresolved_function_warn_baseline.ri`) that
step-15 deliberately did not create, because this fixture already *is* that
baseline. Editing a different prd-gate fixture on that reasoning would be
widening scope on the implementer's own authority, and the correction is not
needed for green. Filed as follow-up ticket `tkt_0RT1EB0JHRTNTMRM2YHRMX23YC`.

The behaviour itself is not left unpinned: `unresolved_function_tests::
the_original_line_observation_now_warns` compiles the same `line(point3(…),
point3(…))` source inline, so the claim the header *should* make is asserted in
Rust regardless of when the header catches up.

### Naming a prd-gate fixture path from Rust is itself a coupling

Worth carrying forward, because #5997 and #6014 both re-tread this ground and
the trap is invisible at authoring time.

`tests/infra/test_verify_scope.sh`'s **PG-DRIFT half (a)** derives its coupled
set with `git grep -o 'tests/prd-gate/fixtures/[A-Za-z0-9_.-]+\.ri'` over ALL
tracked `*.rs` and is deliberately **comment-inclusive** — a doc-comment mention
counts as a reference, by design ("a doc mention is usually the first trace of a
read about to exist"). Every derived path must then classify `RUN_RUST=1`, which
means being listed in `verify.sh`'s `_RUST_COUPLED_RI_FIXTURES`.

Step-15 of this task landed a doc comment on `DELIBERATE_NEGATIVES` that spelled
this fixture's full path while *explaining why the binary deliberately does not
reference it* — so the comment did the precise thing its own text said it was
avoiding. The comment landed in `ccde4f88f1`; measured on `task/5371` at HEAD
`a6be4e30a7`, before the fix below: derived set 13 vs. a 12-entry list, and
`verify.sh --scope staged --print-plan` on the staged fixture reporting
`RUN_RUST=0` — i.e. `test_verify_scope.sh` red on the merge gate.

Fixed by dropping the directory prefix and keeping the basename, not by extending
`_RUST_COUPLED_RI_FIXTURES` (which is out of this task's scope, is a
verify-pipeline file forcing the full `--scope all` gate, and would have bought a
coupling for a file no Rust target opens). The rule for anyone editing this
region: **cite a prd-gate fixture by basename unless a test actually reads it.**
Note PG-DRIFT-DIR — the sibling half that reds on naming the *directory* with a
string/format/glob terminator — is a different check and was never tripped here.

### Files that do not compile cleanly

Skipped silently by the sweep. A file with parse errors or Error-severity
diagnostics is failing for reasons unrelated to #5371, and its warnings would be
downstream noise. `examples_smoke.rs` is the binary that gates *those*; this one
must not duplicate its judgement nor inherit its `SKIP_SET`.
`the_sweep_actually_compiles_most_of_the_corpus` is the guard that stops this
rule from quietly hollowing the gate out.

---

## The OBSERVED-CONSUMER question

#5371's task text asks whether the `std.fea` `MultiCaseResult` accessor family
(`result_for` / `case_names` / `worst_case` / `envelope_*` / `min_max_stress`,
`crates/reify-stdlib/src/fea.rs:50-80`) was inside the original measured 121/231
enumeration or widens it.

**Answer: it is inside, and this sweep does not widen it.** Every one of those
names was already in `EVAL_DEFERRED_BUILTIN_NAMES` before the sweep ran (added
in step-8 by walking the eval dispatch maps), and all are owned by #6006 (τ4),
whose task text enumerates them explicitly — `envelope_max/min -> Field`,
`case_names -> List<String>`, `result_for`, `linear_combine`, `min_max_stress`,
`worst_case -> String`, `worst_buckling_case`, `envelope_critical_load ->
Scalar<FORCE>`, `envelope_argmax/argmin`.

Correspondingly, **zero** of those names appeared in the sweep's violation list:
the manifest was already suppressing them, which is the manifest working as
designed. The sweep widened the enumeration by exactly the two names in
"Dispositions" above.

---

## Re-measurement, 2026-09-14 (base `ed7f60c635`)

Main moved two weeks and ~1.5k lines of `reify-stdlib/src` between the two
bases, so the sweep was re-run to ask whether the closed world had reopened.
It had not: **both dispositions stand unchanged, and no third appeared.**

The substantive change was **#6001 α**, the builtin-signature registry. It
DELETED `analysis_signatures.rs` and `parse_signatures.rs`, whose
`ANALYSIS_FN_NAMES` / `PARSE_FN_NAMES` slices this sweep's oracle unioned over,
and hoisted `registry_dispatch::try_dispatch` to the front of
`reify_stdlib::eval_builtin`'s chain in place of the `eval_analysis` /
`eval_parse` arms. Both sides of the sweep's question therefore moved at once.
The union of the two departed slices was exactly the seven names α seeded as
rows, so nothing was stranded, and `is_known_builtin` now reaches them through
a single `builtin_registry::registry_knows_name` arm.

That arm is also why the oracle is asked of the registry rather than of a copy
of those seven names: a copy would stop covering the first τ task that
registers a row, and every τ task exists to register rows.

Two further drift questions, both asked of a test rather than of judgement:

| question | test | answer |
|---|---|---|
| has any manifest entry acquired a registry row? | `eval_deferred_names_are_disjoint_from_every_registered_family` | no — α seeded none of the manifest's names |
| did a family's resolver grow an arm its slice does not list? | `resolver_only_family_slices_match_their_resolvers` | one, `bbox` (with `bbox_size` / `bbox_center`), from #6081 — a real resolver arm, so it joined `DATUM_CONSTRUCTOR_NAMES`, not the manifest |
| does any corpus call site now name something outside the union? | `corpus_has_no_unresolved_function_calls` | no |

`EVAL_DEFERRED_BUILTIN_NAMES` is therefore unchanged by the integration. That
is the manifest working as designed in the quiet direction: it holds names
*awaiting* a row, and the first tranche of rows landed for names it never held.

---

## Residual gap — a precondition on #5997

The warning has three outcomes at the fallback, not two, and the third is
SILENCE: a callee the enclosing module declares but that this body cannot yet
resolve. Reported and measured 2026-09-15 on `task/5371`.

Why the third state exists: `compile_builder/functions_phase.rs` compiles each
`fn` body against the user-only function table it is still GROWING in source
order, and only merges `ctx.resolution_functions` afterwards. An ENTITY body
gets the merged table and never sees this; a FN body does. `is_known_builtin`
answers "is this a builtin?" correctly in both cases — a user `fn` is not one —
so reading its `false` as "exists nowhere" was only ever sound for the entity
case. The fallback therefore asks a second, separate question ("does this module
declare this name?") via `CompilationScope::declared_callable_names`.

Three routes measured emitting a FALSE `unresolved function` before the fix, all
three silent after it, none of them changing any type:

| route | before | cause |
|---|---|---|
| fn body → later-declared sibling `fn` | `unresolved function: b` | source-order table growth |
| mutually-referential `fn` pair | `unresolved function: odd` | same, and UNFIXABLE by reordering |
| structure constructor in a TRAIT STATIC fn body | `unresolved function: Widget` | `traits_phase` passes `None` for the template registry |
| `fn` param default calling a later sibling | `unresolved function: later` | same table, `compile_function`'s neutral scope |
| structure constructor in an ASSOC fn body — trait default, or either kind of structure override | `unresolved function: Widget` | `compile_assoc_function` likewise sets no template registry (esc-5371-12) |

Five controls bound the fix and were measured green throughout; two of them are
routes a reading of the code suggests are broken and which measurement shows are
NOT, recorded so a later reader does not "fix" them:

* reversing the first route's declaration order compiles clean — the trigger is
  source order, so the fix must not require reordering;
* a genuinely-undeclared callee in a `fn` body **still warns** — the tripwire
  against a blanket in-fn-body disable;
* a constructor call from a REGULAR fn body is clean in BOTH declaration orders
  and still raises `E_CTOR_UNKNOWN_FIELD` for a bad field name, which is what
  proves it is RESOLVED rather than merely silenced;
* **a trait static fn calling a forward-declared top-level `fn` is already
  clean** — `traits_phase` runs after `functions_phase`, so `ctx.functions` is
  complete by then;
* a builtin called from a `fn` body is clean.

### What #5997 must handle before flipping this Warning to an Error

**One diagnostic that existed on the warn-mode branch is now gone, and it is not
replaced.** A call to a declared sibling with the WRONG ARITY inside a `fn` body
is now COMPLETELY silent:

```
pub fn a(x: Real) -> Real { later(x, x) }
pub fn later(x: Real) -> Real { x }
```

measured `UF=[] ERR=[]`. The same mistake in an ENTITY body is a hard error —
`no matching overload for later(Real, Real), candidates: later(Real) -> Real` —
because the entity body resolves against the merged table and so reaches real
overload resolution instead of the fallback. The asymmetry is pre-existing (this
was equally silent before #5371) and is `functions_phase`'s forward-reference
contract, not a defect this warn-only task introduced; but #5997 should not read
the silence as "nothing is wrong here". Closing it means giving fn bodies a
complete table, which is #6014 (registry ω) territory.

**The constructor routes are silenced, not fixed, in BOTH trait-fn positions.**
`Widget(w: 2mm)` still does not lower to a `StructureInstanceCtor` inside a
trait STATIC fn body (`traits_phase` passes `None` for
`prelude_template_registry`, "v1") nor inside an ASSOC fn body —
`compile_assoc_function`, reached from conformance for a trait default and for
either kind of structure override, sets no registry either (esc-5371-12).
Passing `Some(&merged_registry)` at either site would change how those bodies
TYPE constructor calls, which is outside a warn-only task's remit and was
deliberately not done. So a bad field name in either position is still not
caught, where the same call in a regular fn body raises `E_CTOR_UNKNOWN_FIELD`.

Note the two halves of the declared-callable set are needed at DIFFERENT sites,
which is why neither is populated everywhere: the FN half matters only while
`phase_functions` is still growing the table, and the STRUCTURE half matters
wherever no template registry is set. `compile_assoc_function` runs after
`phase_functions`, so it needs the structure half only — measured, not assumed.

A caution for #5997 drawn from getting this wrong once: a probe that declares a
trait with a default body but NO conforming structure never invokes
`compile_assoc_function` at all, so it measures clean VACUOUSLY. Route (4) was
initially missed exactly that way. Any probe of an assoc-fn route must include a
structure that actually conforms.

**What is NOT a gap.** Imported user modules arrive as the `prelude` slice
(`module_dag.rs` collects each import's `CompiledModule` and hands it to
`compile_with_prelude_refs`), so their `fn`s are in `declared_fn_names` and are
covered by the same gate as local ones.

---

## What did NOT change

Typing. Every call above still infers exactly the type it inferred before
#5371 — the fallback still adopts arg0. The only new observable in this task is
a diagnostic, which is what makes it corpus-safe and what lets #5997 do the
severity flip against a green baseline rather than against a moving one.

The claim survives the re-measurement, and is worth stating precisely because
#6001 α also touched the ladder. α moved where seven names' types come FROM
without changing WHAT they are (its PRD §7.3(6): zero corrections), and #5371
adds no typing of its own on top — `is_known_builtin` is consulted only to
decide whether to emit a warning. So the baseline #5997 flips against is still
type-identical to the pre-task one at every call site.

The claim survives the false-positive repair above for the same reason, and it
was ASSERTED rather than assumed: that work only ever WITHHOLDS a diagnostic, and
`forward_reference_typing_is_byte_identical` pins both halves of the fallback's
answer at a withheld site against sources built so the two candidate answers
differ — a one-arg call to a `-> Real` sibling is still `Scalar<LENGTH>` (from
arg0, not from the declared return type), and a zero-arg call to a `-> Length`
sibling still defaults to `Real`. Forward references remain unresolved; the
repair removed a diagnostic, not a lookup.
