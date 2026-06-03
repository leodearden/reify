# Undef Self-Describing (traceable `undef` — spec §9.2.9)

- **Milestone:** v0.6 (spec gap-fill; `reify-language-spec.md` §9.2.9 "Tracing" is an unconditional promise that is wholly unbuilt).
- **Status:** contract — authored 2026-06-03 in an interactive `/prd` session under G1–G6 + META. Decompose batch `undef-self-describing-2026-06-03`, cluster `undef-self-describing`.
- **Approach:** **B + H** (contracts + two-way boundary tests). Blast radius ≈ 5 surfaces (`reify-ir`, `reify-eval`, `reify-expr`, `reify-cli`, plus GUI `src-tauri` + `reify-lsp`). Touches one load-bearing seam — the **eval cell-compute boundary** (every cell value flows through it), so the central correctness property is a *transparency* invariant (capture must not perturb any value / determinacy / content-hash / cache outcome).
- **Source:** `docs/reify-language-spec.md` §9.2.9 (lines 1875–1877). Not in a gap-register row — surfaced fresh; confirmed no existing PRD covers it (the sibling `determinacy-intrinsics-completion.md` owns the *whether*-determined predicates, **not** the *why*).

## §0 — Purpose and scope

When a Reify value is `undef`, the user is told *that* it is undefined (the literal text `undef`, plus a per-cell `DeterminacyState` of `Undetermined`/`Auto`) but **never why**. The "why" already exists in the system — `Vec<Diagnostic>` (code + message + span) rides in `ComputeOutcome` — but it is **decoupled** from the specific undef cell: nothing binds "param `thickness` is undef" to "because `outer_d` is unbound." The spec already mandates closing this gap, and scopes it:

> **§9.2.9 Tracing** — "Tooling should make it easy to trace why a value is `undef` — which upstream parameter's undetermined state is responsible. **This is an implementation concern, not a language semantics concern.**"

This PRD builds that tooling. The decisive framing consequence of §9.2.9 is that **`undef`'s language semantics do not change**: two `undef`s remain interchangeable, all §9.2 propagation rules are untouched, value identity / content-hash / cache-key stability are untouched. The reason is *metadata for tooling*, never part of the value.

**Design decisions (resolved this session, see §4).**
- **Cell + tracer, not a value payload.** A reason is captured **per-cell** (a parallel side-map keyed by `ValueCellId`, alongside the existing `(Value, DeterminacyState)` snapshot map — **not** widening that tuple, **not** a payload on `Value::Undef`). A read-only **tracer** walks the existing cell dependency edges to reconstruct the cause chain. This is spec-faithful (no value-model change → no content-addressing risk) and has the smallest, most concentrated blast radius.
- **All causes, not a first match.** The tracer returns the **complete, deduplicated set** of root causes reachable through `undef`-valued dependency edges and local op-failure sinks — never a single first-found reason. (`c = a + b` with both unbound ⇒ reports **both**; `x = a + sqrt(-1mm)` ⇒ reports the unbound `a` **and** the local domain failure.)
- **Surfaces are tooling only.** CLI, GUI hover/param-panel, LSP hover. The in-language `why(x)` intrinsic was **explicitly rejected** — it would leak implementation-defined reason text into language semantics, contradicting §9.2.9.

**User-observable end state:** an engineer evaluating a partially-specified design sees, for each undef that blocks a requested output/constraint, the *complete set of upstream root causes* — in `reify check`/`eval` output, on GUI hover over the parameter, and on LSP hover in the editor — e.g. `thickness = undef (because: outer_d unbound, wall_ratio unbound)`.

## §1 — Spec / doc grounding

- **`docs/reify-language-spec.md` §9.2.9** (the promise this PRD implements) and §9.2.1–9.2.8 (the propagation rules that must remain byte-for-byte unchanged — the transparency invariant, §4.1).
- **`docs/reify-language-spec.md` §9.2** — "a design with `undef` parameters is a legitimate, partially-specified design." ⇒ the surface must **not** spam a note for every undef (noise gating, §4.4 / Open Q2).
- **`docs/reify-stdlib-reference.md` §12 `std.determinacy`** — the *whether*-determined predicates (`determined`/`undetermined`/`constrained`); this PRD is the *why* complement and reuses `DeterminacyState` read-only to classify origin kind (§4.1).

## §2 — Pre-conditions for activating (substrate verified, G3)

All substrate verified present on main this session:

- **Cell dependency graph is built and walkable.** `DependencyMap { forward: HashMap<ValueCellId, Vec<ValueCellId>>, reverse: … }` (`reify-eval/src/deps.rs:1329–1379`) and `ReverseDependencyIndex` (`deps.rs:67–117`). The tracer walks `forward` from an undef cell to its input cells.
- **Expression → referenced cells.** `CompiledExpr::collect_value_refs() -> Vec<ValueCellId>` (`reify-ir/src/expr.rs:823–958`); higher-level `extract_value_deps` (`deps.rs:1184–1190`). Used for the leaf-op attribution.
- **Per-cell `(Value, DeterminacyState)`.** Snapshot map `PersistentMap<ValueCellId, (Value, DeterminacyState)>` (`reify-eval/src/engine_eval.rs:201`; read at `reify-cli/src/mcp_context.rs:272–276`). A parallel side-map keyed by `ValueCellId` rides alongside with **zero** change to this tuple — `DeterminacyState` (`reify-ir/src/value.rs:3053–3062`) distinguishes `Undetermined` (unbound) vs `Auto`/`Provisional` (solver variable) for origin classification.
- **Value identity precedent for metadata exclusion.** `GeometryHandle.kernel_handle` is already excluded from `==`/`Ord`/`content_hash` (`value.rs:962–965`) — confirms the project's stance that ephemeral/metadata fields stay out of identity. (We go further and keep the reason **off** `Value` entirely.)
- **GUI param payload is extensible.** `ParameterInfo` already carries `determinacy: String` (`reify-mcp/src/types.rs:53–61`); GUI payload builder `gui/src-tauri/src/engine.rs:1472–1501`.
- **LSP hover.** `reify-lsp/src/hover.rs:43–52` assembles the markdown; injection point for the reason line.
- **No new grammar.** The `why()` intrinsic is rejected; nothing adds `.ri` syntax. **`grammar_confirmed = true` for every task.**

**One substrate gap, owned by δ (not a blocker).** There is **no** CLI function that prints parameter values today — `report_eval_output` (`reify-cli/src/main.rs:1199–1210`) prints only constraints + diagnostics; params surface only via MCP `get_parameters`. δ therefore *builds* the CLI surface. Cheapest route: emit each undef cause as an **informational diagnostic** through the existing diagnostics print loop (`main.rs:1207`), which already renders `{severity}: {message}` — binding the reason to the channel where the decoupled "why" already lives. (Tactical: diagnostic-note vs a new `report_parameters` table — §11 Q1.)

## §3 — Consumer (G1)

| Mechanism | Named consumer |
|---|---|
| `UndefCause` reason model + per-cell side-map capture (`reify-ir`, `reify-eval`) — task α | the **tracer** (β) |
| **Undef tracer** — all-causes DAG walk (`reify-eval`) — task β | **`reify check`/`eval` CLI** (δ), **GUI hover/param-panel** (ε), **LSP hover** (ζ) |
| Op/builtin contract-failure **reason sink** (`reify-expr`) — task γ | the side-map (α) → the tracer (β) → all three surfaces |
| CLI / GUI / LSP surfaces (δ/ε/ζ) | **end users** running `reify check`/`eval`, the GUI, or the editor |

No mechanism is a producer-orphan: the tracer's three in-batch user surfaces ship in the same batch. The capture (α) and op-sink (γ) plug into the **eval cell-compute path** (not a new `engine-integration-norm.md §3` seam — they are eval-internal, additive metadata); the tracer is a **read-only** walk over the same dependency edges the freshness walk uses (closest to §3.6 freshness-only walk). No new in-engine seam is added.

## §4 — Contracts (the core design)

### 4.1 Reason model + cell-origin capture (task α)

A new reason vocabulary in `reify-ir` (sibling of `DeterminacyState`):

```
enum UndefCause {
    Unbound        { param: <cell/param identity>, span: SourceSpan },  // no binding, no default
    AwaitingSolve  { param: <…> },                                      // auto cell, solver has not assigned
    SolveFailed    { detail: <unsatisfiable / ambiguous>, … },          // solver ran, no/ambiguous assignment
    OpContractFailed { code: DiagnosticCode, span: SourceSpan },        // local op/builtin failed with determined inputs (γ)
    UserUndef      { span: SourceSpan },                                // explicit `undef` literal in source
}
```

Captured **at the cell-compute boundary** in `reify-eval` into a side-map `HashMap<ValueCellId, UndefCause>` (engine/snapshot-side, keyed by cell). A reason is recorded for a cell only when the undef **originates** at that cell — i.e. the cell is undef but its undef is *not* purely inherited from undef inputs:
- inputs absent / cell unbound ⇒ `Unbound`;
- auto cell, `DeterminacyState::Auto` and unassigned post-solve ⇒ `AwaitingSolve`; solver ran and failed ⇒ `SolveFailed`;
- source `undef` literal ⇒ `UserUndef`;
- all inputs **determined** yet result undef (op/builtin contract failure) ⇒ `OpContractFailed` (filled by γ).

A purely **propagated** cell (undef solely because ≥1 input is undef) records **nothing** — its cause is upstream and is recovered by the tracer's dependency walk (β). Layer-1 (this task) covers `Unbound` / `AwaitingSolve` / `SolveFailed` / `UserUndef`; `OpContractFailed` is γ.

| # | Invariant |
|---|-----------|
| A1 | **Transparency.** With capture enabled, every cell's `(Value, DeterminacyState)`, every content-hash, every realization-cache hit/miss, and every constraint result is **byte-identical** to capture disabled. Reason capture is purely additive metadata. |
| A2 | An unbound param cell records `Unbound`; an auto cell unresolved post-solve records `AwaitingSolve`; a solver-failed cell records `SolveFailed`; a literal `undef` records `UserUndef`. |
| A3 | A purely propagated undef cell records **no** origin (so the tracer attributes upstream, never double-counts). |

### 4.2 Undef tracer — all-causes DAG walk (task β)

`trace_undef_causes(engine/snapshot, cell: ValueCellId) -> Vec<UndefCause>` — a **read-only** walk:

- Start at `cell`; if it has a recorded origin (§4.1), that is a root cause.
- Follow `DependencyMap.forward[cell]` to input cells; for each input that is **itself undef**, recurse.
- Accumulate the **set** of all reachable origins; **deduplicate** (by cause identity); **never short-circuit** at the first.
- Guard against cycles (the dependency graph can contain provisional cycles — visited-set).

| # | Invariant |
|---|-----------|
| B1 | **Completeness (the headline contract).** The returned set contains **every** reachable root cause through undef-valued dependency edges (and §4.3 local sinks) — `c = a + b`, both unbound ⇒ both `Unbound` returned; deduped; order-stable. |
| B2 | **Chain collapse.** `z ← y ← x(unbound)` (y,z pure propagation) ⇒ returns the single root `x: Unbound`, not y/z. |
| B3 | **Multiple independent roots** ⇒ all returned (e.g. two unbound params + one `SolveFailed` ⇒ three causes). |
| B4 | Terminates on cyclic dependency graphs (visited-set); a determined cell or non-undef input is never reported. |

### 4.3 Op/builtin contract-failure reason sink (task γ)

The most invasive cluster. A lightweight **reason sink** threaded through `reify-expr` evaluation: when a local op/builtin returns `Value::Undef` **with all inputs determined** (a genuine contract failure — dimension mismatch, domain error, wrong arity, non-finite), it pushes an `OpContractFailed { code, span }` into the sink (reusing the existing `DiagnosticCode` it already emits). The cell-compute boundary (α) drains the sink into the side-map for that cell. The tracer (β) then reports these as root causes alongside propagated input causes.

- The sink is `Option<&mut Vec<UndefCause>>` (or equivalent) — **off by default** so the hot eval path is unaffected when no capture is requested (supports A1 transparency).
- Only **input-determined** failures push (an undef-input propagation is *not* a local failure — it belongs to the upstream cell, per A3).

| # | Invariant |
|---|-----------|
| G1 | `x = a + sqrt(-1mm)` with `a` unbound ⇒ tracer returns **both** `a: Unbound` and the `sqrt` domain `OpContractFailed` (the all-causes requirement crossing the cell↔sub-expression boundary). |
| G2 | A builtin returning undef purely because an **input** is undef records **no** `OpContractFailed` (no double-attribution; the cause is the upstream undef cell). |
| G3 | Sink off ⇒ eval is byte-identical to pre-γ (transparency, A1 extended to the expr layer). |

### 4.4 Surfaces (tasks δ, ε, ζ)

All three render the same tracer output (`Vec<UndefCause>`) as a short human string; none changes the tracer.

- **δ — CLI.** For each undef that blocks a requested output / constraint, emit an informational note via the existing diagnostics print loop (`main.rs:1207`): `note: <param> is undef (because: <cause set>)`. Commit `examples/undef_self_describing.ri`. **Noise-gated** (§1: partial designs legitimately have many undefs) — default to undefs reachable from an output/constraint; `--explain-undef` to dump all (tactical, Q2).
- **ε — GUI.** Add `reason: Option<String>` to `ParameterInfo` (`reify-mcp/types.rs:53`) and the GUI param payload (`engine.rs:1472–1501`); the param panel / hover shows it. Observable via the `reify-debug` MCP.
- **ζ — LSP.** Append the reason line at `reify-lsp/src/hover.rs:43–52` when hovering an undef param.

| # | Invariant |
|---|-----------|
| S1 | δ: `reify eval examples/undef_self_describing.ri` prints the **complete** cause set for the demonstrated undef output (CI-assertable on stdout/stderr); a fully-determined design prints **no** undef note. |
| S2 | ε: an undef param's panel/hover content includes its cause set (asserted via `reify-debug` MCP); a determined param shows none. |
| S3 | ζ: LSP hover over an undef param includes the cause set; over a determined param it does not. |
| S4 | All three surfaces render the **same** tracer set for the same cell (no surface invents or drops causes). |

## §5 — Approach (G5) and boundary-test sketch (the H component)

**B + H.** The load-bearing seam is the eval cell-compute boundary (α/γ) feeding the read-only tracer (β) feeding three surfaces. The H component is two-way: a **producer** side (the tracer reconstructs the exact expected cause set) and a **transparency** side (capture changes nothing observable about values/determinacy/caching). Boundary suite = task η's signal.

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| BT1 | Single unbound root | `thickness = outer_d` , `outer_d` unbound | tracer ⇒ `{outer_d: Unbound}` (A2/B2) |
| BT2 | Two independent roots | `c = a + b`, both unbound | tracer ⇒ `{a, b}` both, deduped (B1) |
| BT3 | Chain collapse | `z ← y ← x(unbound)` | tracer ⇒ `{x: Unbound}` only (B2) |
| BT4 | Auto / solve | auto cell unresolved post-solve; an unsatisfiable solve | `AwaitingSolve` / `SolveFailed` (A2) |
| BT5 | Op failure crossing boundary | `x = a + sqrt(-1mm)`, `a` unbound | tracer ⇒ **both** `a:Unbound` and `sqrt` `OpContractFailed` (G1/B1) |
| BT6 | No false op cause | `y = sqrt(a)`, `a` unbound | tracer ⇒ `{a: Unbound}` only, no `OpContractFailed` (G2) |
| BT7 | Cycle safety | provisional cyclic deps | tracer terminates, returns reachable origins (B4) |
| BT8 | **Transparency** | full eval of a representative design, capture **on** vs **off** | identical `(Value, DeterminacyState)` per cell, identical content-hashes, identical realization-cache outcomes, identical constraint report (A1/G3) |
| BT9 | CLI surface | `reify eval examples/undef_self_describing.ri` | complete cause set on output undef; **no** note when fully determined (S1) |
| BT10 | GUI surface | undef param via `reify-debug` MCP | panel/hover includes cause set (S2) |
| BT11 | LSP surface | hover an undef param | hover includes cause set (S3) |
| BT12 | Surface agreement | same undef cell across CLI/GUI/LSP | identical cause set (S4) |

## §6 — Cross-PRD relationship (G4 seam ownership)

| Other PRD / cluster | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `determinacy-intrinsics-completion.md` (on main, batch 4197–4200) | adjacent — the *whether* axis | the per-cell `DeterminacyState`; this PRD **reads** it to classify origin kind, never writes it | **that PRD owns `DeterminacyState` + the `determined`/`undetermined` predicates; this PRD owns the `UndefCause` side-map + tracer + surfaces** | resolved — read-only dependency, **no shared write** (side-map is parallel, not a tuple widening) |
| `constraint-solver-completion.md` (#4019, pending) | upstream of `SolveFailed`/`AwaitingSolve` origins | the solver's unsatisfiable/ambiguous outcome → `SolveFailed` | that PRD owns solver outcomes; this PRD **classifies** them into causes | coordinate — α reads whatever the solver records; if solver-outcome detail is thin, `SolveFailed` degrades to a coarse cause (honest, no false detail) |
| `result-and-fallback.md` (#…, v0_6) | adjacent | `map[key]`-absent etc. are "evaluation failure (not undef)" — a *different* failure channel | that PRD owns recovery semantics | no seam — this PRD traces `undef` only, not eval-failures |

**The one real coordination point** is the per-cell record shared with `determinacy-intrinsics`. Resolution: this PRD adds a **separate side-map keyed by `ValueCellId`** and never touches the `(Value, DeterminacyState)` tuple — so there is no file-lock fight over the tuple type, and the `determined()` predicate stays the boolean gate while the tracer is the explanation layer built strictly on top.

## §7 — Grammar gate (G3) — PASSED (vacuously)

No task introduces `.ri` syntax. The `why()` in-language intrinsic is rejected (§0); all surfaces are tooling (CLI diagnostics, GUI payload field, LSP hover markdown). `examples/undef_self_describing.ri` uses only existing syntax (`undef` literal already parses — `tree-sitter-reify/grammar.js:1482`, `ts_parser.rs:2729`). **`grammar_confirmed = true` for every task.**

## §8 — Substrate notes

### 8.1 Why cell + tracer, not a payload on `Value::Undef`
Spec §9.2.9 scopes tracing as "an implementation concern, not a language semantics concern." A payload on `Value::Undef` (even identity-excluded) is a value-model change: it would touch ~2,400 construction/match sites, risk content-addressing (the reason would be lossy across dedup/cache boundaries where two undefs collapse), and couple the language value to tooling metadata. The cell side-map keeps `Value::Undef` a pure unit variant and the §9.2 propagation rules byte-identical (A1/BT8).

### 8.2 Why "all causes" needs both the dep-walk and a sink
A cell can be undef for several reasons at once: propagated undef **inputs** (recovered by the dependency walk, β) **and** a local op failure **inside its own expression** with otherwise-determined inputs (recovered only by γ's sink, since the sub-expression is not its own cell). Completeness (B1) requires both channels — this is the structural reason γ exists as a distinct cluster.

### 8.3 No false detail when the solver is thin
`SolveFailed`/`AwaitingSolve` carry only what the solver actually records. If the constraint/dimensional solver does not expose a structured failure reason, the cause degrades to a coarse "solver could not determine" — honest, never a fabricated specific cause (G6 discipline: assert only what the dependency set can produce).

### 8.4 Noise gating is a real requirement, not polish
Spec §9.2: a partially-specified design with many `undef` params is legitimate and common. An un-gated "note per undef" would bury signal. Default surface scope = undefs **reachable from a requested output/constraint** (the actionable ones); `--explain-undef` dumps all (Q2).

## §9 — Decomposition plan (the DAG)

Greek labels; real IDs assigned at decompose. Modules: `reify-ir`, `reify-eval`, `reify-expr`, `reify-cli`, GUI `src-tauri` + `reify-mcp`, `reify-lsp`, `examples`, `docs`.

- **α — Reason model + cell-origin capture.** `UndefCause` in `reify-ir`; per-cell side-map in `reify-eval`; Layer-1 origins (`Unbound`/`AwaitingSolve`/`SolveFailed`/`UserUndef`) recorded at the cell-compute boundary; **transparency invariant** (A1).
  - *Signal (intermediate → unlocks β):* a `reify-eval` integration test on a committed `.ri` — each Layer-1 cluster records the correct origin in the side-map on a **real eval**, and a capture-on-vs-off run is byte-identical (BT8 transparency). Real eval path, not synthetic-input.
  - *Modules:* reify-ir, reify-eval. *Prereqs:* —. *grammar_confirmed:* true.
- **β — Undef tracer (all-causes DAG walk).** `trace_undef_causes` walking `DependencyMap.forward`; complete, deduped, cycle-safe root-cause set (B1–B4).
  - *Signal (intermediate → unlocks surfaces):* `reify-eval` tests — `c=a+b` both-unbound ⇒ both; chain collapse ⇒ single root; multiple roots ⇒ all; cyclic graph terminates (BT1–BT4, BT7).
  - *Modules:* reify-eval. *Prereqs:* α. *grammar_confirmed:* true.
- **γ — Op/builtin contract-failure reason sink.** Thread an opt-in sink through `reify-expr`; input-determined contract-failure sites push `OpContractFailed{code,span}`; cell boundary (α) drains into the side-map; tracer (β) reports them.
  - *Signal:* `x = a + sqrt(-1mm)` (a unbound) ⇒ tracer returns **both** causes (BT5); `y=sqrt(a)` ⇒ no false op cause (BT6); sink-off byte-identical (G3).
  - *Modules:* reify-expr, reify-eval. *Prereqs:* α, β. *grammar_confirmed:* true.
- **δ — CLI surface.** Build the undef-cause CLI surface via the diagnostics print loop (the CLI param-print gap, §2/§8.4); noise-gated; commit `examples/undef_self_describing.ri`.
  - *Signal (leaf):* `reify eval examples/undef_self_describing.ri` prints the **complete** cause set for the demonstrated undef output; a fully-determined design prints **no** note (BT9/S1) — CI-assertable CLI output difference.
  - *Modules:* reify-cli, examples. *Prereqs:* β. *grammar_confirmed:* true.
- **ε — GUI hover / param-panel surface.** `reason: Option<String>` on `ParameterInfo` + GUI payload; param panel/hover renders it.
  - *Signal (leaf):* `reify-debug` MCP asserts an undef param's panel/hover includes its cause set; a determined param shows none (BT10/S2).
  - *Modules:* reify-mcp, gui/src-tauri, gui/src. *Prereqs:* β. *grammar_confirmed:* true.
- **ζ — LSP hover surface.** Append the cause set at `hover.rs:43–52` for undef params.
  - *Signal (leaf):* LSP hover content over an undef param includes the cause set; over a determined param it does not (BT11/S3).
  - *Modules:* reify-lsp. *Prereqs:* β. *grammar_confirmed:* true.
- **η — B+H integration gate + docs.** The full §5 boundary suite (BT1–BT12, incl. transparency BT8 and cross-surface agreement BT12); reconcile `reify-language-spec.md` §9.2.9 (point it at the implemented tracer + the three surfaces, noting the all-causes contract and the tooling-only scope); add a short user doc / stdlib-reference note describing the feature.
  - *Signal (leaf):* BT1–BT12 green; §9.2.9 updated to match shipped reality.
  - *Modules:* docs, reify-eval/reify-cli/gui/reify-lsp (boundary tests). *Prereqs:* δ, ε, ζ, γ. *grammar_confirmed:* true.

### Dependency view
```
α (model + capture) ─► β (tracer) ─┬─► δ (CLI) ─────────┐
                                   ├─► ε (GUI) ─────────┤
                                   ├─► ζ (LSP) ─────────┤
                                   └─► γ (op sink) ─────┤
                                                        ▼
                                          η (B+H gate + §9.2.9 docs)
```
(`α→β`; `β` fans out to the three surfaces + `γ`; `γ` also deps `α`; `η` joins `δ,ε,ζ,γ`. δ/ε/ζ render whatever the tracer returns, so they ship Layer-1 causes and automatically gain `OpContractFailed` once γ lands — no re-edit. No edge into any other batch.)

## §10 — Out of scope

- **Any change to `undef`'s language semantics** — §9.2 propagation, value identity, content-hash, `Value::Undef` shape. Tooling only (§8.1).
- **An in-language `why(x)` / `explain(x)` intrinsic** — rejected (§0); would leak impl-defined reason text into the language.
- **Reasons for non-`undef` failures** — `map[key]`-absent and similar "evaluation failure (not undef)" channels are `result-and-fallback.md`'s (§6).
- **Persisting reasons / a reason history across snapshots** — the side-map is per-current-evaluation; provenance-over-time is not in scope.
- **Reasons for imported-geometry / kernel-internal undef** beyond the cause kinds in §4.1.

## §11 — Open (tactical) questions

1. **CLI render route** — informational diagnostic notes through the existing print loop (default; minimal, CI-able) vs a new `report_parameters` table. **Decide during δ.**
2. **Noise gating policy** — default to undefs reachable from a requested output/constraint, `--explain-undef` for all; exact reachability predicate. **Decide during δ** (the principle — don't spam every undef — is resolved, §8.4).
3. **Side-map lifetime** — engine-side `HashMap<ValueCellId, UndefCause>` rebuilt per evaluation vs attached to the snapshot. Default: engine-side, rebuilt on the compute pass (smallest blast radius, no IR snapshot-type change). **Decide during α.**
4. **`UndefCause` identity for dedup** — by (kind, originating cell) vs (kind, span). Default: originating cell + kind. **Decide during β.**
5. **Reason string formatting** — terse one-liner shared by all three surfaces vs per-surface formatting (CLI plain / GUI rich / LSP markdown). Default: one shared terse formatter in `reify-eval`, surfaces wrap. **Decide during δ/ε/ζ.**
6. **`SolveFailed` detail depth** — coordinate with `constraint-solver-completion` on what structured failure the solver exposes (§8.3); coarse fallback if thin. **Decide during α/γ.**
