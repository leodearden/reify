# Capability manifest — `angle-units-surface-convergence` (units-gating PRD 3)

Mechanizes G3 + G6 for every leaf of `docs/prds/v0_6/angle-units-surface-convergence.md`.
Built at decompose, **2026-07-29**, against `main` @ `bd10b6d0e1`.

**Drift status:** `git diff --name-only dc83d4fd60..bd10b6d0e1 -- crates/ tree-sitter-reify/
examples/ gui/` is **EMPTY** — the PRD was authored at `dc83d4fd60` and no source file has moved
since, so every §3 file:line anchor holds verbatim. Everything in §Corrections is an *author-time
measurement* re-measured this session, not drift.

**Probe vectors.** `target/release/reify` (built 2026-07-28 20:47; freshness verified — no
`crates/**/*.rs` or `stdlib/*.ri` is newer) via `reify eval` / `reify check`; `tree-sitter parse
--quiet` with CWD `tree-sitter-reify/` **under an isolated `XDG_CACHE_HOME`** (see C2).
Formal harness: `scripts/prd-decompose-verify.py bind` → `scripts/prd-capability-check.py --json`
→ `scripts/prd-decompose-verify.py synthesize`, Enumerator/Prover played in-session, one
independent Adversary agent.

---

## D3 verdict — **PASS, batch does not block**

| Role | Records | PASS | FAIL | UNPROVABLE | HARNESS_ERROR |
|---|---|---|---|---|---|
| Prover (α probe-set, 23 premises) | 23 | **23** | 0 | 0 | 0 |
| Adversary (independent agent, 37 tool calls) | 5 BLOCKING + 11 corrections + 9 unlisted premises | — | — | — | — |

Every Adversary BLOCKING finding was **resolved by rewriting leaf scope before filing** (the
sanctioned G3/G6 resolutions (a)/(b)/(c)) — none was waived, none left open. The corrections are
stamped into the filed task descriptions as `DECOMPOSE ADDENDUM — BINDING`.

---

## ⚠ Corrections applied at decompose (BINDING — supersede the PRD text)

### C1 — κ's scanner contract is factually wrong; coding to it yields a dead branch

PRD §5 C2, **as it read at decompose**: *"`UNIT_MUL_OP` fires on ASCII `*` **or** U+00B7 (UTF-8 `0xC2 0xB7`)"*; §3.8: *"a
scanner-local widening … plus a **UTF-8-aware read**"*. `tree-sitter-reify/src/tree_sitter/parser.h:49`
declares `int32_t lookahead;` — tree-sitter delivers **decoded codepoints**. U+00B7 arrives as the
single value `0xB7`; `0xC2` is never observable and one unmodified `advance()` consumes both bytes.

**Provenance (2026-08-29, task 5949).** Both quoted strings above are the PRD's **pre-correction**
wording and are no longer asserted there as fact: task 5949 corrected §5 C2 and §3.8 **in place**.
Grepping the PRD for either still hits, by design — expect that rather than absence. Every such
hit is one of two kinds, and neither is C2 or §3.8 asserting the byte contract: the superseded
wording quoted inside that PRD's `CORRECTION 2026-08-29` block, or the corrected sentence's own
explicit negation of it (§5 C2: "**not** the UTF-8 byte pair `0xC2 0xB7`"; §3.8: "no UTF-8-aware
decoding is required"). No hit count is pinned here — that would be a transient fact about a
companion file, falsified by any later rewording of C2 or §3.8 and checked by nothing. C1 remains
the binding record of *why* the contract is a codepoint; every anchor, measurement and verdict
below is unchanged.

Controlled experiment (three isolated repo copies, isolated `XDG_CACHE_HOME`, recompilation proven
by a deliberate `#error` variant that failed to build):

| Variant | patch at `scanner.c:250` | `tree-sitter parse --quiet unit_middot_mul.ri` |
|---|---|---|
| baseline (HEAD) | — | **exit 1**, `(ERROR [24,24]-[24,27])` |
| HYP A — codepoint | `(c == '*' \|\| c == 0xB7)` | **exit 0**, correct tree (`left: N`, `right: m`) |
| HYP B — the PRD's byte contract | `c == 0xC2` then require `0xB7` | **exit 1** (branch never fires) |

Under HYP A **all** of §9 B7's postconditions hold with no extra code: `5N·m` 0 · `5N·m/rad` 0 ·
`7850kg·m^-3` 0 · `5m^2·kg·s^-2·rad^-1` 0 · `5N·3` 1 · `5N· m` 1 · `5N · m` 1 · `let x = 5 · 3` 1.
**Bound into κ:** the contract is `lexer->lookahead == 0xB7`; the "UTF-8-aware read" phrase is void.
Corroboration that U+00B7 is genuinely unhandled today: `§`, `×`, `€`, `•`, NBSP all exit 1 in the
same slot; zero U+00B7 in `grammar.js`/`scanner.c`. `build.rs:268` compiles `scanner.c`, so κ's
change reaches the `reify` binary; `src/parser.c` is untracked/generated and
`src/.grammar_hash.stamp` matches `grammar.js`, so **κ needs no regeneration commit**.

### C2 — probe hygiene: `tree-sitter parse` caches by LANGUAGE NAME, not path

`~/.cache/tree-sitter/lib/reify.so` is keyed by language name and invalidated on source mtime, so
a patched build **in any other directory** silently serves this repo's parses. This produced a
**false PASS inside this very session**: mid-decompose readings of `unit_middot_mul.ri` returned
exit 0 while a concurrent HYP-A build held the cache. Re-run under isolated `XDG_CACHE_HOME`, and
again after the shared cache was rebuilt clean:

```
XDG_CACHE_HOME=<fresh> tree-sitter parse --quiet tests/prd-gate/fixtures/unit_middot_mul.ri  → exit 1  ✓
XDG_CACHE_HOME=<fresh> tree-sitter parse --quiet …/unit_nm_torque_immediate.ri               → exit 0  ✓
XDG_CACHE_HOME=<fresh> tree-sitter parse --quiet …/unit_curated_labels_ascii.ri              → exit 0  ✓
XDG_CACHE_HOME=<fresh> tree-sitter parse --quiet …/bare_angle_silently_accepted.ri           → exit 0  ✓
```

**PRD §3.8's baseline is CORRECT and re-confirmed.** Two independent vectors now agree that `·` is
novel syntax: the grammar gate (exit 1, ERROR node) and reify's own parser
(`reify check` → exit 1, `Parse error: syntax error: ·m`). κ carries the isolation instruction so
its implementer cannot repeat the false PASS.

### C3 — μ's (and σ's) test path is a **blocked construction** — harness-layout ratchet

The PRD's `crates/reify-compiler/tests/unit_label_round_trip.rs` hard-fails verify.
`tests/infra/harness-layout-lib.sh:80-82` lists `reify-compiler` (and `reify-eval`) among the five
crates where a **new standalone** `tests/<base>.rs` (base not `harness_*`) violates contract C1.
Enforced at `scripts/verify.sh:2014` → `scripts/check-harness-baseline-registration.sh:99-135`
(`reason=unregistered-standalone`, exit 1) and `tests/infra/test_harness_kloc_cap.sh:312-323`.
Adding a baseline row is **not** the remedy —
`tests/infra/harness-layout-baseline.manifest:30-38` records grandfathering as **SUPERSEDED**
(Leo 2026-07-22, esc-5056-11): *"a shrinking ratchet, not an allow-list to grow"*; rows are
actively being removed (`f9d25824bd`, `9b2c6f38e5`, both 2026-07-28).

**Bound:** μ lands at `crates/reify-compiler/tests/harness_units/unit_label_round_trip.rs` behind a
`harness_units.rs` compile-unit root (5 sibling precedents already exist in that crate:
`harness_auto_binding`, `harness_doc_chunks`, `harness_langcore`, `harness_patterns`,
`harness_traits`). σ inherits the same rule for whichever consolidatable crate it lands in.
The PRD's *other* drift-guard claims are **verified correct** — see μ's binding table.

### C4 — ζ's diff as scoped breaks a live invariant test; the obvious repair panics

`BUILTIN_NAME_FAMILIES` (`builtin_signatures.rs:425-436`) **includes `GEOMETRY_FUNCTION_NAMES`**,
and `arg_slot_keys_are_registered_builtin_names` (`:727`, swept over arities `0..=MAX_PROBED_ARITY
= 14` at `:420`) asserts every name yielding non-empty slots is in
`GEOMETRY_TOPOLOGY_SELECTOR_NAMES` **or** `NON_SELECTOR_ARG_SLOT_KEYS`
(`:474`, today exactly `["generate", "linear_pattern", "linear_pattern_2d"]`). All six angle
producers are `GEOMETRY_FUNCTION_NAMES` members (64 names, `units.rs:21-86`), so **ζ must register
all six in `NON_SELECTOR_ARG_SLOT_KEYS` with justifications** — absent from the PRD entirely.

The task-5652 hazard is live and the assertion message names the wrong slice first: routing them
into `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` instead makes
`expr.rs:3253-3254` `.expect("is_geometry_topology_selector implies result type")` **panic**
(`topology_selector_result_type`, `units.rs:343`, has no entry for any of the six).

**Undeclared cross-PRD co-edit:** PRD 1 task **5750** already binds this exact region
(`units-length-gate-completion.capability-manifest.yaml:230` cites `:474`, `:727`, `:420`). PRD §8
declares only a *message-template* edge η→ζ. The existing hard edge `5750 → ζ` serializes the file
correctly, so this is a **declaration gap, not an ownership contest** — recorded here and in ζ.

### C5 — ε's blast radius omits two byte-for-byte goldens and five test fns

§3.9 says "Tests that invert … **3**". Verified additions:

- `crates/reify-eval/tests/golden/pattern_circular_base.txt:66` and
  `pattern_circular_value.txt:66` — `[diag] Warning "circular_pattern: bare numeric angle \`90\`
  interpreted as 90°; use \`90deg\` or \`1.570796rad\` for explicit units"`, plus `:18`
  `si_value: 1.5707963267948966` (the *converted* value). `include_str!`-ed at
  `compile_geometry_op_characterization.rs:1092,1139`, **byte-compared** at `:229-249`, driven by
  `lit(90.0)` at `:1025,1072`. A byte-for-byte golden is the sharpest possible breakage and the
  PRD names neither file.
- five further fns in `crates/reify-eval/src/geometry_ops/tests.rs`: `:2990`
  `…_circular_pattern_valid_args`, `:3056` `…_bare_f64_converts_to_radians`, `:3102`
  `…_bare_int_converts_to_radians`, `:3150` `…_bare_number_emits_deprecation_warning`, `:3194`
  `…_angle_scalar_passes_through` (this last one **survives**).

### C6 — `draft` is NOT eval-reachable from `.ri` source today (δ's signal weakened)

δ's PRD signal asserts "`5deg` builds". It cannot — every form dies on a pre-existing plane-handle
placeholder before the angle matters:

```
reify eval <draft(b, 0.1, b)>            → exit 1  error: failed to compile geometry operation:
reify eval <draft(b, 0.1rad, b)>         → exit 1    no valid plane handle available for Draft
reify eval <draft(b, 5, plane_xy(0mm))>  → exit 1  (identical — the angle is never the cause)
reify check <draft(b, 0.1, b)>           → exit 0  (eval-only; corroborates §3.4)
```

Cause: `modify_draft` takes `plane` as `step_handles.last()` (in-situ comment: *"a pre-existing
approximation — `plane_xy` yields a `Value::Plane`, not a sub-op"*). `.ri` corpus `draft(` call
sites: **ZERO**. `compile_api_tests.rs:656`'s `draft(w, 0.1, w)` is compile-only and never evals.
Task **2010** ("Fix Draft geometry op: evaluate plane expression instead of using
`step_handles.last()` placeholder") is `done` while the defect is live — a phantom-done, surfaced
but **not owned here** (`geometry-modify-sweep-completion.md` territory).

**G6(b) resolution.** δ's binding assertion becomes the achievable half: the angle is read at
`:1979` *before* the plane resolution, so δ's diagnostic **replaces** the plane error at eval — an
observable, distinguishable text change. The dimensioned control asserts *absence of an angle
diagnostic* (the plane error remaining) rather than a clean build.

### C7 — §11 Q4 refuted: `draft.angle` has ONE read site

`geometry_ops.rs:1979` `let angle = eval_arg("angle")?;` sits **above** the `match faces_expr`
split; `:1991` and `:2049` are mutually-exclusive arms consuming that one binding. One gate site.

### C8 — α is over-scoped, and the set that actually breaks is unenumerated

Two independent enumerations agree: **every** `angle: Value::Real` site builds a `GeometryOp`
by hand and reaches the kernel via `kernel.execute` / `geometry_op_to_operation`, **bypassing** the
eval chokepoints γ/δ/ε gate. None of them breaks. Corrected counts: `Draft` sites are **12**
exact-form + 1 alias (`reify-ir/src/geometry.rs:9651,11025,11043`;
`reify-kernel-occt/src/lib.rs:8666,8747,8756,8805,`**`8848`**`,9221`;
`reify-eval/src/engine_build/tests.rs:6379,7062,`**`7478`** — the last written `angle: r(0.1)`,
invisible to a literal grep — and `reify-test-support/src/mocks.rs:3138`), against the PRD's 8.
The "19 total" figure is right as a literal-grep count.

The set that **would** break is disjoint and source-text-driven — `("angle", literal_f64(...))`
arg pairs fed through `compile_geometry_op`:
`geometry_ops/tests.rs:1449,1865,2054,2099,2142,2186,2694,3075,3123,3168,5351,6892`;
`compile_geometry_op_characterization.rs:577,587,788,1025,1072,1229`;
`harness_fea_solver_e2e/stress_sweep_degenerate.rs:132,313`;
`harness_geometry/geometry_error_handling.rs:1655,1731,1917,2322,2379`;
`harness_sweep/swept_kind_classifier_e2e.rs:312`;
`harness_topology_selector/topology_attribute_extrude_revolve_e2e.rs:240`. **Re-measure — these
drift.**

**Re-scope:** α owns (i) the one `.ri` corpus site — the only pre-gate necessity for γ — (ii) the
behaviour-preserving `Value::angle(..)` retype of the 13 hand-built-IR Draft sites, and (iii) a
committed **ledger** splitting "hand-built IR, bypasses the gate" from "source-text, gate-breaking".
Each gating leaf (γ/δ/ε) then migrates the source-text sites *its own* gate rejects, in its own
diff — PRD 1's C6 landing shape.

**Deliberate negative fixtures STAY bare** (C6 convention). This explicitly includes the PRD's own
committed `tests/prd-gate/fixtures/bare_angle_silently_accepted.ri:45,48`, which the Adversary
correctly notes makes §3.9's "exactly ONE bare-angle `.ri` site / **zero** bare circular_pattern
sites" arithmetic stale at HEAD — that file is γ's and ε's RED fixture, not a migration target.

### C9 — ι's C1 invariant 3 contradicts the diagnostic it targets

ι's host path `crates/reify-compiler/src/entity.rs:486-496` (`check_param_default_type`)
**already carries** `.with_code(DiagnosticCode::ParamDefaultTypeMismatch)` (`:493`). C1 inv. 3 read
literally (*"every diagnostic introduced or **modified** by this PRD carries the same
`DiagnosticCode` minted by PRD 1's task β"*) would force ι to *replace* an established code on a
param-declaration diagnostic with an argument-rejection code. **Carve-out bound into ι:** inv. 3
governs the *argument-rejection* family (β/γ/δ/ε/ζ); an already-coded non-argument diagnostic keeps
its code. ι also updates the verbatim canonical-message doc comment at
`crates/reify-core/src/diagnostics.rs:409`, which its rewrite makes stale.

### C10 — λ's footprint is materially larger than "~14", and three hazards are undeclared

| Item | Evidence |
|---|---|
| occurrence count | `\u{00B2}`/`\u{00B3}` occurrences: `display_units.rs` **20**, `reify-ir/src/value.rs` **22**, `dimension.rs` **2** — 44, vs the PRD's "~14 test expectations". `display_units.rs:722` is a **spurious** PRD entry (it is the `("Volume","L",1e-3,false)` rung). Five expectations the PRD misses are all in `value.rs`: `:10868, 10940, 11016, 11031, 11051` |
| **search artifact** | every curated label is written `"mm\u{00B2}"`, never a literal glyph — a plain `²`/`³` grep finds only comments. Any re-measure must use the escape form |
| **no blanket substitution** | `value.rs:11016/11031/11051` mix a category-(b) **label** with category-(d) `×10ⁿ` **exponent glyphs on the same line** (`SUPERSCRIPT_DIGITS`, §10-excluded). A global `\u{00B3}`→`^3` sweep corrupts the magnitude formatter |
| **undeclared `@display` vocabulary change** | the rung check (task 5233) is `crates/reify-compiler/src/annotations/display.rs:84-89` and matches `unit_ladders()` labels by **exact string equality**. After λ, `@display("mm²")` starts erroring and `@display("mm^2")` starts working. 0 corpus uses so no corpus edit — but ξ must document it |
| **15 GUI TS sites pin superscripts** | `gui/src/__tests__/PropertyEditor.test.tsx:1332,1333,1335,1346,1365,1435,1505,1604,1638`; `App.test.tsx:390,414,442`; `unitLadder.test.ts:48`; `gui/src-tauri/src/tests/types_tests.rs:3046`. They are **self-contained mocks**, so λ does not turn them red — the exposure is *silent divergence*: the shipped dropdown reads `mm^3` while every GUI test asserts `mm³`. λ updates them |
| **`units.ri`'s own note contradicts D7** | `units.ri:56-63` says *"To add SI volume support, extend `si_units.rs` with a cubic-metre base entry first"*. Following it instead of D7's hand-declaration trips the exact-count assert at `crates/reify-compiler/tests/si_units_tests.rs:322-328` (24-entry table). λ replaces that note with a pointer to D7 |
| tripwire that stays green | `every_ladder_dimension_round_trips_through_canonical_name` (`display_units.rs:757-777`) fails the moment anyone adds a `"Torque"` **ladder rung** (because `canonical_name()` returns `"RotationalStiffness"`). Neither η nor λ adds one — recorded so nobody does |

### C11 — G7: the PRD's §13 walk omits `INV-SF-7 parse-is-value-faithful`

`docs/legibility/design-invariants.md:171` (ratified 2026-07-25, three days *before* the PRD)
carries a seventh invariant §13 never walks. Direct hit on κ: *"Does this feature add grammar that
composes with quantity-literal juxtaposition…? If so, does it carry ambiguity-regression corpus
tests pinning that adjacent-token variation cannot change a parsed value?"* **Resolved, not
waived** — κ gains an explicit adjacency-variation corpus obligation (the eight rows in C1's table
are the seed). No other leaf hits it. **Zero G7 waivers in this batch.**

### C12 — the two program gaps, closed

- **(a)** `crates/reify-core/src/dimension.rs:512` still says *"The slice contains 34 entries"*;
  the slice holds **51** (counted), 52 after η. Assigned to PRD 3 by
  `dimensioned-construction-strictness.md:220`. **Folded into η**, not μ as the program brief
  suggested — η is the leaf that edits `dimension.rs` and moves the count, so it lands same-diff
  with no extra lock on a hot file. Same block's *"linear scan **backward**"* claim (`:506`) is
  also wrong — `type_resolution.rs:232-234` is a forward `.iter().find(...)`; η fixes both.
  `chunks/units.md:36`'s independent *"35 standard named dimensions"* is ξ's.
- **(b)** `examples/unit_expressions.ri:10` (`// torque 5kN*m → … (ENERGY; Torque not a named
  alias)`) and `:19` (`param torque : Energy = 5kN*m`) go stale the moment η lands — a corpus
  exemplar that will actively teach the thing ι's diagnostic exists to correct. **Folded into ο**,
  with the runtime coupling flagged: read by path at
  `crates/reify-eval/tests/unit_expressions_e2e.rs:20`, and `:17` cited by
  `crates/reify-compiler/tests/harness_units_materials/materials_fea_tests.rs:274-280`.

### C13 — η may split; co-existence is non-fatal

`reserved_name_lint.rs:61,65` emits `W_RESERVED_TYPE_NAME` when a `Declaration::TypeAlias` shadows
a builtin — but the stdlib gate (`stdlib_loader.rs:499-514`, `stdlib_loader_tests.rs:25-33`) checks
`Severity::Error` only and no test asserts a zero-warning stdlib. Resolution order is
builtin-beats-alias (`type_resolution.rs:642-646`), matching the landed `Velocity`
(`units.ri:89-96`) and `RotationalStiffness` (`flexures.ri:56-58`) precedents. So the PRD's
implicit "must be atomic" framing is unsupported. Deleting `pub type Torque` breaks nothing —
every use site (`ports_mechanical.ri:93,94,99,127,131,132,135,138`;
`ports_stdlib_compile.rs:555-582,670-679,711-793`) resolves to the identical builtin vector.

### C14 — G5: the B+H integration gate had no leaf (σ added)

The PRD is B+H (§5 contract + §9 two-way sketch) but §12 assigns only B5/B6 (μ), B11 (ν) and B12
(ρ) to a leaf — rows **B1, B2, B2b, B3, B4, B7, B8, B9, B10 had no owner**. Under the narrow-lock
orchestrator that is the exact integration-starvation shape G5 exists to prevent. **Leaf σ added**
(18th), mirroring sibling PRD 1's task ο (5761, "the full §6 two-way boundary-test suite").
B2b — `revolve_full` surviving γ — is the two-way PRD1↔PRD3 seam test and is σ's headline row.

---

## Per-leaf bindings

Evidence forms: `probe:` executed command + captured output · `grep:file:line` wired on main ·
`producer:task-N` upstream in the transitive dependency closure · `grammar-fixture:path` ·
`floor:` numeric.

### α — 5777 · dimension the bare-angle corpus + build the migration ledger

| Capability | Evidence | Verdict |
|---|---|---|
| `feature_datum_axis.ri:24` holds a bare `6.283185307179586` in `revolve`'s angle slot | `grep:examples/geometric_relations/feature_datum_axis.ri:24` | PASS |
| it is the sole *non-deliberate* bare-angle `.ri` site | enumeration of 29 `revolve\|rotate\|rotate_around\|arc\|draft\|circular_pattern` `.ri` call sites; the only other bare ones are the PRD's own deliberate negative fixture (C8) | PASS |
| the migration is behaviour-preserving | `grep:crates/reify-ir/src/value.rs:1635` `Value::as_f64` returns `si_value` for `Value::Scalar`; `probe: reify eval <bare-2π revolve>` → **exit 0** today | PASS |
| the file is Rust-coupled (unlisted premise) | `grep:crates/reify-compiler/tests/feature_datum_axis_example_tests.rs:28,58,69,90`; `crates/reify-eval/tests/feature_datum_tests.rs:450,462` — both read it by path and assert zero errors | PASS |
| it is **not** in `_RUST_COUPLED_RI_FIXTURES` | `grep:scripts/verify.sh:1014` — that list is `tests/prd-gate/fixtures/`-scoped (`:992-994`), 5 entries | PASS |
| 13 hand-built-IR Draft sites, none gate-breaking | C8 enumeration | PASS (count corrected 8→13) |

### β — 5778 · `angle_spec()`

| Capability | Evidence | Verdict |
|---|---|---|
| `arg_acceptance` has `length_spec`/`density_spec`/`accept_arg` and **no** `angle_spec` | `grep:crates/reify-eval/src/arg_acceptance.rs:86,103,117`; `angle_spec` absent repo-wide | PASS |
| **the ANGLE rejection mechanism fires today** (G6 branch 4) | `probe: reify eval faces_by_normal(b,0.0,0.0,1.0,0.01)` → **exit 1**, `error: faces_by_normal: tol argument expects Angle, got Real` | PASS — rejection observed |
| the hint is absent today (what β adds is observable) | same probe: **no** `pass a dimensioned angle` clause; `grep:geometry_ops.rs:8755` `resolve_scalar_dim_arg`, call site `:8767-8771` passes `migration_hint: None` | PASS |
| the shared `DiagnosticCode` | `producer:task-5743` (PRD 1 β: *"introduce ONE shared DiagnosticCode … PRDs 3 and 5 reuse this code"*) — **upstream** | PASS |
| `value_short_label` already prints "dimensionless Scalar" (§11 Q5) | `grep:arg_acceptance.rs:134` | PASS |

### γ — 5779 · gate `rotate` / `rotate_around` / `revolve` / `arc`

| Capability | Evidence | Verdict |
|---|---|---|
| all five reads are `eval_named_arg_f64` closures | `grep:geometry_ops.rs:2251,2312,2975,3327,3328` (`f64_arg("angle"\|"start_angle"\|"end_angle")`) | PASS |
| bare angles are silently accepted today | `probe: reify eval` → rotate **0**, rotate_around **0**, revolve(bare 2π) **0**, arc(bare) **0**; zero angle diagnostics in all four | PASS |
| dimensioned angles build and must keep building | `probe:` `rotate(…,45deg)` → 0; `arc(…,0deg,90deg)` → 0 | PASS |
| **B2b: `revolve_full` survives the gate** | `probe: reify eval revolve_full(rectangle(20mm,10mm), -10mm,0mm,0mm, 0.0,1.0,0.0)` → **exit 0** today; `grep:crates/reify-compiler/src/geometry.rs:2064-2067,2080` (TAU literal → the `"angle"` arg); `producer:task-5742` retypes it **ANGLE** (its own text: *"feeding an ANGLE slot → retype ANGLE"*) — **upstream** | PASS — D10 re-ratified |
| `angle_spec()` | `producer:task-5778` — upstream | PASS |

### δ — 5780 · gate `draft.angle`

| Capability | Evidence | Verdict |
|---|---|---|
| `draft.angle` is an R7 raw-`Value` read, ungated | `grep:geometry_ops.rs:1979`; IR `crates/reify-ir/src/geometry.rs:944` `angle: Value` | PASS |
| ONE read site, not two | C7 | PASS (§11 Q4 refuted) |
| the angle read precedes plane resolution → δ's diagnostic is observable | `grep:geometry_ops.rs:1979` (angle) vs `:1984-1989` (`plane_id … ok_or_else`) | PASS |
| **draft is not eval-reachable today** | C6 probes | **signal weakened** per G6(b) |

### ε — 5781 · retire `resolve_bare_angle`

| Capability | Evidence | Verdict |
|---|---|---|
| `resolve_bare_angle` + its 2 call sites | `grep:geometry_ops.rs:880,2585,2626` | PASS |
| warn-and-convert is live and code-less | `probe: reify eval circular_pattern(b,0mm,0mm,0mm,0.0,0.0,1.0,4,360)` → **exit 0** + `warning: circular_pattern: bare numeric angle \`360\` interpreted as 360°; use \`360deg\` or \`6.283185rad\` for explicit units` (verbatim); `grep:crates/reify-core/src/diagnostics.rs:3885` `Diagnostic::warning` sets `code: None` | PASS |
| `360deg` stays warning-free | `probe:` → exit 0, no warning | PASS |
| full breakage set (**corrected**) | 2 inverting tests `grep:crates/reify-eval/tests/circular_pattern_angle.rs:47,86`; **+2 byte-compared goldens** `tests/golden/pattern_circular_{base,value}.txt:66,:18`; **+5 fns** `geometry_ops/tests.rs:2990,3056,3102,3150,3194` — C5 | PASS (3 → 9) |
| the stale doc cite | `grep:docs/prds/v0_6/type-hygiene.md:106` (`:418-439`; real site `:880`) | PASS |
| task 1763 is the ruling being reversed | `get_task 1763` → `done`, *"circular_pattern angle should accept degrees (CAD convention) or convert internally"* | PASS |

### ζ — 5782 · ANGLE `CheckableArg` compile slots

| Capability | Evidence | Verdict |
|---|---|---|
| the ANGLE arm already exists (extend, don't invent) | `grep:crates/reify-compiler/src/builtin_signatures.rs:165-175` — 4 selectors' `tol` @ index 2, `ExpectedArg::Scalar { dimension: ANGLE, type_name: "Angle" }` | PASS |
| arity-keyed machinery + consumer | `grep::145` `builtin_arg_slots(name, arg_count)`; `:325` `check_builtin_arg_types` | PASS |
| no geometry **producer** has a slot | grep over `builtin_arg_slots` arms: all six absent | PASS |
| **`NON_SELECTOR_ARG_SLOT_KEYS` registration is mandatory** | `grep::474` (`["generate","linear_pattern","linear_pattern_2d"]`), `:425-436` `BUILTIN_NAME_FAMILIES ⊇ GEOMETRY_FUNCTION_NAMES`, `:420` `MAX_PROBED_ARITY = 14`, `:727` the invariant test — C4 | PASS (unlisted premise bound) |
| the selector route **panics** | `grep:crates/reify-compiler/src/expr.rs:3253-3254` `.expect(...)`; `units.rs:343` `topology_selector_result_type` has no entry for the six | PASS — anti-repair recorded |
| the reconciled hint-carrying template | `producer:task-5750` (PRD 1 η item 3) — **upstream**; same task also co-owns `:474`/`:727` (C4) | PASS |

### ν — 5783 · closure-guard angle extension

| Capability | Evidence | Verdict |
|---|---|---|
| the harness | `producer:task-5752` (PRD 1 ι) — **upstream** | PASS |
| the probe universe | `grep:crates/reify-compiler/src/units.rs:21-86` `GEOMETRY_FUNCTION_NAMES` = **64** names (PRD's `[drift]` confirmed; research doc's 66 wrong); all seven angle producers present, incl. `revolve_full` | PASS |
| the anti-vacuity template | `version_id_discipline_gate.rs` seeded self-tests (named by PRD 1's ι) | PASS |
| the gates it asserts over | `producer:tasks 5779 (γ) / 5780 (δ) / 5781 (ε) / 5782 (ζ)` — **upstream** | PASS |
| `draft` will answer with the plane error, not a gate | C6 — ν must not read that as "gated" | PASS — bound into ν |

### κ — 5784 · scanner accepts U+00B7

| Capability | Evidence | Verdict |
|---|---|---|
| `·` is novel syntax, on **two** independent vectors | `grammar-fixture:tests/prd-gate/fixtures/unit_middot_mul.ri` → `tree-sitter parse --quiet` **exit 1** (ERROR node, isolated cache); `probe: reify check <5N·m>` → **exit 1**, `Parse error: syntax error: ·m` | PASS — `grammar_confirmed: false`, producer = κ itself |
| the change is scanner-local | `grep:tree-sitter-reify/grammar.js:53-56` (externals), `:1511` (reference); `src/scanner.c:244-258` (`c == '*'`), `:101` `is_unit_start` = `[A-Za-z_(]`; U+00B7 in neither | PASS |
| **the contract is a codepoint** | C1 controlled experiment | PASS — PRD C2 corrected |
| no regeneration commit needed | `src/parser.c` untracked; `src/.grammar_hash.stamp` matches `grammar.js`; `build.rs:268` compiles `scanner.c` into the `reify` binary | PASS |
| corpus + Rust grammar-test homes exist | `tree-sitter-reify/test/corpus/unit_expr.txt`; `tree-sitter-reify/tests/imaginary_literal_grammar_tests.rs` (13 sibling `*_grammar_tests.rs`) | PASS |
| B7 stays an error | `probe: reify check <let x = 5 · 3>` → exit 1, `Parse error: syntax error: · 3`; grammar gate exit 1 | PASS |
| INV-SF-7 obligation | C11 | resolved into scope |

### η — 5785 · `Torque` named dimension

*Every row below was measured at decompose time, **before** η ran, and is retained verbatim as provenance. η itself then deleted the `pub type Torque` alias from `crates/reify-compiler/stdlib/ports_mechanical.ri` and replaced it with a `NAMED_DIMENSIONS` registry row (`crates/reify-core/src/dimension.rs:640`, re-measured 2026-08-03) — so wherever a row below says "the alias", it names a thing that no longer exists. The verdicts still stand: re-probed 2026-08-03 at HEAD, `param t : Torque = 5N*m/rad` still checks clean (exit 0), now resolving via the registry and needing no `import std.ports.mechanical`.*

| Capability | Evidence | Verdict |
|---|---|---|
| `Torque` exists today only as a stdlib alias | `grep:crates/reify-compiler/stdlib/ports_mechanical.ri:29`; header rationale `:9-11` | PASS (pre-η; alias since deleted by η — see section note) |
| the alias is the live consumer to retire | `probe: reify check <param t : Torque = 5N*m/rad>` → exit 0; `reify eval` → `5 m^2·kg·s^-2·rad^-1` | PASS |
| `ROTATIONAL_STIFFNESS` is bit-identical to TORQUE | `grep:crates/reify-core/src/dimension.rs:275-276` exps `[(0,2),(1,1),(2,-2),(7,-1)]`; row `:572` | PASS |
| appending after `:572` is inert | `grep:dimension.rs:358-372` `canonical_name` = forward first-match; no test pins `NAMED_DIMENSIONS.len()`; no VARIANT_COUNT backstop; all 4 consumers forward/table-derived. Alias-row precedents `Curvature`/`AbsorptionCoeff` `:576-583`, `Momentum`/`Impulse` `:585-594` | PASS |
| `unit` decls need a **named dimension**, never an alias | `grep:crates/reify-compiler/src/type_resolution.rs:454-459` `compile_unit` → `resolve_dimension_type`, never the alias registry — the load-bearing reason η gates θ | PASS |
| deleting the alias breaks nothing | 11 use sites all resolve to the identical builtin vector (C13) | PASS |
| co-existence is non-fatal (η may split) | C13 | PASS |
| stale doc block (gap a + backward-scan claim) | `grep:dimension.rs:506,512` | PASS |

### θ — 5786 · `Nm` unit symbol

| Capability | Evidence | Verdict |
|---|---|---|
| `Nm` is unclaimed | `probe: reify check <let t = 5Nm>` → exit 1, `error: unknown unit: Nm`; grep: absent from every `.ri`, Rust table and generator (`si_units.rs:184` `N` prefixes = `Only(["k","M","G"])`) | PASS |
| `5Nm` already parses | `grammar-fixture:tests/prd-gate/fixtures/unit_nm_torque_immediate.ri` → exit 0 (isolated cache) | PASS — `grammar_confirmed: true` |
| lookup is case-sensitive (pin, don't build) | `probe:` `5nm` → exit 0 (`0.000000005 m`); `5NM` → exit 1 `unknown unit: NM` | PASS |
| the `lbf`/`psi` declaration precedent | `grep:crates/reify-compiler/stdlib/units.ri:48,53` | PASS |
| `Torque` as a named dimension | `producer:task-5785` — **upstream** | PASS |

### ι — 5787 · Energy↔Torque teaching diagnostics

| Capability | Evidence | Verdict |
|---|---|---|
| the host path is live, both directions | `probe: reify check <param bad : Torque = 5N*m>` → exit 1, `error: parameter 'bad' declared \`Scalar[m^2·kg·s^-2·rad^-1]\` but its initializer evaluates to \`Scalar[m^2·kg·s^-2]\`; declared type and initializer dimension must agree`; the `Energy = 5N*m/rad` fixture → the mirrored message | PASS |
| the emitter | `grep:crates/reify-compiler/src/entity.rs:486-496` `check_param_default_type` | PASS |
| INV-SF-6 already satisfied there | `grep:entity.rs:493` `.with_code(DiagnosticCode::ParamDefaultTypeMismatch)` — **C9 carve-out**: ι must not replace it | PASS |
| the canonical form is mirrored in a doc comment | `grep:crates/reify-core/src/diagnostics.rs:406-415` — ι updates it | PASS |
| existing Torque-vs-Energy guards | `grep:dimension.rs:1795-1855` (RS ≠ RD ≠ TS ≠ TD) — must pass unmodified | PASS |
| `Torque`/`Nm` by name | `producer:tasks 5785 (η), 5786 (θ)` — **upstream** | PASS |

### λ — 5788 · ASCII curated labels + `unit L : Volume`

| Capability | Evidence | Verdict |
|---|---|---|
| the curated superscript labels | `grep:crates/reify-core/src/display_units.rs:414,422,427,432,440,448,453,463,540,548,553`; `dimension.rs:465,467`; `reify-ir/src/value.rs:3072,3074` | PASS |
| **real footprint 44 escape occurrences across 3 files**, not ~14 | C10 | PASS (corrected) |
| `dimension_unit_label` is private (§11 Q2 live) | `grep:reify-ir/src/value.rs:3068` `fn dimension_unit_label(…)` — no `pub` | PASS |
| the `L` rung names a nonexistent unit — **live red on main** | `grep:display_units.rs:458` (`label: "L"`, `si_scale: 1e-3`); `probe: reify check <let v = 1L>` → **exit 1**, `error: unknown unit: L` | PASS |
| `Volume` is a named dimension, so `pub unit L : Volume` resolves | `grep:crates/reify-core/src/dimension.rs:526` | PASS |
| the ASCII target alphabet already resolves | `probe: reify check <1mm^2 1mm^3 1kg/m^3 1g/cm^3 1USD 1deg>` → **exit 0**; `grammar-fixture:tests/prd-gate/fixtures/unit_curated_labels_ascii.ri` → exit 0 | PASS — no grammar work |
| S1's `·` and `dimension.rs:912,931` goldens stay | C2 scoping | PASS |
| 0 `@display` corpus uses; vocabulary change undeclared | grep for `@display(` in `*.ri`: none; `grep:crates/reify-compiler/src/annotations/display.rs:84-89` exact-string rung match — C10 | PASS |
| `units.ri:56-63`'s note contradicts D7 | C10 | PASS |
| `floor:` scale tolerance | `probe:` `1g/cm^3` → `999.9999999999999 kg·m^-3` ⇒ observed relative error ≈ **1.1e-16**; Invariant R's `1e-12` is 4 orders above it | PASS — `bound > floor` |

### μ — 5789 · round-trip property test (Invariant R)

| Capability | Evidence | Verdict |
|---|---|---|
| RED on main for two independent reasons | `probe:` `1L` → `unknown unit: L`; `7850kg·m^-3` → `Parse error` | PASS |
| the four label surfaces | S1 `dimension.rs:597-628`; S2 `:458-474`; S3 `reify-ir/src/value.rs:3068-3090`; S4 `display_units.rs:379` + 11 label sites | PASS |
| `NAMED_DIMENSIONS` is iterable from a test | `grep:dimension.rs:514` `pub static`; already iterated at `:1086` | PASS |
| resolvable without `reify-eval` (§11 Q3) | `grep:crates/reify-compiler/tests/common/mod.rs:185` `stdlib_param_si_value(param_type, literal) -> (f64, DimensionVector)` (live uses `compound_unit_resolution_tests.rs:38`, `imperial_units_tests.rs:20`); lower-level `reify_compiler::resolve_unit_expr` `units.rs:1617` / `UnitRegistry::lookup` `:1533` | PASS — Q3 answered |
| **drift-guard: no registration needed** — VERIFIED, not assumed | `tests/infra/run-all-classification.manifest` — 208 rows, **0** match `crates/` (header scopes it to `tests/infra/test_*.sh`; `run-all-classification-lib.sh:174-177` never walks `crates/`); `scripts/heavy-test-filter-lib.sh:48` `REIFY_HEAVY_NEXTEST_FILTER` does not name `reify-compiler`, so the test is gate-resident by default; `.config/nextest.toml` overrides are a closed 5-item list; `test_no_new_wallclock_upper_bounds.sh:107` scans `"$dir"/*.sh` only | PASS |
| **the PRD's path is a blocked construction** | C3 | **path corrected** to `tests/harness_units/…` |
| producers | `producer:tasks 5784 (κ), 5788 (λ)`; θ/η reach μ transitively via λ ← θ ← η | PASS |

### ξ / ο / π / ρ — 5790 / 5792 / 5793 / 5794 · docs-truth

| Capability | Evidence | Verdict |
|---|---|---|
| the chunks exist and are stale | `grep:crates/reify-mcp/src/tools/chunks/units.md:36` *"35 standard named dimensions"* (real 51 → 52 after η); `:50` "Angle as Base Dimension" | PASS |
| the exemplar corpus + its bidirectional gate | `examples/best_practices/` (8 files incl. `INDEX.md`, `bolt_circle.ri`); `INDEX.md:53` bolt_circle row; gate `crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs` | PASS |
| gap (b) target + its runtime coupling | `grep:examples/unit_expressions.ri:10,19`; `crates/reify-eval/tests/unit_expressions_e2e.rs:20`; `crates/reify-compiler/tests/harness_units_materials/materials_fea_tests.rs:274-280` | PASS |
| cheatsheet anchors | `grep:.claude/skills/reify-design/SKILL.md:53` (**Quantities:**), `:135` (**Always units**) | PASS |
| ρ asserts findability, not a build | scripted grep-and-read over the chunks + `INDEX.md`; no new capability asserted | PASS |

### σ — 5796 · B+H integration gate, the §9 two-way boundary suite

| Capability | Evidence | Verdict |
|---|---|---|
| every row's substrate | all bindings above | PASS |
| B2b's producer is upstream | `producer:task-5742` → γ → σ | PASS |
| G5 compliance | C14 | resolved |
| harness-layout ratchet | C3 applies to `reify-eval` too (`harness-layout-lib.sh:80-82`) | bound |
| drift-guard | same verification as μ | PASS |

---

## Bindings that had to be resolved before queueing

| # | Binding | Original verdict | Resolution |
|---|---|---|---|
| 1 | μ/σ test path `crates/reify-compiler/tests/unit_label_round_trip.rs` | **blocked construction** — verify hard-fails `unregistered-standalone` | relocated to `tests/harness_units/…`; grandfathering explicitly ruled out |
| 2 | ζ: "extend the `:168` angle arm" | **producer-extent-short** — omits the mandatory `NON_SELECTOR_ARG_SLOT_KEYS` registration; the intuitive repair panics | registration bound into ζ; anti-repair recorded; co-edit with 5750 declared (edge already serializes) |
| 3 | δ: "`5deg` builds" | **producer-absent** — blocked by a pre-existing defect no task in δ's closure fixes | signal weakened per G6(b) to the achievable, observable half |
| 4 | κ: "UTF-8 `0xC2 0xB7`" | substrate-wrong — a branch that can never fire | corrected to codepoint `0x00B7`, proven by controlled experiment |
| 5 | ε: "3 inverting tests" | **producer-extent-short** — 2 byte-compared goldens + 5 fns unlisted | breakage set corrected 3 → 9 |
| 6 | α: "8 Draft fixtures", "exactly ONE bare-angle site" | **producer-extent-short** + stale arithmetic | Draft 8→13; α re-scoped to `.ri` + ledger; the gate-breaking source-text set enumerated and assigned to γ/δ/ε per C6; deliberate negative fixtures stay bare |
| 7 | ι: C1 inv. 3 vs `ParamDefaultTypeMismatch` | contract self-contradiction | carve-out bound into ι |
| 8 | λ: "~14 expectations" | **producer-extent-short** + 3 undeclared hazards | footprint corrected to 44 occurrences; `@display` vocabulary change, GUI TS divergence and the `units.ri` counter-instruction all bound |
| 9 | G5: no integration-gate leaf for B1–B4/B7–B10 | integration-starvation risk | leaf **σ** added (18th) |
| 10 | G7: INV-SF-7 unwalked | unresolved invariant hit | resolved into κ's scope — **no waiver** |

**No binding resolved to `declared-only`, `test-only`, `producer-downstream`, `fixture-ERROR` or
`rejection-absent`.** The single numeric bound in the PRD (Invariant R's `1e-12` relative scale
tolerance) is `bound > floor` by four orders of magnitude (observed floor ≈ 1.1e-16 at `g/cm^3`).
