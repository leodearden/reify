# `UnresolvedFunction` warn-sweep — enumerated violation list (task #5371)

**Measured** 2026-08-29 on branch `task/5371`, base `1d4417977673`.
**Gate:** `crates/reify-compiler/tests/unresolved_function_corpus_sweep.rs`.
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

## What did NOT change

Typing. Every call above still infers exactly the type it inferred before
#5371 — the fallback still adopts arg0. The only new observable in this task is
a diagnostic, which is what makes it corpus-safe and what lets #5997 do the
severity flip against a green baseline rather than against a moving one.
