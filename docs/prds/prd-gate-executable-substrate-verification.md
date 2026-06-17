# PRD — Make `/prd`'s G3+G6 gates *executable* (capability-manifest probe runner + decompose-time verification workflow)

> **Class:** dev-process infra/tooling (not reify-language work). Sibling of `docs/prds/test-run-concurrency-semaphore.md`, `docs/prds/warmer-builds-merge-verify.md`, `docs/prds/reify-audit-ptodo-detector.md`. The only reify-codebase artifacts are a checker tool + a regression-corpus of fixtures; the rest is gate prose + a decompose-phase workflow.
>
> **Approach:** B+H (G5). High stakes — touches the grammar/parser gate (a named load-bearing seam), spans ≥2 repos, has ≥4 components, and **every future `/prd` session is a consumer**. §9 is the H contract; §10 is the two-way boundary-test sketch.

## 1. Consumer & user-observable surface (G1)

This PRD introduces no orphan producer. Every mechanism has a named consumer:

- **D1 — the probe runner** is consumed by **D3** (the decompose workflow runs it per premise) and by **D4** (the orchestrator dispatch hook re-runs the committed probe set). It is also directly runnable by a human at a shell.
- **D3 — the decompose-phase verification workflow** is consumed by **`/prd` decompose mode** (the reify overlay's decompose step invokes it) and by **the decompose-time author**, who reads `FAIL` + captured command output instead of discovering a false premise at task dispatch.
- **D4 — the dispatch-time substrate re-diff** is consumed by **the orchestrator dispatch path** (dark-factory). Its reify-side input is D1's committed-probe-set format.

**User-observable surface:** when `/prd` decomposes a PRD, a deterministic phase fans out per leaf, authors a probe for every premise the leaf signal asserts, **runs it**, and blocks the batch on any `FAIL`/unproven/falsified premise **with the captured command output attached** — instead of tabulating an unexecuted promise. A regression corpus of the historical false premises (4575/4577/4437/4358/4497/3979 + the 4352/4375 producer-extent cases) is committed as fixtures and a CI gate asserts the checker **FAILs each** ("would-have-caught-it" corpus).

## 2. The problem (motivation)

The capability-manifest gate (landed **2026-05-30**) was meant to convert G3/G6 from *"the author promises"* to *"the toolchain proves."* In practice it only converts free-text promises into **tabulated** promises that **nothing executes**. Eight orchestrator escalations are the *same* failure — a leaf's load-bearing premise was false or unowned, discovered only at execution (one agent spin-up + L2 escalation + planner amendment each). **For 6 of 8 the manifest ran and recorded PASS** for the exact capability that later blocked.

| Task | PRD | False/unowned premise | Manifest said |
|---|---|---|---|
| 4575 | stdlib-surface-type-substrate | "`revolute(axis: 1.0)` is rejected" — compiler does **no** arg-vs-param nominal rejection for concrete params | V = all PASS; never bound the rejection capability |
| 4577 | stdlib-surface-type-substrate | bare `Transform3` type-name resolves — affine-map shipped Type/Value/builtins, **not** `resolve_type_name` | P = "affine-map owns Transform3 → PASS producer-upstream" (by name) |
| 4437 | auto-type-param-completion | member access on a `TypeParam`-typed param → **poison literal**, not `ValueRef`; capability unowned by α–ε | ζ = "producers α–ε upstream → PASS"; summary falsely claims "no branch-3 misattribution" |
| 4375 | real-dimensionless-unification | prereq δ/4373 migrated only `param: Scalar`, not `-> Scalar` codomain → 25 violators | γ = "downstream of δ ✓ (DAG-direction)"; never checked extent |
| 4352 | (trig return-sigs) | `Type::Real` exists — sibling 4373 **deleted** the variant (merged 06-12) | G4 caught the 4234 seam, **missed** 4352 |
| 4358 | geometric-relations | constraint IR shape is `IndexAccess` — real shape is `CrossSubGeometryRef \| scoped-ValueRef` | false premise frozen in architect plan.json, below the manifest |
| 4497 | ambient-default-material | purpose-nested `default Material` can reach structures — **grammar forbids structures in purposes** | D = "purpose-nested `default Material=` parse; corpus green" (fragment parsed; scenario unreachable) |
| 3979 | result-and-fallback | `map_or`'s `f:(T)->U` param declarable — **no arrow-type grammar production** | **no manifest** — PRD predates the gate (05-27 < 05-30) |

*(3979 predates the gate; it is evidence the gate *helps*, not that it failed — it is excluded from "the gate failed" but included in the corpus because the gate would now catch it.)*

**Four structural weaknesses the manifest leaves open:**

- **W1 — Enumeration is bounded by author recall.** The failing premise is, by construction, the one not listed. Worst for **negative/RED assertions** ("X is rejected"): 4575's manifest never bound "does a rejection mechanism fire on `revolute(axis:1.0)`?" and logged the *contradicting* silent-accept evidence as test *motivation*.
- **W2 — Producer bindings verify identity, not deliverable extent.** `producer:task-N upstream` passes on a name match. 4577 ("owns Transform3" is true for type/value/builtins, false for type-name resolution); 4375 (δ's own completeness grep `': Scalar[^<a-zA-Z]'` is structurally blind to `-> Scalar`; the manifest checked DAG-direction, not extent).
- **W3 — Gates run once at author time; substrate drifts before dispatch.** 4352 assumed `Type::Real`; sibling 4373 deleted it; no re-verify at dispatch.
- **W4 — Grammar gate parses fragments in isolation, not composed scenarios.** 4497's `default Material=` fragment parsed; the *scenario* (structures nested in a purpose so the override can reach them) is ungrammatical.

## 3. Premise correction — what actually exists (empirically verified 2026-06-14)

This PRD's own G3/G6 premise is *"`reify check` and `tree-sitter parse` can observe these behaviors as a probe vector."* Verified by running the probes (commands + captured output below — these are the manifest evidence and the seed of the D-corpus):

| Probe | Fixture | Result | What it proves |
|---|---|---|---|
| `reify check` — arg-vs-param (4575 class) | `revolute("not-an-axis", 0rad..1rad, point3(0mm,0mm,0mm))` | **exit 0**, `All constraints satisfied.`, *no* rejection diagnostic | The compiler does **no** nominal arg-vs-param rejection for concrete params (premise false). `check` **observes the absence** — the sound vector for the G6 negative-assertion branch. |
| `reify check` — type-name resolution (4577 class) | `param t : Transform3` | **exit 1**, `error: unresolved type: Transform3` | `check` **observes** type-name resolution with a real diagnostic + nonzero exit. A probe asserting "`Transform3` resolves" correctly FAILs. |
| `tree-sitter parse --quiet` — grammar OK | `param x : Length = 5mm; let y = box(...)` | **exit 0**, 0 ERROR nodes | Grammar probe vector, fast path. |
| `tree-sitter parse --quiet` — arrow-type (3979 class) | `param f : (Length) -> Length` | **exit 1**, `(ERROR [1,12]-[1,32])` printed even under `--quiet`, **0.07 ms** | No arrow-type grammar production (premise false). The grammar gate catches it; sub-millisecond. |

**Three of the four semantic behaviors are reachable through `reify check`** (4575 silent-accept, 4577 type-name, and 4437 member-access lowering, which surfaces as a poison-literal / diagnostic at check time). **The fourth — 4358 constraint-IR shape — is NOT reachable through `check`** (`check` reports constraint satisfaction, not IR node kinds). This confirms the brief's flag: D1 must carry a **third probe vector — a targeted eval/IR probe** — alongside `tree-sitter parse` and `reify check`.

**Dogfood correction (the manifest catching a name-match-PASS on *this* batch).** When binding the `ir` vector in this PRD's own manifest, the naive binding "a `reify dev inspect-ir`/`dump-ir` surface exposes the compiled-expr node kind" resolves to **producer-absent**: `reify eval` prints *values* (`EvalSmoke.b = 0.005 m`), not IR kinds, and `grep` finds **no** `dev`/`inspect-ir`/`dump-ir`/`--ir` CLI subcommand. Asserting that vector "exists" by name is exactly the W2 failure. The honest **rewrite-to-existing-capability** resolution: `CompiledExprKind::CrossSubGeometryRef` **panics in `eval_expr`** (`crates/reify-compiler/src/expr.rs:374` — *"`CrossSubGeometryRef` would panic in `eval_expr`"*; it is a leaf consumed by `entity.rs` before eval, `crates/reify-eval/src/engine_purposes.rs:981–987`). So the 4358 IR-shape premise is observable **via an eval-error proxy** — author the scenario, run `reify eval`, observe the characteristic error/panic-signature that betrays `CrossSubGeometryRef` vs the assumed `IndexAccess`. The `ir` probe kind is therefore an **eval-error-signature probe today** (no IR-dump CLI); α MAY add a thin `CompiledExprKind` debug-print for a cleaner, less brittle vector (§12, tactical — a tooling print, not a language change). This is the one premise validated *during authoring* and threaded into the D1 contract (§9).

**Negative-assertion soundness (G6(b)):** observing the *absence* of an error is sound because `reify check` is deterministic — `revolute("not-an-axis", …)` yields exit 0 + the explicit `All constraints satisfied.` line on every run. "Expected a diagnostic, observed none" is a well-defined `FAIL`, not a flake.

## 4. Sketch of approach

Four interlocking deliverables; **keystone first.**

### D1 — Executable capability checker (the "probe runner") — KEYSTONE
A deterministic harness: input `(capability, probe, expected-observation)`, run the probe, return `PASS | FAIL | UNPROVABLE` **with captured command output attached**. Probe vocabulary (all three vectors empirically grounded in §3):
- `tree-sitter parse --quiet <fixture.ri>` — grammar premises; run on the **composed scenario**, not the minimal fragment (closes W4).
- `reify check <fixture.ri>` — semantic/behavioral premises (arg-vs-param rejection, type-name resolution, member-access lowering). This is what **broadens reify's G3 past grammar** and makes 4575/4577/4437 *observable*.
- a **targeted eval/IR probe** where `check` is insufficient (4358 IR-shape) — §3 confirms this vector is required.

Lives in the **reify** repo (it needs the reify toolchain): `scripts/prd-capability-check.*` or a small bin. Owns the **committed-probe-set format** (a serializable `(capability, probe-kind, fixture, expected-observation)` record) that D3 emits and D4 consumes. Reused by D3 **and** D4.

### D2 — Gate-prose strengthening (split by repo ownership — see §8)
- **D2a (dark-factory, cross-project):** the generic `gates.md` gains a **G6 negative-assertion branch** — any "X is rejected / errors / fails to compile" signal MUST bind "the rejection mechanism exists and fires on X" (probe: author X, run `check`, observe NO diagnostic → capability absent → FAIL). Tighten the Capability-Manifest `Capability→producer` row so a `producer:task-N` binding must verify the producer's **deliverable extent** covers the consumed sub-capability, not just task identity.
- **D2b (reify overlay, this batch's β):** broaden the overlay's *"G3 — grammar gate"* → *"G3 — substrate verifier (grammar **and** semantic/behavioral)"*; name the `reify check` probes and the semantic-substrate examples (arg-vs-param rejection, `resolve_type_name`, lowering-to-`ValueRef`, IR shapes); keep the grammar gate as **one verifier among several**. Wire the D3 workflow into the overlay's decompose step. The overlay is reify-tracked, so this is the **operative** reify-side strengthening — it makes reify's `/prd` enforce the new rule even if D2a's upstreaming lags.

### D3 — Decompose-phase verification workflow
A `Workflow` script run as a deterministic phase of `/prd` decompose, fanning out **per leaf** with three roles:
1. **Enumerator** — extract every premise the leaf signal asserts; enforce the negative-assertion mandate (every "X is rejected" must become a probe).
2. **Prover** — for each premise, author a probe and run it through D1; **PASS only on observed evidence**; for producer-extent premises, probe the consumer's *full extent* against the producer's *actual deliverable* (closes W2).
3. **Adversary** (independent, parallel lens) — hunt for *unlisted* premises + *falsifications* of listed ones; author and run its own probes.

Synthesize → any `FAIL`/unproven/falsified premise blocks the batch **with captured output**. A deterministic harness over stochastic agents: the agents *find and author* probes; D1 *adjudicates*.

### D4 — Dispatch-time substrate re-diff — CROSS-PROJECT SEAM (not a reify leaf)
When a task dispatches, the orchestrator re-runs its committed probe set (D1) against current `main`; a PASS→FAIL flip blocks **before** agent spin-up (closes W3/4352). This is an **orchestrator (dark-factory) seam**, filed as a cross-project task, **not** owned as a reify leaf (§8). The reify side owns only the committed-probe-set format the hook consumes (delivered by D1/α).

## 5. Pre-conditions (substrate — all verified present 2026-06-14)

- **`reify check`** — exists (`reify check <file>`); emits diagnostics to stderr with deterministic exit codes (0 = no error / constraints satisfied, 1 = diagnostic emitted). Empirically observes 3 of the 4 semantic behaviors (§3).
- **`tree-sitter parse --quiet`** — exists (`tree-sitter-reify/`); exit 0 / 1 on 0 / ≥1 ERROR nodes; sub-millisecond (§3).
- **The `Workflow` tool** — exists; D3 is a `Workflow` script. Deterministic control flow with per-leaf fan-out is exactly its shape.
- **A built reify worktree** — the checker runs where `target/{debug,release}/reify` and `tree-sitter` are on PATH (the standard task-agent environment).
- **Empty-value / silent-accept sentinels** — the reify overlay already names `Value::Undef` (field-population) and now adds the silent-accept sentinel (exit 0 + no diagnostic where a diagnostic was asserted).

## 6. Resolved design decisions

1. **`reify check` is the semantic probe vector — not a new compiler API.** §3 proves it surfaces arg-vs-param, type-name, and member-access behaviors at the CLI. D1 wraps the existing CLI; it does **not** add inspection endpoints to the compiler. (Keeps D1 in `scripts/`, off the reify-language critical path.)
2. **Three probe kinds, not one.** `tree-sitter parse`, `reify check`, and a targeted eval/IR probe. 4358 (§3) proves a grammar-or-check-only checker is insufficient; the IR vector is in the contract from day one. The IR vector is an **eval-error-signature probe** (no IR-dump CLI exists — §3 dogfood correction), optionally sharpened by a thin `CompiledExprKind` debug-print (§12).
3. **Three verdicts, not two.** `UNPROVABLE` is distinct from `FAIL`: it means "no probe vector can currently observe this behavior" (e.g. a premise about an internal that no CLI surface exposes). `UNPROVABLE` blocks the batch the same as `FAIL` (an unobservable premise is as dangerous as a false one) but routes to a different resolution — "find a vector or move the signal" rather than "the premise is false."
4. **Captured output is mandatory on every verdict.** The manifest's failure was tabulating verdicts with no evidence. Every D1 result carries the exact command + stdout/stderr + exit code, so a human (or D4) can re-derive the verdict without re-running.
5. **The Adversary is net-positive recall, not a proof.** It widens enumeration past author recall (closes W1); it is explicitly **not** exhaustive. The PRD says so and the contract does not claim completeness.
6. **D2 splits by repo.** The generic `gates.md` lives in the dark-factory repo (`/home/leo/src/dark-factory/skills/prd/references/gates.md`); a reify worktree cannot land an edit there through the sanctioned path. So D2a is a **cross-project** deliverable and D2b (the reify overlay, β) carries the operative reify-side strengthening. Reify's `/prd` reads both, so β alone makes reify enforce the new rule.
7. **The integration gate is a separate critical leaf (δ).** The "would-have-caught-it" corpus is the G2 integration-gate signal *and* the producer-side boundary test (§10). It depends on the keystone (α) and proves the harness catches the real historical failures — distinct from γ, which proves the *workflow* blocks on a FAIL.
8. **Composed-scenario fixtures (W4).** Grammar probes target the *composed* scenario, not the minimal fragment. The 4497 corpus fixture is the full purpose-with-nested-structure scenario, which is ungrammatical — not the bare `default Material=` line, which parses.

## 7. Out of scope / non-goals

- **The dark-factory orchestrator hook (D4) is not an owned reify leaf** — cross-project dependency (§8).
- **The Adversary is not made exhaustive** — net-positive recall only (decision 5).
- **No reify-language changes.** D1 does not add a new arg-vs-param rejection mechanism, a `resolve_type_name` for `Transform3`, or an arrow-type grammar production — it *observes* their current absence so a PRD asserting them FAILs. Building those capabilities is the owning PRDs' job. (A thin read-only `CompiledExprKind` debug-print for the `ir` vector, if α adds one, is **tooling**, not a language/semantics change — explicitly in scope.)
- **Not a replacement for the manifest** — the manifest stays; D1 makes its bindings *executable*. The manifest is still the committed artifact; D1 + D3 are what *run* it.
- **No retroactive re-verification of already-landed PRDs** — the corpus reproduces the historical premises as a regression gate, but does not re-decompose old PRDs.

## 8. Cross-PRD / cross-repo relationship & seam ownership (G4)

| Deliverable | Artifact | Repo / owner | Mechanism |
|---|---|---|---|
| **D1** (α) | `scripts/prd-capability-check.*` + committed-probe-set format | **reify** | reify leaf (this batch, keystone) |
| **D2b** (β) | `.claude/skills/prd/project.md` overlay broadening + D3 wire-in | **reify** | reify leaf (this batch) — reify-tracked file |
| **D3** (γ) | `Workflow` script (Enumerator/Prover/Adversary) | **reify** | reify leaf (this batch) |
| **D-corpus** (δ) | historical-false-premise fixtures + CI gate | **reify** | reify leaf (this batch, critical integration gate) |
| **D2a** | generic `gates.md` G6 negative-assertion branch + producer-extent tightening | **dark-factory** | cross-project task — `gates.md` is in the dark-factory repo; β is the reify-side mirror so reify is not blocked on it |
| **D4** | orchestrator dispatch-time re-diff hook | **dark-factory** | cross-project task `dark_factory:NNNN`; depends cross-project on reify-α's committed-probe-set format |

**No reciprocal-ownership ("the other owns it") inversion.** The two dark-factory seams (D2a, D4) consume reify artifacts (β mirrors D2a's rule; D4 consumes α's format) but neither blocks a reify leaf — they are downstream/parallel. **Hand-back:** Leo files the two `dark_factory:` tasks (D2a prose; D4 hook) and wires D4→reify-α cross-project; this PRD does not file them (per the brief, D4 is not folded into the reify decomposition; D2a cannot be landed from a reify worktree).

## 9. Contract (H) — the D1 probe-runner contract

**Input record (one per premise — the committed-probe-set format):**
```
{ capability:   <free-text capability the leaf signal asserts>,
  probe_kind:   "grammar" | "check" | "ir",
  fixture:      <path to a throwaway .ri fixture under a probe-fixture dir>,
  expected:     { observation: "present" | "absent",
                  match: <exit-code and/or stderr-substring predicate> } }
```

**Probe-kind semantics (grounded in §3):**
- `grammar` → run `tree-sitter parse --quiet <fixture>`. `present` ⇔ exit 0 (0 ERROR nodes); `absent` ⇔ exit 1 (≥1 ERROR node).
- `check` → run `reify check <fixture>`. The predicate names the diagnostic. `present` ⇔ the named diagnostic appears (typically exit 1); `absent` ⇔ it does not (typically exit 0 + `All constraints satisfied.`). **The negative-assertion branch lives here:** a "X is rejected" signal binds `probe_kind:check, expected:{observation:"present", match:<the rejection diagnostic>}`; if `check` exits 0 with no diagnostic, observed=absent ≠ expected=present → **FAIL**.
- `ir` → run the targeted eval/IR probe and match against an IR-shape predicate. For premises `check` cannot reach (4358). **No IR-dump CLI exists today (§3 dogfood correction)** — the vector is an **eval-error-signature probe**: run `reify eval <fixture>` and match the characteristic error/panic the IR shape produces (`CrossSubGeometryRef` panics in `eval_expr`). α MAY add a thin `CompiledExprKind` debug-print as a cleaner vector (§12).

**Verdicts:**
- `PASS` — observed matches expected. Carries the command + captured stdout/stderr + exit code.
- `FAIL` — observed contradicts expected. Carries the same captured output (the evidence that the premise is false). Blocks the batch.
- `UNPROVABLE` — no probe kind can observe the behavior (the predicate references an internal no surface exposes). Blocks the batch; routes to "find a vector or move the signal," not "premise is false."

**Exit codes (the harness's own):** `0` = all probes PASS; `1` = ≥1 FAIL; `2` = ≥1 UNPROVABLE (and 0 FAIL); reserved `>2` for harness/usage errors. Deterministic: same probe set + same `main` ⇒ same verdicts.

**Fixture location:** throwaway `.ri` fixtures under a dedicated probe-fixture dir (committed alongside the probe set for the corpus; ephemeral under `/tmp/prd-gate-fixtures/` for ad-hoc decompose-time probes). Committed probe sets reference committed fixtures so D4 can re-run them at dispatch.

## 10. Boundary-test sketch (H — facing both ways)

**Producer side (δ — the "would-have-caught-it" corpus, CI-gated):** each historical false premise is a committed probe-set record + composed-scenario fixture; running D1 over the corpus yields the verdict that *would have blocked the original task*:

| Corpus case | probe_kind | expected | D1 must return |
|---|---|---|---|
| 4575 (arg-vs-param rejection) | check | observation=present (rejection diag) | **FAIL** (observed: exit 0, no diag — §3) |
| 4577 (`Transform3` resolves) | check | observation=present (type resolves) | **FAIL** (observed: `unresolved type: Transform3` — §3) |
| 4437 (member-access → ValueRef) | check | observation=present (no poison) | **FAIL** (poison literal / diagnostic observed) |
| 4358 (IR shape = IndexAccess) | ir (eval-error proxy) | observation=present (IndexAccess) | **FAIL** (observed: `CrossSubGeometryRef` eval-error signature, expr.rs:374) |
| 4497 (purpose-nested structure) | grammar | observation=present (parses) | **FAIL** (composed scenario ERRORs — W4) |
| 3979 (arrow-type param) | grammar | observation=present (parses) | **FAIL** (observed: ERROR node — §3) |
| 4375 (`-> Scalar` extent) | check | producer-extent covers codomain | **FAIL** (extent grep blind to `-> Scalar` — W2) |
| 4352 (`Type::Real` exists) | check/ir | observation=present | **FAIL** at dispatch re-diff (variant deleted — W3) |

The corpus CI gate asserts **D1 returns FAIL/UNPROVABLE for every row**. A row that flips to PASS means either the substrate changed (a real capability now exists — update the corpus) or the checker regressed (the gate fires).

**Consumer side (γ + D4):**
- **γ (decompose):** given a leaf whose signal carries a false premise, the workflow's Prover/Adversary author a probe, D1 returns FAIL, and the decompose batch **blocks with the captured output** — the consumer-side proof that the workflow honors D1's verdict.
- **D4 (dispatch, cross-project):** given a committed probe set that PASSed at author time, a PASS→FAIL flip on current `main` blocks dispatch before agent spin-up (the 4352 drift case). Reify-side: the committed-probe-set format is re-runnable; orchestrator-side hook is dark-factory's.

## 11. Decomposition plan (one bullet per leaf → its observable signal) (G2)

- **α — D1 probe runner + contract + committed-probe-set format (KEYSTONE).** `scripts/prd-capability-check.*` accepts a probe-set record, dispatches the three probe kinds (`grammar`/`check`/`ir`), and returns `PASS`/`FAIL`/`UNPROVABLE` with captured command output + deterministic harness exit codes (0/1/2). **Signal:** running the harness on a 3-record probe set (one per kind) emits the correct verdict + captured output for each; golden tests assert one PASS, one FAIL (the §3 4575 silent-accept case), and one UNPROVABLE; the committed-probe-set format round-trips. *Deps: none.*
- **β — D2b overlay broadening + D3 wire-in.** Rewrite the overlay's "G3 — grammar gate" → "G3 — substrate verifier (grammar **and** semantic/behavioral)", naming the `reify check` probes and the four semantic-substrate examples (arg-vs-param, `resolve_type_name`, member-access lowering, IR shape); add a decompose-mode instruction to run the D3 workflow. **Signal:** `.claude/skills/prd/project.md` contains the broadened G3 section naming the check-probes + the decompose-step invoking the D3 workflow by path; a `tree-sitter`/grep assertion (or a doc-lint) confirms the section + the four examples are present. *Deps: γ (the overlay names a real workflow script).*
- **γ — D3 decompose-phase verification workflow.** A `Workflow` script with Enumerator/Prover/Adversary roles that fans out per leaf, authors probes, runs them through α, and synthesizes a batch verdict that blocks on any FAIL/unproven/falsified premise with captured output. **Signal:** running the workflow over a leaf with a known-false premise (the §3 4575 silent-accept fixture) returns a blocking verdict with the captured `reify check` exit-0/no-diagnostic output; running it over an all-true-premise leaf passes. *Deps: α.*
- **δ — Integration gate: historical-false-premise regression corpus (CRITICAL).** Commit the 8 historical premises (4575/4577/4437/4358/4375/4352/4497/3979) as probe-set records + composed-scenario fixtures, and a CI gate that asserts α returns **FAIL/UNPROVABLE for every row** (§10 producer-side table). **Signal:** the corpus CI gate runs `prd-capability-check` over all 8 records and asserts each is FAIL/UNPROVABLE with the documented captured-output evidence; the gate is wired into the reify test suite (`tests/infra/run_all.sh` or equivalent) so it runs in CI. *Deps: α.*

**DAG:** `α` (keystone, no deps) → `γ` → `β`; `α` → `δ`. β and δ are the two consumer leaves; δ is the critical integration gate.

## 12. Open (tactical) questions

- **Probe-set serialization format** — JSON vs a small TOML/`.ri`-adjacent record. Tactical; α picks one and δ/D4 follow. (JSON is the obvious default for round-trip + diff.)
- **`ir` probe surface** — §3 establishes the floor: no IR-dump CLI exists, so the default vector is the **eval-error signature** (`reify eval` → `CrossSubGeometryRef` panic, expr.rs:374). Tactical residual: whether α adds a thin `CompiledExprKind` debug-print for a less brittle, signature-independent vector. α picks the lightest vector that distinguishes `CrossSubGeometryRef` from `IndexAccess`; a tooling debug-print is in scope (it is not a language change).
- **Where the D3 workflow script lives** — `.claude/workflows/` vs `scripts/`. Tactical; β references whichever path γ commits.
- **Corpus fixture provenance** — reproduce each historical premise from its escalation/PRD verbatim, or distill the minimal composed scenario. Tactical; δ distills to the minimal *composed* (not fragment) scenario per W4.
