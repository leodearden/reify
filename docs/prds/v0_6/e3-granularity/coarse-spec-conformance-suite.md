# E3 coarse-arm re-decomposition — spec-conformance-suite

Part of experiment **E3** (`docs/prds/v0_6/e3-decomposition-granularity-ab.md`). This PRD
randomized to the **coarse arm** (stratum B — its 1.5 avg files/task makes it the strongest
coalescing material). Carve-outs: α #6758 (done) and β #6759 (in-progress at freeze time,
2026-08-28) stay untouched and are EXCLUDED from both arms' endpoint analysis. The remaining 22
standard leaves are retired to `deferred` and replaced by the 10 coarse tasks below. The
dep-free ratchet root κ, the coverage-ratchet promotion ψ, and the PRD-close ω are reused in
place as singletons (κ to preserve a parallel dispatch root; ψ and ω per the gate/close rules).

**Mapping:**

| Coarse task | Constituents | Reduction |
|---|---|---|
| SCS-C1 | γ #6761 + ε #6763 + ι #6767 | 3→1 |
| SCS-C2 | δ #6762 + ο #6774 | 2→1 |
| SCS-C3 | ζ #6764 + η #6765 + θ #6766 | 3→1 |
| SCS-C4 | μ #6770 + ν #6771 + ξ #6772 | 3→1 |
| SCS-C5 | π #6776 + ρ #6778 | 2→1 |
| SCS-C6 | σ #6780 + τ #6782 + φ #6785 | 3→1 |
| SCS-C7 | λ #6769 + υ #6783 + χ #6787 | 3→1 |
| SCS-C8 (reused #6768) | κ — uncoded-diagnostics ratchet, dep-free root, unchanged | 1→1 |
| SCS-C9 (reused #6789) | ψ — coverage ratchet promotion, deps rewired | 1→1 |
| SCS-C10 (reused #6791) | ω — PRD close, deps rewired | 1→1 |

22 leaves → 10 tasks (2.2×). Out-of-PRD edges preserved: C1 ← #6758, #6759; C6 ← #6015,
GPS-C1 (was #6803); C7 ← #5403, #5404, #6800, #6806, #5517–#5521.

## SCS-C1 (γ+ε+ι): fixture harness + manifest, divergence baseline, E_*↔DiagnosticCode bridge

**Deps:** #6758 (done), #6759 (in-progress standard leaf — real edge kept). **Priority:** high.
**Files (4):** crates/reify-spec-conformance/manifest.json,
crates/reify-spec-conformance/divergence-baseline.txt, crates/reify-core/src/diagnostics.rs,
tests/prd-gate/fixtures/spec_conformance_directive_block.ri (evidence, kept working)

The Phase-1/2 substrate as one subsystem: γ (//@ directive + //~ annotation harness, hard-fail
on unknown keys, verdict algebra PASS|FAIL|UNPROVABLE|BASELINED|WAIVED, results.json, generated
manifest with regeneration-idempotency gate), ε (shrink-only divergence baseline with the four
gate lanes and L2-marked additions), ι (mnemonic()/from_mnemonic() bridge + reconciler +
uniqueness tests; switch γ's code: resolution to the bridge — an intra-task ordering, γ's
placeholder table lives only inside this task's branch). All D14 seeded-fire self-tests included.
Internal order γ → (ε ∥ ι).

Combined signal: unknown-directive fixture reds naming key+file; hand-edited manifest reds the
drift gate; all four ε red directions fire on seeded mutants; unknown-mnemonic cite reds naming
it; doc-comment/bridge mismatch reds the reconciler.

## SCS-C2 (δ+ο): check-faithful test-support rung + loud-rename migration ratchet

**Deps:** SCS-C1 (δ needs γ+ε). **Priority:** high (δ's).
**Files (2 + first-tranche test files):** crates/reify-test-support/src/helpers.rs,
crates/reify-spec-conformance (parity gate), plus tests/infra registrations and the
conformance-adjacent first-tranche files ο migrates.

δ (fix reify-test-support itself per ruled decision 11: check_faithful::check_source_as over the
four verified infidelity axes; library-vs-binary parity gate with sha-sidecar freshness and
UNPROVABLE-not-skip; seeded-fire divergence) then ο (loud-rename the unfaithful rungs to
bootstrap_*/internal_* with infidelity-axis doc comments; shrinking call-site baseline; migrate
the conformance-adjacent first tranche; no crate-wide rustfmt). Internal order δ → ο.

Combined signal: the parity gate reds on an injected library-vs-binary divergence naming fixture
+ codes; a new loud-rung call site outside the baseline reds; the baseline is strictly smaller
than its seed after the tranche.

## SCS-C3 (ζ+η+θ): PSPEC detector, §9.2 vertical slice + graduation, verify wiring

**Deps:** SCS-C1, SCS-C2 (η runs on δ's rung). **Priority:** high.
**Files (8):** crates/reify-audit/src/lib.rs, crates/reify-spec-conformance/fixtures/9.2,
docs/reify-language-spec.md, docs/notes/conformance-scope-boundary-draft.md, scripts/verify.sh,
scripts/verify-pipeline-infra-tests.txt, scripts/heavy-test-filter-lib.sh, tests/infra/

ζ (PSPEC audit pattern: cite lane hard-gated both directions, coverage lane report-only,
baseline-liveness lane built ONCE and shared with ε's lane (c) — under E3 that shared lane is an
intra-C1/C3 seam: consume C1's liveness determination, do not duplicate it), η (the §9.2 corpus
end-to-end: generated Kleene truth-table enumeration, per-clause verdicts, fix-task+baseline-row
per divergence, the graduation edit, anti-vacuity seeded red — the wave protocol every Phase-3
task copies), θ (verify wiring: fast tier on ordinary nextest, conformance infra wrapper with
same-diff manifest row, decide_scope spec-coupling escape, selective injection row, heavy-tier
plumbing pattern; verify-pipeline-guard full-gate cost expected). Internal order ζ ∥ η, then θ.

Combined signal: PSPEC reds on dangling AND tombstoned anchor cites; per-clause §9.2 verdicts in
results.json with a deliberately-broken fixture redding verify; spec-silent-zones strictly
shorter; both decide_scope directions probed plus a red fast-tier fixture blocking a merge.

## SCS-C4 (μ+ν+ξ): bless governance — request_bless MCP server, merge-gate ledger check, PBLESS

**Deps:** SCS-C1 (μ needs γ). **Priority:** medium.
**Files (5):** crates/reify-spec-conformance (server + bless-ledger/), .mcp.json, tests/infra/,
crates/reify-audit/src/lib.rs

μ (STDIO request_bless server, fresh-context adjudicator, closed taxonomy, never-blessable
semantic layer observed to refuse, hash-keyed ledger, .mcp.json fourth entry surviving the
setup-worktree-debug-port rewrite, vehicle decision Q1/Q2; the dark-factory allowed_tools
config follow-up filed cross-project once the tool name is fixed), ν (diff-scoped offline
merge-gate presence check, seeded-fire both directions), ξ (PBLESS sampled re-adjudication,
trailing never blocking). Internal order μ → (ν ∥ ξ). One subsystem, one reviewer.

Combined signal: a bless attempt on a never-blessable diff is refused and writes nothing; an
approved bless writes the hash-keyed artifact; B11 both directions red on a probe branch; PBLESS
emits sampled findings with disagreements flagged.

## SCS-C5 (π+ρ): grammar accept/reject wave + static-semantics rejection wave

**Deps:** SCS-C3 (wave protocol + verify wiring), SCS-C1 (ρ needs ι's bridge), SCS-C8/#6768
(ρ migrates under κ's ratchet). **Priority:** medium.
**Files:** crates/reify-spec-conformance/fixtures, docs/reify-language-spec.md,
docs/notes/conformance-scope-boundary-draft.md, plus the compiler sites ρ codes.

π (§2/§15–§17 accept/reject at parse phase; §15 EBNF demotion marker + rule-census parity gate —
ship both or neither; §17 keyword census; Lezer ledger left alone) and ρ (§3/§4/§7/§8 typed-code
reject fixtures through the bridge; every asserted rejection observed to fire; silent-accepts
each fixed or baselined with a live owner). Wave protocol per η.

Combined signal: B14 — a grammar.js rule absent from §15 reds the census naming it; per-clause
verdicts for §2/§15–§17 and §3/§4/§7/§8 with typed codes and no unowned silent-accept FAILs.

## SCS-C6 (σ+τ+φ): constraint/@test/purpose wave + stdlib-registry wave + geometry/export wave

**Deps:** SCS-C3, #6015 (registry λ integration gate — τ's hard edge), GPS-C1 (was #6803 —
σ's purpose-seam edge). **Priority:** medium.
**Files:** crates/reify-spec-conformance/fixtures, docs/reify-language-spec.md,
docs/notes/conformance-scope-boundary-draft.md, scripts/heavy-test-filter-lib.sh, tests/infra/

σ (§10/§12.1 + purpose semantics; Indeterminate-is-not-a-pass in-harness; CLI @test rows tagged
deferred pending the driver-contract runner fix), τ (§11 + stdlib-ref derived from the registry
as machine truth, planned-is-not-FAIL), φ (geometry/export property-level probes from re-read
STEP/STL/3MF artifacts, no byte-goldens ever, heavy-tier atoms with same-diff count pins).
Three independent waves sharing the η protocol; internal order free.

Combined signal: per-clause verdicts for §10/§12.1 incl. the Indeterminate pair; per-builtin
rows demonstrably DERIVED from the registry; property-probe verdicts from exported artifacts
within declared tolerances with zero byte-goldens asserted mechanically.

## SCS-C7 (λ+υ+χ): the external-driver tiers — CLI runner, freshness/determinism, cross-driver

**Deps:** SCS-C1 (λ needs γ+ι), SCS-C3, #5403, #5404 (exit-code trust — λ's hard prereqs),
#6800 (DCI σ --json envelope), #6806 (DCI ψ verdict split — χ's edge), #5517 #5518 #5519 #5520
#5521 (resolution-unification leaves — χ's edges). **Priority:** medium.
**Files:** crates/reify-spec-conformance, tests/infra/, scripts/heavy-test-filter-lib.sh

λ (external-driver runner over tier:cli fixtures; sha-sidecar staleness → UNPROVABLE with
reason; --json assertions UNPROVABLE until the DCI egress lands, then light up), υ (§9.3/§9.6 +
the five ruled Ring-1 freshness clauses; per-regime determinism, run-twice-stable for iterative;
cache-transparency as a producible comparison), χ (cross-driver tier: same fixture through
check/eval/GUI/LSP; PARITY-DIVERGENCE verdict naming drivers, distinct from clause FAIL;
absent-substrate arms UNPROVABLE with reason). Internal order λ → (υ ∥ χ).

Combined signal: a stale binary yields UNPROVABLE never PASS/FAIL; cache-vs-cold verdicts agree
per regime with seeded staleness divergence attributed; driver disagreement yields a
parity-divergence verdict naming the drivers.

## SCS-C8 = #6768 (κ) — uncoded-diagnostics ratchet, reused unchanged

Dep-free root, kept standalone deliberately to preserve a parallel dispatch root in the coarse
arm. Deps unchanged (none).

## SCS-C9 = #6789 (ψ) — coverage ratchet promotion + dashboard, reused unchanged

Deps rewired: was {ζ #6764, π, ρ, σ, τ, υ, φ, χ} → now {SCS-C3, SCS-C5, SCS-C6, SCS-C7}.
Text untouched.

## SCS-C10 = #6791 (ω) — PRD close, reused unchanged

Deps rewired: was {all 23 siblings} → now {#6758, #6759, SCS-C1…C7, SCS-C8/#6768, SCS-C9/#6789}.
Text untouched.
