# Dimensioned-construction strictness — strict `DimensionVector` equality at construction boundaries

**Milestone:** v0.6 · **Status:** active · **Authored:** 2026-07-28 · **Anchors re-verified at HEAD `dc83d4fd60`**

PRD 4 of the five-PRD **units-gating program**. Evidence base:
`docs/notes/units-gating-gap-research-2026-07-28.md` (tracked, landed `0d1737790743`) —
**PHASE 2, sub-class 1 (compiler)**. Cite that document rather than duplicating its tables.
Every file:line anchor below was re-verified against `dc83d4fd60`; drift from the
5465/5627-era anchors is recorded in §2.4.

---

## §0 The ruling, and the reversal history it re-reverses

### 0.1 The ruling (made, not open)

**Strict `DimensionVector` equality at every value-construction boundary.** A value may
occupy a slot declared `Scalar<Q>` only when its own type is `Scalar<Q>` for the *same*
`Q`. Three classes that are silent today all become diagnosed:

| Class | Example | Today | After |
|---|---|---|---|
| **bare** dimensionless at a dimensioned slot | `Steel(density: 7850)` | silent | diagnosed |
| **cross-dimension** | `Steel(youngs_modulus: 200mm)` | silent | diagnosed |
| **non-scalar** | `Steel(density: "heavy")` | silent | diagnosed |

Ratified by Leo, 2026-07-28 (program-wide decision 2). This resolves **task 5627** as its
**candidate 4** — *fix the corpus call sites to carry units, then promote the family under
strict `type_compatible`* — **widened beyond 5627's scoped question** to the cross-dimension
and non-scalar cases, which fall through the *same* exclusion but are not named in 5627's
ruling question (research doc, PHASE 2 §1). Bare `0` is **not** special-cased; it rejects
like any other bare number (program decision 1).

### 0.2 What is REINSTATEMENT and what is WIDENING — the distinction is load-bearing

`docs/prds/v0_6/real-dimensionless-unification.md` decision **D5** (`:51`) reads, in full:

> **Struct-param default type-check = hard error**, mirroring fn-param strictness
> (`fn_param_default_compatible` exact-equality). Reuse `FnParamDefaultTypeMismatch`.

**D5 is specifically and only about struct-param defaults** — `param t : Length = 1.0`, the
rule spelled out at `:17` and tabulated in the boundary row at `:134` (*"Struct-param default
mismatch | `param t : Length = 1.0` | `E_FN_PARAM_DEFAULT_TYPE_MISMATCH`"*). It is **not** a
general ruling about bare-at-dimensioned. Conflating the two would misattribute authority to
a decision that never carried it — the same class of error this PRD exists to correct. So:

| Gate (§2.1 numbering) | This PRD's act | Authority |
|---|---|---|
| **3** — literal `param` defaults | **REINSTATEMENT of D5** | `real-dimensionless-unification.md:51` (+ `:17`, `:134`) |
| 1 — struct-ctor field slots | **WIDENING** — same policy, never ruled by D5 | ratified decision 2 (2026-07-28), task 5627 candidate 4 |
| 2 — param defaults, conformance pass | **WIDENING** (rides gate 1's predicate, §4.2) | ratified decision 2 |
| 4 — literal `let` annotations | **WIDENING** — the `let` twin post-dates D5 | ratified decision 2 |
| 5 — constraint-def args | **WIDENING** | ratified decision 2 |

Two deliberate deviations from D5's *letter*, even at gate 3, are recorded in §5 (D4-6: the
diagnostic code that actually shipped; D4-7: severity sequencing).

### 0.3 The reversal history — recorded here because it was never recorded anywhere

At **gate 3**, this is the third position the project has held, and the second transition was
silent:

| # | When | Position at gate 3 | Where recorded |
|---|---|---|---|
| 1 | real-dimensionless-unification authoring | `param t : Length = 1.0` is a **hard error** (D5) | PRD `:17`, `:51`, `:134` |
| 2 | task **4318** shipped | the same expression is **silently accepted** | **nowhere** — see below |
| 3 | this PRD | **hard error again** — D5 reinstated at gate 3, and the *policy* widened to gates 1/2/4/5 (§0.2) | this section |

**How transition 2 happened without a record.** D5 was assigned to leaf **ε**
(`real-dimensionless-unification.md:97`). At decompose time ε was **dropped and folded into
task 4318** (`:82`: *"→ Drop ε; fold its literal case into 4318"*). Task 4318 then shipped
the *opposite* rule — the numeric-literal tolerance now at
`crates/reify-compiler/src/entity.rs:479-485` — **and additionally created gate 4**, the
`let`-annotation twin at `:563-569`, which D5 never contemplated. No document recorded either.

This is a textbook instance of the known failure shape **"PRD leaf DROPPED at decompose;
folded into task N" ⇒ prime silent-reversal site**, whose tell is *a test asserting the
negation of the PRD decision, added deliberately*. **Four** such tests exist (all measured,
§6.2):

| Test | Anchor |
|---|---|
| `param_int_and_real_literal_on_dimensioned_scalar_do_not_error` | `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs:172` |
| `param_negative_literal_on_dimensioned_scalar_does_not_error` | `param_default_type_mismatch_tests.rs:203` |
| `let_annotation_int_and_real_literal_on_dimensioned_scalar_do_not_error` | `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs:173` |
| `port_member_let_annotation_numeric_literal_on_dimensioned_scalar_do_not_error` | `let_annotation_type_mismatch_tests.rs:578` |

plus a module-doc statement of the tolerance as intended behaviour
(`param_default_type_mismatch_tests.rs:1-14`: *"Int-literal guard (`param x : Length = 1` must
NOT error)"*).

Inverting those four tests (§11, leaf **δ₁**) is therefore not incidental cleanup — it is the
load-bearing act that makes the reversal-of-the-reversal legible. **Any future session that
finds those tests must find this section from them**; δ₁ must leave a doc-comment pointer at
each inverted test back to §0.3, naming task 4318 as the origin.

### 0.4 The absurdity this closes

Within *one* function, `crates/reify-compiler/src/entity.rs` today rules:

```reify
param x : Length = 1.0          // silent      (literal tolerance, :479-485)
param x : Length = ratio * 2.0  // hard Error  (falls through to type_compatible, :486)
```

The *more* obviously-wrong spelling is accepted and the subtler one rejected — pinned
deliberately by `param_nonliteral_dimensionless_compound_on_dimensioned_scalar_errors`
(`param_default_type_mismatch_tests.rs:405`).

---

## §1 Goal — what a `.ri` author observes

The spec's promise at `docs/reify-language-spec.md:125` —

> Bare numbers are dimensionless: `3.14` is `Real` (dimensionless). To get a dimensioned
> quantity, a unit must be written. **There is no "default unit system."**

— finally holds **at construction**, not only in arithmetic. Concretely, after this PRD:

```
$ reify check bad.ri     # structure Steel { param youngs_modulus : Pressure ... }
                         # let s = Steel(youngs_modulus: 200mm, density: "heavy")
warning: argument 'youngs_modulus' has type 'Scalar[m]' but param 'youngs_modulus'
         requires type 'Scalar[kg/(m·s^2)]'; pass a dimensioned Pressure literal such as `200GPa`
warning: argument 'density' has type 'String' but param 'density' requires type 'Scalar[kg/m^3]';
         pass a dimensioned MassDensity literal such as `7850kg/m^3`
```

where today **both lines are absent and the command exits 0** — probe-verified on a fresh
`target/release/reify` (§6.1). And:

```
$ reify check bad_default.ri     # param t : Length = 1.0
error: parameter 't' declared `Scalar[m]` but its initializer evaluates to `Real`;
       declared type and initializer dimension must agree
$ echo $?
1
```

**Why this matters beyond tidiness.** The research doc's PHASE 2 headline
("THE TRAP") is that **~25 stdlib params already declare `Pressure`/`Density`/`Force`/…
and enforce nothing** — both sweep agents initially misread the declarations as gates.
Declared-but-unenforced is fiction (INV-SF-3 `declared-intent-consumed-or-diagnosed`). This
PRD is what converts those ~25 declarations from documentation into contract.

---

## §2 Background — the gate map [CONTRACT, part 1]

### 2.1 The position-dependence table (before / after) — 5627's six gates, re-verified as ten

Task 5627's central artifact, re-verified at `dc83d4fd60`. "Dimensionless arg at a
dimensioned `Scalar<Q>` slot":

| # | Gate | Anchor (current main) | Today | After this PRD | Owning leaf |
|---|---|---|---|---|---|
| 1 | **struct-ctor field slots** | `conformance/mod.rs:1691-1697` (`general_leaf_param_family_is_validated`); caller `:1563` | **LEGAL** (silent) | ILLEGAL | **γ** |
| 2 | **param defaults, conformance pass** | `conformance/mod.rs:517-532` (`check_param_default_conformance` `_ =>` arm) | **LEGAL** (silent, same predicate) | ILLEGAL | **γ** (same one-line change) |
| 3 | **literal `param` defaults** | `entity.rs:479-485` (`check_param_default_type`; helper `is_numeric_literal_expr` `:390-404`) | **LEGAL** | ILLEGAL | **δ₁** |
| 4 | **literal `let` annotations** | `entity.rs:563-569` (`check_let_annotation_type`) | **LEGAL** | ILLEGAL | **δ₁** |
| 5 | **constraint-def args** | `type_compat.rs:1482-1486` (Rule 4 numeric leniency in `constraint_arg_type_conforms`, fn at `:1464`) | **LEGAL** | ILLEGAL | **δ₂** |
| 6 | user-fn param slots | `type_compat.rs:1201/1221/1289` (`param_ty == arg_ty` in `resolve_function_overload` `:1155`) | ILLEGAL | unchanged | — |
| 7 | fn-param defaults | `type_compat.rs:385-390` (`fn_param_default_compatible`), caller `functions.rs:201` | ILLEGAL | unchanged | — |
| 8 | ambient defaults | `entities_phase.rs:530` (`implicitly_converts_to`) | ILLEGAL | unchanged | — |
| 9 | compound-expression `param`/`let` initializers | `entity.rs:486` / `:570` (fall-through to `type_compatible`) | ILLEGAL | unchanged | — |
| 10 | arithmetic / comparison operators | spec `:240-241`; `expr.rs` binop paths | ILLEGAL | unchanged | — |

5627's table listed six gates; the re-verification found **ten** — gate 2 (the conformance
param-default entry) and gate 4 (the `let` twin of gate 3) were not separately named, and
they matter because gate 2 rides the *same predicate* as gate 1 (§4.2).

### 2.2 The type predicate already says ILLEGAL — in both directions

No new comparison logic is needed. `implicitly_converts_to` (`type_compat.rs:52-180`) has
**no `Scalar`-vs-`Scalar` arm** beyond the `from == to` identity short-circuit, and
`type_compatible`'s (`:220`) only scalar relaxation is gated on the **param** side being
dimensionless (`:232-237`, `Int` → dimensionless-`Scalar` widening). Therefore, today and
unchanged:

| pairing | `type_compatible` |
|---|---|
| `Scalar{LENGTH}` ← `Scalar{LENGTH}` | **true** (identity) |
| `Scalar{LENGTH}` ← `Scalar{DIMENSIONLESS}` (bare `Real`) | **false** |
| `Scalar{LENGTH}` ← `Int` | **false** (widening needs a dimensionless *param*) |
| `Scalar{PRESSURE}` ← `Scalar{LENGTH}` | **false** |
| `Scalar{LENGTH}` ← `String` | **false** |

**The mechanism is one predicate arm, not a new algorithm** (§4.1). Everything else in this
PRD is migration, severity sequencing, false-positive fencing, and the doc record.

### 2.3 Probe-verified consequences today

From the research doc PHASE 2 §1, re-confirmed this session against a fresh release binary
(§6.1): `Steel(youngs_modulus: 200mm)`, `Steel(youngs_modulus: "oops")`,
`Material(density: "heavy")` — **no diagnostic, exit 0, at both `reify check` and
`reify eval`**. The exclusion keys on `param_type` alone, so although it was written for
*dimensionless-literal leniency* it also swallows strictly wrong-typed args (`String` at a
`Pressure` field).

### 2.4 Anchor drift found (5627's record is 5465-era)

| Anchor as cited by 5627 / the probe test | Current main `dc83d4fd60` | Drift |
|---|---|---|
| `conformance/mod.rs:1691` predicate | `:1691-1697` | **none** |
| `conformance/mod.rs:32` severity const | `:32` | **none** |
| `entity.rs:459-478` literal-defaults gate | doc comment `:459-478`; **executable gate `:479-485`** | comment-vs-code |
| `entity.rs:473-478` compound-initializer rejection | `:486-499` | **+13** |
| `type_compat.rs:366-371` `fn_param_default_compatible` | `:385-390` | **+19** |
| `type_compat.rs:1466-1470` constraint-def leniency | `:1482-1486` (fn at `:1464`) | **+16** |
| `type_compat.rs:233-237` dimensionless relaxation | `:232-237` | −1 |
| `type_compat.rs:52-179` `implicitly_converts_to` | `:52-180` | +1 (extent) |
| `type_compat.rs:1182/1202/1270` overload equality | `:1201/1221/1289` | **+19** |
| `entities_phase.rs:531` ambient defaults | `:530` | −1 |
| `printer_print_envelope.ri:146-147` bare trajectory args | `:146-147` is the **comment**; the bare args are at **`:153`, `:154`, `:155`** | **+7/+8** |
| `tots_optimal_ptp.ri:78-79` bare trajectory args | `:78`, `:79` — **plus a third site at `:67`** prior research missed | none, **+1 site** |
| `examples/bearing_auto_seal.ri` (5627 `files`) | **ZERO ctor sites.** Its only issue is `param durometer : Length = 70.0` at `:46` — **gate 3, not gate 1** | mis-attributed |
| `fea_multi_case.ri:315` load-struct decls | `:315-317` (`PointLoad`), `:418-419`, `:446-448`, `:476-478` — **all four still `Real`** | confirmed |
| `examples/**/analysis.ri` yield fields | **no such file.** `stdlib/analysis.ri:46` `yield_strength : Real` is dimensionless, documented at `:24-28` as not participating in dimension checking | non-existent |
| `NAMED_DIMENSIONS` doc-comment "34 entries" | actually **51** (`reify-core/src/dimension.rs:514-597`) — **both halves superseded by η; see §6.2's note**: the doc comment now deliberately quotes *no* count, and the table is **52 rows** at `:576-663` (2026-08-03) | **+17, stale** (PRD 3's) |
| `main.rs:546-552` check compile-diagnostic gate | `:546-552` | **none** |
| `real-dimensionless-unification.md:17/51/64/82/97/134` | unchanged | **none** |
| `struct_ctor_field_conformance_tests.rs` pinning probe | `:1414-1461` | (not previously anchored) |

The probe test's own doc comment (`struct_ctor_field_conformance_tests.rs:1432`) cites the
stale `entity.rs:459-478`; leaf **γ** rewrites that comment wholesale anyway.

---

## §3 The three walker entries and their severities [CONTRACT, part 2]

`general_leaf_param_family_is_validated` gates the general concrete-leaf arm of the shared
walker `walk_param_against_arg` (`conformance/mod.rs:669`), which is reached from **three**
call sites with **two different severities**:

| Entry | `conformance/mod.rs` | Severity | Reaches a dimensioned `Scalar` leaf? |
|---|---|---|---|
| `check_trait_arg_conformance` (ctor field slots) | `:294`, walk at `:333`, severity at `:331` | `CTOR_FIELD_CONFORMANCE_SEVERITY` = **Warning** (`:32`) | **YES** — the primary target |
| `check_param_default_conformance` (param defaults) | `:403`, walk at `:532`, severity at `:530` | `CTOR_FIELD_CONFORMANCE_SEVERITY` = **Warning** | **YES** |
| `check_fn_arg_conformance` (fn-call args) | `:359`, walk at `:382`, severity at `:380` | **`Severity::Error`**, hard-coded | **effectively NO** — see 3.1 |

### 3.1 Premise correction — the fn-call Error surface is *not* armed by this promotion

Task **5646**'s record carries a CAUTION that promoting the family "arms an Error surface,
not just the Warning ctor knob", because `check_fn_arg_conformance` hard-codes
`Severity::Error`. **The severity claim is true; the reachability claim is not**, for this
family. Verified at `compile_builder/entities_phase.rs`:

1. The only production call site is `:1503`, inside `check_expr_fn_calls`' param loop, which
   is guarded by `if !type_carries_trait_object(param_ty) { continue; }` — a param typed
   `Scalar<Velocity>` carries no trait object and is **skipped before the walker is reached**.
2. That loop runs only on `OverloadResolution::Resolved` (`:1508-1511`), and
   `resolve_function_overload` already applies strict `param_ty == arg_ty` — so a
   dimension-mismatched arg at a concrete scalar param is `NoMatch` and returns early
   *anyway* (gate 6, already ILLEGAL).

A dimensioned `Scalar` can reach the Error entry only nested inside a trait-object-carrying
wrapper whose lockstep recursion bottoms out on a `Scalar` leaf (e.g. a
`Map<Scalar<Length>, SomeTrait>` key). **Leaf α must measure whether any such site exists**;
if the count is zero, γ is severity-safe pre-δ by construction, and if it is non-zero those
sites are named and migrated in β before γ lands.

**Consequence for leaf signals:** pre-δ, the ctor and param-default entries emit **Warning**,
so `reify check` **prints but exits 0**. Exit-code signals are therefore *not* available to
γ pre-δ; γ's signal is **diagnostic emission** (§11). δ₁ and δ₂, which touch
`Diagnostic::error` sites, do exit 1 pre-δ.

### 3.2 `reify check` visibility — VERIFIED, no PRD-2 dependency required

The program brief asked this PRD to verify whether ctor-conformance diagnostics are
genuinely check-visible via the compiler-diagnostic path. **They are** — traced and
empirically confirmed this session:

| Question | Answer | Evidence |
|---|---|---|
| Is the conformance walker on the compile path `reify check` executes? | **YES** | `main.rs:144` → `cmd_check` (`:475`) → `parse_and_compile_with_cfg` (`:541`) → `compile_with_prelude_context_checked_with_config` (`lib.rs:450-461`) → `phase_fn_arg_conformance` (`lib.rs:655`) → `entities_phase.rs:1297/1367/1593/1648` → `conformance/mod.rs:294` → walker. It is part of `compile_*`, **not** `engine_build`. |
| Are Warning-severity compile diagnostics **printed**? | **YES** | `main.rs:283-285`, unconditional over all severities. Empirically: a ctor-conformance Warning appears verbatim on `reify check`. |
| Do Error-severity compile diagnostics **gate the exit code**? | **YES** | `main.rs:546-552`, evaluated *before* any engine work. Empirically confirmed by an A/B on the **same walker arm** differing only in `ctx.severity`: ctor entry → exit 0, fn-call entry → exit 1. |
| Does `--strict` help? | **NO** | `main.rs:2299-2305` — `--strict` promotes `SomeIndeterminate` only; zero interaction with diagnostics. |

**Therefore this PRD's leaf signals may legitimately use `reify check`, and take NO
`add_dependency` edge onto PRD 2.** Program-brief seam rule ("any check-visible signal takes
a real edge onto PRD 2's task") is satisfied vacuously: the visibility this PRD relies on
already exists on main and is owned by neither PRD.

**Correction owed to the research doc (not this PRD's work to land):** the note's
`reify check` false-green section claims *"exit code is computed solely from
ConstraintOutcome; diagnostics NEVER gate exit"*. That is **FALSE as written** — it
describes the `ConstraintOutcome` / build / eval-diagnostic layer correctly but overlooks
the pre-engine compile-diagnostic gate at `main.rs:546-552`. Its sibling claims about
`Engine::eval` (PARTLY — `main.rs:621` *does* build on the geometric-Conforms/DFM arm) and
`let _ = engine.build(...)` (TRUE, `main.rs:621`) stand. **PRD 2 owns acting on this**; §10
records the seam.

---

## §4 Sketch of approach

### 4.1 The mechanism

```rust
// crates/reify-compiler/src/conformance/mod.rs:1691-1697
 fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
     match param_type {
         Type::Bool | Type::Int | Type::String => true,
-        Type::Scalar { dimension } => dimension.is_dimensionless(),
+        Type::Scalar { .. } => true,
         _ => false,
     }
 }
```

One arm. `type_compatible` supplies strict equality already (§2.2). Everything downstream —
`reject_if_incompatible` (`:1194`), `emit_arg_type_mismatch` (`:1084-1099`),
`DiagnosticCode::ArgTypeMismatch` — is unchanged and already exercised by the four families
task 5465 promoted.

### 4.2 The four couplings a naive implementation gets wrong

1. **Gate 1 and gate 2 are the same change.** The predicate gates *both* the ctor-arg entry
   and the param-default entry. So the promotion **partially closes the literal-defaults
   gate on its own** — at Warning, with code `ArgTypeMismatch`, from a *different pass* than
   `entity.rs`. It cannot be sequenced away. See D4-3.
2. **Double-diagnosis.** Once δ₁ removes `entity.rs`'s tolerance, `param t : Length = 1.0`
   is judged by *two* passes: `entity.rs` (`ParamDefaultTypeMismatch`, Error) and
   `conformance` (`ArgTypeMismatch`, Warning). One authoring mistake, two diagnostics, two
   codes. See D4-4.
3. **`Type::ScalarParam` false positives.** `arg_type_is_unverifiable`
   (`conformance/mod.rs`, doc block immediately preceding it) **deliberately excludes**
   `Type::ScalarParam(_)` — the unresolved *dimension*-parameter placeholder — because
   adding it there "would silence genuine family-level mismatches such as
   `String ← Scalar<Q>` at every arm at once". Dimension-generic user functions ship today
   (tasks 4234/4235). A concrete `Scalar<Length>` param receiving a `ScalarParam("Q")` arg
   would newly false-reject. See D4-5.
4. **`arg_acceptance` is not reusable *today*.** The house rejection-wording template with
   `migration_hint` lives at `crates/reify-eval/src/arg_acceptance.rs` — in **reify-eval**,
   which *depends on* reify-compiler (`reify-eval/Cargo.toml`), not the reverse
   (`reify-compiler/Cargo.toml` deps: ast, core, ir, syntax, config). Importing it today
   inverts the dependency graph. Adopt the **wording shape**, not the type — **but note that
   PRD 5 relocates the module to `reify-ir`, which reify-compiler *does* depend on**, after
   which the type itself becomes reusable. See D4-6.

### 4.3 Landing shape

**Migrate the corpus first, then land the gate; workspace green at every commit.** This is
the template `real-dimensionless-unification.md:64` established and the reason candidate 4
was chosen over candidates 1–3. The measurement instrument and the migration's own proof are
the same object: `crates/reify-compiler/tests/examples_smoke.rs:198`
`no_example_emits_ctor_field_conformance_diagnostics` — a corpus-wide, severity-blind,
zero-tolerance gate over every `.ri` under `examples/` (≥40 files, `:215-220`), which
accumulates and reports **all** violations in one panic rather than failing fast.

**Trap — that gate's panic message encodes the *old* ruling.** It currently instructs
(`:230-236`): *"Every one is a … false positive from the conformance walker, not a broken
example. Fix the walker … either the family's dedicated shape-based arm … or the
`general_leaf_param_family_is_validated` allowlist …; do NOT add a SKIP_SET entry."* After
γ, a firing diagnostic is a **true positive** — a genuinely un-migrated example — and an
implementer following the message verbatim would **revert the promotion**. Leaf γ must
rewrite that message. This is the same failure shape as §0.3: a guard whose prose preserves
a superseded decision.

---

## §5 Resolved design decisions

**D4-1 — Strict `DimensionVector` equality, all three classes.** Bare, cross-dimension and
non-scalar all reject. Bare `0` included; no literal special-case; no warn-and-convert (any
default unit re-creates the silent-guess class — program decision 1).

**D4-2 — Promote via the shared predicate, not a dedicated arm.** 5627's candidate list
included a "literal-only tolerance" arm mirroring `entity.rs`. Rejected: it would freeze the
inconsistency of §0.4 into the walker, and it is the arm shape that made gate 3 wrong in the
first place. One predicate, one rule.

**D4-3 — γ owns gates 1 **and** 2; δ₁ owns gates 3 and 4.** Because gates 1–2 ride one
predicate they cannot be split. δ₁ **must not** land before γ: doing so leaves a window in
which `param t : Length = 1.0` is an `entity.rs` Error while the sibling ctor spelling
`Limit(velocity_limit: 300.0)` is silent — a *worse* inconsistency than today's. Hard
`add_dependency` edge γ → δ₁, not prose ordering.

**D4-4 — `entity.rs` owns param/let-default diagnosis; the conformance pass defers.** After
δ₁ a param-default dimension mismatch emits **exactly one** diagnostic:
`ParamDefaultTypeMismatch` (params) / `LetAnnotationTypeMismatch` (lets), at Error, from
`entity.rs`. Rationale: `entity.rs` already owns the compound-expression case (`:486-499`),
its message names the *dimension* contract ("declared type and initializer dimension must
agree") rather than a generic type mismatch, and D5 named a param-default-specific code. The
conformance param-default entry must skip a param whose declared type is scalar-comparable
(`Type::Int | Type::Scalar{..}`) — the exact set `entity.rs:452` already claims. δ₁ pins
this with an **exactly-one-diagnostic** assertion.
*Note (G6-honest):* this double-report is **structurally implied**, not observed —
`param x : Real = "hi"` should already double-report today, since `Real` is in the vetted
family. δ₁'s first step is a **probe that establishes the current count**; if the
double-report already exists for dimensionless params, δ₁ fixes a pre-existing wart too, and
if some existing de-dup already prevents it, δ₁ extends that mechanism instead. Either way
the asserted post-state is "exactly one".

**D4-5 — `Type::ScalarParam` args are accepted through `is_numeric_placeholder_leaf`, NOT by
widening `arg_type_is_unverifiable`.** This is the house pattern the code itself prescribes:
the `Point` and `Matrix`/`Tensor` arms already tolerate a scalar-family arg via
`is_numeric_placeholder_leaf` (`Type::Int | Type::Scalar{..} | Type::ScalarParam(_)`),
"which is where its scalar-ness — not its unknown-ness — is the load-bearing property".
Widening `arg_type_is_unverifiable` instead would silence `String ← Scalar<Q>` at every arm
at once. γ carries a value-floor test proving `String` at a `Scalar<Q>` param still fires.

**D4-6 — Comply with PRD 1's D9; adopt its obligations, not its mechanism.** PRD 1
(`units-length-gate-completion.md:208-218`, landed `54afdee50b`) sets the program-wide
convention **D9 — one rejection-wording template, one diagnostic code**, with two
obligations: (i) every rejection carries a **migration hint** (spec §14.5, `:2597-2601` —
*"a diagnostic identifying the affected construct and the migration path"*); (ii) **INV-SF-6**
— every rejection carries a **`DiagnosticCode`**.

This PRD complies with both, and **does not invent a parallel convention**. Two boundary
notes, so nobody later "reconciles" them wrongly:

- **Obligation (ii) is already met by construction.** Unlike PRD 1's Contract C rejections
  (which carry no code until its task β mints one), this PRD's surface already emits coded
  diagnostics: `ArgTypeMismatch` (`reify-core/src/diagnostics.rs:617`),
  `ParamDefaultTypeMismatch` (`:419`), `LetAnnotationTypeMismatch` (`:456`),
  `ConstraintArgTypeMismatch` (`:312`). **This PRD mints no new code and must not adopt PRD
  1's new eval-layer code** — different layer, different surface, no double-filing.
- **Obligation (i) is what this PRD adds**, and **today it cannot route through D9's
  mechanism**. D9's template is `ArgRejection::message` (`arg_acceptance.rs:71-79`) — in
  **reify-eval**, which *depends on* reify-compiler, not the reverse (§4.2 item 4). D9
  correctly scopes byte-identical wording to **PRDs 1/3/5**, all eval-layer. This PRD
  therefore adopts the **wording shape** — `<base message>; pass a dimensioned <Dimension>
  literal such as `<example>`` — authored inside reify-compiler, with no `reify-eval` import.
  **A future session must not "fix" this by adding an upward dependency.**

  **However — PRD 5 dissolves the obstacle, and the decomposer should know it.**
  `dimension-checked-readers.md` (landed `efba5a8036`) **relocates `arg_acceptance` verbatim
  to `reify-ir`** (`:174-182` — "the lowest crate that owns `Value`", `pub use`d from
  reify-eval so existing call sites are untouched, semantics FROZEN). `reify-compiler`
  **already depends on `reify-ir`**. So once PRD 5's relocation lands, `ArgRejection::message`
  becomes legitimately reachable from this PRD's surface, and unifying onto D9's real template
  becomes both possible and desirable — *"one dimension-acceptance rule and one rejection
  wording"* is PRD 5's own stated goal. **Disposition:** γ implements the wording *shape*
  and must not block on the relocation; if PRD 5's relocation has already landed when γ
  dispatches, γ **should** use the relocated `ArgRejection::message` directly instead. Either
  way the user-visible wording is identical, so no leaf signal changes. Recorded as Open
  question 6.

**Code-name deviation from D5.** D5 named `E_FN_PARAM_DEFAULT_TYPE_MISMATCH`; the code that
actually shipped for this surface is `ParamDefaultTypeMismatch` (minted by 4318). **Reinstate
D5's rule under the shipped code**, not its literal code name — a recorded deviation, not
drift.

**D4-7 — δ sequencing, stated per leaf.** `CTOR_FIELD_CONFORMANCE_SEVERITY` is `Warning`
(`conformance/mod.rs:32`); the planned Warning→Error flip (**δ**) is **not blocked on this
PRD and this PRD does not perform it**. Every leaf in §11 states its δ-position and the
severity its signal asserts. Because §3.1 shows the Error entry is effectively unreachable
for this family, **γ is severity-safe pre-δ**: it adds Warnings only, and cannot turn a
previously-compiling design into a hard failure. δ₁/δ₂ *do* add Error-severity behaviour
pre-δ, which is why they land **after** their migrations.

**D4-8 — Assert on `DiagnosticCode` identity, not message substrings.** Per INV-SF-6
`diagnostics-carry-codes` and the house pattern (tasks 2255/3416 flipped substring tests to
code identity). Every leaf signal below names a code; message text in this PRD is
illustrative. Codes in play, all extant: `ArgTypeMismatch`
(`reify-core/src/diagnostics.rs:617`), `ParamDefaultTypeMismatch` (`:419`),
`LetAnnotationTypeMismatch` (`:456`), `ConstraintArgTypeMismatch` (`:312`).

**D4-9 — Corpus fix form is scope-dependent.** The corpus-standard fix is the compound-unit
literal (`300mm/s`) in ordinary user/example scope. In a **registry-less bootstrap scope**
it does not compile: `expr.rs:1455-1462` emits *"compound unit expression requires a unit
registry in scope"*, which is why `crates/reify-compiler/stdlib/units.ri` writes
`STANDARD_GRAVITY()` as `9.80665 * 1m / (1s * 1s)` and documents the constraint inline
(`units.ri:141-148`). **Migration leaves must use the compound literal in `examples/**` and
the compositional `<scalar> * <unit-literal>` form in registry-less stdlib scope**, and must
verify which regime each edited file is in rather than assuming.

---

## §6 Measured blast radius

> **G6 posture.** The authoritative counts are **leaf α's deliverable**, produced by the
> method 5627's architect used and 5646 records: *flip the predicate locally, run
> `examples_smoke` + the targeted suites, classify every hit*. The figures below are
> **scoping measurements** — sufficient to size the decomposition, not a substitute for α.
> Any leaf signal asserting a count cites α, never this section.

### 6.1 Probe-verified, this session (fresh `target/release/reify`)

Binary freshness established before use: mtime `2026-07-28 20:47:36 +0100`; every commit
landed after the build touched only test files, `docs/`, `scripts/`, `tests/infra/` — no
source linked into the release binary. Newer than both load-bearing sources
(`conformance/mod.rs`, `reify-cli/src/main.rs`).

| Probe | Result today |
|---|---|
| `Material(youngs_modulus: 200mm)` — Length into Pressure | **no diagnostic, exit 0** |
| `Material(youngs_modulus: "oops")` — String into Pressure | **no diagnostic, exit 0** |
| `Material(density: "heavy")` — String into MassDensity | **no diagnostic, exit 0** |
| same, against a locally-declared `param e : Pressure` | **no diagnostic, exit 0** |
| control: `Bool` param given a `String` | `warning: argument 'flag' has type 'String' but param 'flag' requires type 'Bool'`, exit 0 |
| control: same arm at the fn-call entry | `error: …`, **exit 1** |

The last two rows are the A/B that proves both the printing path and the exit-code path
(§3.2).

### 6.2 Gates 3 + 4 (δ₁) — measured this session

Swept over the 600 `.ri` files of the main worktree (stale `.claude/worktrees/` and
`.eval-worktrees/` copies deliberately excluded — including them inflates every count ~6×
with no meaning), driven from the authoritative registry `NAMED_DIMENSIONS`
(`crates/reify-core/src/dimension.rs:514`, **51 names**) **extended by 16 dimensioned type
aliases the registry does not name** — `Torque` (`stdlib/ports_mechanical.ri:29`), `Stress`
(`stdlib/analysis.ri:13`), `HeatFlux`/`ThermalResistance` (`stdlib/ports_thermal.ri:34,39`),
`VolumetricFlowRate` (`stdlib/ports_fluid.ri:34`), `Jerk`, `Permittivity`, … A sweep driven
by the registry alone **under-counts**; α must reproduce the alias extension.

> **Partially superseded by η (task 5785, merge `61300d09`).** This measurement predates η.
> `NAMED_DIMENSIONS` has since moved — re-measured 2026-08-03 at `dimension.rs:576-663` — and
> now holds **52 rows** — 51 distinct constant identifiers but only **49 distinct
> `DimensionVector` values**, because three alias pairs share a vector: `STIFFNESS`/
> `TRANSLATIONAL_STIFFNESS`, `ABSORPTION_COEFF`/`CURVATURE`, and `IMPULSE` registered twice
> as `"Impulse"` and `"Momentum"` — up from the 51 rows cited above, because η added
> `Torque` as a new registry row. `Torque` is therefore no longer one of the "aliases the
> registry does not name": the `pub type Torque` alias this paragraph cites at
> `ports_mechanical.ri:29` was deleted by η (that line is now a pointer comment), so the
> alias-extension count drops from 16 to **15**. The other exemplars all still exist, but
> three of their four line cites above have drifted — re-measured 2026-08-03: `Stress`
> `stdlib/analysis.ri:13` (still exact), `HeatFlux` `stdlib/ports_thermal.ri:39`,
> `ThermalResistance` `stdlib/ports_thermal.ri:44`, `VolumetricFlowRate`
> `stdlib/ports_fluid.ri:37`. The `dimension.rs:514` anchor in the paragraph above no longer
> resolves to the registry either — at HEAD that line sits inside
> `DimensionVector::to_display_units` (`:504`), not `NAMED_DIMENSIONS`.
> The underlying claim (a registry-only sweep under-counts) still holds.

| Category | Count |
|---|---|
| **A.** `.ri` `param <n> : <DIMENSIONED> = <bare literal>` | **2** |
| **B.** `.ri` `let <n> : <DIMENSIONED> = <bare literal>` | **0** |
| **C.** Rust fixture-string occurrences (22 files) | **63** |
| — c1: **would break** | **25** |
| — c2: **deliberate pins to invert** | **10** (in the 4 tests of §0.3) |
| — c0: no break (28) — of which parse-only, needs no change | 28 (11 parse-only) |
| — prose blocks stating the tolerance as intent | 23 |
| **Total real work** | **27 hard breaks + 10 inversions** |

The two `.ri` sites:

```
examples/bearing_auto_seal.ri:46                      param durometer : Length = 70.0
tree-sitter-reify/test/fixtures/mv-2-priv-param.ri:4  priv param rated_torque : Torque = 5
```

Only the first is type-checked; it breaks `examples_smoke.rs` **and**
`crates/reify-eval/tests/auto_type_param_determinism_tests.rs:391`
(`bearing_auto_seal_fixture_compiles_with_three_seal_candidates`). **It is also a genuine
latent bug** — Shore durometer is dimensionless, so `: Length` is simply the wrong
annotation. β fixes the *annotation*, not the literal.

**Is the bare form the idiom or the exception?** Of **983** dimensioned `param`/`let`
declarations carrying a default in `.ri`:

| Form | Count | Share |
|---|---|---|
| unit-suffixed quantity literal (`= 5mm`, `= 200GPa`) | 818 | **83.2 %** |
| dimensionally-explicit compound expr (`= 2000.0 * 1N / 1m`) | 68 | 6.9 % |
| `auto` / `auto(free)` / `undef` | 96 | 9.8 % |
| **bare numeric literal** | **2** | **0.2 %** |

**Decisively the exception.** 90.1 % are already dimensionally explicit, and the 68
compound-expression sites read like authors migrating away from bare literals by hand. This
is what makes D4-1 nearly free at gates 3/4 — the same reason candidate 4 was affordable at
gate 1.

Two findings that reshape δ₁'s scope:

- **Trait-lets are already strict.** `let score : Mass = 1.5` inside a trait already errors
  via the `TypeMismatchForTraitMember` path (`crates/reify-compiler/tests/m9_error_cases.rs:276`).
  So structure params/lets are permissive while trait lets are strict — δ₁ **removes an
  existing inconsistency** rather than introducing strictness.
- **One fixture's doc comment is a rationale that dies with the tolerance.**
  `crates/reify-eval/tests/eval_param_overrides.rs:1608-1630` explains that `param p : Money = 0`
  uses an `Int` literal *"because a bare `Real` initializer on a dimensioned `Money` param
  trips `ParamDefaultTypeMismatch`, whereas a whole-number `Int` literal is accepted for any
  dimensioned Scalar."* δ₁ must retire that paragraph and set the fixture to `= 0USD`.

**Reusable precedent for both the sweep and a permanent guard:**
`crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` already walks exactly the trees
this PRD cares about (`examples/**/*.ri`, `crates/**/*.ri`, `crates/**/*.rs` inline fixtures,
`gui/**`) and already solves the hard parts — comment stripping, and excluding
`crates/reify-syntax/tests` / `crates/reify-ast/tests` as parse-only. **That exclusion list
matches the parse-only bucket above exactly.** α and δ₁ should reuse it rather than
re-deriving a sweep.

### 6.3 Gate 1 (γ/β) — measured this session

Swept **595 `.ri`** + **1 707 `.rs`** files at `dc83d4fd60` (same worktree-exclusion discipline
as §6.2). Inventory: **932 `structure` blocks → 444 distinct structures declaring ≥1
dimensioned-`Scalar` field → 1 157 distinct (structure, field) pairs**. Of **2 770 ctor call
sites**, **474 named args land on a dimensioned-Scalar field**.

| Class | Sites | `examples/**` | stdlib `.ri` | Rust fixtures | other `.ri` |
|---|---|---|---|---|---|
| **BARE** | **17** | 6 | **0** | 5 | 6 |
| **CROSS-DIMENSION** | **0** | 0 | 0 | 0 | 0 |
| **NON-SCALAR** | **0** | 0 | 0 | 0 | 0 |
| OK (unit literal / dimensioned arithmetic / dimensioned param-ref / `auto`) | 454 | 136 | 7 | 237 | 74 |

**17 arg-sites across 16 source lines in 5 files.** Two facts reshape the PRD's risk profile:

- **`stdlib` is completely clean** — every stdlib ctor arg at a dimensioned field already
  passes a unit literal, a dimensioned param ref, or `STANDARD_GRAVITY()`.
- **Classes (ii) cross-dimension and (iii) non-scalar have ZERO existing call sites.** The
  probe fixtures of §6.1 are *synthetic*; nothing in the corpus does this today. So the
  widening beyond 5627's scoped question costs **nothing** in migration and buys the guard
  outright. This is the strongest possible evidence for D4-1.

The complete BARE list (β's migration set). The **Disposition** column records what β
actually did — added by task 5758, which migrated **15 of the 17** sites; the measurement
itself is unchanged.

| File:line | Field | Source | Disposition after β |
|---|---|---|---|
| `examples/trajectory/tots_optimal_ptp.ri:67` | `JointLimit.max_force : Scalar<Force>` | `let jl = JointLimit(joint: 0.0, max_force: 1000.0)` | MIGRATED `1000N` — magnitude-preserving |
| `examples/trajectory/tots_optimal_ptp.ri:78` | `TOTSShaper.velocity_limit` | `velocity_limit: 300.0,` | MIGRATED `300mm/s` — **RULED mm-scale** (esc-5758-2 option B); SI 300 → 0.3, intended |
| `examples/trajectory/tots_optimal_ptp.ri:79` | `TOTSShaper.acceleration_limit` | `acceleration_limit: 5000.0,` | MIGRATED `5000mm/s^2` — **RULED mm-scale**; SI 5000 → 5, intended |
| `examples/trajectory/printer_print_envelope.ri:153` | `JointLimit.max_force` | `actuator_limits: [JointLimit(joint: 0.0, max_force: 1000.0)],` | MIGRATED `1000N` — magnitude-preserving |
| `examples/trajectory/printer_print_envelope.ri:154` | `TOTSShaper.velocity_limit` | `velocity_limit: 300.0,` | **DEFERRED — stays bare after β.** Task #5847, gated on #5412 (esc-5758-4 option D1) |
| `examples/trajectory/printer_print_envelope.ri:155` | `TOTSShaper.acceleration_limit` | `acceleration_limit: 5000.0,` | **DEFERRED — stays bare after β.** Task #5847, gated on #5412 |
| `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs:1420` (×2 args) | the pinning probe's own fixture | `let l = Limit(velocity_limit: 300.0, acceleration_limit: 5000.0)` | MIGRATED `300mm/s` / `5000mm/s^2`; probe renamed (see §11 γ) |
| `crates/reify-eval/tests/input_shape_eval_e2e.rs:248/:255/:256` | `JointLimit.max_force`, `TOTSShaper.{velocity,acceleration}_limit` | bare `100.0` / `300.0` / `5000.0` | MIGRATED `100N` (magnitude-preserving) / `300mm/s` / `5000mm/s^2` (**RULED mm-scale**) |
| `gui/test/fixtures/large_assembly.ri:18/19/27/28/36/37` | `Material.{density : Density, youngs_modulus : Pressure}` | `density: 7850.0,` / `youngs_modulus: 200000000000.0` (×3 materials) | MIGRATED `7850kg/m^3` / `200GPa` etc. — all magnitude-preserving |

**Why `printer_print_envelope.ri:154/:155` are deferred (measured, esc-5758-4).**
Dimensioning them **alone** makes the TOTS solve `ConstraintInfeasible`: `input_shape`
returns `Undef`, `track_tots` comes back empty and `peak_tots` becomes exactly 0 — with
**exit 0 and zero diagnostics**, which is how this defect class hides. The cause is that this
file's `Waypoint.values` are dimensionless `JointValue` (`trajectory.ri:77`,
`kinematic.ri:306`) running 0 → 50 → 200 → 0 at `:115-118`, demanding ~200 units/s. The
limits and the waypoints must be corrected **together**; task #5847 (gated on #5412) owns
that. β pins the split state positively in
`crates/reify-eval/tests/printer_print_envelope_e2e.rs` section (6) so a later sweep cannot
silently migrate them.

**The GUI fixture is a degraded copy with a correct twin already in the repo.**
`gui/test/fixtures/large_assembly.ri` is loaded live by `gui/src-tauri/src/debug_server.rs:1236`
and `gui/test/visual/assertions.ts:47`; `examples/large_assembly.ri:51-53` is the *same
assembly written correctly* (`density: 7850kg/m^3`, `youngs_modulus: 200GPa`). β copies the
spelling rather than inventing one.

**Excluded as false positives — do not budget for these.** Three raw BARE hits are
parser-only unit tests where the structure is never declared and the walker never runs
(`crates/reify-syntax/tests/harness_syntax/auto_binding_sites_lowering_tests.rs:245`, `:360`;
`function_call_named_args_tests.rs:128`), and five more declare their own *dimensionless*
shadowing structure (`crates/reify-cli/tests/fixtures/stdlib_sim_ready_material_ok.ri:11`;
`purpose_compile_tests.rs:1814`; `purpose_activation.rs:1547`;
`termination_check_tests.rs:78`, `:346`). α must reproduce the file-local-shadowing check —
a naive sweep over-counts by ~8.

### 6.4 Scope boundary — which param shapes the promotion actually reaches

Only a param whose type resolves to a **bare `Type::Scalar` leaf** reaches
`general_leaf_param_family_is_validated`. Two different exclusions follow, and conflating
them would overstate what this PRD closes:

- **`Option<Q>` / `List<Q>` / `Set<Q>` / `Map<K,V>`** — the walker recurses lockstep into
  these (`conformance/mod.rs:669-701`), so the **leaf** is judged and these **are covered**.
- **`Vector3<Q>` / `Point3<Q>` / `Matrix` / `Tensor` / `Field`** — these have their own
  dedicated shape-based arms (task 5465's four promoted families), which accept a
  scalar-family arg via `is_numeric_placeholder_leaf` and therefore stay **dimension-blind**
  in the quantity slot. **This PRD does not change that**, and the residual is real:
  e.g. `crates/reify-compiler/stdlib/kinematic.ri:130-132/:148-150` types joint stiffness as
  `Option<TranslationalStiffness>` (covered), while `dynamics.ri:79`
  `MassProperties.inertia : Matrix<3,3,MomentOfInertia>` is not.

**Named residual, deliberately out of scope (§12):** dimension-checking the *quantity slot*
of `Vector`/`Point`/`Matrix`/`Tensor`/`Field` params. This is 5627's candidate-2 territory
(the `reify-core/src/ty.rs` quantity-slot convention) and a separate ruling. **α records the
count; the decomposer files it as a follow-up, not as a leaf of this PRD.**

### 6.5 What α must still measure

1. **Gate 2** (conformance param-default entry) hits — never separately measured, by anyone.
2. **Gate 5** (constraint-def Rule 4) — Rule 4 conflates two tolerances (cross-dimension
   scalars **and** plain `Int`-for-`Length`); removing the whole rule is a wider change than
   removing its cross-dimension half. α reports the two sub-counts **separately** so δ₂ can
   scope correctly (Open question 1).
3. **`Type::ScalarParam` false positives** (D4-5) — the dimension-generic corpus.
4. **Fn-call-entry reachability** (§3.1). Corroborating evidence that it is zero: the sweep
   found **0 bare-at-dimensioned user-fn call sites in the entire `.ri` corpus** — already
   rejected by gate 6. If α nonetheless finds a reachable leaf, it is Error-severity pre-δ
   and must be migrated in β.
5. **The `Vector`/`Point`/`Matrix` quantity-slot residual count** (§6.4) — for the follow-up.
6. **The load-struct intersection flag for PRD 5** (§10.2).
7. **Re-confirmation of §6.2/§6.3 against HEAD**, not re-derivation.

### 6.6 The one place a "zero new rejects" prediction could be wrong

**57 of the 474 in-scope args are non-literal expressions** — `span / 2`,
`5 * STANDARD_GRAVITY()`, `100mm * 1mm` at an `Area` slot, `self.env.build_x`, and
dimensioned param references. All 57 were hand-resolved as dimensionally **correct**, so the
corpus fact is settled. But strict `DimensionVector` equality only stays green if the
walker's **arg-side inference** returns the precise dimension for arithmetic results and
param refs, rather than a widened or fallback type. **This is an implementation risk, not a
corpus risk**, and it is the single most likely source of an unexpected γ failure. γ's I1
value floor must include at least one arithmetic-derived and one param-ref arg.

### 6.7 Known-safe (do not migrate)

Arithmetic-derived values are already correct: `2*10mm`, `sqrt(100mm*100mm)`,
`max(10mm,20mm)` all produce a LENGTH `Scalar` (research doc, red-team item 9). Literal `0`
in a comparison against a dimensioned operand is already handled by the shipped
polymorphic-literal-zero rewrite (task 4485), which is why
`crates/reify-compiler/stdlib/trajectory.ri:598-599` legitimately writes
`constraint velocity_limit > 0`.

---

## §7 Contract [B+H]

G5 verdict: **B+H, yes.** Trigger: a language-semantics change reaching trait conformance
and annotation checks across ≥3 gates, with ≥2 cross-PRD consumers (PRD 5's retyped load
structs; the stdlib's ~25 already-dimensioned params). 5627 records the global reach of its
candidate 1; candidate 4 is narrower but still multi-gate.

### 7.1 Predicate contract

```rust
/// Vets a param family for judgement by raw `type_compatible` at the general
/// concrete-leaf arm. Dimensioned `Scalar` is ADMITTED (this PRD): the language
/// rule at every construction boundary is strict DimensionVector equality.
fn general_leaf_param_family_is_validated(param_type: &Type) -> bool
```

Invariants the arm must satisfy, each with a value-floor test in γ:

| # | Invariant | Test shape |
|---|---|---|
| I1 | `Scalar{Q}` ← `Scalar{Q}` is silent — for a **unit literal**, an **arithmetic-derived** value (`span / 2`, `100mm * 1mm` at an `Area` slot, `5 * STANDARD_GRAVITY()`) and a **dimensioned param reference** | clean fixture, all three arg shapes (§6.6 — the highest-risk invariant) |
| I2 | `Scalar{Q}` ← `Scalar{R}`, `Q ≠ R` emits one `ArgTypeMismatch` naming the param | value floor |
| I3 | `Scalar{Q}` ← bare `Real`/`Int` emits one `ArgTypeMismatch` | value floor |
| I4 | `Scalar{Q}` ← `String`/`Bool` emits one `ArgTypeMismatch` | value floor |
| I5 | `Scalar{Q}` ← `Type::ScalarParam(_)` is **silent** (D4-5) | anti-false-positive floor |
| I6 | `Scalar{Q}` ← `Type::Error` is silent (anti-cascade, `reject_if_incompatible`) | regression |
| I7 | every emitted message carries a migration hint (D4-6) | wording assertion |
| I8 | dimensionless `Scalar` ← `Int` stays silent (`type_compatible` widening; pinned today by `param_default_valid_int_and_real_is_clean`, `struct_ctor_field_conformance_tests.rs:647`) | non-regression |

### 7.2 Severity contract

| Entry | pre-δ | post-δ | Exit code pre-δ |
|---|---|---|---|
| ctor field slots (`:331`) | Warning | Error | 0 (printed only) |
| param defaults, conformance (`:530`) | Warning | Error | 0 (printed only) |
| fn-call args (`:380`) | Error | Error | 1 — **unreachable for this family**, §3.1 |
| `entity.rs` param/let defaults (δ₁) | **Error** | Error | **1** |
| constraint-def args (δ₂) | **Error** | Error | **1** |

γ **must not** touch `CTOR_FIELD_CONFORMANCE_SEVERITY`. δ is a separate, unblocked work
item owned elsewhere.

### 7.3 Corpus-gate contract

`examples_smoke::no_example_emits_ctor_field_conformance_diagnostics` must be **green at
every commit**. Its meaning inverts at γ (§4.3); γ rewrites its panic prose. `SKIP_SET`
must not grow — the gate's own guard `skip_set_entries_exist_under_examples_dir`
(`examples_smoke.rs:248`) and the ≥40-file floor (`:215-220`) stay untouched.

### 7.4 Boundary-test sketch (facing both sides of the seam)

| # | Scenario | Preconditions | Postconditions (assertions) |
|---|---|---|---|
| B1 | **Author side.** `.ri` passes `200mm` to a `Pressure` ctor field | γ landed; β migration complete | `reify check` prints ≥1 diagnostic with code `ArgTypeMismatch` naming `youngs_modulus`; message contains a migration hint; exit 0 pre-δ |
| B2 | **Author side.** `.ri` passes `"heavy"` to a `MassDensity` field | γ landed | one `ArgTypeMismatch`; **not** silenced by the ScalarParam fence |
| B3 | **Author side.** `.ri` passes `7850kg/m^3` to the same field | γ landed | **zero** ctor-conformance diagnostics |
| B4 | **Compiler side.** dimension-generic fn forwards `Scalar<Q>` into a concrete `Scalar<Length>` ctor field | γ landed; D4-5 fence in place | **zero** diagnostics (I5) — and `String ← Scalar<Q>` at the same arm still fires (I5's fence is not a blanket) |
| B5 | **Corpus side.** whole `examples/` tree | γ + β landed | `no_example_emits_ctor_field_conformance_diagnostics` green over ≥40 files |
| B6 | **Author side.** `param t : Length = 1.0` | δ₁ landed | **exactly one** diagnostic, code `ParamDefaultTypeMismatch`, `Severity::Error`, `reify check` exit **1** |
| B7 | **Author side.** `let t : Length = 1.0` | δ₁ landed | exactly one `LetAnnotationTypeMismatch`, Error, exit 1 |
| B8 | **Author side.** `param t : Length = 5mm` and `param r : Real = 1.0` | δ₁ landed | zero diagnostics (no over-rejection) |
| B9 | **Constraint side.** a constraint over a strictly-typed dimensioned field | γ + ε landed | the constraint reaches a definite Satisfied/Violated verdict — **not** Indeterminate-satisfied |
| B10 | **Downstream side.** PRD 5's retyped `PointLoad(force: …)` | γ landed; PRD 5 retyping landed | a bare `5000` at a `Force` field is diagnosed; `5000N` is clean (today the polarity is *inverted* — §10.2) |

---

## §8 Pre-conditions & substrate (G3)

**No novel grammar.** Every syntax form this PRD relies on parses today — verified this
session with the grammar gate (`tree-sitter parse --quiet`, run from `tree-sitter-reify/`,
**exit 0, zero ERROR nodes**) over a fixture carrying all four forms:

```reify
module test.g3_ctor_strict
structure def Limit {
    param velocity_limit : Scalar<Velocity>
    param acceleration_limit : Scalar<Acceleration>
}
structure def Root {
    let a = Limit(velocity_limit: 300mm/s, acceleration_limit: 5000mm/s^2)
    let b = Limit(velocity_limit: 300.0 * 1mm / 1s, acceleration_limit: 5000.0 * 1mm / (1s * 1s))
    param speed : Scalar<Velocity> = 300mm/s
    let c : Scalar<Force> = 5000.0 * 1N
}
```

Both the compound-unit and compositional spellings parse (D4-9 concerns *compilation* scope,
not grammar). Fixture committed at
`docs/prds/v0_6/fixtures/dimensioned_construction_strictness.ri`.

**Substrate, verified present on main (not merely declared):**

| Capability | Evidence |
|---|---|
| strict `Scalar`-vs-`Scalar` semantics in `type_compatible` | `type_compat.rs:52-180`, `:220-237` — no cross-dimension arm exists (§2.2) |
| the predicate and its three live callers | `conformance/mod.rs:1691`, callers `:333/:382/:532` |
| `ArgTypeMismatch` emission machinery | `conformance/mod.rs:1084-1099`, `reject_if_incompatible` `:1194` |
| the `ScalarParam` fence pattern | `is_numeric_placeholder_leaf`, used by the shipped Point/Matrix arms |
| corpus gate | `examples_smoke.rs:198`, ≥40 files, severity-blind |
| `reify check` prints + exits on compile diagnostics | `main.rs:283-285`, `:546-552` (§3.2, empirically confirmed) |
| all four diagnostic codes | `reify-core/src/diagnostics.rs:617/419/456/312` |
| the corpus fix forms | `units.ri:141-148`; `expr.rs:1455-1462` registry guard |

**No prerequisite substrate task is required.** G3: clear.

---

## §9 Design-invariant walk (G7, advisory in author mode)

Against `docs/legibility/design-invariants.md`:

| Invariant | Bearing |
|---|---|
| **INV-SF-3** `declared-intent-consumed-or-diagnosed` | **This PRD is a remediation of it.** ~25 stdlib params declare a dimension and enforce nothing; the declaration is design intent that no pass consumes. γ makes it consumed. |
| **INV-SF-4** `indeterminate-attributable-transient` | **Confirmed violation, and this PRD is its remedy** — §10.3. γ makes the permanently-Indeterminate input class impossible at construction; ε pins the recovery and files the one residual defect (operand-kind `Undef` emitted at Warning) by name rather than leaving it silent. |
| **INV-SF-5** `placeholders-owned-and-loud` | The trajectory bare-number call sites are acknowledged placeholders (`printer_print_envelope.ri` calls its own args *"(mm/s placeholder Real)"*); β makes them real. New placeholders are not introduced. |
| **INV-SF-6** `diagnostics-carry-codes` | D4-8: every new/changed diagnostic carries a code; every assertion is on code identity. |
| **INV-SF-2** `error-severity-exits-nonzero` | Respected and *relied upon* (§3.2). This PRD adds no per-code escalation and no Error-severity output on a healthy path. |

No waiver required.

---

## §10 Cross-PRD relationship & in-flight tasks (G4)

### 10.1 Seam table

| Other PRD / surface | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `units-length-gate-completion` (PRD 1) — **landed `54afdee50b`** | consumes a **convention** | PRD 1's **D9** (`:208-218`): one rejection-wording template + `DiagnosticCode` on every rejection. File-wise disjoint: PRD 1 gates **eval-layer geometry** chokepoints + `builtin_signatures` compile slots; this PRD touches **neither**. | PRD 1 owns D9 | **complied with, no edge** — D4-6 adopts D9's two obligations; D9's *mechanism* (`ArgRejection::message`, reify-eval) is unreachable from reify-compiler, and D9 correctly scopes byte-identical wording to PRDs 1/3/5 |
| `check-diagnostic-truthfulness` (PRD 2) — **landed `0780c7604c`** | consumes (already-landed behaviour) | `reify check`'s compile-diagnostic print + exit gate (`main.rs:283-285`, `:546-552`) | **PRD 2** owns `cmd_check`/`finish_check` semantics | **no edge needed, reciprocal check PASSED** — §3.2: the visibility this PRD's signals rely on exists on main today, and PRD 2's own seam row reads *"produces-for … for ctor-slot conformance promotion … no seam today"*. PRD 2 independently confirms the `:546-552` pre-engine gate (`check-diagnostic-truthfulness.md:269`). PRD 2 also owns acting on the research-doc correction in §3.2. |
| `angle-units-surface-convergence` (PRD 3) | adjacent | ANGLE is a `DimensionVector` like any other, so γ gates `Scalar<Angle>` **ctor fields** automatically. PRD 3 owns ANGLE **eval/compile-slot** gating, `resolve_bare_angle` retirement, `Nm`, and unit grammar/display. | PRD 3 for angle surface; this PRD for the ctor boundary generically | **name-only overlap**; α reports any angle-typed ctor field it migrates so PRD 3 does not re-file it |
| `dimension-checked-readers` (PRD 5) | **produces for** | Load-struct retyping: `fea_multi_case.ri` `PointLoad.force` / `PressureLoad.magnitude` etc. | **PRD 5** owns the retyping **and** its call-site migration | **candidate edge, coordinator-wired** — §10.2 |
| stdlib `.ri` — the ~25 already-dimensioned params | produces for | γ makes their declarations enforced | this PRD | in β/γ |
| `real-dimensionless-unification.md` | amends | D5 record (§0.2/§0.3) | this PRD | leaf **ζ₀** |
| units doc chunks + best-practices corpus | produces for | authoring surface | this PRD | leaves **ζ₁–ζ₄** |
| `docs/reify-mcp/.../units.md` "35 standard named dimensions" (actually 51 names / 49 vectors) | adjacent | stale count in the units chunk | **PRD 3** (unit grammar/display/labels) | **not this PRD's** — named so it is not double-filed |

### 10.2 The PRD-5 seam, precisely (polarity is the opposite of the obvious reading)

`fea_multi_case.ri` declares `PointLoad.force` / `PressureLoad.magnitude` as **`Real`**
today. `Real` *is* dimensionless `Scalar`, which is **already** in the vetted family — so
today `PointLoad(force: 5000N)` (the units-**correct** spelling) already emits a
ctor-conformance warning, while `PointLoad(force: 5000)` is clean. That is exactly the
research doc's PHASE 2 §3 observation that the correct spelling "warns + contributes ZERO
force".

Therefore **γ changes nothing about `PointLoad` until PRD 5 retypes the field.** The
coupling is one-directional and low-risk:

- **Named seam:** the `Real → Force`/`Pressure` retyping of the fea/analysis load structs —
  `PointLoad.force`, `PressureLoad.magnitude`, `TractionLoad.traction`,
  `BodyForce.force_density`. **All four verified still `Real` at HEAD**
  (`crates/reify-compiler/stdlib/fea_multi_case.ri:315-317`, `:418-419`, `:446-448`,
  `:476-478`); `Gravity.magnitude : Acceleration` (`:516`) is the *only* dimensioned Scalar
  field in the entire FEA-load cluster today. So PRD 5's retype **moves these four out of the
  already-strict dimensionless family and into the newly-strict dimensioned one** — and every
  bare-number call site of theirs becomes a new reject *at that moment*, which is exactly
  PRD 5's 90-site migration.
- **Owner:** **PRD 5** (retyping *and* its call-site migration; they are inversely coupled
  with PRD 5's reader fix — either alone worsens the silent drop).
- **Reciprocal check — PASSED.** `dimension-checked-readers.md` (landed `efba5a8036`) states
  the same split from its side: *"**PRD 4** owns `conformance/mod.rs` / `type_compat.rs` … ;
  **this PRD** owns the `.ri` retype + the 90-site migration"*, and *"the migration must
  cover the bare `PointLoad(force: 1000.0)` corpus **regardless** of PRD 4's landing order;
  the coordinator wires the definitive cross-batch edge once both batches exist"*. No
  reciprocal "the other owns it" ambiguity.
- **PRD 5's measured migration: 90 in-scope constructor sites** (78 `PointLoad` + 2
  `TractionLoad` + the remainder), split 35 `.ri` / 77 `.rs`. **This is PRD 5's work, not
  α's.** α's ledger must flag — and then **exclude** — every load-struct field it
  encounters, so the two migration lists are provably disjoint and no site is double-counted
  or double-edited.
- **Candidate edge (recommended direction):** PRD 5's retyping leaf **depends on** this
  PRD's **γ**. Rationale: retyping into a world without the gate is silently useless (the
  field still accepts anything), whereas retyping after γ gives an immediate, observable
  diagnostic on every un-migrated call site. The reverse edge buys nothing. PRD 5 has
  deliberately made itself order-independent, so this edge is an optimisation, not a
  correctness requirement.
- **Filing rule:** this PRD **files no PRD-5-owned work**. The coordinator wires the edge
  once both batches exist.
- **Ownership clarification — the δ flip.** PRD 5's seam row phrases PRD 4 as owning "the δ
  severity flip". **It does not.** This PRD owns the *file* `conformance/mod.rs`; the
  `CTOR_FIELD_CONFORMANCE_SEVERITY` Warning→Error flip is a separate, pre-existing work item
  that is **not blocked on this PRD and is not performed by it** (D4-7, §12). **The
  decomposer must not file a δ leaf from this PRD.**

### 10.3 The constraint backstop — resolved at authoring time

The brief asked this PRD to establish whether strict ctor conformance makes the
`constraint magnitude > 0N` **Indeterminate-satisfied** case reachable, or whether an
independent constraint-evaluation defect remains. **Both questions are now answered**, by
tracing the full path this session.

**Verdict: the Indeterminate is a CONSEQUENCE. γ fixes it. There is no defect in
order-comparison evaluation for well-typed operands.**

The causal chain is single-threaded and cut at the constructor:

1. The **compile-time** comparison guard `emit_comparison_operand_diagnostics`
   (`crates/reify-compiler/src/expr.rs:476-583`, called from `compile_binop` at `:1627`/`:1952`)
   *is* competent — a cross-dimension comparison is a hard `Severity::Error`
   (`type_compat.rs:1335-1366`, `DiagnosticCode::DimensionMismatch`). It adjudicates
   **declared** types, and `param magnitude : Force` vs `0N` is `Scalar{FORCE}` vs
   `Scalar{FORCE}` — clean. **The compiler is right given what it is told.**
2. The only thing that makes the declaration a lie is the exclusion at
   `conformance/mod.rs:1694` — i.e. gate 1.
3. The ctor then persists the lie **verbatim**, with no dimension stamping
   (`crates/reify-expr/src/lib.rs:1085-1087`).
4. At eval, `eval_cmp` (`crates/reify-expr/src/lib.rs:5070-5110`) hits the
   Real-vs-dimensioned arm (`:5099-5103`) → `Value::Undef` → `Satisfaction::Indeterminate`
   (`crates/reify-constraints/src/lib.rs:183-215`) → treated as non-violating
   (`reify-cli/src/main.rs:2299-2305`, deliberate and documented at `:2364-2371`) → **exit 0**.

Close (2) and the `Real` never reaches that cell, so arm `:5099-5103` is unreachable for this
input class. Note the symmetry that proves the compile layer is fine: for a field declared
*dimensionless* holding a `Real`, `constraint efficiency > 5mm` **already is** a hard compile
error today, via that very same arm.

**The stdlib already documents this exact failure mode** —
`crates/reify-compiler/stdlib/modal_analysis.ri:497-499`: *"Dimensioned literal `0N` … is
required to keep eval_cmp dim-equality matching at runtime; a bare `0` would yield
`Indeterminate` per task #3115 esc-3115-112"*. The research doc's framing ("constraints are
NOT a backstop") is **correct as stated**; the reason is the constructor, not the comparator.

**One genuinely independent defect survives, and one asymmetry.** Neither is fixed by this
PRD; ε files both by name.

- **Residual defect — operand-kind `Undef` is silently green.** The `Undef → Indeterminate`
  mapping (`crates/reify-constraints/src/lib.rs:183-215`) does **not** distinguish "input
  genuinely undefined (unresolved auto-param, unrealized geometry)" from "all inputs defined,
  the *operator* refused these operand kinds" — even though `classify_undef` (`:78-111`)
  computes exactly that distinction and puts it in the message
  (*"operator undefined for these operand kinds: Real"*, `:190-203`). Both emit
  `Diagnostic::warning` (`:211`), so `reify eval` exits 0 (`main.rs:1661-1665`) and
  `reify check` exits 0 without `--strict`. **Any future operand-kind `Undef` in a constraint
  is silent-and-green, whatever produced it.** Minimal independent fix (for the follow-up, not
  here): when `classify_undef` reports `has_undef == false`, emit `Severity::Error` or a
  distinct code rather than `warning` — the "never a false `Violated`" discipline is
  preserved because the *satisfaction* stays `Indeterminate`; only the severity changes.
- **Asymmetry — `==` does not behave like `>`.** `eval_eq`
  (`crates/reify-expr/src/lib.rs:5048-5052`) returns `Value::Bool(false)` for
  Real-vs-dimensioned, **not** `Undef`. So on the *same* corruption,
  `constraint magnitude == 0N` reports **VIOLATED** (definite, exit 1) while
  `constraint magnitude > 0N` reports **INDETERMINATE** (silent, exit 0). Any
  "constraints as backstop" reasoning must not generalize across the two families.

**INV-SF-4 assessment.** This *is* a violation as written — for the input class "`StepForce`
constructed with a bare number", the constraint is Indeterminate in **every** run, and no
runtime condition clears it. It is worse in kind than the invariant's own cited evidence
(inert `frame_align` selectors): this constraint *looks* enforced, is asserted by a dedicated
compiler test (`crates/reify-compiler/tests/modal_options_validation_tests.rs:1465-1505`
`step_force_constrains_magnitude_positive`), is cited in three stdlib comment blocks as the
reason a dimensioned zero was chosen — and disarms itself precisely on the inputs it exists
to catch. **The correct INV-SF-4 remedy is not "make this constraint a compile error"** — the
constraint text is well-formed and definite for every well-typed instance. It is *"make the
input class that renders it permanently Indeterminate impossible at construction"* — i.e.
exactly γ.

### 10.4 In-flight tasks

| Task | Status | Disposition |
|---|---|---|
| **5627** — ctor-slot ruling | pending | **This PRD's ruling leaf (γ).** Ruling already made (candidate 4, widened). **Never cancel.** Decompose binds it to this PRD and wires edges. Its description's four-candidate menu is now *history*, not an open question. |
| **5646** — "close the two gates 5627 left open" (`entity.rs` literal defaults; constraint-def args), deps `[5627]`, pending, low | pending | **Owns this PRD's δ₁ + δ₂ exactly.** **Not listed in the program brief's in-flight table — surfaced by this PRD's dup-check.** Decompose **binds and rescopes 5646, does not duplicate it**. Two premise corrections it needs (readback-verified): (a) its BACKGROUND states 5627 *"formally RETIRED PRD decision D5 … affirming only its direction"* — that is a **projection of a ruling that never landed** (5627 is still pending, no code changed); the ratified decision **REINSTATES** D5 (§0.2). (b) its CAUTION about arming the `check_fn_arg_conformance` Error surface is **severity-true but reachability-false** for this family (§3.1). Its "not urgent / not a live defect" framing is superseded. |
| **4580** — trajectory param tightening | done | β finishes it: 4580 tightened `velocity_limit`/`acceleration_limit` to `Scalar<Velocity>`/`Scalar<Acceleration>` (`stdlib/trajectory.ri:568-569`) and left the call sites bare. |
| **4318** — param declared-type vs initializer | done | The task that silently reversed D5 (§0.3). δ₁ retargets its two pinning tests; δ₁'s doc comments must name 4318 as the reversal's origin. |
| **5465** — promoted four of five families | done | Held this fifth family deliberately, filing 5627. γ completes 5465's programme. |
| 5623 / 5658 / 5661 / 5662 | pending | **PRD 1's.** Not touched here. |

---

## §11 Decomposition plan

Labels are PRD-local; task ids are assigned at decompose. Every edge below is a **real
`add_dependency`**, never prose ordering.

```
ζ₀ ──┐
     ├──► α ──► β ──► γ ──┬──► δ₁ ──► δ₂
     │                    ├──► ε
     │                    └──► ζ₁ ──► ζ₂ ──► ζ₃ ──► ζ₄
```

### ζ₀ — Record the ruling and the reversal history *(doc-only; lands first)*

- **Modules:** `docs/prds/v0_6/real-dimensionless-unification.md`, this PRD.
- **Work:** amend real-dimensionless-unification with a D5-status note pointing at §0 here —
  D5 **reinstated**, with the 4318 reversal recorded and the `:82` ε-fold named as its
  origin. No code.
- **Signal:** a reader of `real-dimensionless-unification.md:51` reaches the current ruling
  in one hop; `git log -S` on the D5 text shows a recorded transition where previously there
  was none. *(Doc-surface leaf — the project's doc-reconcile signal shape, cf. task 4376.)*
- **δ-position:** pre-δ, no severity claim. **Prereqs:** none.
- *Lands first so β/γ's commit messages can cite a landed ruling.*

### α — Blast-radius measurement + migration ledger *(intermediate)*

- **Modules:** measurement only; no behaviour change. Ledger committed under
  `docs/notes/`.
- **Work:** flip the predicate locally (do **not** land it), run
  `examples_smoke::no_example_emits_ctor_field_conformance_diagnostics` plus the targeted
  suites, and classify **every** hit as BARE / CROSS-DIM / NON-SCALAR / SCALARPARAM-FALSE-POSITIVE,
  split by `examples/**` · stdlib `.ri` · Rust fixture strings, and by
  *breaks* vs *deliberate negative test to invert*. **Delivers the seven items of §6.5.**
  **Gates 1, 3 and 4 are already measured** (§6.2, §6.3) — α **re-confirms** those counts
  against HEAD rather than re-deriving them, and inherits their two methodological
  requirements or it will get the wrong answer:
  - reproduce §6.2's **type-alias extension** to `NAMED_DIMENSIONS` — dimensioned aliases
    (`Stress`, `HeatFlux`, `ThermalResistance`, …) are declared in `.ri` and absent from the
    registry, so a registry-only sweep **under-counts**. The requirement is a **derivation,
    not a number**: sweep `pub type <X> = <dimensioned expr>` across
    `crates/reify-compiler/stdlib/*.ri` and subtract the names already registered in
    `NAMED_DIMENSIONS`; α re-confirms the count by re-running that sweep against HEAD, not by
    copying a figure written down here. *(As-measured figures — re-derive, do not match:
    §6.2 recorded **16**;
    **15** as of 2026-08-03, after η — task 5785, merge `61300d09` — deleted the `Torque`
    alias from `stdlib/ports_mechanical.ri` and registered `Torque` as a `NAMED_DIMENSIONS`
    row. Both rot on the next stdlib alias add/remove, exactly as 16 did. Do **not** re-add a
    `Torque` alias.)*;
  - reproduce §6.3's **file-local shadowing check** — a naive sweep **over-counts by ~8**
    (parser-only fixtures where the walker never runs, plus locally-declared *dimensionless*
    structures of the same name).

  Reuse `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`'s tree walk and
  parse-only exclusion list rather than writing a new sweep.
- **Unlocks:** β (which sites to migrate), γ (which fences are needed), δ₁/δ₂ (their own
  blast radii), the §6.4 quantity-slot follow-up, and every count any later leaf asserts.
- **Signal (intermediate):** a committed, per-site classified ledger + the captured
  `examples_smoke` failure transcript from the flipped-predicate run. **Prereqs:** ζ₀.

### β — Migrate the corpus to dimensioned construction *(behaviour-preserving)*

- **Modules:** the **5 files / 16 lines / 17 arg-sites** enumerated in §6.3 —
  `examples/trajectory/{tots_optimal_ptp,printer_print_envelope}.ri`,
  `gui/test/fixtures/large_assembly.ri`,
  `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs`,
  `crates/reify-eval/tests/input_shape_eval_e2e.rs` — plus anything α adds at HEAD.
  **`crates/reify-compiler/stdlib/*.ri` needs no edit: measured clean.**
- **Work:** dimension every BARE site in §6.3's table, finishing task 4580's tightening.
  **Fix form per D4-9**: compound literal in `examples/**` and the GUI fixture; the
  compositional `<scalar> * <unit-literal>` form only where a registry-less scope demands it.
  **Measured: no β target is in a registry-less scope** — `examples/**` and the GUI fixture
  compile with the stdlib prelude, and both Rust fixtures go through prelude-loading helpers
  (`compile_source_with_stdlib`, `parse_and_compile_with_stdlib`), so the compound form is
  used in all five files and the compositional fallback nowhere.
- **SI-value rule — split by Leo's esc-5758-2 ruling.** The flat *"SI values must not
  change"* holds for **11 of the 15** migrated sites and is asserted **numerically** at the
  Value layer for each (not inferred from compile-cleanliness — see the note below). It is
  **superseded for the 4 ruled velocity/acceleration sites**
  (`tots_optimal_ptp.ri:78/:79`, `input_shape_eval_e2e.rs:255/:256`): a bare literal at a
  `Scalar<Velocity>` slot was being read as SI, i.e. 300 m/s on a desktop-printer-class
  mechanism. The mm-scale values are what the design physically means, so the resulting
  **1000× SI change is deliberate**. Its user-visible consequence — `tots_optimal_ptp`'s
  optimal second waypoint re-timing from t = 0.07745966692423394 s to t = 0.98 s — was
  previously unpinned by any test and is now pinned in
  `crates/reify-eval/tests/harness_engine/dimensioned_ctor_migration_si_values.rs`.
- **Evidence discipline.** "It still compiles" is not evidence for either half of that rule.
  Every pin matches `Value::Scalar { si_value, dimension }` **explicitly** and compares
  `dimension` against a named `DimensionVector` constant. An f64 coercion helper of the
  `printer_print_envelope_e2e.rs::num()` kind folds `Real` and `Scalar` together and would
  make every such assertion vacuous.
- Three corrections to 5627's `files` list, measured (§2.4):
  - `printer_print_envelope.ri` — the bare args are at **`:153-155`**, not `:146-147` (those
    are the *comment* that calls them *"(mm/s placeholder Real)"*).
  - `tots_optimal_ptp.ri` — a **third** site at `:67` (`max_force: 1000.0`) that prior
    research missed, alongside `:78`/`:79`.
  - `examples/bearing_auto_seal.ri` — **has no ctor site at all.** Its only issue is the
    `param durometer : Length = 70.0` default at `:46`, which is **δ₁'s** gate, and which δ₁
    fixes by correcting the *annotation* (durometer is dimensionless). **β must not look for
    a ctor site there.**
  - `gui/test/fixtures/large_assembly.ri` — copy the spelling from its correct twin
    `examples/large_assembly.ri:51-53`, rather than inventing one.
- **Signal:** `reify eval examples/trajectory/tots_optimal_ptp.ri` reports the trajectory
  limits as dimensioned quantities where it previously reported bare `Real`s; the **11
  magnitude-preserving sites** are numerically unchanged, the **4 ruled sites** sit on their
  mm-scale values, and `printer_print_envelope.ri:154/:155` are still bare with that file's
  e2e gate green and its four outputs (budget / peak_impulse / peak_tots / peak_unshaped)
  bit-identical. The corpus still passes **both**
  `all_examples_parse_and_compile_with_stdlib` **and**
  `no_example_emits_ctor_field_conformance_diagnostics`.

  Measured bonus, not predicted by this PRD: dimensioning `Material.density` also **corrects
  a latent wrong-dimension on derived quantities**. `gui/test/fixtures/large_assembly.ri`'s
  `mass = volume × density` cells rendered as `m^3` while density was a bare Real and render
  as `kg` now, with every magnitude bit-identical (`total_mass` 6.299433614916351 in both).
  When migrating a site, diff the **whole** eval output, not just the migrated field.
- **β migrated 15 of the 17 arg-sites**; `printer_print_envelope.ri:154/:155` remain bare
  pending #5847 (see §6.3).
- **δ-position:** pre-δ; asserts **no** severity — it is a migration, and at this point the
  gate does not yet exist. **Prereqs:** α.

### γ — Promote the dimensioned-`Scalar` family under strict equality *(the ruling leaf; binds 5627)*

- **Modules:** `crates/reify-compiler/src/conformance/mod.rs`,
  `crates/reify-compiler/tests/struct_ctor_field_conformance_tests.rs`,
  `crates/reify-compiler/tests/examples_smoke.rs`.
- **Work:** the §4.1 predicate arm; the D4-5 `ScalarParam` fence via
  `is_numeric_placeholder_leaf`; the D4-6 migration hint; rewrite the `examples_smoke` panic
  message (§4.3 trap); add the I1–I8 value floors (§7.1).
- **Amended by β (task 5758) — the probe retarget is already done.** β migrated the
  `SRC_FAMILY_DIMENSIONED_SCALAR` fixture to unit literals, replaced the 38-line "HELD
  pending ruling" doc comment with a statement of the ruling, and renamed the probe
  `excluded_family_dimensioned_scalar_given_dimensionless_real_is_silent` →
  **`family_dimensioned_scalar_given_unit_literal_arg_is_silent`** (the const name is
  unchanged, and the former test name is quoted verbatim in the new doc comment so this
  anchor still greps). It now pins that β's **fix form is accepted**. So γ's job there is to
  **ADD a bare-arg negative probe as one of its I1–I8 value floors**, not to invert an
  existing probe in place. α's ledger
  (`docs/notes/dimensioned-construction-blast-radius-2026-07-29.md:2069`) still records this
  site as γ's to invert; that is a committed historical measurement and was deliberately not
  rewritten — this bullet is the current authority (divergence filed as esc-5758-5, info).
- **Transitive-risk flag (esc-5758-4).** If γ's gate rejects bare scalars at dimensioned ctor
  fields, then `printer_print_envelope.ri:154/:155` — which β left bare on purpose — would
  make the corpus warn, putting γ behind #5412 → #5847. **Verify this against γ's actual gate
  predicate rather than assuming it**: the two sites are in `examples/**`, so whether they
  matter depends on whether γ's `examples_smoke` assertion is diagnostic-count-based.
- **Signal:** `reify check` on a fixture containing `Steel(youngs_modulus: 200mm)` and
  `Steel(density: "heavy")` **prints two diagnostics with code `ArgTypeMismatch`**, each
  naming its param and carrying a migration hint, where the identical file today prints
  **nothing** (§6.1 probe is the before-image). Exit code **0** pre-δ (Warning severity) —
  the signal is diagnostic emission, not exit status.
- **δ-position:** **pre-δ, Warning.** Severity-safe by §3.1 — adds Warnings only; cannot
  break a previously-compiling design. Post-δ the same signal becomes Error + exit 1 with no
  further work here. γ **does not** touch `CTOR_FIELD_CONFORMANCE_SEVERITY`.
- **Prereqs:** β (hard edge — the corpus gate must be green when γ lands).

### δ₁ — Remove the literal `param`/`let` default tolerance; reinstate D5 in code *(§2.1 gates 3+4; task 5646's "gate 1")*

- **Modules:** `crates/reify-compiler/src/entity.rs`,
  `crates/reify-compiler/tests/param_default_type_mismatch_tests.rs`,
  `crates/reify-compiler/tests/harness_langcore/let_annotation_type_mismatch_tests.rs`,
  `crates/reify-compiler/src/conformance/mod.rs` (D4-4 de-dup), plus the 27 measured sites.
- **Work:** delete the literal early-returns at `entity.rs:479-485` (params) and `:563-569`
  (lets) — `is_numeric_literal_expr` (`:390-404`) becomes dead and goes with them, unless α
  finds another consumer (Open question 2). Apply D4-4: the conformance param-default entry
  defers for scalar-comparable declared types; **first** run the probe that establishes the
  current diagnostic count. **Invert all four pinning tests** listed in §0.3 — the two
  `param` ones (`:172`, `:203`) **and the two `let` ones**
  (`let_annotation_type_mismatch_tests.rs:173`, `:578`) — each gaining a doc comment naming
  task **4318** as the origin of the reversal and pointing at §0.3. Update the module doc at
  `param_default_type_mismatch_tests.rs:1-14` and the 23 prose blocks that state the
  tolerance as intent. Retire the stale rationale at `eval_param_overrides.rs:1608-1630`
  (`= 0` → `= 0USD`). Migrate the **27** measured break sites (§6.2); reuse
  `corpus_no_bare_scalar.rs`'s tree walk and parse-only exclusion list rather than
  re-deriving one.
- **Scope note (measured, §6.2):** 27 hard breaks + 10 assertion inversions. 11 of the
  no-break sites are `reify-syntax` parse-only fixtures that need **no change at all**.
  `examples/bearing_auto_seal.ri:46` is fixed by correcting the *annotation*
  (`durometer : Length` → dimensionless), not by adding a unit — Shore durometer is
  dimensionless and the annotation is a latent bug.
- **Signal:** `reify check` on `param t : Length = 1.0` exits **1** and emits **exactly one**
  diagnostic, code `ParamDefaultTypeMismatch`, `Severity::Error`; the `let` twin emits
  exactly one `LetAnnotationTypeMismatch`; `param t : Length = 5mm` and `param r : Real = 1.0`
  emit **zero**. (Boundary rows B6/B7/B8.)
- **δ-position:** **pre-δ, Error** — `entity.rs` uses `Diagnostic::error`, so `main.rs:546-552`
  fails the command immediately, independent of the δ knob.
- **Prereqs:** γ (hard edge, D4-3 — landing δ₁ first creates a *worse* inconsistency).

### δ₂ — Remove constraint-def numeric leniency *(§2.1 gate 5; task 5646's "gate 2")*

- **Modules:** `crates/reify-compiler/src/type_compat.rs`,
  `crates/reify-compiler/tests/constraint_def_compile_tests.rs`.
- **Work:** `constraint_arg_type_conforms` Rule 4 (`type_compat.rs:1482-1486`) currently
  accepts any numeric-vs-numeric pairing. Remove the **cross-dimension** half. Rule 4's
  `Int`-for-`Length` half is a separate tolerance whose removal α sizes separately (§6.5
  item 2); **scope δ₂ to what α measures as affordable and state explicitly which half is
  deferred**, with a named follow-up if one is. Update the unit tests that pin the current
  behaviour (`constraint_arg_type_conforms_mass_for_length_is_true`, `type_compat.rs:5041`;
  `constraint_arg_type_conforms_dimensionless_for_length_is_true`, `:5050`) **and the
  integration pin of the deliberate `Int`-leniency rule at
  `constraint_def_compile_tests.rs:1447-1470`** — that is where the leniency is documented as
  intentional, so δ₂ must **restate the ruling there**, not merely delete the assertion
  (§0.3's lesson).
- **Signal:** `reify check` on a `.ri` invoking a constraint def with a cross-dimension
  scalar arg exits **1** with code `ConstraintArgTypeMismatch`; the same-dimension call
  stays clean.
- **δ-position:** pre-δ, Error. **Prereqs:** δ₁.

### ε — Constraint backstop: pin the recovered verdict; file the residual defect by name

**The investigation this leaf was going to perform has been done at authoring time** (§10.4).
The verdict is settled, so ε is now a *pin-and-file* leaf, not an open investigation.

- **Modules:** a constraint-outcome pin under `crates/reify-eval/tests/`; the follow-up is
  **filed, not fixed, here**.
- **Work, part 1 — pin the recovery.** With a strictly-typed field, `constraint magnitude > 0N`
  takes the same-dimension arm of `eval_cmp` (`crates/reify-expr/src/lib.rs:5073-5088`) and
  returns a definite `Bool` → `Satisfaction::Satisfied` / `Violated`
  (`crates/reify-constraints/src/lib.rs:171-183`). Pin it against the real stdlib structure:
  `StepForce` (`crates/reify-compiler/stdlib/modal_analysis.ri`: `param magnitude : Force`
  at `:486`, `constraint magnitude > 0N` at `:500`), whose live well-typed call site
  is `examples/modal/transient_step_response.ri:102-107`. Assert a **definite** verdict for a
  well-typed instance and a **`Violated`** verdict for a negative one — the pair, so a
  future regression to Indeterminate cannot pass as "still Satisfied".
- **Work, part 2 — file the residual defect (do NOT fix it here).** §10.4 identifies one
  genuinely independent defect that survives strict construction, plus one asymmetry. Both
  get a filed, cited follow-up task; ε's test doc comment records the id. **Silence is not
  an option** (INV-SF-4).
- **Signal:** `reify check examples/modal/transient_step_response.ri`-shaped fixture reports
  its `StepForce` magnitude constraint as **satisfied-definitely**, and the
  negative-magnitude sibling as **violated** with exit 1 — where a bare-number instance
  previously produced `"No constraints violated (1 indeterminate)"` and **exit 0**. Plus: a
  live task id for the residual defect, named in the test.
- **δ-position:** pre-δ; asserts a **constraint outcome**, not a diagnostic severity.
- **Prereqs:** γ.

### ζ₁ — Doc-chunk update, registry-verified *(docs-truth 1 of 4)*

- **Modules:** `crates/reify-mcp/src/tools/chunks/{units,parameters,structures}.md`.
- **Work:** state the **construction-boundary rule** explicitly. Today `parameters.md` and
  `structures.md` *use* the correct spelling (`param width : Length = 50mm`) but nowhere
  state that the bare spelling is an error — an author can only learn the rule by hitting
  the diagnostic. `units.md` gets the rule as a first-class section. Every documented
  signature verified against the compiler arms/registries.
- **Signal:** each documented form compiles as written in a smoke `.ri`; an author reading
  `units.md` learns the rule without running the compiler.
- **δ-position:** doc-only. **Prereqs:** γ.

### ζ₂ — Best-practices exemplar *(docs-truth 2 of 4)*

- **Modules:** `examples/best_practices/dimensioned_construction.ri` (new) +
  `examples/best_practices/INDEX.md` row.
- **Work:** one worked exemplar showing the idiom and the anti-pattern it replaces —
  including the D4-9 two-spelling rule (compound literal vs the registry-less compositional
  form), which is exactly the sort of thing design sessions currently burn probe cycles on.
  Normative claims in code/constraints, not comments.
- **Signal:** the file is compile-gated by `examples_smoke.rs` (auto, by directory walk) and
  its INDEX row satisfies `best_practices_index_matches_corpus_directory`.
- **δ-position:** doc/corpus-only. **Prereqs:** ζ₁.
- *Drift-guard: adds no new gate-resident test file — `examples_smoke.rs` discovers it by
  directory walk, so no `run-all-classification.manifest` or nextest registration is due.*

### ζ₃ — `reify-design` cheatsheet index line *(docs-truth 3 of 4)*

- **Modules:** `.claude/skills/reify-design/SKILL.md`.
- **Work:** a one-line index entry pointing at ζ₂'s corpus file — an index line, **not** an
  inline playbook.
- **Signal:** a design session grepping the cheatsheet index for "units"/"dimensioned" finds
  the exemplar. **Prereqs:** ζ₂.

### ζ₄ — Discoverability acceptance *(docs-truth 4 of 4; closes the loop)*

- **Work:** verify intent-level findability: an author who knows the **goal** ("give this
  material a stiffness", "set a speed limit") but not the feature name reaches the rule and
  the exemplar from the chunks or the corpus index. Fix the topic chunk / index wording that
  fails the test.
- **Signal:** for each of three goal-phrased queries, the right chunk/index line names the
  capability **in intent terms**. **Prereqs:** ζ₃.

### Registration / drift-guard note

No leaf adds a new `tests/infra/test_*.sh`, a new gate-resident `crates/*/tests/*.rs`
**binary**, or any wall-clock assertion. γ, δ₁, δ₂ and ε add tests to **existing** test
binaries (`struct_ctor_field_conformance_tests.rs`, `param_default_type_mismatch_tests.rs`,
`type_compat.rs`'s `mod tests`, an existing `reify-eval` test target) — so no
`run-all-classification.manifest` row, no `.config/nextest.toml` partition entry, and no
`test_no_new_wallclock_upper_bounds.sh` registration is due. **If decompose finds a leaf
needs a NEW test binary, its registration must land same-diff or as a hard upstream
`add_dependency` edge** (the esc-4914-162 failure).

---

## §12 Out of scope

- **The δ flip itself** (`CTOR_FIELD_CONFORMANCE_SEVERITY` Warning→Error). Separately owned,
  not blocked on this PRD, and not performed by it.
- **Eval-layer readers and solver extraction** — `arg_acceptance` adoption at the ~13 reader
  chokepoints, `extract_loads` / `extract_density` / `extract_total_load`, flexures/joints/
  dynamics/modal/fea `Value::as_f64` sites. **PRD 5.**
- **Load-struct `.ri` retyping** (`PointLoad`/`PressureLoad`/`TractionLoad`/`BodyForce`,
  `analysis.ri` yield fields, `trajectory.ri` `JointValue`). **PRD 5** — §10.2.
- **Geometry eval chokepoints, compile `builtin_signatures` slots, compiler-desugaring
  literal dimensioning, the kernel tripwire, the closure-guard harness, the GUI param
  editor.** **PRD 1.**
- **ANGLE convention convergence, `resolve_bare_angle` retirement, the `Nm` torque symbol,
  Energy↔Torque diagnostics, middle-dot/curated-label round-tripping, and the stale
  "35 standard named dimensions" count in `units.md`.** **PRD 3.**
- **`reify check` exit-code policy, `--strict` semantics, build-diagnostic collection.**
  **PRD 2** — including acting on the research-doc correction in §3.2.
- **Dimension-checking the quantity slot of `Vector3<Q>` / `Point3<Q>` / `Matrix` / `Tensor`
  / `Field` params** (§6.4). Those families have their own shape-based arms and stay
  dimension-blind after this PRD. This is 5627's candidate-2 territory (the
  `reify-core/src/ty.rs` quantity-slot convention) and needs its own ruling. **α records the
  count; the decomposer files a follow-up task — it is not a leaf of this PRD.**
- **Typed-IR newtypes for dimensioned fields** (research doc Q3 option B). Recorded endgame,
  not this PRD.
- **Gates 6–10** (§2.1) — already strict; untouched.

---

## §13 Open questions (tactical — deferred to implementation)

1. **How much of constraint-def Rule 4 comes out.** Rule 4 conflates two tolerances
   (cross-dimension scalars, and `Int`-for-`Length`). **Suggested resolution:** remove the
   cross-dimension half; keep `Int`-widening unless α shows it is cheap to remove too, and
   name a follow-up for whatever is deferred. Decide during **α → δ₂**.
2. **Whether `is_numeric_literal_expr` (`entity.rs:390-404`) is fully dead after δ₁.**
   **Suggested resolution:** delete if α finds no other consumer; otherwise leave with a
   comment naming the survivor. Decide during **δ₁**.
3. **Exact migration-hint wording per dimension.** D4-6 fixes the *shape*; the per-dimension
   example literal (`200GPa`, `7850kg/m^3`, `300mm/s`) is a table an implementer fills.
   **Suggested resolution:** derive from the `NAMED_DIMENSIONS` registry rather than
   hand-listing, so it cannot rot. Decide during **γ**.
4. **Scope of ε's filed follow-up.** §10.3 identifies two things to file: the operand-kind
   `Undef`-at-Warning policy defect, and the `==` vs `>` asymmetry in `eval_cmp`/`eval_eq`.
   **Suggested resolution:** one follow-up task covering both — they share a file and a root
   framing ("the constraint layer's answer to a malformed comparison"). Split only if α's
   evidence shows independent blast radii. Decide during **ε**.
5. **Whether any fn-call-entry `Scalar` leaf is reachable** (§3.1). Expected zero.
   **Suggested resolution:** if α finds any, migrate them in β; they are Error-severity
   pre-δ, so they cannot be left. Decide during **α**.
6. **Whether γ authors its own migration-hint wording or reuses `ArgRejection::message`.**
   Depends on whether PRD 5's relocation of `arg_acceptance` into `reify-ir` has landed when
   γ dispatches (D4-6). **Suggested resolution:** reuse the relocated type if it is
   available, author the wording shape locally if it is not; **never block γ on the
   relocation, and never add a reify-compiler → reify-eval dependency.** The user-visible
   wording — and therefore every leaf signal — is identical either way. Decide during **γ**.
