# Struct-ctor field-type conformance enforcement (general)

**Status:** active — authored 2026-07-20 (design session Leo + escalation-watcher 2026-07-20, brief `~/.claude/spawn-briefs/general-struct-ctor-conformance-prd.md`); committed for review **before** decomposition.
**Placement:** root `docs/prds/` (version-agnostic compiler/type-system capability; not an FEA PRD).
**Type:** extension of a shipped mechanism — extends task 4584's struct-ctor conformance chokepoint (merged `9c6a5a80e2`) to **all concrete field types**. Approach **B + H** (contract §6 + boundary-test sketch §7): corpus-wide breaking change, ≥ 2 cross-PRD consumers.
**Intended breakage:** this is a language-semantics **tightening** and a deliberate source-breaking, corpus-wide change (Leo: "get the breakage out of the way before these tools have external users").

## 1. Goal

`reify check` (and every path that compiles `.ri` — `eval`, GUI, LSP) enforces declared struct-ctor field types against supplied argument types at check-time, for **all concrete field types** — selectors AND primitives — emitting a structured diagnostic (field name, expected type, found type, span at the offending arg) and a non-zero exit where today the mismatch is silently accepted.

Headline user-observable signal (G2): `reify check` on `PressureLoad(face: frame3(...))`, `Widget(label: 42)` (Int→String), or the legacy string form `PressureLoad(face: "x_max")` emits a structured compile-time diagnostic and exits non-zero — where today each exits 0 with `All constraints satisfied.`

## 2. Background — the hole, verified

All claims re-verified against `main` @ `33d9bb3e21` (2026-07-20) with live `reify check` probes (fixtures re-committed as boundary tests in task α).

The struct-ctor conformance chokepoint **exists** (task 4584) but reaches only an allowlist of param types:

- `StructureInstanceCtor` lowering (`crates/reify-compiler/src/expr.rs:2677-2847`) binds args by name/position, emits only a duplicate-named-arg diagnostic (:2716-2728), appends unknown-named/over-arity args leniently as `__arg{i}` (:2769-2786), and returns at :2837 **before** the fn-call path's `type_compat`/overload machinery. The empty-collection expected-type push-down is short-circuited for struct ctors (:2469), so ctor args compile with no expected type.
- The post-compile chokepoint `check_expr_struct_ctor_args` (`crates/reify-compiler/src/compile_builder/entities_phase.rs:1575-1623`, invoked at :1344 over every template root expr, fn body, and assoc-fn body) walks each named ctor arg through the shared conformance walker — but a **param-type allowlist** (:1598-1608) scopes it to `List<TraitObject>` (4444), `StructureRef` (4584), `Vector` (4622), and bare `Selector`/`AnySelector` (4598). Every other param type — **primitives (`String`/`Int`/`Scalar`/`Bool`/…), any `Option<...>`-wrapped type, non-trait collections** — is silently skipped.
- The type-level walker `walk_param_against_arg_type` (`crates/reify-compiler/src/conformance/mod.rs:864-1005`) has diagnostic-emitting leaf arms for exactly those four families; its final arm's own comment names the gap: *"Non-wrapper non-trait params (Real, Int, etc.) fall through silently; a fully general arg-shape pass is future work."* This PRD is that pass.
- The eval-time ctor (`crates/reify-expr/src/lib.rs:1074-1105`) inserts evaluated args verbatim — no type comparison (by design; see D7).

Probe matrix (scratchpad fixtures, `target/debug/reify check`, 2026-07-20):

| # | Context | Param type | Arg | Today | After this PRD |
|---|---|---|---|---|---|
| 1 | value-cell let | `Option<FaceSelector>` | `frame3(...)` (pose) | **silent, exit 0** | error (pose-vs-set) |
| 2 | value-cell let | `String` | `42` | **silent, exit 0** | error |
| 3 | value-cell let | `Option<FaceSelector>` | `face(body,"x_max")` | clean | **stays clean** (implicit-Some) |
| 4 | `sub p = Ctor(...)` | `String` | `42` | **silent, exit 0** | error |
| 5 | `sub p = Ctor(...)` | `Option<FaceSelector>` | bare selector | **error, exit 1** (wrapper-shape arm; post-4370 — see amendment) | **clean** (implicit-Some C1.6 — α fixes this live hole) |
| 6 | value-cell let | bare `FaceSelector` | `edges(body)` | **error, exit 1** (4598 arm) | error (code refined, D2) |
| 7 | value-cell let | `Option<FaceSelector>` | `"x_max"` (legacy string) | **silent, exit 0** | error (disallow-string) |
| 8 | `sub p = Ctor(...)` | `Option<FaceSelector>` | `42` | **error, exit 1** (wrapper-shape arm, misleading `TypeNotConformingToTrait`; post-4370 — see amendment) | error (uniform `E_ARG_TYPE_MISMATCH`, context-independent with row 4) |
| 9 | value-cell let | bare `FaceSelector` | `"x_max"` | **error, exit 1** (4598 arm) | error (unchanged) |

Probes 6/9 prove the rejection **mechanism** (walker → `type_compatible` → structured diagnostic) is live and firing for allowlisted types; probes 1/2/4/7 prove the targets are silently accepted. The delta is exactly the allowlist + walker leaves (G6 branch-4 evidence for the RED signals in §8).

**Decompose-time amendment (2026-07-20, fresh binary at `56380a8f8a`):** rows 5/8 above were originally recorded as pre-4370 behavior — the authoring session's probe binary (built 2026-07-19 21:38) predated task 4370's merge (`1d886f3f45`, 2026-07-20 05:31), which flipped `PressureLoad.face` to `Option<FaceSelector>` in the `include_str!`-embedded stdlib. Against current main, the sub `=` path (which routes named args through `PendingBoundCheck::TraitArgConformance` — disproving the original §10 Q4 claim that it takes the expression path) reaches the walker's final-arm wrapper-shape check, which rejects **any** non-wrapper arg to an `Option<T>` param (`TypeNotConformingToTrait`, "does not match wrapper shape"): row 8 already errors (wrong code/message; α re-codes it), and row 5 — legitimate implicit-Some — **errors today**, a live hole that α's Option-unwrap arm fixes. No design decision changes: D1's Option-unwrap arm + allowlist flip are exactly the fix; the value-cell rows (1/2/3/7) and row 4 (sub `String`←`Int`) re-verified as tabled.

Consumers blocked on precisely this capability (G1, verified live 2026-07-20):
- **Task 4833** (P4-π pose-vs-set guard) — blocked; its dry-run RCA concludes *"a real fix requires an all-structures compiler-level type-conformance chokepoint that doesn't exist yet"* and proposes splitting off exactly this prerequisite.
- **Task 4371** (FEA BT1–BT7 boundary-test gate) — blocked; BT1 (`E_SELECTOR_KIND_MISMATCH` at a ctor field) and BT4's "non-selector target rejected" need this enforcement to fire at ctor sites.
- **Revised v0.6 FEA migration** (`docs/prds/v0_6/fea-load-support-selector-migration.md`, re-gated separately) — its "disallow string" is this chokepoint applied to migrated selector fields.
- Every `.ri` author: real field-type errors instead of silent `Undef`/silent-accept across all `structure def`s.

Red herring (do not repeat): `crates/reify-stdlib/src/helpers.rs::validate_selector_target` is **not** the fix site — its only callers are the native `DisplacementSupport`/`RollerSupport` builtins (`supports.rs:120,138`), unreachable from `structure def` ctors; a `None` return maps to `Value::Undef` with no diagnostic channel. A prior plan (P4 §4 D3) to wire there was a false premise; 4833's RCA independently confirms.

## 3. Sketch of approach

Three stages inside one PRD, each landing green through the merge gate:

1. **Enforce at Warning severity** (α, ε): generalize the 4584 chokepoint — flip the allowlist, add the missing walker leaves (Option-unwrap, general concrete leaf), refine selector diagnostics — with severity held at `Warning` by a single const knob. Main stays green; the corpus now *reports* every latent mismatch.
2. **Survey + fix-forward** (β, γ): the warn-stage compiler over all tracked `.ri` (493 files) **is** the survey instrument — mechanized, not a hand audit. Commit the survey artifact; fix every site under the D9 rules until the corpus is warning-free.
3. **Flip Warning→Error** (δ): one-line severity flip + committed negative fixtures asserting the exit-1 diagnostics. This is the language-semantics change and the PRD's headline signal.

Sequencing with the v0.6 FEA migration: **this chokepoint lands first** (green on the pre-migration corpus, fixing non-FEA latent mismatches); the v0.6 field flips + call-site migration land on top and are enforced by the then-live chokepoint.

## 4. Resolved design decisions

**D1 — Wire-site: extend the 4584 conformance chokepoint (walker + entry filter), NOT a new `type_compat` comparison in the `expr.rs:2758` binding loop.** This *overrides* the originating brief's Agent-B recommendation, on evidence the brief lacked:
- The chokepoint already exists with wrapper recursion, the anti-cascade skip list, numeric→`StructureRef` promotion, and four leaf arms (4584/4598/4622/4081). The remaining work is two walker arms + a filter flip — strictly smaller than a new site.
- The binding loop runs during expression compilation, when forward-referenced templates' param types are not reliably resolved; the post-compile phase exists precisely for this (`entity.rs` defers sub-arg checks *"so forward-referenced structures are available"*). A binding-loop check would be structurally wrong for forward refs.
- A second enforcement site would duplicate the predicate + skip list in lock-step with the walker — the exact `no-lockstep-duplication` violation (design-invariants INV-5) the walker exists to prevent.
- Diagnostic wording/codes stay uniform with the landed 4584/4598/4622 tests.

Concretely: (a) `walk_param_against_arg_type` gains an **Option-unwrap arm** — `(Type::Option(inner_p), arg_ty)` where `arg_ty` is not `Option` recurses against `inner_p` (implicit-Some, C1 rule 6) — and a **general concrete-leaf arm** replacing the silent final fall-through: for any remaining concrete param leaf (`Scalar`/`Int`/`String`/`Bool`/`Enum`/`Point`/`Frame`/`Direction`/`Axis`/`Plane`/`Transform`/…), apply `reject_if_incompatible` (predicate `type_compatible`, skip list unchanged: `Error`/`TypeParam`/`Geometry`/`TraitObject`). (b) `check_expr_struct_ctor_args`'s allowlist (:1598-1608) flips to check **all** named params except bare `TraitObject` (D6). (c) The unfiltered colon-form sub path (`PendingBoundCheck::TraitArgConformance`, `entity.rs:2596`) inherits the walker extension automatically — no second filter exists to flip.

**D2 — Diagnostic taxonomy.**
- **Selector kind-vs-kind** (arg is `Selector(j)`/`AnySelector`, param `Selector(k)`, incompatible): `DiagnosticCode::SelectorKindMismatch` — refining `emit_selector_mismatch`, which today emits `ArgTypeMismatch` for this case. Rationale: task 4581's BT1↔BT6 ruling made `E_SELECTOR_KIND_MISMATCH` the uniform code for wrong-kind selector-vs-selector across composition and param-binding paths; 4371's BT1 asserts it verbatim at a ctor field. Message names expected/found kinds.
- **Pose-vs-set** (param selector-typed, arg `Frame`/`Transform`/`Point`): `ArgTypeMismatch` + fixed hint substring `a coordinate pose is not a region target; select a face/edge/vertex instead` — the deterministic string task 4833's fixtures assert on. (No new code; a message variant, not a new failure class.)
- **All other concrete mismatches** (Int→String, String→selector field, Frame→String, struct-vs-primitive, …): `ArgTypeMismatch`, message shape as landed by 4598: `argument 'f' has type 'X' but param 'f' requires type 'Y'`.
- **Severity:** the staged knob (D4) — `Warning` at α, `Error` at δ. **Span:** anchor at the offending arg's own span where the `CompiledExpr` carries one, falling back to the representative cell span (tactical plumbing, §10).
- New codes minted (ε only: `CtorUnknownField`, `CtorArity`) follow the strum-completeness discipline — run `--lib` tests + the asymmetry scan in the same diff.

**D3 — Coercion policy: what stays LEGAL** (normative table in §6 C1). Headline entries: `Int` → dimensionless `Scalar`; the Scalar/Tensor/Vector/Matrix implicit conversions; `Selector(k)` → `List<Geometry>` (one-way, 4117β); `Selector(k)` → `AnySelector` param (one-way, 4369); **bare `T` → `Option<T>`** (implicit-Some — new walker rule; legality proven by probe 3/5 and `examples/fea_pressure_smoke.ri`); `none` → `Option<T>`; trait-conformant `StructureRef` → trait param (4444/4584 machinery unchanged); Vector loose-quantity + strict arity (4622). Newly **illegal**: implicit `String` → any selector-typed field (Leo: "disallow string" — callers move to typed ctors `vertex(b,"tip")`/`face(b,"x_max")`), pose values at selector fields, and every other concrete `type_compatible` failure. No change to `type_compat.rs` rules is required or permitted by this PRD except where the survey (β) proves an existing legitimate coercion is missing — any such addition is a reviewed γ change with a boundary test.

**D4 — Staged landing: warn → fix-forward → error, via a single const severity knob.** The knob is one `const` in `conformance/mod.rs` (e.g. `CTOR_FIELD_CONFORMANCE_SEVERITY`), flipped by δ — not a config flag, not an env var; the warn window is a landing convenience, never a supported dual mode. Rationale vs atomic: each task lands green through the merge gate; the survey is mechanized by the compiler itself; the atomic alternative concentrates enforcement + an unbounded corpus diff in one branch — the exact integration-starvation shape G5 exists to prevent. No external users exist, so the warn window costs nothing. Bounded window: δ depends only on α/ε/γ — no other work is allowed to enter the gap.

**D5 — Lenient behaviors at the ctor:**
- **Unknown-named + over-arity `__arg{i}` acceptance (`expr.rs:2769-2786`): tightened to Error in the same flag-day** — severable task ε (Leo may strike it in review without touching the rest). Rationale: the same silently-accepted-author-error class (a typo'd field name is exactly a field-type bug the author can't see); one corpus break beats two. New codes `E_CTOR_UNKNOWN_FIELD` / `E_CTOR_ARITY`; ε follows the same warn→fix→flip staging (its warnings enter the β survey; δ flips both severities).
- **Empty-collection push-down short-circuit (`expr.rs:2469`): unchanged.** Ctor args still compile with no expected type; the walker must **skip, never reject,** unresolved element types (the existing `TypeParam` skip covers it — boundary test #10). Typed enforcement of empty-collection ctor args arrives when `docs/prds/expected-type-pushdown-return-field.md` (stub) extends the expected-type channel to struct-field-init positions — that PRD's seam, not this one's (§9).

**D6 — Bare `TraitObject` params stay exempt at ctor sites** (4444's deliberate escape hatch — `entities_phase.rs:1560-1562` names the regression risk for `ConstitutiveLawInput`-class call sites). Wrapped trait params (`Option<TraitObject>`) become checked — consistent with the already-enforced `List<TraitObject>` (4444). Breadcrumb comment at the exemption site names this PRD and the revisit condition (escape-hatch call sites migrated).

**D7 — Eval stays permissive.** The eval-time ctor keeps verbatim field insertion (`Undef`-tolerant per SIR design). Every user path compiles first, so the compile-time chokepoint is the single enforcement point; no runtime double-check, no drift between two checkers (INV-5).

**D8 — Param defaults conform via the same predicate.** `check_param_default_conformance` (4584: `StructureRef` + `Geometry` only) extends to all concrete types under the same staging — `param label : String = 42` errors at δ.

**D9 — Fix-forward rules (γ).** For **FEA** structure defs (`fea_multi_case.ri`, `fea.ri`, `solver_*.ri`, …): call-site changes ONLY, conforming to *current* declared types — field-type flips (incl. `PointLoad.point`/`FixedSupport.target` String→selector, the Bmig2 gap in 4371's RCA) remain v0.6-owned. For **non-FEA** defs: per-case judgment — fix the call site or the declared field type, whichever is the actual bug — with each choice recorded in the survey artifact and eval behavior preserved.

**D10 — Every ctor-bearing context is covered by the two existing entries.** Expression-position ctors (value-cell lets, `sub x = Ctor(...)`, constraints, realizations, ports, guards, forall bodies, fn + assoc-fn bodies) route through `check_expr_struct_ctor_args` (:1344 walk); colon-form sub args route through `PendingBoundCheck::TraitArgConformance`. α adds a per-context coverage fixture for each form that parses (§10 Q4).

## 5. Pre-conditions

None unlanded. All substrate verified on `main` @ `33d9bb3e21`: 4584 chokepoint (done, merged `9c6a5a80e2`), 4598 selector arms, 4622 vector arm, 4368/4369 selector type resolution (`type_resolution.rs:556-567`), `type_compat.rs` rules (:220-281), diagnostic infra (`reify-core/src/diagnostics.rs`, `Diagnostic::warning` :3836). No novel grammar (G3 grammar gate N/A — all fixtures use existing syntax, parsed by the probes above).

## 6. Contract (H) — the conformance predicate

**C0 — Enforcement sites.** Exactly two production entries, both funnelling into `walk_param_against_arg` → `walk_param_against_arg_type`:
1. `check_expr_struct_ctor_args` — every `StructureInstanceCtor` in every template root expr / fn body / assoc-fn body (`entities_phase.rs:1344`).
2. `PendingBoundCheck::TraitArgConformance` executor (`conformance/mod.rs:261`) — colon-form sub-component args.

No third site may re-implement the predicate (INV-5).

**C1 — Legality table** (normative; each row carries a boundary test):

| # | Param declares | Arg supplies | Verdict | Basis |
|---|---|---|---|---|
| 1 | `T` | `T` | legal | identity |
| 2 | dimensionless `Scalar` (`Real`) | `Int` | legal | `type_compat.rs:233` |
| 3 | `Scalar`/`Tensor`/`Vector`/`Matrix` family | per Rules 2a/2b/3 | legal | `implicitly_converts_to` |
| 4 | `List<Geometry>` | `Selector(k)` | legal (one-way) | 4117β |
| 5 | `AnySelector` | `Selector(k)` | legal (one-way) | 4369 |
| 6 | `Option<T>` | bare `T'` where `T'` legal for `T` | legal (implicit-Some) | **new arm**, probe 3/5 |
| 7 | `Option<T>` | `none` | legal | existing |
| 8 | trait / `List<TraitObject>` / `Option<TraitObject>` | conforming `StructureRef` | legal | 4444/4584 |
| 9 | `Vector{n}` | vector-shaped, same arity, any quantity | legal | 4622 |
| 10 | any | arg type `Error`/`TypeParam`/`Geometry`/`TraitObject` | **skip** (never reject) | anti-cascade / undecidable |
| 11 | `Selector(k)` | `Selector(j)`, j≠k, or `AnySelector` | **illegal** — `E_SELECTOR_KIND_MISMATCH` | D2 |
| 12 | selector-typed | `Frame`/`Transform`/`Point` | **illegal** — pose-vs-set hint | D2, 4833 |
| 13 | selector-typed (bare or `Option`-wrapped) | `String` | **illegal** | disallow-string |
| 14 | any concrete leaf | `type_compatible` = false | **illegal** — `E_ARG_TYPE_MISMATCH` | D1 |
| 15 | bare `TraitObject` | anything | exempt at ctor sites | D6 |

**C2 — Invariants.** (i) Anti-cascade: `Type::Error` on either side short-circuits — never a second diagnostic atop an upstream error. (ii) At most one diagnostic per (arg, leaf). (iii) No false positive on unresolved types — skips are skips, not passes to be revisited at eval. (iv) Severity is read from the single knob; codes and messages are severity-invariant. (v) The predicate is `type_compatible` — walker arms may not embed ad-hoc type rules (Vector's landed arity logic is the one grandfathered exception, 4622).

**C3 — Error semantics.** Diagnostics carry: field name, expected type (as declared, wrapper included), found type, span at the offending arg (fallback: cell span). Exit code of `reify check` is non-zero iff any Error-severity diagnostic — unchanged mechanics; δ makes these diagnostics Errors.

## 7. Boundary-test sketch (H) — facing both sides

Producer side = the compiler chokepoint; consumer side = FEA/P4/v0.6 surfaces. Rows 1–9 are the committed probe fixtures; each names its stage-1 (Warning) and stage-3 (Error) expectation.

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| 1 | `PressureLoad(face: frame3(...))` value-cell | silent on main | pose-vs-set hint + `E_ARG_TYPE_MISMATCH`; exit 1 at δ |
| 2 | `Widget(label: 42)` Int→String | silent on main | `E_ARG_TYPE_MISMATCH` |
| 3 | `PressureLoad(face: "x_max")` legacy string | silent on main | `E_ARG_TYPE_MISMATCH` (disallow-string) |
| 4 | `sub p = PressureLoad(face: 42)` | wrapper-shape error on main (post-4370; §2 amendment) | same diagnostic as value-cell context (context-independence, uniform `E_ARG_TYPE_MISMATCH`) |
| 5 | bare selector → `Option<FaceSelector>` (probe 3/5 + `examples/fea_pressure_smoke.ri`) | value-cell: clean on main; sub `=`: wrapper-shape error on main (post-4370; §2 amendment) | **clean in both contexts** (C1.6) |
| 6 | `Holder(loc: edges(b))` kind-vs-kind | `E_ARG_TYPE_MISMATCH` today | `E_SELECTOR_KIND_MISMATCH` naming Face expected / Edge found (4371 BT1 shape) |
| 7 | `Int` literal → `Real` field (`magnitude: 1`) | clean | stays clean (C1.2) |
| 8 | `[]` → `List<T>` field | clean | stays clean (C1.10; ownership: pushdown stub PRD) |
| 9 | `Beam(material: Steel_AISI_1045())` bare trait param | clean | stays clean (D6) |
| 10 | `param label : String = 42` default | silent on main | error at δ (D8) |
| 11 | `Widget(labl: "x")` unknown field | silent `__arg{i}` on main | (ε) `E_CTOR_UNKNOWN_FIELD` |
| 12 | over-arity positional arg | silent `__arg{i}` on main | (ε) `E_CTOR_ARITY` |
| 13 | `Option<TraitObject>` param + non-conforming struct arg | silently skipped on main | `TypeNotConformingToTrait` (consistency with `List<TraitObject>`, 4444) |
| 14 | consumer: 4833's three pose fixtures | blocked | `face` case delivered here (row 1); `point`/`target` become String-mismatch errors here, kind-specific after v0.6 migration |
| 15 | consumer: corpus regression | `--scope all` gate green pre-PRD | gate green after every task (staging discipline) |

## 8. Decomposition plan (G2 signals; filed at decompose time, NOT now)

DAG: α → ε → β → γ → δ → ζ. If Leo strikes ε, β depends on α alone.

- **α — Generalize the chokepoint at Warning severity.** `crates/reify-compiler` (+ `reify-core` diagnostics if message helpers move). Walker Option-unwrap + general-concrete-leaf arms; allowlist flip (D1); selector-code refinement + pose hint (D2); param-default extension (D8); severity knob const = `Warning`; probe fixtures 1–9 + rows 5/7/8/9/13 committed as tests. Any new gate-resident test binary carries its drift-guard registrations **in the same diff** (`.config/nextest.toml` partitions; `tests/infra/run-all-classification.manifest` if a new `tests/infra/test_*.sh`). *Signal (intermediate → unlocks β/γ/δ):* `reify check` on the committed probe fixtures emits the new warnings naming field/expected/found where main is silent today; legality fixtures stay clean; full merge gate green.
- **ε — Unknown-field/over-arity tightening at Warning (severable).** Removes `__arg{i}` leniency semantics behind the same knob; new codes + strum-completeness updates. *Signal (intermediate):* warnings fire on fixtures 11/12; corpus effects flow into β.
- **β — Corpus survey artifact (mechanized).** Run the α(+ε) compiler over all tracked `.ri` (493 files) + the Rust test suite (inline fixtures/goldens); commit `docs/prds/struct-ctor-field-type-conformance.survey.md`: every warning site, classified per D9 (call-site bug / wrong declared type / FEA-deferred-to-v0.6), with the regeneration command. *Signal (intermediate → sizes γ):* committed artifact; site count stated; zero hand-derived entries.
- **γ — Corpus fix-forward.** Apply D9 across every β site (split into γ1/γ2… by area at decompose time if β's count warrants; the split is mechanical once β lands). *Signal:* `reify check` across the corpus emits **zero** ctor-conformance warnings; full `--scope all` suite green.
- **δ — Severity flip Warning→Error + negative fixtures.** One-const flip; committed negative fixtures assert exit-1 + diagnostic on rows 1/2/3 (and 11/12 if ε kept); positive corpus stays green. *Signal (leaf — the PRD headline, G2):* `reify check` on `PressureLoad(face: frame3(...))` / `Widget(label: 42)` / `PressureLoad(face: "x_max")` emits the structured diagnostic and exits non-zero, where pre-PRD main exits 0 silently.
- **ζ — Language-spec + docs.** Spec section for ctor field-type conformance: the C1 legality table, the codes, the implicit-Some rule, the bare-TraitObject exemption; breadcrumbs at the knob + exemption sites naming deferred variants (eval-time checking, empty-coll typing, TraitObject revisit). *Signal (leaf):* the committed spec section; docs checks green. G7 waiver: no-lockstep-duplication — the spec section restates the C1 table as the living normative copy for `.ri` authors; agreement with the implementation is pinned by the committed boundary-test fixtures (rows 1–13), not hand-sync; render-from-source spec infra is out of scope.

G6 bindings (drafted here so decompose only re-checks): every negative signal's rejection mechanism is **observed live** for allowlisted types (probes 6/9, exit 1) and its extension predicate verified in code (`type_compatible(String, Int) = false`, `(Selector(Face), Frame)` no rule → false, etc.); every "stays clean" signal is observed clean on main (probes 3/5, `fea_pressure_smoke.ri` in CI). No numeric bounds anywhere (G6 branches 1/2 N/A). All capabilities live in the task's own diff or upstream tasks — no downstream dependency delivers any asserted capability.

G7 walk (advisory): no detectors/suppressors (no storm-escape need); the contract is machine-checked (fixtures + the compiler itself); no log-scraping; no snapshot-action; D1/D7 explicitly chosen to avoid lock-step duplication. No waivers needed.

## 9. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/fea-load-support-selector-migration.md` (re-gated separately) | consumes | Error-severity chokepoint enforcing migrated selector fields ("disallow string"); FEA field flips + ~159 call sites + Bmig2 (`PointLoad.point`/`FixedSupport.target`) stay THERE | field flips + BT gate (4371): **that PRD**; chokepoint: **this PRD** | that PRD blocked pending its re-gate; sequencing: this PRD lands first |
| `docs/prds/naming-convergence/P4-region-ref-fea-selector-unification.md` (being revised separately) | consumes | pose-vs-set diagnostic (D2 hint string) consumed by task 4833's fixtures; P4's stale §2/§3/§4 D1/D3/D4 corrected in P4's revision, not here | diagnostic: **this PRD**; fixtures/verification: **P4 (4833)** | 4833 blocked on this capability (its dry-run RCA names it) |
| `docs/prds/expected-type-pushdown-return-field.md` (stub) | adjacent-producer | typed expected-type at struct-field-init positions (empty-coll ctor args); this PRD skips-not-rejects unresolved element types (C1.10) | **stub PRD** | queued (triggered after pushdown ε) |
| `docs/prds/v0_6/stdlib-surface-type-substrate.md` | extends | 4584's `walk_param_against_arg_type` StructureRef arm + ctor chokepoint | landed | wired (`9c6a5a80e2`) |
| `docs/prds/topology-selector-value-type.md` | consumes substrate | `Type::Selector`/`AnySelector` resolution (4368/4369) + 4598 ctor arm | landed | wired |

No contested-ownership pair from the audit's known set is touched; no new seam is introduced (this is a compiler diagnostic phase, not an in-engine §3 seam — consumers are the CLI/LSP diagnostic surface + the named tasks).

## 10. Open questions (tactical only)

1. **Per-arg span plumbing.** `check_expr_struct_ctor_args` passes the representative cell span; each compiled arg's own span should anchor the diagnostic when present. Decide the fallback shape in α.
2. **Wrapper-shape mismatch code.** The landed wrapper-shape arm emits `TypeNotConformingToTrait` even for non-trait wrappers (misleading name, harmless behavior). Rename/re-code only if α touches that arm anyway; otherwise leave.
3. **Severity-knob mechanics.** A `const` read at the two emit helpers vs a parameter on `WalkCtx`. Either satisfies C2(iv); decide in α.
4. **Which surface syntaxes reach `PendingBoundCheck::TraitArgConformance`.** ~~`sub x = Ctor(...)` provably routes through the expression path (probe 8)~~ — corrected at decompose time (§2 amendment): the sub `=` form's named args DO reach the pending-bound-check walker path (the wrapper-shape rejections on probes 4/5 prove it); the colon-form's arg-bearing variant needs a parse-confirmed fixture (probe attempt `sub w : Widget2(label: 42)` did not parse). α enumerates the parsing forms, covers each, and pins which path(s) fire per form (double-emission risk: if both entries reach the walker for the same arg, C2(ii) at-most-one-diagnostic must still hold).
5. **γ split.** Whether γ ships as one task or γ1/γ2… by corpus area — decided from β's site count at decompose time.
6. **Survey artifact format.** Flat list vs grouped-by-def; regeneration command shape. Decide in β.

## 11. Out of scope

- FEA selector field-type flips, the ~159 FEA call-site migration, Bmig2 (`PointLoad.point`/`FixedSupport.target`), and "truly consume" solver work — **revised v0.6 PRD**.
- Pose-vs-set guard *verification* fixtures + P4 stale-text corrections — **P4 revision / task 4833** (the diagnostic they rely on is delivered here).
- Typed enforcement of empty-collection ctor args — **expected-type-pushdown-return-field.md** (stub).
- Bare-`TraitObject` ctor params (D6 exemption) — revisit gated on escape-hatch call-site migration.
- Eval-time type checking (D7) and any `type_compat.rs` rule changes beyond survey-proven gaps (D3).
- Fn-call argument checking — already enforced via overload resolution + 4081/4232 conformance walks; untouched.
