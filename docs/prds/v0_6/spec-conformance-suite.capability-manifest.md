# Capability manifest — `spec-conformance-suite` (Ring 1)

**PRD:** `docs/prds/v0_6/spec-conformance-suite.md` (landed `a35b11740a`, 2026-08-26)  
**Decomposed:** 2026-08-26 · **Substrate HEAD:** `a35b11740a` (re-verified against `da8091cbe8`;
the one intervening commit is docs-only — `git diff --name-only a35b11740a..da8091cbe8` touches only
`docs/**/*.md`)  
**Batch:** 24 leaves (**#6758–#6791**) · **80 dependency edges** — 72 intra-batch, 8 out-of-batch
(λ ← #5403 #5404; τ ← #6015; χ ← #5517 #5518 #5519 #5520 #5521) *(corrected 2026-08-27: now **11
out-of-batch / 83 total** after the three cross-PRD wirings — λ ← #6800, χ ← #6806, σ ← #6803)*

| Label | Task | Prio | Prereqs | Kind |
|---|---|---|---|---|
| α | **#6758** | high | — | leaf · anchor substrate |
| β | **#6759** | high | — | leaf · crate + placement |
| γ | **#6761** | high | α β | leaf · format + harness |
| δ | **#6762** | high | γ ε | leaf · check-faithful rung + parity gate |
| ε | **#6763** | high | α β | leaf · ratchet |
| ζ | **#6764** | high | α γ ε | leaf · detector |
| η | **#6765** | high | γ δ ε | leaf · **the vertical slice** |
| θ | **#6766** | high | α γ ζ η | leaf · verify wiring |
| ι | **#6767** | high | γ | leaf · vocabulary bridge |
| κ | **#6768** | medium | — | leaf · ratchet |
| λ | **#6769** | medium | γ ι · **#5403 #5404** | leaf · CLI tier |
| μ | **#6770** | medium | γ | leaf · bless server + ledger |
| ν | **#6771** | medium | μ | leaf · **merge gate (makes μ authoritative)** |
| ξ | **#6772** | medium | μ | leaf · audit arm |
| ο | **#6774** | medium | δ | leaf · migration ratchet |
| π | **#6776** | medium | η θ | leaf · wave |
| ρ | **#6778** | medium | η θ ι κ | leaf · wave |
| σ | **#6780** | medium | η θ | leaf · wave |
| τ | **#6782** | medium | η θ · **#6015** | leaf · wave |
| υ | **#6783** | medium | η θ λ | leaf · wave |
| φ | **#6785** | medium | η θ | leaf · wave |
| χ | **#6787** | medium | η θ λ · **#5517 #5518 #5519 #5520 #5521** | leaf · wave |
| ψ | **#6789** | medium | ζ π ρ σ τ υ φ χ | leaf · ratchet promotion + dashboard |
| ω | **#6791** | medium | all 23 | leaf · **PRD close** |

**Machine-readable twin:** `docs/prds/v0_6/spec-conformance-suite.capability-manifest.yaml`
(24 labels · **75 capability bindings** · **0 FAIL** · 37 mechanical `delivered_check`s, 38 `manual`).

---

## G3 grammar gate — RUN, and it PASSED

Unlike most PRDs in this corpus, this one *does* introduce a novel textual surface — the `//@ key: value`
directive block and the rustc-style `//~ ERROR E_X` / `//~ WARN W_X` annotations (D4). Whether those parse is
the difference between a fixture format and a grammar fiction, so the gate was **executed**, not reasoned about:

```
$ tree-sitter parse --quiet tests/prd-gate/fixtures/spec_conformance_directive_block.ri
exit=0            # 0 ERROR nodes
```

The fixture carries a seven-line `//@` block, one **trailing** `//~ ERROR` on a source line, and one
**own-line** `//~ WARN`. All three placements parse: they are ordinary `//` line comments to the Reify
grammar. **No grammar change and no upstream grammar-producer task are required**, and `grammar_confirmed: true`
is honest on every leaf. The fixture is committed at `tests/prd-gate/fixtures/` per the overlay's placement
standard, cited by full repo-relative path, and is bound as γ's grammar-fixture evidence.

A first probe attempt **failed** and is worth recording, because it is the trap: it used `structure Probe { … }`
and `body = …`, which are not Reify syntax (`structure def Probe { … }` and `let …` are). The parse error
landed on the *param default* line, not the comments — a false negative that would have read as "the directive
syntax does not parse". Anchor a grammar probe on a known-good fixture before trusting its verdict.

## Substrate verification — what was executed, and what was not

Every mechanical `delivered_check` in the sidecar was **executed at decompose time** via `git grep -E` with its
declared `paths`, and its polarity compared against `expect`:

- **37/37 ran.**
- **7 agree with today's tree** — these are *precondition or regression* guards: α's spec-consumer heading key,
  θ's verify-pipeline-guard membership, κ's INV-SF-6 row, σ's `parse_purpose_flag`, υ's ruled-freshness note,
  φ's heavy-filter constant, and φ's `expect: absent` byte-golden guard.
- **30 are deliberately deliverable-shaped** — they fail now and pass when their producer lands.

**Two checks were rewritten during that sweep because they passed VACUOUSLY**, which is worse than failing:

- λ's sha-sidecar check originally scoped `paths: [crates/reify-spec-conformance, scripts]`. `scripts/` already
  contains `reify-bin-sha` (in `reify-bin-freshness.sh`), so the check was green before a line of λ existed.
  Narrowed to the crate.
- ω's freeze-header check originally grepped `AS-AUTHORED` in the PRD. That string already appears — **in §9's
  own ω row**, describing the deliverable. Re-anchored to the freeze sentence's distinctive phrase
  `AS-AUTHORED design record`.

Both are the same failure mode: a pattern that the *description* of the work satisfies as readily as the work.

### D3 decompose-verify workflow — disposition

`scripts/prd-decompose-verify.mjs` was **not** run as a workflow. This is a recorded disposition, not a skipped
gate. Two reasons, in order of weight:

1. This session was launched under an explicit standing instruction not to use workflows or spawn agents.
2. The workflow's probe runner (`scripts/prd-capability-check.py`) drives exactly three vectors —
   `tree-sitter parse`, `reify check`, `reify eval`. This PRD's premises are overwhelmingly *repo-structural*
   (does this symbol/file/mechanism exist, is this crate absent, is this walker explicit-inclusion), which grep
   answers directly and exactly. The one premise that genuinely needed a `tree-sitter parse` probe — the
   directive/annotation syntax — **was run**, above.

What this does **not** cover, stated so it is not mistaken for coverage: no adversarial pass hunted for premises
the PRD does not state. The bindings below are the premises this session could enumerate from §3/§6/§8/§9.

## Gate walk

**G1 (consumer named).** Every mechanism has a named downstream consumer in the sidecar's `consumer_ref`. The one
structural producer-orphan risk is **μ (bless server) → ν (merge-gate ledger check)**: μ alone is an authoring
convenience with no enforcement. ν is a separate leaf on a real edge, and the manifest binds
`the-LEDGER-not-the-server-is-the-authority` to that split.

**G2 (user-observable leaf signal).** All 24 signals are CLI-output differences, gate-red observations, or
committed artifacts. None is "a unit test passes against synthetic input". Note the shape the PRD imposes and
this manifest preserves: most signals are stated as *a seeded mutant is observed to red*, not *the gate exists*.

**G6 (premise validity).** Three branch-3 cases were resolved by **scoping the signal to its own dependency set**
rather than by relaxing it — λ, σ, χ (see the sidecar bindings). One branch-1 case, α's "exactly one `13.1`
heading remaining", was checked by counting: there are exactly two today, so the assertion is achievable. Two
count-shaped premises (κ's ~700 uncoded sites, ο's ~773 consuming files) are explicitly recorded as
**expectations, not asserted bounds** — the gen binary's output is the seed, and neither leaf may red on a
differing count. Branch-2 (closed-form exactness) is handled structurally by the mandatory `regime:` directive:
`regime: iterative` forbids byte/exact assertions at the harness level, so the esc-5618-9 error cannot be
written by accident.

**G7 (design invariants).** No hit, no waiver. The suite *enforces* INV-SF-6 (κ is its ratchet), INV-SF-2
(λ's #5403/#5404 alignment), INV-SF-4 (every `UNPROVABLE` carries an attributable reason — applied to the suite's
own tooling) and INV-SF-5 (ο's loud-named rungs). Single-derivation discipline — γ's generated manifest, ι's
reconciler, ε/κ/ο's one-gen-binary rule, and ζ/ε sharing ONE liveness determination — is applied against the
no-lockstep-duplication invariant at four separate places.

## Bindings

### α — #6758 · spec-conformance α: spec hygiene + sc-anchor substrate + anchor lint

- **`spec-has-no-anchor-mechanism-today`** — G3 branch: NEW SUBSTRATE, verified absent. `grep -c 'sc-anchor' docs/reify-language-spec.md` = 0 at HEAD a35b11740a; no `{#id}` attribute syntax and no HTML anchor tags anywhere in the 2,958-line file. The anchor mechanism is owned here, never assumed.
  - verdict **PASS** · check: `grep -E 'sc-anchor: sc-[0-9a-f]{6}'` in `docs/reify-language-spec.md` → **present**
- **`exactly-two-13.1-headings-exist-so-the-renumber-is-achievable`** — G6 branch 1 (exactness). The leaf signal asserts 'exactly one 13.1 heading remaining'. Verified at decompose: `grep -n '^### 13\.1' docs/reify-language-spec.md` returns exactly two hits — `### 13.1 Doc Comments` and the mis-numbered `### 13.1 Newline and Continuation Rules` inside §15. Renumbering the second yields exactly one. The assertion is achievable, not aspirational.
  - verdict **PASS** · check: `grep -E '^### 13\\.1 Newline and Continuation Rules'` in `docs/reify-language-spec.md` → **absent**
- **`renumber-cannot-break-the-compiled-spec-consumer`** — G6 branch 3 (misattributed capability) — CLEARED. tree-sitter-reify/tests/spec_purpose_example_grammar.rs `include_str!`s the spec and splits on literal heading strings, so a renumber is a real hazard. Probe-verified at decompose: it splits on exactly TWO keys, `### 9.5 Purposes` and `### 4.4 Purpose Declarations`. Neither is a 13.1 heading, so the §15 renumber is safe. Recorded here so the implementer does not re-probe.
  - verdict **PASS** · check: `grep -E '### 9.5 Purposes'` in `tree-sitter-reify/tests/spec_purpose_example_grammar.rs` → **present**
- **`stale-version-header-is-real-and-fixable-here`** — Field-population analogue: docs/reify-language-spec.md:3 carries `**Version:** 0.1` with `**Date:** 2026-03-13`, contradicted by 24 in-body v0.6 references. Producer is this leaf; the fix is a same-file edit with no downstream consumer to re-wire.
  - verdict **PASS** · check: `grep -E '^\\*\\*Version:\\*\\* 0\\.1'` in `docs/reify-language-spec.md` → **absent**

### β — #6759 · spec-conformance β: reify-spec-conformance crate + fixture-tree placement

- **`crate-name-collides-with-neither-in-repo-conformance-false-friend`** — G3: `crates/reify-spec-conformance` is ABSENT at HEAD (`ls crates/ | grep spec-conform` empty). The two existing 'conformance' artifacts are different things — crates/reify-compiler/src/conformance/ (struct-ctor/GD&T field conformance) and crates/reify-kernel-conformance (kernel-pair seam matrix). Neither is extended.
  - verdict **PASS** · check: `grep -E 'reify-spec-conformance'` in `Cargo.toml` → **present**
- **`exactly-one-walker-sweeps-the-new-tree`** — Anti-orphan / anti-collision: verified at decompose that crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs is the ONE zero-Error walker whose glob is `crates/**/*.ri` (its module docs name that root explicitly). Every other walker, including the Lezer CORPUS_ROOTS set, is explicit-inclusion and therefore blind to a new tree. So exactly one registered exclusion arm is required, not a survey.
  - verdict **PASS** · check: `grep -E 'spec-conformance'` in `crates/reify-cli/tests/harness_cli/corpus_no_bare_scalar.rs` → **present**
- **`prd-gate-placement-standard-has-a-tier-table-to-extend`** — tests/prd-gate/README.md exists and carries the 4-tier 'Where fixtures live' table the overlay cites as canonical. The conformance-corpus row is an ADD to a live table, not a new document.
  - verdict **PASS** · check: `grep -E 'reify-spec-conformance/fixtures'` in `tests/prd-gate/README.md` → **present**

### γ — #6761 · spec-conformance γ: //@ directive + //~ annotation harness + generated manifest.json

- **`rustc-style-annotation-harness-is-absent-so-it-is-owned-here`** — G3: NEW SUBSTRATE, verified absent. `git grep -l '//~' -- '*.ri' '*.rs'` returns nothing across tracked source. There is no rustc-style expected-diagnostic harness anywhere in the workspace to extend; this leaf builds it.
  - verdict **PASS** · check: `grep -E '//~ (ERROR|WARN)'` in `crates/reify-spec-conformance` → **present**
- **`directive-and-annotation-syntax-PARSES-as-ordinary-comments`** — G3 GRAMMAR GATE — RUN AT DECOMPOSE, PASS. The `//@ key: value` directive block and BOTH `//~` annotation placements (trailing on a source line, and on its own line) parse with 0 ERROR nodes: `tree-sitter parse --quiet tests/prd-gate/fixtures/spec_conformance_directive_block.ri` exits 0. They are ordinary `//` line comments to the Reify grammar, so the fixture format needs NO grammar change and NO upstream grammar-producer task. Fixture committed as evidence at the full repo-relative path above (tests/prd-gate/fixtures/, per the overlay's placement standard).
  - verdict **PASS** · check: `grep -E '//~ (ERROR|WARN) [EW]_'` in `tests/prd-gate/fixtures/spec_conformance_directive_block.ri` → **present**
- **`unknown-directive-is-a-hard-error-not-a-skip`** — G6 branch 4 (rejection-mechanism-backed). §8.2 asserts an unknown `//@` key, a malformed value, an unresolvable `code:` mnemonic and an `anchors:` cite to a missing/tombstoned ID are all HARNESS ERRORS. The rejection must be observed to fire — a seeded-fire self-test per failure mode is mandatory in this same leaf (D14). A directive parser that silently ignores unknown keys satisfies the letter and fails the gate.
  - verdict **PASS** · check: `grep -E 'unknown directive'` in `crates/reify-spec-conformance` → **present**
- **`manifest-is-generated-never-handwritten`** — Single-derivation (G7 / INV alignment): manifest.json is derived solely by the generator binary from fixture directives, and a regeneration-idempotency gate reds on drift. This is the PTODO single-derivation model (`ptodo::fingerprint` + sole regenerator `ptodo-baseline-gen`), which IS landed and citable — verified: crates/reify-audit/src/bin/ptodo-baseline-gen.rs, crates/reify-audit/ptodo-baseline.txt, crates/reify-audit/tests/ptodo_baseline.rs, tests/infra/test_reify_audit_ptodo.sh all present.
  - verdict **PASS** · check: `grep -E 'cost_ms'` in `crates/reify-spec-conformance/manifest.json` → **present**
- **`mode-reject-silent-accept-is-a-FAIL-verdict`** — G6 branch 4, built into the harness rather than left to authors (D6): a `mode: reject` fixture that exits clean with no diagnostic is FAIL (silent-accept), never a skip. This is the overlay's negative-assertion sentinel (task 4575's `revolute("not-an-axis", …)` precedent) promoted from an authoring rule to harness code.
  - verdict **PASS** · check: `manual` — The silent-accept verdict is a runtime behaviour of the harness, observable only by running a seeded reject fixture; a grep for the string would pass vacuously on a comment.

### δ — #6762 · spec-conformance δ: check-faithful reify-test-support rung + library-vs-binary parity gate

- **`four-named-infidelity-axes-are-real-and-symbol-level-verified`** — G6 branch 3: the PRD asserts reify-test-support is unfaithful to `reify check` on four axes. Verified at decompose: (1) the real `SimpleConstraintChecker` is injected only by reify-cli (crates/reify-cli/src/main.rs, mcp_context.rs) and the GUI — compile rungs default to the stub `CompileTimeIndeterminateChecker` (crates/reify-compiler); (2) crates/reify-test-support/src/helpers.rs `compile_source` compiles with an empty prelude while Engine::new evals against the full stdlib (`with_prelude`, `prelude_backed_functions`); (3) module identity is hardcoded; (4) MockConstraintChecker defaults every constraint Satisfied. All four are producer-present and fixable in-crate.
  - verdict **PASS** · check: `grep -E 'check_source_as'` in `crates/reify-test-support/src` → **present**
- **`trusted-binary-freshness-substrate-is-LANDED`** — G3: the parity gate's staleness refusal leans on the sha-sidecar mechanism from task #5133 (status `done`). Verified present at HEAD: `resolve_trusted_reify_bin` / `target/.reify-bin-sha` in scripts/reify-bin-freshness.sh, scripts/verify.sh and scripts/prd-gate-substrate-guard.sh. A stale binary must yield UNPROVABLE with an attributable reason, never a verdict.
  - verdict **PASS** · check: `grep -E 'reify-bin-sha'` in `crates/reify-spec-conformance, crates/reify-test-support` → **present**
- **`parity-target-is-the-RULED-driver-contract-not-todays-cmd_check`** — G4 seam, resolved by construction. D9 pins faithfulness against the RULED check driver while cmd_check is mid-migration under the driver-contract PRD (it gains solver + FEA trampolines there). Transitional divergences are BASELINED rows (leaf ε) owned by driver-contract tasks — which is why this leaf has a hard intra-batch edge on ε, not a prose ordering. Without that edge the leaf's own signal ('corpus green-or-baselined') is unproducible from its dependency set (G6 branch 3).
  - verdict **PASS** · check: `manual` — Ownership split between this PRD (baseline rows) and the driver-contract PRD (fixes); not expressible as a repo grep.
- **`speed-cost-is-measured-not-assumed`** — G6 branch 1 discipline: D9 forbids an assumed speed bound. The leaf must REPORT the measured delta from the manifest cost_ms fields rather than assert a target. No numeric floor is claimed, so none can be violated.
  - verdict **PASS** · check: `manual` — A measured-and-reported delta is a run artifact, not a repo-state predicate.

### ε — #6763 · spec-conformance ε: divergence baseline (shrink-only + owner-liveness + L2-marked additions)

- **`PTODO-is-the-LANDED-ratchet-model-cited-as-substrate`** — G3, and the PRD §6 correction carried through: the landed shrinking-baseline model is PTODO — crates/reify-audit/ptodo-baseline.txt, single-derivation `ptodo::fingerprint`, sole regenerator crates/reify-audit/src/bin/ptodo-baseline-gen.rs, gates crates/reify-audit/tests/ptodo_baseline.rs + tests/infra/test_reify_audit_ptodo.sh. All verified present at HEAD. The doc-chunk fence gate #5479/#5480 is PENDING and must NOT be cited as substrate anywhere in this leaf.
  - verdict **PASS** · check: `grep -E 'divergence-baseline'` in `crates/reify-spec-conformance` → **present**
- **`owner-liveness-lane-reuses-the-PTODO-cite-liveness-substrate`** — Anti-orphan: a baseline row's `#NNNN` owner cite going done/cancelled must red as orphaned. That liveness determination is exactly PTODO's live/non-terminal cite check (CLAUDE.md 'TODO citation convention'; grammar in docs/prds/reify-audit-ptodo-detector.md §8), so the lane reuses a landed determination rather than inventing a second one.
  - verdict **PASS** · check: `manual` — Reuse of PTODO's liveness determination is a code-structure judgement; a grep for '#NNNN' would match the row grammar itself, vacuously.
- **`all-four-gate-lanes-carry-seeded-fire-self-tests`** — G6 branch 4 across the board: §8.4 names four red directions — unbaselined live divergence, stale row whose fingerprint no longer fires, terminal owner cite, unmarked post-seeding addition. Each must be OBSERVED to fire on a seeded mutant, per D14 house policy (exemplars: tests/infra/test_reify_audit_ptodo.sh scenario (b); test_flock_detached_fork_guard.sh `_corpus_plus_mutant_flags_once`).
  - verdict **PASS** · check: `manual` — Seeded-fire coverage across four lanes is verified by running the self-tests, not by a pattern match.

### ζ — #6764 · spec-conformance ζ: PSPEC reify-audit detector (cite / coverage / baseline-liveness lanes)

- **`reify-audit-has-a-live-pattern-registration-convention-to-follow`** — G3: verified at HEAD — crates/reify-audit/src/lib.rs carries the PTODO and PDOCCOVER pattern variants with their doc-comments, module files (ptodo.rs, pdoccover.rs) and `--pattern` dispatch. PSPEC is a fourth sibling in an established convention, not new plumbing.
  - verdict **PASS** · check: `grep -E 'PSPEC'` in `crates/reify-audit/src` → **present**
- **`coverage-lane-quantifies-over-ANCHORED-paragraphs-only`** — G6 branch 3 (producible from the dependency set). Anchor seeding is incremental (D3), so a universal quantifier over ALL normative paragraphs would be unsatisfiable at ζ's landing time. The ruled scope is: quantify over ANCHORED paragraphs, and separately REPORT unanchored sections so incompleteness is visible rather than silent. Report-only until leaf ψ promotes it to a ratchet.
  - verdict **PASS** · check: `manual` — The quantifier's domain is a runtime property of the coverage report, not a repo-state grep.
- **`cite-lane-is-a-hard-gate-with-both-red-directions-seeded`** — G6 branch 4: a fixture `anchors:` entry that resolves to no live anchor (dangling) OR to a tombstoned ID must both be High findings that red the gate. Both directions carry seeded fixtures in this leaf.
  - verdict **PASS** · check: `manual` — Both red directions are observed by running the detector against seeded fixtures.

### η — #6765 · spec-conformance η: §9.2 corpus end-to-end + graduation edit (the vertical slice)

- **`per-clause-json-verdicts-are-producible-from-this-leafs-dependency-set`** — G6 branch 3. η's signal is per-clause JSON verdicts for §9.2. Its producers are all in-batch and hard-wired: γ (directive harness + runner + results.json schema), δ (the check-faithful fast-tier rung the verdicts run on), ε (BASELINED verdicts for discovered divergences), α transitively (§9.2 anchors). Nothing in the signal depends on an out-of-batch or unlanded mechanism.
  - verdict **PASS** · check: `manual` — Verdict production is a run artifact (target/spec-conformance/results.json), which is deliberately NOT committed (D6).
- **`graduation-edit-has-a-real-committed-backlog-to-shrink`** — G3: the graduation loop (decision 1) edits the spec and shrinks the spec-silent-zones list in docs/notes/conformance-scope-boundary-draft.md — a COMMITTED normative input (commit 9a992fc2f2). The backlog exists; this is a shrink, not a creation.
  - verdict **PASS** · check: `grep -E 'sc-anchor'` in `docs/reify-language-spec.md` → **present**
- **`undef-kleene-truth-tables-are-enumeration-candidates-not-hand-fixtures`** — Authoring guidance rather than a capability risk: §9.2's undef/Kleene truth tables are finite and total, so generated enumeration is the right shape. Recorded so the wave does not hand-author a partial table and call the clause covered.
  - verdict **PASS** · check: `manual` — An enumeration-vs-hand-authoring choice is an authoring judgement.

### θ — #6766 · spec-conformance θ: verify wiring + spec-diff selective injection

- **`decide_scope-docs-arm-is-the-live-coupling-gap-and-it-is-real`** — G6 branch 3 — the premise is TRUE and the gap is live. docs/reify-language-spec.md is already an `include_str!` build input to a compiled test (tree-sitter-reify/tests/spec_purpose_example_grammar.rs) while decide_scope's `docs/*|*.md|*.yaml|*.yml` arm in scripts/verify.sh classifies spec edits no-heavy-checks on staged/branch scopes. The `_RUST_COUPLED_RI_FIXTURES` escalation arm in the same script is the landed precedent to imitate.
  - verdict **PASS** · check: `grep -E 'reify-language-spec'` in `scripts/verify.sh` → **present**
- **`infra-test-keying-file-exists-and-is-live`** — G3: scripts/verify-pipeline-infra-tests.txt is present at HEAD (314 rows) and is the sanctioned path→infra-test keying surface. Adding the spec row is an ADD to a live mechanism.
  - verdict **PASS** · check: `grep -E 'reify-language-spec'` in `scripts/verify-pipeline-infra-tests.txt` → **present**
- **`verify-pipeline-guard-must-be-consulted-before-claiming-config-only`** — Operational trap from CLAUDE.md: verify-pipeline files are NEVER trivially config-only. This leaf edits scripts/verify.sh and scripts/verify-pipeline-infra-tests.txt, both in scripts/verify-pipeline-paths.txt, so `bash scripts/verify-pipeline-guard.sh requires-full-gate <files>` exits 0 and the full --scope all gate is required. Not a defect — a cost to plan for.
  - verdict **PASS** · check: `grep -E 'verify-pipeline-infra-tests.txt'` in `scripts/verify-pipeline-paths.txt` → **present**
- **`prose-only-docs-edits-must-STAY-fast`** — G6 branch 4 (the negative half). The signal has two directions and the second is the one that rots silently: a staged prose-only docs edit elsewhere must still produce the empty/fast plan. Probe BOTH directions, do not assert the fast path.
  - verdict **PASS** · check: `manual` — Both scope-decision directions are observed by running decide_scope against two staged states.

### ι — #6767 · spec-conformance ι: E_*-to-DiagnosticCode bridge (mnemonic/from_mnemonic + reconciler)

- **`no-machine-mapping-exists-in-either-direction-today`** — G3: NEW SUBSTRATE, verified. crates/reify-core/src/diagnostics.rs carries ~187 fieldless DiagnosticCode variants on `Diagnostic.code: Option<DiagnosticCode>`; `E_*`/`W_*` mnemonics live only as ~163 doc-comment phrases and ~138 message-prefix string literals. The sole prior art is ONE ad-hoc tuple pairing DiagnosticCode::FallbackType with its E_FALLBACK_TYPE prefix in crates/reify-compiler/src/expr.rs. There is nothing to extend.
  - verdict **PASS** · check: `grep -E 'fn from_mnemonic'` in `crates/reify-core/src/diagnostics.rs` → **present**
- **`the-two-copies-are-reconciled-not-duplicated`** — G7 / single-derivation. The bridge and the doc-comment mnemonics are two copies of the same fact. The leaf ships a RECONCILER test binding doc-comment to bridge (plus a uniqueness test), so the copies cannot silently diverge — the same discipline D5/D10 apply to the manifest and baseline generators.
  - verdict **PASS** · check: `grep -E 'fn mnemonic'` in `crates/reify-core/src/diagnostics.rs` → **present**
- **`message-text-is-explicitly-not-conformance`** — Scope fence (decision 6): the harness compares TYPED codes. Message-text quality is ratified Ring 3 (RQ-8) and out of scope. A fixture asserting on message text would be a scope breach, not a stronger test.
  - verdict **PASS** · check: `manual` — A negative scope fence; enforced by review of the fixture directive vocabulary, which has no message-text key.

### κ — #6768 · spec-conformance κ: uncoded-diagnostics ratchet (INV-SF-6)

- **`the-uncoded-population-is-real-and-roughly-sized`** — G6 branch 1, hedged deliberately. Measured at authoring: ~1,139 `Diagnostic::error/warning/info` construction sites vs ~441 `.with_code` calls, i.e. roughly 700 uncoded (~60%). The constructors hardcode `code: None`; coding happens only via `.with_code`. The seed count is an EXPECTATION, not an asserted bound — the gen binary's output is the truth, and the leaf must not red on a count that differs from ~700.
  - verdict **PASS** · check: `manual` — The seed row count is whatever the gen binary derives; pinning a number here would manufacture a false floor.
- **`shrink-only-gate-follows-the-landed-PTODO-model`** — G3: fingerprint rows, sole gen binary, shrink-only check — the PTODO machinery verified present at HEAD. Migration is per-section as rejection-surface waves reach each area (leaf ρ), never a big-bang sweep.
  - verdict **PASS** · check: `manual` — The ratchet's baseline path is not fixed by the PRD; the gate's shape is verified by its own seeded-fire test.
- **`this-ratchet-ENFORCES-INV-SF-6-rather-than-violating-it`** — G7 walk: docs/legibility/design-invariants.md INV-SF-6 `diagnostics-carry-codes` is the invariant this leaf mechanizes. No waiver required; the leaf is the invariant's enforcement arm.
  - verdict **PASS** · check: `grep -E 'INV-SF-6'` in `docs/legibility/design-invariants.md` → **present**

### λ — #6769 · spec-conformance λ: CLI-observable tier (external driver runner + staleness UNPROVABLE)

- **`check-exit-code-trust-is-a-LIVE-non-terminal-hard-prereq`** — G3 / G6 branch 3: #5403 and #5404 (check exit-code trust, INV-SF-2 convergence) are both status `pending` at decompose — live, non-terminal, and wired as REAL add_dependency edges. The CLI tier reads exit codes as verdicts, so it cannot dispatch before they land. The fast tier reads diagnostics in-process and is deliberately NOT gated on them.
  - verdict **PASS** · check: `manual` — Dependency satisfaction is scheduler state, not repo state.
- **`--json-egress-DOES-NOT-EXIST-so-code-assertions-must-degrade-to-UNPROVABLE`** — G6 branch 3 — the signal was scoped to its own dependency set. Verified at decompose: there is NO `--json` flag on any reify CLI command (`git grep -nE '"--json"|long = "json"' -- crates/reify-cli/src` is empty). `--json` diagnostics egress is owned by the driver-contract implementation PRD, which was UNCOMMITTED at this decompose, so no cross-PRD edge could be wired. RESOLUTION, not a deferral: the leaf's signal is 'same fixture yields binary-tier verdicts; a stale binary yields UNPROVABLE, never PASS/FAIL' — both halves are producible today from exit codes and stderr. `code:` assertions on the CLI tier must emit UNPROVABLE with the attributable reason '--json egress absent' until driver-contract lands. That IS the D6 algebra working as designed, not a weakened gate.
  - verdict **PASS** · check: `manual` — Absence of --json today is the verified premise; the deliverable is an UNPROVABLE-with-reason path, observable only at run time.
- **`staleness-refusal-uses-the-landed-sha-sidecar`** — G3: #5133 is `done`; `resolve_trusted_reify_bin` / `target/.reify-bin-sha` are live in scripts/reify-bin-freshness.sh and scripts/verify.sh. A stale binary yields UNPROVABLE with a staleness reason, never a false PASS or FAIL (boundary B8).
  - verdict **PASS** · check: `grep -E 'reify-bin-sha'` in `crates/reify-spec-conformance` → **present**

### μ — #6770 · spec-conformance μ: request_bless STDIO MCP server + hash-keyed verdict ledger

- **`no-adjudicated-bless-precedent-exists-in-repo`** — G3: NEW SUBSTRATE, verified. The three existing re-bless mechanisms are env-var + rerun + commit with NO approval record — REIFY_REGENERATE_GOLDEN, REIFY_UPDATE_GOLDEN, UPDATE_SNAPSHOTS (three divergent names). dark-factory's orchestrator/mcp/verdict_tools.py is a SHAPE TEMPLATE only, not shared code.
  - verdict **PASS** · check: `grep -E 'reify-bless'` in `.mcp.json` → **present**
- **`mcp.json-has-three-live-servers-and-a-sibling-preservation-contract`** — G3 + collision check: verified at HEAD that .mcp.json declares exactly `fused-memory`, `escalation`, `reify-debug`. scripts/setup-worktree-debug-port.sh rewrites this file per-worktree (and `git update-index --skip-worktree`s it), so a fourth `type: stdio` entry MUST survive that rewrite. Verify compatibility with that script's sibling-preservation behaviour explicitly — this is the esc-4202-61 blast zone.
  - verdict **PASS** · check: `grep -E 'reify-bless'` in `.mcp.json` → **present**
- **`the-LEDGER-not-the-server-is-the-authority`** — Anti-orphan by design (G1). The server is an authoring convenience; the enforcement consumer is leaf ν's diff-scoped, offline, mechanical merge-gate check over the committed ledger. If μ landed alone the mechanism would be a producer with no gate — which is exactly why ν is a separate wired leaf and not a prose promise.
  - verdict **PASS** · check: `manual` — The authority split is a design property realized across μ and ν; ν's gate is the observable.
- **`never-blessable-surfaces-are-refused-by-the-tool`** — G6 branch 4: anchors, codes, phases, tolerances and fixture frontmatter/annotations are never blessable; only golden/expected-output payloads are. The refusal must be OBSERVED to fire on a diff touching a directive (boundary B12), not merely documented.
  - verdict **PASS** · check: `manual` — A refusal path is observed by invoking the tool against a seeded never-blessable diff.

### ν — #6771 · spec-conformance ν: bless merge-gate presence check (diff-scoped, offline)

- **`diff-scoped-not-whole-tree`** — Operational lesson carried forward from check-harness-baseline-registration.sh: a whole-tree gate on a shared ledger thrashes innocent rebasers. This check recomputes the hash from the DIFF's new golden bytes and requires an exact ledger match, touching nothing outside the diff.
  - verdict **PASS** · check: `grep -E 'bless-ledger'` in `tests/infra` → **present**
- **`both-red-directions-are-seeded`** — G6 branch 4: golden-changed-without-verdict must red (B11), AND verdict-hash-mismatch must red. A gate that only catches the first is half-vacuous.
  - verdict **PASS** · check: `manual` — Both directions are observed by running the infra test against two seeded probe branches.
- **`gate-runs-offline-with-no-server-involvement`** — Vacuity guard: if the presence check needed the MCP server it would be unrunnable at merge time in the offline lane and would degrade to a graceful skip — the overlay's named vacuity vector. The check is pure ledger + diff arithmetic.
  - verdict **PASS** · check: `manual` — Offline-runnability is observed by running the infra test with no MCP server present.

### ξ — #6772 · spec-conformance ξ: PBLESS audit arm (sampled re-adjudication of ledger rulings)

- **`pbless-is-trailing-quality-control-not-a-blocking-gate`** — Scope fence, deliberate: sampling re-adjudication rides /audit and is NON-blocking. Blocking on a sampled LLM re-adjudication would make merge outcomes non-deterministic. The blocking gate is ν's mechanical hash check.
  - verdict **PASS** · check: `grep -E 'PBLESS'` in `crates/reify-audit/src` → **present**
- **`audit-pattern-registration-convention-is-live`** — G3: same convention as ζ — enum variant + module + dispatch arm + `--pattern` registration in crates/reify-audit, verified present for PTODO/PDOCCOVER at HEAD.
  - verdict **PASS** · check: `grep -E 'PBLESS'` in `crates/reify-audit/src/lib.rs` → **present**

### ο — #6774 · spec-conformance ο: loud-rename unfaithful rungs + call-site migration ratchet

- **`the-consuming-population-is-large-and-must-migrate-on-a-ratchet`** — G6 branch 1, hedged: ~773 files consume reify-test-support today. The leaf seeds a shrinking baseline of remaining call-site files and migrates a FIRST TRANCHE (the conformance-adjacent files) — it does not assert a final count. The gate is 'a new call site of a loud rung outside the baseline reds' plus 'the baseline is strictly smaller than its seed after the tranche'.
  - verdict **PASS** · check: `grep -E 'bootstrap_'` in `crates/reify-test-support/src` → **present**
- **`unfaithful-rungs-are-PRESERVED-loud-named-not-deleted`** — Scope fence: bootstrap/internals tests legitimately need the unfaithful rungs. Renaming to `bootstrap_*`/`internal_*` with doc comments naming the deliberate infidelity axis makes the choice visible at the call site — the INV-SF-5 `placeholders-owned-and-loud` posture applied to test scaffolding.
  - verdict **PASS** · check: `manual` — Doc-comment quality on each loud rung is a review judgement.

### π — #6776 · spec-conformance π: grammar accept/reject wave (§2, §15-§17) + §15 EBNF demotion

- **`grammar.js-is-normative-and-15-becomes-informative`** — RQ-3, ruled. §15's EBNF gains an explicit 'informative; tree-sitter-reify/grammar.js is normative for accept/reject' marker, PLUS a rule-census parity check between grammar.js's rule inventory and §15's production names (boundary B14). The marker without the census would be an unenforced claim.
  - verdict **PASS** · check: `grep -E 'grammar\\.js'` in `docs/reify-language-spec.md` → **present**
- **`the-grammar-gate-probe-vector-is-LANDED-and-is-the-right-one`** — G3: `tree-sitter parse --quiet <fixture.ri>` is the overlay's landed grammar probe (references/grammar-gate.md), and scripts/prd-capability-check.py drives it. Parse-phase accept/reject fixtures are directly observable through it.
  - verdict **PASS** · check: `manual` — Wave-shaped: per-clause parse verdicts are run artifacts.
- **`wave-protocol-is-inherited-not-reinvented`** — Each Phase-3 wave follows ONE protocol: seed the section's anchors, author tiered fixtures, run, file fix tasks + baseline rows for divergences, graduation edit, coverage update. π is hard-wired behind η (the §9.2 vertical slice that proves the protocol) and θ (verify wiring), so the protocol is demonstrated before it is repeated.
  - verdict **PASS** · check: `manual` — Protocol adherence is a review judgement against PRD §9 Phase 3.

### ρ — #6778 · spec-conformance ρ: static-semantics rejection wave (§3, §4, §7, §8)

- **`typed-code-reject-fixtures-need-the-bridge-and-the-ratchet`** — G6 branch 3 (producible from the dependency set): asserting a rejection by MNEMONIC requires ι's E_*-to-DiagnosticCode bridge, and migrating a clause's emission from uncoded to coded rides κ's ratchet. Both are hard intra-batch edges, never prose ordering.
  - verdict **PASS** · check: `manual` — Wave-shaped: per-clause typed-code verdicts are run artifacts.
- **`every-asserted-rejection-must-be-OBSERVED-to-fire`** — G6 branch 4, the wave's central discipline. The overlay's negative-assertion sentinel: `reify check` exiting 0 with `All constraints satisfied.` and no diagnostic where a rejection was asserted is silent-accept = FAIL (task 4575 precedent). γ's harness enforces this mechanically for `mode: reject`, so the wave inherits it rather than relying on author care.
  - verdict **PASS** · check: `manual` — Enforced by the harness at run time; a repo grep cannot observe a rejection firing.

### σ — #6780 · spec-conformance σ: constraint-system + @test + purpose wave (§10, §12.1)

- **`indeterminate-is-not-a-pass-under-the-ruled-semantics`** — The wave's headline clause and a G6 branch 4 case: the harness applies the RULED @test semantics — Indeterminate is NOT a pass unless the fixture is explicitly annotated indeterminate-tolerant — INDEPENDENTLY of today's `reify test` exit behaviour, whose fix is the driver-contract PRD's. The signal is fast-tier-only for exactly this reason.
  - verdict **PASS** · check: `manual` — A verdict-semantics assertion, observed by running a seeded Indeterminate fixture.
- **`check---purpose-EXISTS-today-so-the-CLI-purpose-arm-is-producible`** — G3, verified at decompose: `--purpose` flag parsing is live in crates/reify-cli/src/main.rs (`parse_purpose_flag`, `PurposeActivation`). Purpose activation fixtures via `check --purpose` need nothing unlanded. The GUI-driver purpose fixtures DO wait on the GUI purpose-surface PRD's `activate_purpose_session()` seam — that PRD was UNCOMMITTED at this decompose, so no edge could be wired; those fixtures are explicitly OUT of this leaf's signal.
  - verdict **PASS** · check: `grep -E 'fn parse_purpose_flag'` in `crates/reify-cli/src/main.rs` → **present**
- **`cli-tier-@test-rows-are-deferred-not-silently-included`** — Scope honesty: CLI-tier @test rows are gated on the driver-contract PRD's test-runner fix. They are named as deferred in the leaf text rather than authored against today's exit behaviour, which would bake a divergence into a fixture.
  - verdict **PASS** · check: `manual` — A deferral, recorded in the leaf text and in the fixture directives' `deferred` tags.

### τ — #6782 · spec-conformance τ: stdlib-contract wave (§11 + stdlib-ref), registry-driven

- **`registry-is-the-machine-truth-and-is-NEVER-re-enumerated`** — G1 anti-duplication and the leaf's defining constraint. The builtin-signature registry (#6001-#6016) is the single derivation for builtin signatures; this wave DERIVES per-builtin conformance rows from it. Hand-enumerating signatures here would create the second copy the registry PRD exists to delete.
  - verdict **PASS** · check: `grep -E 'reify_builtins|reify-builtins'` in `crates/reify-spec-conformance` → **present**
- **`the-registry-completeness-gate-is-a-single-real-edge`** — G3 / dependency hygiene: rather than 16 edges across #6001-#6016, this leaf is wired to #6015 — the registry PRD's λ INTEGRATION GATE ('§8 boundary rows green in CI on one commit', signatures_common.rs confirmed deleted). That is the registry's own definition of complete-and-green, so it is the correct single gate. Status at decompose: pending (live, non-terminal).
  - verdict **PASS** · check: `manual` — Dependency satisfaction is scheduler state.
- **`planned-is-not-fail`** — G6 branch 3: shipped/planned annotations in the stdlib reference are respected — a builtin annotated planned yields a WAIVED or deferred verdict, never FAIL. Asserting conformance against an unshipped builtin would be the classic false-premise leaf.
  - verdict **PASS** · check: `manual` — Verdict-mapping behaviour, observed at run time.

### υ — #6783 · spec-conformance υ: freshness + determinism wave (§9.3, §9.6, the five ruled Ring-1 clauses)

- **`the-five-freshness-clauses-are-RULED-and-COMMITTED`** — G3: the five Ring-1 freshness clauses — cache transparency including diagnostics-replay, completed-eval fixpoint, attributed staleness, D1 orthogonality, failures-never-silent — are ruled in docs/notes/conformance-scope-boundary-draft.md, committed at 9a992fc2f2. Do not relitigate them; implement them as clauses.
  - verdict **PASS** · check: `grep -E 'freshness'` in `docs/notes/conformance-scope-boundary-draft.md` → **present**
- **`determinism-is-asserted-PER-REGIME-never-globally`** — G6 branch 2 (closed-form exactness), and the reason `regime:` is mandatory on value assertions. Byte-stability is correct for `regime: closed-form` and WRONG for `regime: iterative` (esc-5618-9), where the assertion is run-twice-stable on the same binary. A single global byte-identity claim here would be a false premise.
  - verdict **PASS** · check: `manual` — Per-regime assertions are enforced by the directive contract (§8.2) and observed at run time.
- **`cache-transparency-is-observed-as-evaluate-with-cache-equals-evaluate-cold`** — The signal, stated as a producible comparison rather than an internal property: evaluate-with-cache and evaluate-cold verdicts must agree per regime over the corpus subset. Runs on CLI/heavy tiers, hence the hard edge on λ.
  - verdict **PASS** · check: `manual` — A run-time comparison over two evaluation modes.

### φ — #6785 · spec-conformance φ: geometry + export-fidelity wave (property-level, no byte-goldens)

- **`geometry-is-asserted-at-PROPERTY-level-only`** — D14, ruled: volume, bbox, mass, topology counts and watertightness within a DECLARED tolerance. Export fidelity means property probes computed FROM the exported artifact (STEP/STL/3MF), never a comparison of artifact bytes.
  - verdict **PASS** · check: `manual` — Property-level assertion shape is a review judgement over the wave's fixture directives.
- **`no-byte-golden-geometry-artifacts-EVER`** — A hard, mechanically checkable negative (G6 branch 4 expressed as `expect: absent`). Byte-goldens over geometry artifacts are wrong for the same reason iterative byte-identity is wrong: kernel and tessellation output is not byte-reproducible across versions. Any committed .stl/.step/.3mf golden in this crate is a defect.
  - verdict **PASS** · check: `grep -E 'golden.*\\.(stl|step|stp|3mf)'` in `crates/reify-spec-conformance` → **absent**
- **`heavy-tier-atoms-carry-their-count-pins-same-diff`** — Standing rule (esc-4914-162) applied to this wave: heavy-tier atoms join REIFY_HEAVY_NEXTEST_FILTER in scripts/heavy-test-filter-lib.sh, and the atom-count pins in tests/infra/test_heavy_filter_atoms.sh and test_verify_offline_partition.sh update in the SAME diff. Verified at decompose that REIFY_HEAVY_NEXTEST_FILTER is live in scripts/heavy-test-filter-lib.sh.
  - verdict **PASS** · check: `grep -E 'REIFY_HEAVY_NEXTEST_FILTER'` in `scripts/heavy-test-filter-lib.sh` → **present**

### χ — #6787 · spec-conformance χ: cross-driver conformance tier (Ring-1 FAIL vs Ring-2 parity divergence)

- **`ring-1-failure-and-ring-2-parity-failure-are-DISTINCT-verdicts`** — The leaf's defining contract, and the whole reason the suite can quantify over Ring 1 while testing THROUGH Ring-2 drivers. A fixture whose drivers disagree emits a parity-divergence verdict NAMING the drivers — never a clause FAIL, which would misattribute a driver bug to the language.
  - verdict **PASS** · check: `manual` — Verdict-distinction behaviour, observed by running a seeded cross-driver disagreement.
- **`the-relevant-RU-leaves-are-wired-and-were-identified-precisely`** — G3 / dependency hygiene. The PRD gates this tier on 'the relevant RU leaves' of resolution-unification (#5516-#5529). Identified at decompose by label: #5517 (γ, eval+build multi-file/--cfg), #5518 (δ, test/report/explain/doc multi-file/--cfg), #5519 (ε, GUI wired to compile_program), #5520 (ζ, LSP diagnostics multi-file), #5521 (η, the P1 cross-surface parity harness). All five are `pending` — live, non-terminal — and wired as real edges. The remaining RU leaves (θ..ο: DefEnv internals, flat-slice migration, import narrowing) are NOT what this tier consumes; wiring them would serialize the tier for no substrate gain.
  - verdict **PASS** · check: `manual` — Dependency satisfaction is scheduler state.
- **`the-eta-5521-parity-harness-DOES-NOT-EXIST-YET`** — PRD §6 correction, carried into the leaf so it is not mis-cited as substrate: no in-tree test asserts cross-driver diagnostic-set equality today. #5521 is `pending`. This leaf's cross-driver comparison is NEW work built on top of it, and the GUI/LSP parity-gate EXTENSIONS belong to the driver-contract implementation PRD (uncommitted at this decompose, so no edge could be wired).
  - verdict **PASS** · check: `manual` — Absence verified at decompose by survey; the deliverable is the new tier itself.

### ψ — #6789 · spec-conformance ψ: coverage ratchet (pinned anchor set) + per-section dashboard

- **`pinned-SET-never-a-count-floor`** — The Lezer lesson, ruled: promote ζ's coverage report to a committed PINNED SET of covered anchor IDs, not a numeric floor. The survey's CLEAN_FLOOR citation is STALE — that mechanism was deliberately removed as inventory-coupled. Do not resurrect it. Boundary B16: a covered anchor losing its last fixture reds the ratchet, NAMING the anchor.
  - verdict **PASS** · check: `manual` — Pinned-set-vs-count-floor is a design shape verified by reading the committed ratchet artifact.
- **`the-ratchet-lands-after-the-waves-so-the-pinned-set-is-meaningful`** — Ordering, wired as real edges: ψ depends on ζ (the report it promotes) and on every Phase-3 wave (π ρ σ τ υ φ χ). Promoting an almost-empty set would produce a green ratchet that proves nothing — the overlay's armed-but-vacuous failure mode.
  - verdict **PASS** · check: `manual` — Dependency satisfaction is scheduler state.

### ω — #6791 · spec-conformance ω: PRD close - terminal status stamp + AS-AUTHORED freeze

- **`terminal-vocabulary-is-CLOSED-to-exactly-three-values`** — Overlay contract: the terminal token is SHIPPED, SUPERSEDED or WITHDRAWN, matched case-insensitively as the first token after the Status label within the first ~10 lines. SHIPPED requires every leaf done (cancelled leaves tolerated alongside, provided at least one landed). Anything else is non-terminal.
  - verdict **PASS** · check: `grep -E '\\*\\*Status.*SHIPPED'` in `docs/prds/v0_6/spec-conformance-suite.md` → **present**
- **`the-freeze-header-has-a-RATIFIED-shape-do-not-invent-one`** — Copy the three-part shape from docs/prds/v0_6/data-carrying-enums.md and docs/prds/kernel-seam-contracts.md: (1) terminal token plus the landed leaf task IDs; (2) the sentence 'The body below is the AS-AUTHORED design record ... they are not current statements of fact.'; (3) an explicit LIVE vs AS-AUTHORED map naming which sections stay maintained. Apply the same header to the .capability-manifest.md. Do NOT copy #4438's or #3847's output — both predate the rule and are non-conformant.
  - verdict **PASS** · check: `grep -E 'AS-AUTHORED design record'` in `docs/prds/v0_6/spec-conformance-suite.md` → **present**
- **`cancelled-siblings-satisfy-this-leafs-dependency-edges`** — Overlay cancelled-dependency disposition, recorded so the close leaf is not left permanently blocked: a `cancelled` sibling counts as satisfied for ω's edge. If the scheduler treats the edge as unmet, the decompose steward removes it by hand and applies the stamp directly in a docs-only commit on main.
  - verdict **PASS** · check: `manual` — A dispatch-policy disposition, not a repo-state predicate.
- **`sections-that-production-code-DEFERS-to-are-LIVE`** — The LIVE/AS-AUTHORED map's load-bearing criterion: a section that production code defers to (rustdoc citing it as the authority) stays LIVE, because a false claim there propagates into code. For this PRD the likely LIVE sections are §8.1 (anchor contract), §8.2 (directive contract) and §8.5 (bless contract) — confirm against actual rustdoc cites at close time rather than copying this list.
  - verdict **PASS** · check: `manual` — Which sections production code cites is determined by grep at close time.

---

*Every binding resolves to PASS. No binding resolved to `declared-only`, `test-only`, `producer-downstream`,
`producer-absent`, `producer-extent-short`, `fixture-ERROR`, `bound≤floor` or `rejection-absent`, so no leaf was
re-scoped, re-homed or relaxed to clear the gate.*
