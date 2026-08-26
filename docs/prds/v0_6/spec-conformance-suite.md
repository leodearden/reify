# Language-spec conformance suite (Ring 1)

**Milestone:** v0_6 · **Status:** active — chartered by Leo 2026-08-26 (spec-conformance program) · **Date:** 2026-08-26

**Code anchors** verified against main `9a992fc2f2` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Normative inputs (committed, ruled — do not relitigate):**
`docs/notes/conformance-scope-boundary-draft.md` (the three-ring scope, RQ-1..9 rulings, the five Ring-1 freshness clauses), `docs/notes/driver-contract-matrix-draft.md` (the ruled Ring-2 driver contract), `docs/notes/cross-driver-divergence-survey-draft.md` (evidence base). The paths keep the `-draft` suffix; the content is ruled. This PRD implements the program's 12-point decision ledger for **Ring 1** — the language-conformance ring the suite quantifies over.

## 1. Goal

An exhaustive, spec-anchored conformance suite for the Reify language. When this PRD's program completes:

- Every normative clause of `docs/reify-language-spec.md` (2,958 lines, 18 sections) carries a **stable opaque paragraph-level anchor ID**, and every conformance fixture cites the anchors it tests.
- A **fixture corpus** runs split-tier — a fast in-process tier on a check-faithful library environment at every verify role, a CLI-observable tier against the real `reify` binary, and heavy tiers (kernel, geometry, export-fidelity, cross-driver) in the offline deep lane — emitting **per-clause JSON verdicts** as the primary artifact.
- **PSPEC** (a reify-audit detector) reddens on a dangling or tombstoned anchor cite and reports per-section covered/uncovered/waived coverage, later promoted to a pinned-set ratchet.
- Known divergences live in a **seeded baseline that only shrinks**, each row owned by a live non-terminal fix task (PTODO-grammar cite; orphaned cite = red); post-seeding additions are L2-only.
- Golden re-blessing is governed: implementer and steward never bless; a **`request_bless` STDIO MCP tool** assembles an adjudicator briefing from repo state, runs a fresh-context adjudicator under a closed ruling taxonomy, and writes a content-hash-keyed verdict into a **committed ledger** that a mechanical merge-gate check enforces.
- A conformance test landing **graduates its clause** from PRD-normative to spec-normative (decision 1), shrinking the spec-silent-zones backlog tracked in the boundary doc.

Posture (decision 12): important, not urgent — do it right over fast. Expect an early embarrassing-bug harvest; the value is mid/long-term. Consumer-first vertical slice (anchors + PSPEC + fixture format + one spec section end-to-end) over horizontal completeness.

## 2. Consumers (G1)

| Mechanism | Consumer |
|---|---|
| Fixture corpus + runners + JSON verdicts | The merge gate / verify pipeline (fast tier at every role; the skim wrapper condenses cargo text, so JSON is the fleet's interface); the agent fleet (machine-readable per-clause results); Leo (adjudications, dashboard) |
| Spec anchors + anchor lint | PSPEC; every fixture's `anchors:` directive; spec authors (stable cite targets replacing fragile heading-string splitting — see the `spec_purpose_example_grammar.rs` precedent) |
| PSPEC coverage report → ratchet | Leo (coverage dashboard); spec authors (graduation rule); /audit |
| Check-faithful test-support environment + parity gate | The suite's fast tier; every existing test-support consumer (773 files) as migration proceeds; the merge gate (parity gate) |
| Divergence baseline | The merge gate (ratchet + owner-liveness); L2 (sole authority for post-seeding additions) |
| Bless MCP + verdict ledger + presence gate | Implementer agents (mid-task bless unblocks); the merge gate (mechanical ledger check); /audit (PBLESS sampling) |
| `E_*`↔`DiagnosticCode` bridge | The fixture harness (`code:` directives); the driver-contract PRD's `--json` egress (mnemonic rendering); LSP/docs (future) |
| Uncoded-diagnostics ratchet | INV-SF-6 enforcement; the rejection-surface waves (codes are the contract, decision 6) |

The suite quantifies over Ring 1 but *tests through* Ring-2 drivers, so a Ring-2 parity failure and a Ring-1 conformance failure are distinguishable verdicts (boundary doc, ruled).

## 3. Background + premise (G6)

Verified by an 8-agent survey (2026-08-26) and re-verified by a 5-agent substrate pass in this session: **nothing spec-keyed exists.** The closest artifacts are PDOCCOVER (chunks↔registry census), the GUI Lezer grammar ledger (`EXPECTED_CLEAN` pinned set in `gui/src/__tests__/reifyGrammarCorpus.test.ts`), the prd-gate probe harness (`scripts/prd-capability-check.py`, PASS/FAIL/UNPROVABLE), and `examples_smoke`. "Conformance" elsewhere in-repo is a **false friend twice over**: `crates/reify-compiler/src/conformance/` is struct-ctor/GD&T field conformance; `crates/reify-kernel-conformance/` is the kernel-pair seam matrix. Hence the crate name below.

Substrate facts this design leans on (session-verified):

- The spec has **no anchor mechanism** (no HTML anchors, no `{#id}`), **no normative/informative markings** (zero occurrences), a stale `**Version:** 0.1` / `**Date:** 2026-03-13` header contradicted by 24 in-body `v0.6` references, and exactly one duplicate heading number: `### 13.1 Doc Comments` (§13) vs a mis-numbered `### 13.1 Newline and Continuation Rules` inside §15.
- The spec **is already a compiled-test input** (`tree-sitter-reify/tests/spec_purpose_example_grammar.rs` `include_str!`s it and splits on literal heading strings, first-match), yet `decide_scope`'s `docs/*` arm classifies spec edits no-heavy-checks on staged/branch scopes — a live coupling gap the spec-diff trigger (leaf θ) closes.
- `DiagnosticCode` (`crates/reify-core/src/diagnostics.rs`) has 187 fieldless variants on `Diagnostic.code: Option<DiagnosticCode>`; the `error/warning/info` constructors hardcode `code: None`, coding happens only via `.with_code`; roughly 1,139 construction sites vs ~441 `.with_code` calls (~700 uncoded, ~60%). `E_*`/`W_*` mnemonics exist as ~163 doc-comment phrases and ~138 message-prefix string literals with **no machine mapping in either direction** (the sole prior art is one ad-hoc tuple pairing `DiagnosticCode::FallbackType` with its `E_FALLBACK_TYPE` prefix in `crates/reify-compiler/src/expr.rs`).
- There is **no rustc-style `//~ ERROR` harness** anywhere (zero `//~` hits) — the fixture annotation harness is new substrate, built by this PRD.
- There is **no adjudicated-bless precedent**: existing re-bless mechanisms are env-var + rerun + commit (`REIFY_REGENERATE_GOLDEN`, `REIFY_UPDATE_GOLDEN`, `UPDATE_SNAPSHOTS` — three divergent names, no approval record). The landed shrinking-baseline/ratchet model is **PTODO** (`crates/reify-audit/ptodo-baseline.txt`, single-derivation `ptodo::fingerprint`, sole regenerator `ptodo-baseline-gen`, gates `crates/reify-audit/tests/ptodo_baseline.rs` + `tests/infra/test_reify_audit_ptodo.sh`). The doc-chunk fence gate **#5479/#5480 is pending, not landed** — cite it as a sibling design, never as substrate.
- The test-support ladder (`crates/reify-test-support`) is unfaithful to `reify check` on four axes, each verified at symbol level: every compile rung uses the stub `CompileTimeIndeterminateChecker` (the real `SimpleConstraintChecker` is injected only by `reify-cli` `parse_and_compile`/`parse_and_compile_with_cfg` and the GUI); `compile_source` compiles with an empty prelude while `Engine::new` always evals against the full stdlib (`engine_admin.rs` `with_prelude`; `helpers.rs` `prelude_backed_functions` is the canonical write-up); module identity is hardcoded `"test"`; `MockConstraintChecker::new()` defaults every constraint `Satisfied` with zero diagnostics.
- The η/#5521 cross-driver parity harness **does not exist yet** (task pending; no parity test in-tree asserts cross-driver diagnostic-set equality). The suite's library-vs-binary parity gate (leaf δ) is new work owned here; the GUI/LSP parity extensions are the driver-contract PRD's.

## 4. Resolved design decisions

The program ledger (12 decisions, Leo 2026-08-26) is adopted wholesale; restated here only where this PRD binds a concrete mechanism to a ruled direction. Session-level decisions:

- **D1 — Crate:** `crates/reify-spec-conformance` (dedicated crate, decision 7; named to dodge both in-repo "conformance" false friends; outside the C1/C2 harness-layout scope's five consolidatable crates, so its test binaries need no baseline row).
- **D2 — Fixture tree:** `crates/reify-spec-conformance/fixtures/<section>/…` (in-crate, per-section subdirs). Must-reject fixtures are chartered residents. Walker interactions verified: Lezer `CORPUS_ROOTS` and every zero-Error walker are explicit-inclusion, so the tree is unswept by default — with one exception, `corpus_no_bare_scalar` (`crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs`) sweeps all `crates/**/*.ri`; leaf β adds a registered exclusion arm for the fixture tree (bare-`Scalar` rejection is itself a spec clause the suite must test). `tests/prd-gate/README.md` gains a new tier row: conformance corpus → this tree, cited by full repo-relative path.
- **D3 — Anchor syntax:** a standalone HTML-comment line immediately preceding the anchored paragraph: `<!-- sc-anchor: sc-XXXXXX -->` (6 lowercase hex chars, random, opaque, no positional meaning; invisible in rendered markdown; greppable). Heading-attached anchors anchor the heading's whole run of intro prose. Tombstones live in a sidecar `docs/reify-language-spec.tombstones` (sorted rows `sc-XXXXXX <YYYY-MM-DD> <reason / forwarding anchor>`); deleting an anchored paragraph requires moving its ID to the tombstone file in the same diff. The anchor lint (fast, deterministic) enforces: ID format, uniqueness, live/tombstone disjointness, and tombstone-file sortedness. Anchor seeding is **incremental** — anchors land section-by-section with each section's first fixture wave; PSPEC's universal quantifier ranges over *anchored* paragraphs, and the coverage report separately lists unanchored sections so incompleteness is visible, never silent.
- **D4 — Fixture format** (decision 5): a directive block of `//@ key: value` comment lines at the top of each `.ri` fixture (inline-YAML-ish, line-oriented), plus rustc-style inline annotations `//~ ERROR <E_MNEMONIC>` / `//~ WARN <W_MNEMONIC>` bound to the preceding source line. Core keys: `anchors` (≥1 required), `tier` (`fast|cli|kernel|geometry|cross-driver`), `driver`, `mode` (`accept|reject|value|test|artifact`), `phase` (`parse|check|eval` — for reject), `code` (mnemonic, resolved through the D7 bridge), `query`/`tolerance` with **unit-carrying tolerances**, `regime` (`closed-form|iterative` — mandatory on value assertions, per the byte-identity ruling), `feature`/`deferred` tags. **The harness hard-fails on any unknown directive** (seeded-fire self-test mandatory). Behavioral positives use `@test` structures inside the fixture; the harness applies the **ruled** `@test` semantics — Indeterminate ≠ pass unless the fixture is explicitly annotated indeterminate-tolerant — independent of today's `reify test` exit behavior (whose fix is the driver-contract PRD's).
- **D5 — Generated manifest** (decision 5/8): `crates/reify-spec-conformance/manifest.json`, **generated never handwritten** — a committed derivation of the fixture directives carrying, from day one, per-case `cost_ms` (measured) and `tier` fields. A regeneration-idempotency gate reds on hand-edits or drift. **Subset selection is a manifest query, never a refactor**: the merge gate runs the fast tier; heavier tiers are selected by tier field into the CLI/offline lanes.
- **D6 — Verdict algebra + JSON results:** per-clause verdicts `PASS | FAIL | UNPROVABLE | BASELINED | WAIVED`, aligned with the prd-capability-check algebra (`UNPROVABLE` = no probe vector can currently observe the clause — blocks like FAIL at authoring time, and is attributable at run time: stale binary, absent kernel). Runners emit `target/spec-conformance/results.json` (rows: anchor, fixture, tier, driver, verdict, cost_ms, detail); the committed artifacts are the manifest, the baseline, the coverage ratchet, and the bless ledger — run results are not committed. A `mode: reject` fixture that exits clean with no diagnostic is a **FAIL** (silent-accept), never a skip — the G6 branch-4 discipline is built into the harness, not left to authors.
- **D7 — `E_*`↔`DiagnosticCode` bridge** (decision 6 prereq): `DiagnosticCode::mnemonic() -> Option<&'static str>` + `DiagnosticCode::from_mnemonic(&str)` in `reify-core`, hand-seeded from the 163 doc-comment mnemonics, with a consistency test binding doc-comment ↔ bridge (a reconciler, so the two copies cannot silently diverge) and a uniqueness test. Fixture `code:` directives name mnemonics; the harness compares **typed codes** — message text is explicitly not conformance (decision 6).
- **D8 — Uncoded-diagnostics ratchet** (decision 6 prereq, INV-SF-6): a committed baseline of uncoded `Diagnostic::error/warning` construction sites (PTODO model: fingerprint rows, sole gen binary, shrink-only gate; ~700 seed rows expected). New uncoded emissions red. Migration to coded emissions happens per-section as rejection-surface waves reach each area — the ratchet is the mechanism, not a big-bang sweep.
- **D9 — Check-faithful environment** (ruled, decision 11): fix `reify-test-support` itself — not a separate rung ladder. A faithful default rung family (working name `check_faithful::check_source_as(module_name, src)`) compiles exactly as the **ruled** check driver: `parse_with_stdlib` with the caller's explicit module name (the anonymous-source contract: an explicit module name behaving exactly as `reify check` on a file with that stem), real `SimpleConstraintChecker` injection (`compile_with_stdlib_checked`), stdlib prelude at compile, diagnostics returned untruncated. Unfaithful rungs are preserved **loud-named** (`bootstrap_*`/`internal_*`) for bootstrap/internals tests; migration of the ~773 consuming files rides a shrinking-baseline ratchet (PTODO model). **Faithfulness is pinned by a parity gate** — same source through the library rung and through a trusted `reify check` binary (freshness via `resolve_trusted_reify_bin` / `target/.reify-bin-sha`, task #5133; a stale binary yields UNPROVABLE, never a verdict) must yield identical diagnostic sets compared as (severity, code, phase, span) — message text excluded. Because faithfulness is specified against the **ruled** driver contract while `cmd_check` is mid-migration (it gains solver + FEA trampolines under the driver-contract PRD), parity failures during the transition are baselined divergence rows owned by driver-contract tasks — the baseline absorbs the transition without weakening the gate. Speed cost is **measured, not assumed** (manifest cost fields; the parity leaf reports the delta).
- **D10 — Divergence baseline** (decision 2): `crates/reify-spec-conformance/divergence-baseline.txt` — sorted rows `fingerprint :: anchor :: #NNNN :: description`, sole gen binary, shrink-only gate. Owner-liveness lane: a row whose `#NNNN` cite is done/cancelled is **orphaned = red** (same liveness substrate as PTODO). Post-seeding additions require an `l2: esc-N-N` field naming the ruling; the gate rejects new unmarked rows after the seeding commit. Seeded-fire self-tests on all three failure directions (unbaselined divergence, orphaned cite, unmarked addition).
- **D11 — Bless governance** (decisions 10/10b): a self-contained **STDIO MCP server** exposing `request_bless(fixture)` (zero-argument-ish; spawned on demand, no daemon). The server — never the implementer — assembles the adjudicator briefing **from repo state** (fixture + directives, cited spec-anchor text, current golden, candidate output, diff context, oracle tier), spawns a fresh-context adjudicator, and accepts rulings only from the **closed taxonomy**: `spec-changed | impl-fixed | regression-refuse | mis-tiered-oracle-refuse`. Deny-by-default escalates to L2, so L2 sees only hard cases. An approval writes a verdict artifact keyed to **(fixture, new-golden content hash)** into the committed ledger `crates/reify-spec-conformance/bless-ledger/`. **The ledger, not the server, is the authority**: a diff-scoped, offline, mechanical merge-gate infra test asserts every golden change in the diff has a matching hash-keyed verdict (diff-scoped per the `check-harness-baseline-registration.sh` lesson — whole-tree gates on shared ledgers thrash innocent rebasers). **Never-blessable semantic layer**: anchors, codes, phases, tolerances, and fixture frontmatter/annotations — the tool refuses; only golden/expected-output payloads are blessable. Mid-task bless is ratified: code + golden + verdict land atomically in one reviewed merge. Wiring: a `type: stdio` entry in `.mcp.json` (reaches interactive sessions and orchestrator agents alike — `strict_mcp_config` defaults false in dark-factory's invoker); dark-factory contributes **config only** (role `allowed_tools` additions), no new DF agent role. Precedent: dark-factory's `orchestrator/mcp/verdict_tools.py` (single-tool STDIO server, ~477 lines) — a shape template, not shared code; reify has no adjudicated-bless precedent to extend. Ruling-quality sampling rides /audit as a **PBLESS** detector (re-adjudicate an N-sample; trailing, not blocking).
- **D12 — PSPEC detector** (decision 3): a reify-audit pattern (enum variant + module + dispatch arm + `--pattern` registration, per the crate's convention). Three lanes: **cite lane** (every fixture `anchors:` entry resolves to a live anchor; dangling or tombstoned = High, hard gate), **coverage lane** (every anchored normative paragraph is cited by ≥1 fixture, waived in a committed waiver file with reason, or reported uncovered — report-only until the ratchet), **baseline-liveness lane** (shared with D10). Coverage output: per-section covered/uncovered/waived JSON. Promotion to ratchet (leaf ψ): a **pinned set** of covered anchor IDs (the Lezer `EXPECTED_CLEAN` lesson — pinned sets over count floors; the survey's `CLEAN_FLOOR` cite is stale, that mechanism was deliberately removed as inventory-coupled). Merge-time-only posture, like the PTODO gate.
- **D13 — Spec-edit trigger** (decision 4, selective injection): two hooks. (a) `decide_scope` coupling: `docs/reify-language-spec.md` + the tombstone sidecar escalate out of the `docs/*` no-heavy-checks arm (the `_RUST_COUPLED_RI_FIXTURES` precedent), so a spec edit runs the spec-coupled set even at staged scope; (b) a `scripts/verify-pipeline-infra-tests.txt` row keying the spec path to the conformance infra wrapper (anchor lint + PSPEC + fast tier). Prose-only docs edits elsewhere keep the seconds-fast path. This also closes the live `spec_purpose_example_grammar.rs` coupling gap.
- **D14 — Oracles** (decision 9): policy = enforced semantic correctness; exact-vs-toleranced-vs-property is an authoring guideline **inside** the suite, encoded in the directive vocabulary: `regime: closed-form` permits byte/exact assertions; `regime: iterative` requires toleranced or property assertions (byte-identity is wrong there — esc-5618-9). Geometry is asserted at **property level** (volume/bbox/mass/topology counts/watertightness within declared tolerance; export fidelity = property probes computed FROM the exported artifact); **no byte-golden geometry artifacts, ever**. Goldens are minimized and closed-form-only. Seeded-fire anti-vacuity self-tests are mandatory house policy for every gate this PRD adds (exemplars: `test_flock_detached_fork_guard.sh` `_corpus_plus_mutant_flags_once`, `test_reify_audit_ptodo.sh` scenario (b)).
- **D15 — Graduation loop** (decision 1): each section wave's landing edits the spec (clause text where PRD-normative today, anchored) and shrinks the boundary doc's spec-silent-zones list. §15 EBNF is demoted to informative **with a parity gate** (RQ-3): a rule-census parity check between `grammar.js`'s rule inventory and §15's production names, plus an explicit "informative; `tree-sitter-reify/grammar.js` is normative for accept/reject" marker in §15.

## 5. Sketch of approach

```
docs/reify-language-spec.md ──anchors──┐            ┌── fast tier: in-process, check-faithful
docs/reify-language-spec.tombstones    │            │   rungs (reify-test-support), every role
                                       ▼            │
crates/reify-spec-conformance/         fixtures ────┤── cli tier: real `reify` binary,
  fixtures/<section>/*.ri  (//@ + //~) │            │   sha-sidecar freshness, --json codes
  manifest.json      (GENERATED)       │            │
  divergence-baseline.txt (shrink-only)│            └── heavy tiers: kernel / geometry /
  coverage-waivers / coverage ratchet  │                export-fidelity / cross-driver,
  bless-ledger/  (hash-keyed verdicts) │                offline deep lane (REIFY_HEAVY_NEXTEST_FILTER)
  goldens/       (minimized,closed-form)▼
                            target/spec-conformance/results.json  (per-clause verdicts)
reify-audit: PSPEC (cites, coverage, baseline liveness) + PBLESS (bless-ruling sampling)
verify.sh:   spec-diff-keyed selective injection; fast tier in ordinary nextest passes
bless MCP:   request_bless(fixture) → fresh-context adjudicator → ledger verdict
```

Verify placement (session-verified seams): the fast tier rides the crate's ordinary nextest tests (runs at task/merge/background/offline roles alike); the conformance infra wrapper is a `tests/infra/test_*.sh` with its same-diff `run-all-classification.manifest` row; heavy-tier atoms join `REIFY_HEAVY_NEXTEST_FILTER` in `scripts/heavy-test-filter-lib.sh` (atom-count pins in `test_heavy_filter_atoms.sh` and `test_verify_offline_partition.sh` updated same-diff), so the offline lane selects them positively and the gates negate them.

## 6. Pre-conditions and external dependencies (G3)

Hard prerequisites (live, non-terminal, verified 2026-08-26):

- **#5403/#5404** — check exit-code trust (INV-SF-2 convergence). Hard prereq for the **CLI tier** only; the fast tier reads diagnostics in-process and does not wait.
- **Driver-contract implementation PRD** (parallel session, uncommitted at authoring; brief `~/.claude/spawn-briefs/prd-driver-contract-implementation.md`) — owns CLI `--json` diagnostics (codes+spans+phase in egress), the shared engine constructor, exit unification, `@test` Indeterminate/diagnostics fixes, and the GUI/LSP parity-gate extensions. The CLI tier's code assertions and the cross-driver tier consume these.
- **#6001–#6016** — builtin-signature registry: the stdlib-contract wave **consumes** it as machine truth and never re-enumerates signatures.
- **RU 5516–5529** — resolution unification: multi-file/cfg conformance fixtures against non-check drivers gate on the relevant RU leaves.

Pattern precedents (landed; cite, don't wait): PTODO baseline machinery; `_RUST_COUPLED_RI_FIXTURES`; `scripts/verify-pipeline-infra-tests.txt`; sha-sidecar #5133 (done); `EXEMPTION_LEDGER` (`mul_div_static_runtime_parity.rs`); Lezer pinned-set ledger. **Correction to the program brief carried here:** #5479/#5480 (doc-chunk fence gate) are *pending* — they share the shape but are not substrate; the landed model is PTODO.

New substrate this PRD builds (verified absent, so owned here, never assumed): the anchor mechanism + lint; the `//~`/`//@` fixture harness; the faithful rung + parity gate; PSPEC/PBLESS; the bless MCP + ledger + gate; the E_* bridge; the uncoded-diagnostics ratchet.

## 7. Cross-PRD relationship (G4)

| Other PRD / program | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| Driver-contract implementation PRD (in-flight sibling session) | consumes | CLI `--json` diagnostics; shared engine constructor; exit unification; `@test` verdict fixes; GUI/LSP parity gates (η/#5521 shape) | other PRD | queued (cross-PRD dep at decompose) |
| Driver-contract implementation PRD | produces | the ruled driver contract is the **faithfulness target** of D9; transitional check-behavior divergences land as D10 baseline rows owned by driver-contract tasks | this PRD (baseline rows), other PRD (fixes) | wired via baseline |
| GUI purpose surface PRD (in-flight sibling session) | consumes | `activate_purpose_session()` seam; purpose fixtures beyond `check --purpose` wait on it | other PRD | queued |
| resolution-unification (5516–5529) | consumes | multi-file import/cfg semantics on non-check drivers | other PRD | queued |
| docs-truth program (PDOCCOVER, #5479/#5480) | none (Ring 3) | MCP language-reference chunks stay theirs; PSPEC and PDOCCOVER are sibling detectors in one crate | other program | no overlap |
| builtin-signature registry (#6001–#6016) | consumes | registry as machine truth for the stdlib wave | other PRD | queued |
| precision PRD / gui-on-demand-measurement PRD | none | GUI measurement arms are Ring 2; ReprWithin GUI posture stays theirs | other PRDs | no overlap |
| dark-factory | consumes (config only) | `.mcp.json` stdio entry is reify's; DF adds `mcp__reify-bless__*` to role `allowed_tools` (no new DF role, no DF code) | this PRD specifies; DF applies config | queued (cross-repo, config-only) |
| spec doc (`docs/reify-language-spec.md`) | produces | anchors, hygiene fixes, §15 demotion marker, graduation edits | this PRD | this PRD's leaves |
| tests/prd-gate placement standard | produces | new conformance-corpus tier row in the README table | this PRD | leaf β |
| Lezer grammar ledger (#5495 context) | none | the pinned ledger remains the editor grammar's sanctioned-divergence mechanism (ruled); the suite does not gate the editor grammar | GUI test infra | ratified boundary |

Contested-ownership check: none of the overlay's three known contested pairs is touched. The one genuinely shared seam — *who fixes check's behavior vs who records the divergence* — is resolved by construction: driver-contract owns fixes, this PRD owns baseline rows citing their tasks.

## 8. Contract sections (G5 — B+H)

Blast radius: new crate + spec docs + `reify-test-support` + `reify-core` + `reify-audit` + verify pipeline + tests/infra + `.mcp.json` (≥ 3 crates, mechanism count ≫ 8, grammar/parser-adjacent, ≥ 2 cross-PRD consumers) — B+H is mandatory, not optional.

### 8.1 Anchor contract

- ID grammar: `sc-[0-9a-f]{6}`, generated randomly at assignment; never positional; never reused (a tombstoned ID is retired forever).
- Placement: one HTML-comment line `<!-- sc-anchor: sc-XXXXXX -->` immediately preceding the anchored paragraph (or heading).
- Deletion ⇒ same-diff tombstone row. The lint (uniqueness, format, disjointness, sortedness) runs in the spec-coupled verify set and hard-fails.
- Consumers may resolve an anchor only through grep for the literal ID; no consumer may parse section numbers for identity.

### 8.2 Fixture directive contract

- Directive lines match `^//@ [a-z-]+:` and appear before the first non-comment line; annotations match `^\s*//~ (ERROR|WARN) [EW]_[A-Z0-9_]+` and bind to the nearest preceding source line.
- Unknown directive key, malformed value, unresolvable `code:` mnemonic, or `anchors:` citing a missing/tombstoned ID ⇒ **harness error** (red), never skip. Every failure mode above has a seeded-fire self-test.
- `mode: reject` requires `phase:`, and at least one of `code:` or a `//~` annotation; observed silent-accept ⇒ FAIL.
- `mode: value` requires `regime:`; `regime: iterative` forbids exact/byte equality assertions.
- `tier:` is authored intent; the manifest generator copies it and attaches measured `cost_ms`; the runner refuses a fixture whose tier's substrate is unavailable, emitting UNPROVABLE with the reason.

### 8.3 Verdict + manifest contract

- `manifest.json` is derived solely by the generator binary from fixture directives; a regeneration-idempotency gate reds on drift; hand edits are impossible to land silently.
- Result rows: `{anchor, fixture, tier, driver, verdict, cost_ms, detail}`; verdict ∈ `PASS|FAIL|UNPROVABLE|BASELINED|WAIVED`. UNPROVABLE always carries an attributable reason (INV-SF-4 discipline applied to the suite's own tooling).
- The merge-gate subset is exactly `tier == fast` (a manifest query); tier membership changes are one-field diffs, never refactors.

### 8.4 Baseline + ratchet contract

- Row grammar `fingerprint :: sc-XXXXXX :: #NNNN :: <description>` (+ optional `:: l2: esc-N-N`); fingerprints line-number-erased (PTODO model); one gen binary is the sole derivation for generation and check alike.
- Gate lanes: (a) live divergence ∉ baseline ⇒ red; (b) baseline row whose fingerprint no longer fires ⇒ stale row, red (the `EXEMPTION_LEDGER` staleness-guard pattern); (c) owner task terminal ⇒ orphaned, red; (d) new row post-seeding without `l2:` ⇒ red.

### 8.5 Bless contract

- `request_bless(fixture)` preconditions: fixture exists in the manifest; the working diff contains a golden change for it; the diff touches **no** never-blessable surface for that fixture (anchors, codes, phases, tolerances, directives) — else refuse with the reason.
- Briefing assembly reads only: the fixture file, the cited spec-anchor paragraphs, the committed golden, the candidate output, `git diff` context. No implementer-supplied prose enters the briefing.
- Adjudicator output must be exactly one taxonomy verdict + rationale; anything else ⇒ deny ⇒ L2 escalation.
- Verdict artifact: `bless-ledger/<fixture-stem>.<sha256[..8]>.json` = `{fixture, golden_hash, ruling, rationale, adjudicated_at}`. The merge-gate check recomputes the hash from the diff's new golden bytes and requires an exact ledger match; it is diff-scoped and runs offline with no server involvement.

### 8.6 Check-faithful environment contract

- `check_faithful::check_source_as(module_name, src)` returns the full diagnostic set (severity, code, phase, span per diagnostic) that the **ruled** check driver produces for a file with stem `module_name` containing `src` — the anonymous-source contract.
- The parity gate compares that set against the trusted binary's on the fast-tier corpus; compare keys exclude message text; a stale binary ⇒ UNPROVABLE (skip), never a verdict; an injected divergence must fire (seeded-fire).
- Loud-named rungs (`bootstrap_*`) carry doc comments naming their deliberate infidelity axis; the migration baseline lists remaining call-site files and only shrinks.

### 8.7 Boundary-test sketch

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Seeded duplicate anchor ID | lint built, §9.2 anchored | anchor lint red, names both sites |
| B2 | Fixture cites tombstoned anchor | PSPEC live | PSPEC High finding; verify red |
| B3 | Anchored paragraph deleted without tombstone | lint live | lint red naming the vanished ID |
| B4 | Unknown `//@` directive in a fixture | harness live | harness error naming key + fixture |
| B5 | `mode: reject` fixture silently accepted by the driver | fast tier live | FAIL verdict (silent-accept), verify red |
| B6 | Hand-edit to `manifest.json` | regen gate live | drift gate red |
| B7 | Injected library-vs-binary diagnostic divergence | parity gate live | parity gate red naming fixture + differing codes |
| B8 | Stale `reify` binary at CLI-tier run | sha-sidecar mismatch | UNPROVABLE with staleness reason; no false verdict |
| B9 | Divergence found, baseline row filed with live owner task | baseline live | BASELINED verdict; verify green; PSPEC counts it |
| B10 | Owner task of a baseline row goes done/cancelled | liveness lane live | orphaned-cite red at next merge-gate audit |
| B11 | Golden edited, no `request_bless` run | ledger gate live | merge-gate infra test red, names golden + expected hash |
| B12 | `request_bless` on a diff touching fixture directives | bless tool live | tool refuses (never-blessable), no ledger write |
| B13 | Spec-only edit staged in project_root | D13 wired | spec-coupled checks run at pre-commit; prose-only docs edit elsewhere stays fast |
| B14 | grammar.js gains a rule absent from §15 EBNF | parity census live | §15 parity gate red |
| B15 | New uncoded `Diagnostic::error` site lands | D8 ratchet live | ratchet gate red naming the site |
| B16 | Covered anchor loses its last fixture | coverage ratchet (ψ) live | pinned-set ratchet red naming the anchor |

## 9. Decomposition plan

Filed at decompose time with `planning_mode=True`; Greek labels below; real IDs stamped back into this section at decompose (overlay obligation). **Standing rule for every leaf:** a leaf adding a gate-resident test carries its drift-guard registrations in the same diff — `run-all-classification.manifest` row for a new `tests/infra/test_*.sh`, wallclock-bounds compliance, heavy-filter atom-count updates, `# ld-ok:` markers for new verify plan lines — or the registration is a hard `add_dependency` edge, never prose-ordered (esc-4914-162). Every gate ships its seeded-fire self-test in the same leaf (D14).

**Phase 1 — skeleton (the ruled vertical slice: anchors + PSPEC + format + §9.2 end-to-end).**

- **α — Spec hygiene + anchor substrate + anchor lint.** Fix the stale `**Version:** 0.1` header (living-document header naming the current language version and pointing §14 at the terminology); renumber §15's mis-numbered `### 13.1 Newline and Continuation Rules` to `### 15.1` (and confirm `spec_purpose_example_grammar.rs`'s heading-split keys survive); define the D3 anchor syntax in a short spec-front authoring note; seed anchors over §9.2; create the tombstone sidecar; build the anchor lint + its infra test. *Modules:* docs, `scripts/`, tests/infra. *Signal:* anchor lint red on a seeded duplicate and on a tombstone violation; spec committed with §9.2 anchored and exactly one `13.1` heading remaining.
- **β — Crate + fixture-tree placement.** Create `crates/reify-spec-conformance` (lib + test target skeleton); add the conformance-corpus tier row to `tests/prd-gate/README.md` (correcting that table's stale `examples_smoke.rs` path in passing); add the registered fixture-tree exclusion arm to `corpus_no_bare_scalar.rs`. *Signal:* a deliberately unparseable seeded fixture in the tree leaves every existing walker/gate green while the workspace builds.
- **γ — Fixture format, harness, generated manifest.** D4 directive parser + `//~` annotation binding; unknown-directive hard-fail; manifest generator binary; committed `manifest.json` + regeneration-drift gate; cost/tier fields populated. *Signal:* unknown-directive fixture reds the harness naming key+file; hand-edited manifest reds the drift gate.
- **δ — Check-faithful rung + parity gate (D9).** Faithful rung family in `reify-test-support`; parity gate over the fast-tier corpus vs the trusted binary (sha-sidecar refusal); seeded-divergence self-test; speed delta measured and reported. Divergences discovered here become ε-format baseline rows citing driver-contract or new fix tasks. *Signal:* parity gate red on an injected divergence; corpus green-or-baselined.
- **ε — Divergence-baseline mechanism (D10).** Schema, gen binary, shrink-only + staleness + liveness + L2-marker lanes, seeded-fire on all failure directions. *Signal:* a baseline row citing a terminal task reds the gate; an unbaselined divergence reds; an unmarked post-seeding addition reds.
- **ζ — PSPEC detector (D12).** reify-audit pattern (cite + coverage + baseline-liveness lanes), per-section JSON coverage report, infra gate (merge-time posture), seeded-fire fixtures. *Signal:* `reify-audit --pattern PSPEC` red on a dangling anchor cite; coverage report artifact lists §9.2 per-clause status.
- **η — §9.2 corpus end-to-end.** Fast-tier fixtures covering §9.2's anchored clauses (undef/Kleene truth tables are generated-enumeration candidates); per-clause verdicts in results.json; discovered divergences → fix tasks filed + baseline rows; graduation edit (§9.2 clauses confirmed spec-normative; boundary-doc backlog updated). *Signal:* per-clause JSON verdicts for §9.2; a deliberately-broken seeded fixture reds verify.
- **θ — Verify wiring + spec-diff trigger (D13).** Fast tier in ordinary nextest passes; conformance infra wrapper + same-diff classification row; `decide_scope` spec coupling + `verify-pipeline-infra-tests.txt` row; prose docs stay fast. *Signal:* probed — a staged spec-only edit runs the spec-coupled set; a staged prose-only docs edit produces the empty plan; a red fast-tier fixture blocks a merge.

**Phase 2 — vocabulary, governance, CLI tier.**

- **ι — `E_*`↔`DiagnosticCode` bridge (D7).** `mnemonic()`/`from_mnemonic` + doc-comment reconciler test + uniqueness test; harness `code:` resolution switched to the bridge. *Signal:* a fixture citing an unknown mnemonic reds naming it; a doc-comment/bridge mismatch reds the reconciler.
- **κ — Uncoded-diagnostics ratchet (D8).** Fingerprint gen binary, seeded baseline (~700 rows), shrink-only gate. *Signal:* a new uncoded emission site reds the gate naming path+context.
- **λ — CLI-observable tier.** External-driver runner over `tier: cli` fixtures (check/eval/test/build), UNPROVABLE algebra, sha-sidecar freshness refusal, `--json` code assertions, infra wrapper + registrations. *Depends:* #5403/#5404; driver-contract `--json` (cross-PRD). *Signal:* same fixture yields binary-tier verdicts; a stale binary yields UNPROVABLE, never PASS/FAIL.
- **μ — Bless MCP server + verdict ledger (D11).** `request_bless` flow, briefing-from-repo-state, closed taxonomy, hash-keyed ledger writes, never-blessable refusal, `.mcp.json` stdio entry (+ compatibility with `setup-worktree-debug-port.sh`'s sibling-preservation contract). *Signal:* a bless attempt on a never-blessable diff is refused with the reason; an approved bless writes the hash-keyed verdict artifact.
- **ν — Bless merge-gate presence check.** Diff-scoped offline infra test binding golden changes to ledger verdicts; seeded-fire both directions (golden-without-verdict red; verdict-hash mismatch red). *Depends:* μ (schema). *Signal:* B11 observed on a probe branch.
- **ξ — PBLESS audit arm.** /audit-integrated sampling re-adjudication of ledger rulings (trailing quality control, non-blocking). *Depends:* μ. *Signal:* `reify-audit --pattern PBLESS` emits sampled-ruling findings with disagreement flagged.
- **ο — Ladder migration mechanics.** Loud-rename unfaithful rungs, seed the call-site shrinking baseline (~773 files), ratchet gate, migrate a first tranche (the conformance-adjacent test files). *Depends:* δ. *Signal:* a new call site of a loud rung outside the baseline reds; the baseline is strictly smaller than its seed after the tranche.

**Phase 3 — breadth waves + heavy tiers.** Each wave follows one protocol: seed the section's anchors → author fixtures (tiered) → run → file fix tasks + baseline rows for divergences → graduation edit → coverage update. Waves beyond these are filed in follow-up decompose sittings against the same protocol.

- **π — Grammar accept/reject wave (§2, §15–§17).** grammar.js-normative accept/reject fixtures (parse phase); §15 EBNF demotion marker + rule-census parity gate (D15); §17 keyword census check. *Signal:* B14; per-clause parse verdicts.
- **ρ — Static-semantics rejection wave (§3, §4, §7, §8).** Typed-code reject fixtures (needs ι); per-clause coded emissions migrated under κ as reached. *Signal:* per-clause verdicts with typed codes; every asserted rejection observed to fire.
- **σ — Constraint-system + `@test` + purpose wave (§10, §12.1, purpose semantics).** Verdict meanings, residual satisfaction, Indeterminate-≠-pass (in-harness policy for the fast tier; CLI-tier `@test` rows gated on the driver-contract fix); purpose activation via `check --purpose` (GUI-driver purpose fixtures wait on the purpose PRD seam). *Signal:* per-clause verdicts incl. a fixture demonstrating Indeterminate ≠ pass under the ruled semantics.
- **τ — Stdlib-contract wave (§11 + stdlib-ref).** Registry-driven (#6001–#6016 consumed, never re-enumerated); shipped/planned annotations respected (planned ≠ fail). *Signal:* per-builtin conformance rows derived from the registry.
- **υ — Freshness + determinism wave (§9.3, §9.6, RQ-7 clauses).** The five ruled Ring-1 freshness clauses (cache transparency incl. diagnostics-replay, completed-eval fixpoint, attributed staleness, D1 orthogonality, failures-never-silent) + per-regime determinism (closed-form byte-stable; iterative run-twice-stable, same binary). CLI/heavy tiers. *Signal:* evaluate-with-cache ≡ evaluate-cold verdicts per regime on the corpus subset.
- **φ — Geometry + export-fidelity wave.** Property-level geometry denotation and exported-artifact fidelity (probes computed FROM STEP/STL/3MF artifacts), offline heavy tier via `REIFY_HEAVY_NEXTEST_FILTER` atoms (+ atom-count pin updates same-diff). *Signal:* property-probe verdicts on exported artifacts within declared tolerances; no byte-goldens anywhere in the diff.
- **χ — Cross-driver conformance tier.** Same fixture through check/eval/GUI/LSP where applicable; Ring-1-failure vs Ring-2-parity-failure emitted as distinct verdicts; offline lane. *Depends:* driver-contract parity substrate (cross-PRD), RU leaves. *Signal:* a fixture whose drivers disagree yields a parity-divergence verdict naming drivers, distinct from clause FAIL.
- **ψ — Coverage ratchet + dashboard.** Promote the PSPEC report to a committed pinned-set ratchet of covered anchors; per-section dashboard artifact for Leo. *Signal:* B16; dashboard artifact rendering per-section covered/uncovered/waived.
- **ω — PRD-close leaf.** Terminal-status stamp per the overlay contract (SHIPPED + landed leaf IDs + AS-AUTHORED freeze + LIVE map, mirrored onto the capability manifest). Depends on all siblings. *Signal:* the committed header.

## 10. Out of scope

- **Ring 2 implementation** — every driver-contract fix (shared constructor, `--json`, exit unification, `@test` runner fixes, GUI/LSP parity gates, module-header spread): the driver-contract PRD's. This suite consumes and, during the transition, baselines.
- **Ring 3 wholesale** (ratified RQ-8): diagnostic message text quality; MCP chunk truth (docs-truth); performance/latency/caching speed; FEA numerical accuracy (solver validation suites — the *contract* stays Ring 1); GUI presentation; `reify doc` rendered format; LSP protocol niceties; tree-sitter `.txt` corpus CI wiring (#5492 context).
- **The implementation-defined annex** as spec *content* (kernel-variable observables, tessellation, ULP behavior, …) — an early spec deliverable of the program, authored as spec work, not by this PRD's leaves; the suite's directive vocabulary reserves `annex:` tags for it.
- **Purpose GUI surface** (purpose PRD), **GUI measurement arms** (#6666/#6667, gui-on-demand-measurement PRD), **mcp-server deletion** (#6665).
- **Retro-fitting the ~115 prose `spec §` comments in `.rs` files** to anchors — a possible later hygiene wave, not chartered here.
- **Docs-truth gate note:** this PRD adds no language surface a `.ri` author observes (no grammar, builtins, or CLI-user behavior changes) — the four doc-leaf obligations are N/A.

## 11. Open questions (tactical)

1. **Bless server implementation vehicle.** Candidates: a self-contained Python fastmcp script (dark-factory `verdict_tools.py` shape; needs a python/uv spawn line in `.mcp.json`) vs a small Rust binary in the conformance crate (reify-mcp's JSON-RPC core is newline-delimited; verify framing against the MCP stdio spec before reuse). Decide at μ; the contract (§8.5) is vehicle-independent.
2. **Adjudicator spawn line** — headless `claude -p` invocation parameters (model, effort) for the fresh-context adjudicator. Decide at μ; deny-by-default makes a conservative first cut safe.
3. **Anchor ID entropy** — 6 hex chars (16.7M ids) is proposed; collision handling is regenerate-on-lint-failure. Revisit only if the lint ever trips.
4. **results.json field naming / schema versioning** for fleet consumers. Decide at γ; include a `schema_version` field from day one.
5. **Wave ordering after π–χ** — remaining sections (§5/§6 expression/statement dynamics, §12 pragmas, §13 doc extraction, §14 versioning) are filed in follow-up decompose sittings; order by dogfood pain.
6. **Fast-tier `@test` isolation fidelity** — whether the in-process behavioral-positive runner reproduces `build_isolated_module` semantics exactly or via the ruled per-test module isolation. Decide at γ/σ against the driver-contract PRD's test-runner leaf.

## 12. G7 walk (advisory, author mode)

Walked against `docs/legibility/design-invariants.md`: the suite *enforces* INV-SF-2/-SF-6 rather than violating them (κ is INV-SF-6's ratchet; #5403/#5404 alignment is INV-SF-2's). Suite-internal checks: every UNPROVABLE carries an attributable reason (INV-SF-4 applied to tooling); harness failures are loud errors, never skips (INV-SF-5 discipline — any stub in suite code cites a live task per PTODO); single-derivation rules (manifest generator, baseline/ratchet gen binaries, bridge reconciler test) prevent the two-copy drift the mnemonic doc-comments would otherwise invite. No angle-crossing surface is touched. No hit requiring a waiver.
