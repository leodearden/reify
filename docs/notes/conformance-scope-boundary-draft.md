# Conformance scope boundary — DRAFT for case-by-case ruling

Status: draft 2026-08-26, spec-conformance program. Companion to
`cross-driver-divergence-survey-draft.md` and `driver-contract-matrix-draft.md`
(same directory; the ruled matrix is this document's driver chapter). Every
classification below is a PROPOSAL unless marked RULED; the consolidated ruling
questions are at the end.

## Governing principle (Leo, ruled)

**The language spec defines what must be constant between interoperable
implementations — different runtimes of the same reified language.** Conformance to
that is the program's focus. Related surfaces that turn up get discussed and decided
explicitly, never silently ignored.

## Proposed structure: three rings + an implementation-defined annex

- **Ring 1 — Language conformance.** Observables that must be constant across *any*
  interoperable implementation. Normative source: the layered stack (spec +
  stdlib-ref + PRD corpus, with the test-landing graduation rule). This is what the
  conformance suite quantifies over.
- **Ring 2 — Reify product contract.** Observables that must be constant across *this
  implementation's own drivers and surfaces* (CLI subcommands, GUI, LSP), but that a
  different implementation would legitimately ship differently. Normative source: the
  ruled driver-contract matrix (candidate future home: a spec appendix or product
  doc). Tested by cross-driver parity gates (the η/#5521 shape), not by spec clauses.
- **Ring 3 — Explicitly out.** Quality/validation concerns with their own programs.
  Listing them here is the decision that their absence from the suite is deliberate.
- **Implementation-defined annex** (proposed spec deliverable): the C-standard move —
  a spec annex enumerating exactly what MAY vary (per kernel, per version, per run).
  Combined with ruling question 2 below, this is what makes "exhaustive" meaningful.

The suite's relationship between rings: Ring-1 clauses are *tested through* Ring-2
drivers (the same fixture through check / eval / GUI / LSP where applicable), so a
Ring-2 parity failure and a Ring-1 conformance failure are distinguishable verdicts.

## Ring 1 — Language conformance (proposed)

| Surface | Normative source today | Notes |
|---|---|---|
| Lexical structure; grammar accept/reject; parse shape; precedence; keywords; newline/continuation rules | spec §2/§15/§16/§17 — but see RQ-3: four grammar statements exist and §15 EBNF is machine-unread | one normative grammar must be crowned |
| Static semantics: types & dimensional analysis (§3), declarations/traits/generics (§4), modules (§7), scoping/visibility (§8), module-header rule (ruled: enforced everywhere) | spec | largest surface; operator tables + truth tables are generated-enumeration candidates |
| The rejection surface: WHICH programs are rejected, at which phase (parse/check/eval), with which `DiagnosticCode` | spec + the typed code registry | decision 6; needs the `E_*`↔enum bridge and coding of `code: None` emissions |
| Expression/statement dynamic semantics (§5/§6); undef/Kleene truth tables (§9.2); `auto` meta-properties (§9.3); Result/Option recovery; forall/keyed re-elaboration | spec | §9.2 is the best-specified area — the model |
| Constraint-system semantics (§10): verdict meaning (Satisfied/Violated/Indeterminate), solve/propose contract via **residual satisfaction**, objectives/cost semantics | spec §10/§10.8 | solver *internals* → annex |
| Stdlib contract: names, signatures, dimensional types, determinacy, behavioral semantics | stdlib-ref + builtin-signature registry (#6001–#6016) as machine truth for builtins | shipped/planned annotations respected (planned ≠ fail) |
| Geometry denotation at **property level**: the realized geometry's volume/bbox/mass properties/topology counts/watertightness within declared tolerance; op-stream (IR) semantics for closed-form constructions | spec + stdlib-ref §3 | mesh bytes, tessellation → annex; shape-level (reference-construction) equality reserved for shape-critical clauses |
| Exported-artifact fidelity: an exported STEP/STL/3MF/gcode artifact denotes the Ring-1 geometry within tolerance (property probes ON the artifact) | RQ-5 | format bytes/writer version → annex; which formats each driver offers → Ring 2 |
| `@test` verdict semantics (§12.1): PASS/FAIL/INDETERMINATE meaning; Indeterminate ≠ pass (ruled) | spec | the exit-code *mapping* is Ring 2 |
| Purpose semantics: declaration, activation effect on the constraint set | spec §4.x | which surfaces can activate → Ring 2 (GUI chartered) |
| Annotations/pragmas (§12): semantic effect, incl. the "pragmas never change program meaning" meta-clause; `#deterministic`; `#kernel`; `#no_prelude` | spec | |
| Determinism guarantees per regime: closed-form byte-stable; iterative run-twice-stable (same binary) | byte-identity decision (esc-5618-9) → graduate into spec | regime tags in the suite's expectation format |
| `reify doc` extraction semantics: which doc comments attach to which entities (§13's language half) | RQ-4 | rendered output format → Ring 3 |

## Ring 2 — Reify product contract (proposed)

- The **ruled driver-contract matrix** wholesale: engine capabilities per driver
  (all-get-all + named subtractions), constraint-verdict run/gate rules, exit-code
  semantics, purpose/cfg/strict flag surfaces, module-header enforcement reach.
- **Cross-driver agreement gates**: same entry → same diagnostic set (η/#5521),
  extended per the matrix to GUI/LSP; the library check-faithful parity gate
  (test-support ≡ `reify check`); warm-edit ≡ full-recompile (GUI self-oracle).
- CLI conventions: flag availability, `--json` diagnostics (to be added), exit-code
  conventions. Open minor item: `doc`'s exit-2 usage convention (ratify or normalize
  — the one matrix cell never ruled).
- Export surface per driver: which formats, declarative `: Output` mode, η-refusal
  reach (#6190).
- LSP/GUI **language answers** (codes preserved, verdicts shown) — parity with check.
  LSP *protocol* behaviors (hover content, completion ranking) are Ring 3.
- Editor (Lezer) grammar: RULED — governed subset under the pinned ledger; the ledger
  is the sanctioned-divergence mechanism.
- Debug-MCP surface: not a conformance object itself; it is the *observation channel*
  parity gates use to see GUI state.

## Ring 3 — Explicitly out (each with the program that owns it)

| Surface | Owned by |
|---|---|
| Diagnostic message TEXT quality/wording/rendering | RULED out (decision 6); separate diagnostics-quality concern |
| MCP language-reference chunks truth | docs-truth program (PDOCCOVER, fence gate #5479/#5480) |
| Performance, latency, caching efficiency, incrementality speed | product/perf work; warm-lane infra |
| FEA / solver **numerical accuracy** (analytic benchmark bands, mesh-error tolerances) | solver validation suites (reify-solver-elastic, eval-fea-tests) — the *contract* (types, units, determinacy, verdict shape) stays Ring 1; RQ-6 |
| GUI presentation/UX (viewport rendering, panel layout, staleness styling) | GUI product work (purpose charter carries its own UX rulings) |
| `reify doc` rendered output format; doc-site generation | doc tooling; RQ-4 |
| LSP protocol niceties (hover formatting, completion, goto-def ranking) | editor tooling |
| tree-sitter `.txt` corpus CI wiring; grammar test infra | product test infra (standing open decision, #5492 context) |

## Implementation-defined annex (proposed contents — to be authored as spec work)

Tessellation details and mesh bytes; trailing-ULP behavior of iteratively-derived
floats; kernel selection and the per-kernel-variable observable set (**must be
enumerated per observable** — this is the annex's hardest and most valuable content);
solver iteration internals, seeds and tiebreak choices (the §9.3 meta-requirement
"deterministic and documented" stays Ring 1); geometry handle identities; evaluation
order beyond spec'd freshness semantics; STEP writer version strings and banners;
default `auto` bounds breadth (§10.8's "wide default bounds").

## Spec-silent zones → graduation backlog (implementation is the de-facto spec today)

Per the survey: the diagnostic catalog (~556 `E_*` mnemonics, ~22 in spec; two
identity systems); 2D sketches (**PRD-only** — the constrained-2d-sketch PRD is the
spec); kinematics/mechanism semantics; eval freshness/caching semantics (§9.6 thin,
INV-EVAL mostly `proposed`); the real stdlib module inventory (matches neither
documented tree); solver numeric defaults; export behavioral semantics; GUI-only
warm-edit machinery. Disposition: each graduates into Ring 1 via the decision-1 rule
(conformance test lands → clause becomes spec-normative), tracked as a shrinking
list in this document; none may be silently dropped.

## Consolidated ruling questions

- **RQ-1**: Adopt the three-ring structure, with the ruled driver matrix as Ring 2's
  normative source?
- **RQ-2 (the big one)**: Constancy default. Propose: **everything observable through
  Ring-1 surfaces is intended-constant unless the implementation-defined annex names
  it** — spec silence means "constant, to-be-specified" (backlog above), not
  "unspecified". The alternative (silence = unspecified) makes "exhaustive"
  unmeasurable. Adopting this makes authoring the annex an early spec deliverable.
- **RQ-3**: Grammar normativity. Four statements exist (spec §15 EBNF — informative
  in practice; `tree-sitter-reify/grammar.js` — the de-facto authority and the
  production parser's actual grammar; Lezer subset — ruled; `ts_parser.rs` lowering).
  Propose: crown **grammar.js** as normative for accept/reject, demote §15 EBNF to
  generated-or-informative *with a parity gate*, spec prose keeps semantic intent.
- **RQ-4**: `reify doc` split — extraction semantics Ring 1, rendered format Ring 3?
- **RQ-5**: Exported-artifact fidelity as a Ring-1 obligation at property level
  (probes computed FROM the artifact), formats/bytes implementation-defined?
- **RQ-6**: FEA split — contract Ring 1 (signatures, units, determinacy, verdict
  shape), numerical accuracy Ring 3 (owned by the solver validation suites)?
- **RQ-7**: Freshness/caching (§9.6): the spec'd freshness *semantics* Ring 1; event
  journal and cache mechanics Ring 2/3 until graduated?
- **RQ-8**: Ratify the Ring-3 table as-is, so absence of conformance testing there is
  recorded as a decision rather than discovered as a gap?
- **RQ-9 (minor)**: `doc` exit-2 — ratify the idiosyncrasy or normalize to exit 1?

## RULINGS (Leo, 2026-08-26)

- **RQ-1, RQ-2, RQ-3, RQ-4, RQ-5, RQ-6, RQ-8: RATIFIED as proposed.** Three-ring
  structure; constancy default = intended-constant unless the implementation-defined
  annex names it (annex = early spec deliverable); grammar.js crowned normative with
  §15 EBNF demoted-with-parity-gate; reify doc split; export fidelity Ring 1 at
  property level; FEA contract/accuracy split; Ring-3 table ratified wholesale.
- **RQ-9: RULED — normalize `reify doc` usage errors to exit 1** (uniformity). Joins
  the mechanical-alignment list (driver-contract implementation).
- **RQ-7: proposed resolution below, pending Leo.**

### RQ-7 resolution (proposed): the Ring-1 freshness clauses

Spec §9.6 today: the 4-variant Freshness enum (`Final` / `Intermediate{generation}` /
`Pending{last_substantive}` / `Failed{error}`), the graph-vs-language orthogonality
clause (D1), and the 4-step failure-surfacing list. INV-EVAL-1..6 (docs/invariants.md,
all `proposed`) are reify enforcement machinery. The split:

**Ring 1** — five observable, implementation-independent clauses:
1. **Cache transparency**: caching, incremental recomputation, and warm-state reuse
   are observationally invisible — evaluate-with-cache ≡ evaluate-cold per
   determinism regime, *diagnostics included* (a hot-served node replays the same
   diagnostics as a cold one). This is the master clause; the ruled GUI
   warm-edit ≡ full-recompile contract is its reify-product instantiation, and
   INV-EVAL-3/INV-BUILD-3 are its enforcement mechanisms.
2. **Completed evaluation is a fixpoint**: at a completed observation point every
   cell is `Final` or `Failed` — never eternally `Pending`/`Intermediate`
   (INV-EVAL-6's observable half).
3. **Staleness is attributed**: a runtime may serve a stale value only under an
   attributed `Pending{last_substantive}` state, never presented as `Final`; and no
   cell retains a value (incl. Undef) whose cause no longer exists once evaluation
   completes (the no-stale-Undef invariant's observable half, INV-EVAL-5).
4. **Graph-vs-language orthogonality (D1)**: graph-`Failed` is uncatchable from
   `.ri`, never implicitly reified as language `none`; a determined `none` never
   marks a node `Failed`. Already normative §9.6 prose — directly fixture-testable.
5. **Failures are never silent**: a failed node's downstream carries a diagnostic
   chain surfaced through the driver's diagnostic egress.

The 4-state Freshness taxonomy enters Ring 1 as the *abstract observation model*
(it is what "attributably stale" means); its representation is implementation detail.

**Ring 2/3**: the event journal and EventKind taxonomy (reify's observation channel),
cache keys/eviction/persistence, scheduling and recompute order (free so long as
clauses 1–2 hold), snapshot/hash agreement (INV-EVAL-4), atomic commit mechanics
(INV-EVAL-1/2), generation counters, warm-state pools. Pattern: INV-EVAL-*'s
observable consequences are Ring 1; their mechanisms are Ring 2.
