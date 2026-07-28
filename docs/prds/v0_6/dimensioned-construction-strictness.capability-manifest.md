# Capability manifest — `dimensioned-construction-strictness` (units-gating PRD 4)

PRD: `docs/prds/v0_6/dimensioned-construction-strictness.md` (landed `638d97d8ab`).
Decomposed 2026-07-28. Machine-readable twin: `dimensioned-construction-strictness.capability-manifest.yaml`.

**Anchor drift check — NONE.** The PRD re-verified every anchor at `dc83d4fd60`; `git diff
dc83d4fd60..HEAD` over `crates/**` is **empty** (the only commits since are the four units-program
PRD `.md` files + one `.ri` grammar fixture). Every §2.4 anchor therefore holds verbatim at
decompose HEAD `638d97d8ab`, re-confirmed by direct read (predicate `conformance/mod.rs:1691-1697`;
severity const `:32` = `Warning`; `entity.rs:479-485` / `:563-569` tolerances; `type_compat.rs`
Rule 4 `:1482-1486`; corpus gate `examples_smoke.rs:198` + `≥40` floor + the stale panic prose;
codes `312/419/456/617`; `main.rs:283-285` print / `:546-552` exit gate).

**D3 verification.** The `Workflow` tool is not available in this session, so the D3 roles were run
directly: Enumerator + Prover in-session against the deterministic harness
(`scripts/prd-capability-check.py`, 19 probes, **19 PASS / 0 FAIL / 0 UNPROVABLE**), Adversary as an
independent agent. Probe set + fixtures + captured output: see §D3 at the end of this file.
Two premises were **falsified and repaired at decompose time** (ε's fixture configuration, δ₂'s
before-image) — recorded in the affected leaves' bindings below.

Evidence vocabulary: `grep:<file>:<line>` (wired on main) · `probe:<id>` (executed α probe, see §D3)
· `producer:task-N` (upstream in the dependency closure) · `grammar-fixture:<path>` ·
`rejection-check` (negative-assertion mandate — the rejection was **observed to fire**).

---

## ζ₀ — Record the ruling and the reversal history *(doc-only)*

| Capability | Evidence | Verdict |
|---|---|---|
| `real-dimensionless-unification.md` D5 text exists at the cited anchor | `grep:docs/prds/v0_6/real-dimensionless-unification.md:51` — *"Struct-param default type-check = hard error… Reuse `FnParamDefaultTypeMismatch`"*; supporting `:17`, `:82` (the ε-fold), `:134` | PASS |
| the reversal is genuinely unrecorded (the thing ζ₀ fixes) | `grep` over `real-dimensionless-unification.md`: task **4318 is mentioned exactly once**, at `:82`, and *only* as the leaf-ε fold target — nothing records that 4318 then shipped the **opposite** rule, nothing records a D5 status change (`grep -iE "reinstat\|superseded\|reversal"` → **zero hits**), and `dimensioned-construction-strictness` is absent. ζ₀'s `delivered_check` is therefore the PRD-4 backlink, not the (already-present) `4318` string. | PASS |
| target file is tracked and hook-gated-committable | `git ls-files` hit; docs-only path → `pre-commit` no-heavy-checks class | PASS |

## α — Blast-radius measurement + migration ledger *(intermediate)*

| Capability | Evidence | Verdict |
|---|---|---|
| the corpus gate α drives exists, is severity-blind and accumulates every hit | `grep:crates/reify-compiler/tests/examples_smoke.rs:198` (`no_example_emits_ctor_field_conformance_diagnostics`), `≥40`-file floor at `:215-220`, one-panic accumulation at `:230-236` | PASS |
| the predicate α flips locally is a single arm | `grep:crates/reify-compiler/src/conformance/mod.rs:1691-1697` — read verbatim at HEAD, matches PRD §4.1 exactly | PASS |
| the sweep α must reuse (tree walk + parse-only exclusion list) is on main | `grep:crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` exists; already walks `examples/**`, `crates/**/*.ri`, `crates/**/*.rs` inline fixtures, `gui/**` | PASS |
| the alias-extension requirement is real (registry-only sweep under-counts) | `grep:crates/reify-core/src/dimension.rs:514` `NAMED_DIMENSIONS`; PRD §6.2 lists 16 `.ri`-declared dimensioned aliases absent from it (`Torque` `stdlib/ports_mechanical.ri:29`, `Stress` `stdlib/analysis.ri:13`, …) | PASS |
| **α is an intermediate, not a leaf** — its downstream consumers are named | β (migration set), γ (fence sizing), δ₁/δ₂ (their own blast radii), FU1 (§6.4 count) — all real `add_dependency` edges in this batch | PASS |

## β — Migrate the corpus to dimensioned construction

| Capability | Evidence | Verdict |
|---|---|---|
| the 17 BARE arg-sites exist at the cited anchors | direct read at HEAD: `tots_optimal_ptp.ri:67/78/79`, `printer_print_envelope.ri:153/154/155`, `struct_ctor_field_conformance_tests.rs:1420`, `input_shape_eval_e2e.rs:248/255/256`, `gui/test/fixtures/large_assembly.ri:18/19/27/28/36/37` | PASS |
| **the fix form compiles** — compound-unit literals at dimensioned ctor fields in ordinary scope | `probe:beta-fix-form` — `Limit(velocity_limit: 300mm/s, acceleration_limit: 5000mm/s^2)` → `reify check` exit **0**, zero diagnostics | PASS |
| the registry-less alternative spelling exists and is documented | `grep:crates/reify-compiler/stdlib/units.ri:141-148`; registry guard `grep:crates/reify-compiler/src/expr.rs:1455-1462` | PASS |
| the GUI fixture has a correct twin to copy from (β invents no spelling) | `grep:examples/large_assembly.ri:51-53` — `density: 7850kg/m^3`, `youngs_modulus: 200GPa`; degraded copy at `gui/test/fixtures/large_assembly.ri:18-19` reads `7850.0` / `200000000000.0` | PASS |
| `bearing_auto_seal.ri` has **no** ctor site (β must not hunt one) | direct read: `examples/bearing_auto_seal.ri:46` is `param durometer : Length = 70.0` — a **gate-3** default, δ₁'s | PASS |
| **stdlib needs no edit** | PRD §6.3 measured `stdlib` BARE count = 0; α re-confirms at HEAD | PASS |

## γ — Promote the dimensioned-`Scalar` family *(the ruling leaf; binds task 5627)*

| Capability | Evidence | Verdict |
|---|---|---|
| **rejection mechanism exists and FIRES** (negative-assertion mandate) | `rejection-check` / `probe:gamma-rejection-mechanism` — at the **same** walker entry, a vetted-family mismatch (`Bool ← String`) emits `warning: argument 'flag' has type 'String' but param 'flag' requires type 'Bool'` on `reify check`, exit 0. γ widens the vetted set; it builds no new emission path. | PASS |
| **before-image is a genuine silent-accept** | `probe:gamma-before-image` — `Steel(youngs_modulus: 200mm, density: "heavy")` → `All constraints satisfied.`, exit **0**, **zero** diagnostics | PASS |
| `type_compatible` already supplies strict equality (no new algorithm) | `grep:crates/reify-compiler/src/type_compat.rs:52-180` (`implicitly_converts_to`, no `Scalar`-vs-`Scalar` arm past identity), `:220-237` (relaxation gated on the **param** side being dimensionless) | PASS |
| emission machinery downstream of the predicate is unchanged and live | `grep:crates/reify-compiler/src/conformance/mod.rs:1084-1099` (`emit_arg_type_mismatch`), `:1194` (`reject_if_incompatible`); code `grep:crates/reify-core/src/diagnostics.rs:617` | PASS |
| **I1 / §6.6 arg-side inference is precise** — the single highest-risk invariant | `probe:gamma-arg-inference` — `span / 2.0`, `100mm * 1mm` at an `Area` slot, `5.0 * STANDARD_GRAVITY()` at `Acceleration`, and a dimensioned param ref all pass **gate 6**, which already applies strict `param_ty == arg_ty` (`type_compat.rs:1201/1221/1289`). exit 0, zero diagnostics. The inference γ relies on is therefore live and exact today. | PASS |
| I8 non-regression: dimensionless `Scalar ← Int` stays silent | `probe:gamma-i8` — `Ratio(k: 3)` at `param k : Real` → exit 0, zero diagnostics; pinned today by `struct_ctor_field_conformance_tests.rs:647` | PASS |
| the `ScalarParam` fence pattern D4-5 prescribes exists | `grep:crates/reify-compiler/src/conformance/mod.rs` `is_numeric_placeholder_leaf` (`Type::Int \| Type::Scalar{..} \| Type::ScalarParam(_)`), already used by the shipped `Point`/`Matrix` arms | PASS |
| the corpus gate's panic prose encodes the **old** ruling (the §4.3 trap is real) | `grep:crates/reify-compiler/tests/examples_smoke.rs:230-236` — read verbatim at HEAD: *"a false positive from the conformance walker, not a broken example… do NOT add a SKIP_SET entry"* | PASS |
| γ is severity-safe pre-δ | `grep:crates/reify-compiler/src/conformance/mod.rs:32` = `Severity::Warning`; `probe:gamma-rejection-mechanism` shows the ctor entry prints and exits **0** | PASS (see Adversary note) |
| `reify check` visibility without a PRD-2 edge | `probe:gamma-rejection-mechanism` (Warning printed on `check`) + `grep:crates/reify-cli/src/main.rs:283-285` / `:546-552`; PRD §3.2 traced, PRD 2's seam row reciprocally confirms *"no seam today"* | PASS |

## δ₁ — Remove the literal `param`/`let` default tolerance *(binds task 5646; §2.1 gates 3+4)*

| Capability | Evidence | Verdict |
|---|---|---|
| **rejection mechanism exists and FIRES (params)** | `rejection-check` / `probe:delta1-mechanism-param` — `param x : Length = ratio * 2.0` → `error: parameter 'x' declared 'Scalar[m]' but its initializer evaluates to 'Real'; declared type and initializer dimension must agree`, exit **1** | PASS |
| **rejection mechanism exists and FIRES (lets)** | `rejection-check` / `probe:delta1-mechanism-let` — `let t : Length = ratio * 2.0` → `error: let binding 't' declared … initializer type must agree`, exit **1** | PASS |
| before-image is a genuine silent-accept, both gates | `probe:delta1-before-param` (`param t : Length = 1.0` → exit 0, silent) and `probe:delta1-before-let` (`let t : Length = 1.0` → exit 0, silent) | PASS |
| no over-rejection (B8 floor) | `probe:delta1-no-over-rejection` — `param t : Length = 5mm` **and** `param r : Real = 1.0` in one file → exit 0, zero diagnostics | PASS |
| the tolerance early-returns δ₁ deletes are exactly where the PRD says | `grep:crates/reify-compiler/src/entity.rs:479-485` (params) and `:563-569` (lets), helper `is_numeric_literal_expr` `:390-404` | PASS |
| the four deliberate pins to invert exist | `grep:param_default_type_mismatch_tests.rs:172`, `:203`; `grep:harness_langcore/let_annotation_type_mismatch_tests.rs:173`, `:578` | PASS |
| codes asserted by δ₁'s signal are extant (D4-8) | `grep:crates/reify-core/src/diagnostics.rs:419` `ParamDefaultTypeMismatch`, `:456` `LetAnnotationTypeMismatch` | PASS |
| the one `.ri` break site is real and is an annotation bug, not a literal bug | `grep:examples/bearing_auto_seal.ri:46` `param durometer : Length = 70.0`; also breaks `crates/reify-eval/tests/auto_type_param_determinism_tests.rs:391` | PASS |

## δ₂ — Remove constraint-def numeric leniency *(§2.1 gate 5)*

| Capability | Evidence | Verdict |
|---|---|---|
| **rejection mechanism exists and FIRES** | `rejection-check` / `probe:delta2-mechanism` — a cross-**category** arg (`Bool` at a `Length` constraint param) → `error: type mismatch: argument 'w' for constraint 'MinWall' has type Bool but parameter expects Scalar[m]`, exit **1** (Rule 5). δ₂ moves the cross-**dimension** case from Rule 4 into that same path. | PASS |
| Rule 4 conflates the two tolerances the PRD says it does | `grep:crates/reify-compiler/src/type_compat.rs:1482-1486` — `is_numeric = matches!(t, Type::Int \| Type::Scalar{..} \| Type::ScalarParam(_))`, accepted symmetrically. Note `ScalarParam` is in the closure: removing the cross-dimension half must not break dimension-generic constraint defs. | PASS |
| the deliberate `Int`-leniency pin to restate (not merely delete) exists | `grep:crates/reify-compiler/tests/constraint_def_compile_tests.rs:1447-1470` `int_literal_for_length_param_no_constraint_arg_type_mismatch`, whose doc comment states the leniency as **intent** | PASS |
| code asserted by δ₂'s signal is extant | `grep:crates/reify-core/src/diagnostics.rs:312` `ConstraintArgTypeMismatch` | PASS |
| **PREMISE REPAIRED — the PRD's before-image is only silent in the UNMASKED shape** | `probe:delta2-before-unmasked` — `constraint def Positive { param w : Length; w > 0 }` invoked as `Positive(w: m)` with `m : Mass` is **completely silent, exit 0** (`OK Positive#0[0]`). But `probe:delta2-masking` — the same cross-dimension arg with a **dimensioned** literal in the predicate (`w > 0mm`) **already exits 1 today** under a *different* code (`error: dimension mismatch in comparison: Scalar[kg] vs Scalar[m]`, task 4490's comparison guard, a different pass). **Consequence, carried into δ₂'s task text:** a naive exit-code RED test written against the masked shape is GREEN before the change. δ₂'s signal must (a) assert `DiagnosticCode::ConstraintArgTypeMismatch` **identity**, not exit code alone, and (b) use the unmasked fixture shape as its RED. | PASS (repaired) |

## ε — Constraint backstop: pin the recovered verdict; file the residuals

> **PREMISE FALSIFIED AND REPAIRED AT DECOMPOSE TIME.** The PRD's §11 ε names
> `StepForce` on `examples/modal/transient_step_response.ri:102-107` as the pin fixture and
> asserts a definite verdict for the well-typed instance. **Both halves are false today, and γ
> cannot make them true.** Evidence below. ε's configuration is changed (G6 resolution (c)) and its
> prereq moves γ → **δ₁** (the leaf that actually produces the observable).

| Capability | Evidence | Verdict |
|---|---|---|
| **FALSIFICATION 1** — the PRD's chosen fixture is Indeterminate today **even when units-correct** | `probe:epsilon-falsification` — `let p = Pusher(magnitude: 10N)` on `structure def Pusher { param magnitude : Force; constraint magnitude > 0N }` → `INDETERMINATE Pusher#constraint[0]`, `warning: … undefined inputs: Pusher.magnitude`, exit 0. On the real example, the `StepForce` constraint is not even listed: `reify check examples/modal/transient_step_response.ri` → `All constraints satisfied.`, exit 0, and negating the magnitude to `-10N` changes **nothing**. | FALSIFIED → repaired |
| **FALSIFICATION 2** — the cause is dimension-**independent**, so it is not this PRD's defect | `probe:epsilon-dimension-independence` — the identical Indeterminate (`undefined inputs: Pusher.eff`) fires for a **dimensionless** ctor-supplied param (`param eff : Real`, `constraint eff > 0.0`, `Pusher(eff: -0.5)`). Isolated by `probe:epsilon-nonregression-floor`: the **same** structure valued from its own param **default** reaches a definite `VIOLATED` + exit 1. ⇒ ctor-supplied args are not seeded into the nested instance's constraint inputs. **Filed as FU2** (new task, this batch). | FALSIFIED → filed |
| **the recovery ε CAN pin** — the mechanism exists in the param-default configuration | `probe:epsilon-mechanism` — `param magnitude : Force = -10N` + `constraint magnitude > 0N` → `VIOLATED Root#constraint[0]`, `error: constraint … violated`, exit **1**; the positive twin → `OK`, exit 0 | PASS |
| **the recovery target's before-image** — PRD §10.3's causal chain, observed, at **gate 3** | `probe:epsilon-recovery-target` — `param magnitude : Force = -10.0` (bare literal at a dimensioned default) → `INDETERMINATE`, `warning: … operator undefined for these operand kinds: Real`, exit **0**. This is the `eval_cmp` Real-vs-dimensioned → `Undef` → Indeterminate chain **verbatim**, and the gate that admits the bare literal is `entity.rs:479-485` — **δ₁'s**, not γ's. | PASS |
| §10.3's residual-defect analysis is sound | `grep:crates/reify-constraints/src/lib.rs:183-215` (`Undef → Indeterminate`, `Diagnostic::warning` at `:211`), `classify_undef` `:78-111` computes the distinction and puts it in the message — confirmed verbatim by the two probes above (`operator undefined for these operand kinds` vs `undefined inputs`) | PASS |
| the stdlib documents this exact failure mode (ε's doc pointer) | `grep:crates/reify-compiler/stdlib/modal_analysis.ri:497-499` — *"a bare `0` would yield `Indeterminate` per task #3115 esc-3115-112"* | PASS |

## ζ₁–ζ₄ — Docs-truth quartet

| Leaf | Capability | Evidence | Verdict |
|---|---|---|---|
| ζ₁ | the three chunk files exist and are the right ones | `grep:crates/reify-mcp/src/tools/chunks/units.md`, `parameters.md`, `structures.md` — all present | PASS |
| ζ₁ | the documented form compiles as written (chunk-truth acceptance) | `probe:delta1-no-over-rejection` — `param t : Length = 5mm` (the spelling `parameters.md` already uses) compiles clean | PASS |
| ζ₂ | the corpus dir is compile-gated by directory walk (no new registration due) | `grep:crates/reify-compiler/tests/examples_smoke.rs` walks `examples/` recursively; `INDEX.md` states it verbatim | PASS |
| ζ₂ | the bidirectional INDEX invariant test exists | `grep:crates/reify-compiler/tests/examples_smoke.rs:548` `best_practices_index_matches_corpus_directory` | PASS |
| ζ₃ | the cheatsheet index target exists **and is tracked** | `git ls-files` hit on `.claude/skills/reify-design/SKILL.md`; existing corpus index lines at `:22`, `:61-63`, `:158`, `:172-175` | PASS |
| ζ₄ | discoverability is checkable against a real read path | the MCP chunk tool + `examples/best_practices/INDEX.md`'s own "grep this index before probing" contract | PASS |

## Cross-cutting

| Capability | Evidence | Verdict |
|---|---|---|
| **G3 grammar** — no novel syntax | `grammar-fixture:docs/prds/v0_6/fixtures/dimensioned_construction_strictness.ri` — `tree-sitter parse --quiet`, exit **0**, zero ERROR nodes (`probe:g3-grammar`) | PASS |
| **drift-guard registration** — no leaf adds a new gate-resident test **binary** or `tests/infra/test_*.sh` | γ/δ₁/δ₂/ε add tests to **existing** binaries (`struct_ctor_field_conformance_tests.rs`, `param_default_type_mismatch_tests.rs`, `let_annotation_type_mismatch_tests.rs`, `constraint_def_compile_tests.rs`, `type_compat.rs`'s `mod tests`, an existing `reify-eval` test target); ζ₂ is discovered by directory walk. No `run-all-classification.manifest` row, no `.config/nextest.toml` partition entry, no wall-clock bound. **If an implementer finds a NEW binary is needed, its registration lands same-diff — never a downstream sibling** (esc-4914-162). | PASS |
| **no PRD-2 edge required** | §3.2 verified + `probe:gamma-rejection-mechanism`; PRD 2's own seam row reciprocally reads *"no seam today"* | PASS |
| **no PRD-5-owned work filed** | the four `fea_multi_case.ri` load-struct fields are still `Real` at HEAD (`:315-317`, `:418-419`, `:446-448`, `:476-478`) — out of the dimensioned family, so γ changes nothing about them. α's ledger flags-and-excludes them. Candidate cross-batch edge (PRD 5 retyping **depends on** γ) is **named, not wired** — coordinator's. | PASS |
| **no δ severity-flip leaf filed** | PRD §10.2 / §12 / D4-7: the `CTOR_FIELD_CONFORMANCE_SEVERITY` Warning→Error flip is separately owned and **not performed by this PRD**. Confirmed: no leaf in this batch touches `conformance/mod.rs:32`. | PASS |

---

## §D3 — probe set, fixtures, captured verdicts

Harness: `python3 scripts/prd-capability-check.py --json <probes.json>` → **exit 0, 19/19 PASS**.
Probe-set JSON and all fixtures:
`/tmp/claude-1000/-home-leo-src-reify/b502b1c3-c17a-48bb-b822-8e260cd5ae5c/scratchpad/prover/`
(session-scoped; every probe is reproducible from the one-file fixtures quoted inline in the
bindings above). Binary: `target/release/reify`, mtime `2026-07-28 20:47:36`, newer than every
source in the workspace (`git diff dc83d4fd60..HEAD` touches only `docs/`).

| probe id | kind | expected | observed |
|---|---|---|---|
| `g3-grammar` | grammar | present | exit 0, 0 ERROR nodes |
| `gamma-before-image` | check | **absent** (silent-accept today) | no diagnostic, exit 0 |
| `gamma-rejection-mechanism` | check | present | `requires type 'Bool'`, exit 0 |
| `gamma-i8` | check | absent | no diagnostic, exit 0 |
| `gamma-arg-inference` | check | present | exit 0 |
| `beta-fix-form` | check | present | exit 0 |
| `delta1-before-param` | check | absent | silent, exit 0 |
| `delta1-before-let` | check | absent | silent, exit 0 |
| `delta1-mechanism-param` | check | present | `…initializer dimension must agree`, exit 1 |
| `delta1-mechanism-let` | check | present | `…initializer type must agree`, exit 1 |
| `delta1-no-over-rejection` | check | present | exit 0 |
| `delta2-mechanism` | check | present | `…for constraint 'MinWall' has type Bool`, exit 1 |
| `delta2-before-unmasked` | check | absent | silent, exit 0 |
| `delta2-masking` | check | present | `dimension mismatch in comparison`, exit 1 |
| `epsilon-mechanism` | check | present | `violated`, exit 1 |
| `epsilon-recovery-target` | check | present | `operator undefined for these operand kinds`, exit 0 |
| `epsilon-falsification` | check | present | `undefined inputs: Pusher.magnitude`, exit 0 |
| `epsilon-dimension-independence` | check | present | `undefined inputs: Pusher.eff`, exit 0 |
| `epsilon-nonregression-floor` | check | present | `violated`, exit 1 |

### Adversary role — independent findings (all folded into the affected tasks' `details` as binding addenda)

| # | Finding | Verdict | Disposition |
|---|---|---|---|
| 1 | **§3.1's reachability argument is half wrong.** Guard (i) (`if !type_carries_trait_object(param_ty) { continue; }`, `entities_phase.rs:1493`) confirmed verbatim. Guard (ii) **falsified as a backstop**: `resolve_function_overload` filters at `type_compat.rs:1194-1200` with `type_carries_trait_object(param_ty) \|\| … \|\| param_ty == arg_ty` — the *same* predicate as guard (i) is the **first disjunct**, so the two guards are **mutually exclusive, not conjunctive**. Reachability proved: `fn takes(m : Map<String, MaterialSpec>)` + `takes(map{ 1 => Steel() })` → `error: argument 'm' … requires type 'String'`, **exit 1** — the general-leaf arm at hard Error from the fn-call entry. | FALSIFIED (argument) / **CONCLUSION SURVIVES** | γ *is* severity-safe pre-δ, but **by corpus measurement, not by construction** (independent count of reachable sites: **ZERO**). α's §6.5 item-4 upgraded from formality to load-bearing. → γ (5627) A1, δ₁ (5646) B5, α (5756) C1 |
| 2 | §6.6 arg-side inference re-confirmed independently by a **direct ctor-path readout**: 27 arg shapes, 27 diagnostics, every dimension exact; no widening, no `Real` fallback. | CONFIRMED | strengthens γ's I1 → γ A4 |
| 3 | **Unlisted coupling:** D4-5's `ScalarParam` fence sits on the *shared* arm, so it also flips the already-shipped `Real ← Scalar<Q>` warning (task 5465's dimensionless family) from warn→silent. | UNLISTED PREMISE | γ must declare it intended and pin the post-state → γ A2 |
| 4 | **Unlisted premise, load-bearing for the corpus gate:** `auto` / `auto(free)` / `undef` are **not** conformance-judged at either entry. Without it, γ's gate-2 half fires on the 96 measured auto/undef default sites and turns the gate RED. | UNLISTED PREMISE | γ carries a value floor → γ A3, α C6 |
| 5 | §6.2 (=2) and §6.3 (=17/16/5) **independently reproduced exactly** over all 605 main-tree `.ri` files, zero additional true positives. | CONFIRMED | α's items 6/7 are genuinely re-confirmation → α C4 |
| 6 | **Two sweep traps the PRD names neither of:** (a) `pub unit cm : Length = 0.01` (~20 stdlib lines) is textually identical to the target pattern but must stay a bare conversion factor; (b) `corpus_no_bare_scalar.rs` — the sweep the PRD says to reuse — is **blind to 167 `.ri` files**, several trees compiled by no gate at all. | NEW | α C2/C3, δ₁ B3/B4, β D1 |
| 7 | **ε is falsified more deeply than the Prover found — two mechanisms.** (i) Structure-def constraints evaluate against the **template's own defaults**, once; the ctor arg is *ignored*, not missing (`engine_constraints.rs:868`, `for template in &module.templates`). (ii) **Prelude/stdlib template constraints are never evaluated at all** — that loop never iterates `self.prelude`, so `StepForce.magnitude > 0N` yields no verdict in any command at any magnitude. Also `reify eval` runs **no** constraints. | **BLOCKING → REPAIRED** | ε must use its **own local** structure def, param-default-bound, `reify check`-driven; §7.4 row B9 re-phrased identically. Task 5765 rewritten to carry both facets. → ε (5763) F1–F6, 5765 |
| 8 | δ₂'s masking is wider than the Prover found — 4490's guard fires for **every** numeric arg kind on a comparison predicate. A cleaner silent shape exists: a predicate that does not dimension-compare the param (`is_determined(v)`). Separability of Rule 4 **confirmed** without touching `ScalarParam` (a `(Scalar, Scalar) => false` pre-arm suffices; corpus has **zero** dimension-generic constraint defs). | REFINED | δ₂ (5762) E1–E3 |
| 9 | §3.2 A/B re-confirmed. **Bonus:** D4-4's double-report is now **observed**, not merely implied — `param i2 : Int = "no"` emits an `entity.rs` Error *and* a conformance Warning today. δ₁'s gate-3 scope is also narrower than assumed: `= "x"` and `= 5kg` already hard-error; only `= 1.0` / `= 1` are silent. | CONFIRMED + REFINED | δ₁ B1/B2 |
| 10 | **Stderr trap:** `reify check` prints engine-owned `error:` lines and still exits 0 (`main.rs:471-474`, `@optimized` trampolines), so any signal grepping stderr for `error:` misfires on `examples/modal/*`. | NEW | δ₂ E4, ε F5 — all signals assert on `DiagnosticCode` identity / verdict lines |

**Batch verdict: DOES NOT BLOCK.** Prover: 19/19 PASS. Adversary: one blocking finding (#7), three
premise corrections (#1, #8, #9), four unlisted premises (#3, #4, #6, #10). Every one was repaired
**in-batch** rather than deferred to dispatch:

- **ε** reconfigured (own local structure def, param-default-bound, `reify check`-driven) and
  **reparented γ → δ₁**, which is the leaf that actually produces its observable;
- **δ₂**'s RED fixture shape replaced and its assertion pinned to `DiagnosticCode` identity;
- **γ**'s severity-safety premise restated as measured-not-structural, with two new value floors;
- **α**'s item 4 upgraded to load-bearing and its sweep given an explicit reach + exclusion contract;
- **β** given a manual verification obligation for its un-gated GUI-fixture target;
- two independent defects **filed by name** rather than left silent — task **5765** (constraint
  evaluation ignores ctor args; stdlib constraints never evaluated) and task **5766** (the §6.4
  quantity-slot residual) — per INV-SF-4 and PRD §12's filing instruction.
