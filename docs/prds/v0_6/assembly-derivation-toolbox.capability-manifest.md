# Capability manifest — assembly-derivation-toolbox

PRD: `docs/prds/v0_6/assembly-derivation-toolbox.md`. Evidence bound 2026-08-26
against main `96041f850b`; symbol evidence grep-verified in the authoring
session. Grammar probes run directly (`tree-sitter parse --quiet`, captured
exit codes) and re-walked by the scoped D3 run (workflow `wf_754c3c43-0cb`).
Machine-readable twin: `assembly-derivation-toolbox.capability-manifest.yaml`.

## A-α — grammar producer

- `novel-arm-absent-today` — grammar-fixture (deliberate failure):
  `tests/prd-gate/fixtures/adt_mirror_of_arm.ri` FAILS `tree-sitter parse`
  (ERROR [3,0]-[9,1], exit 1; probe 2026-08-26). A-α is the named grammar
  producer (G3 resolution b); every syntax consumer is downstream. PASS
  (producer-self).
- `existing-arm-regression-floor` — grammar-fixture:
  `tests/prd-gate/fixtures/adt_relation_verbs.ri` parses today (exit 0, no
  ERROR nodes) — the surrounding sub/relate grammar the new arm must not
  disturb. PASS.
- `contextual-keyword-precedent` — capability→producer: grammar.js declares no
  `word:` rule; `relate`/`joint`/`with`/`priv` landed as contextual keywords
  (the pattern `mirror of`/`image of`/`across`/`under`/`keep`/`exclude`
  follow; `symmetry` reserved by comment). PASS.

## A-β — compiler lowering

- `subdecl-extensible` — `SubDecl` (crates/reify-ast/src/decl.rs) and
  `SubComponentDecl` (crates/reify-compiler/src/types.rs) are the additive
  extension points; keyed-member-overrides compilation
  (crates/reify-compiler/src/entity.rs) is the per-instance-override
  compile precedent. PASS.
- `rejection-diagnostics` — G6 branch 4: every §6 rejection
  (`E_DERIVED_SUB_EXPLICIT_AT`, `E_DERIVED_SUB_LET_OVERRIDE`, cycle,
  disposition path, auto-prototype) is producer-self (A-β delivers and its
  fixtures observe each fire); no rejection is claimed of today's substrate.
  PASS (producer-self).

## A-γ — eval value plane

- `merged-args-substrate` — producer upstream: PRD-0 α #6586 + β #6592
  (hard `add_dependency` edges). Without β, image instantiation realizes
  template defaults (#6592's exact disease) — DAG direction verified: both are
  upstream of A-γ. PASS.
- `transform-algebra` — `transform_compose`/`orient_compose` + inverse/log/exp
  (crates/reify-stdlib/src/geometry.rs, orientation.rs), bit-identical to
  operators; the D2 algebra is expressible today. PASS.
- `instantiation-entry` — `elaborate_child_instance` family
  (crates/reify-eval/src/unfold.rs), the instantiation entry A-γ drives with
  merged args. PASS.

## A-δ — geometry plane + dispositions

- `mirror-op-wired` — `GeometryOp::Mirror` (crates/reify-ir/src/geometry.rs) →
  `pattern_mirror` (crates/reify-eval/src/geometry_ops.rs) →
  `mirror_shape`/`gp_Trsf::SetMirror`
  (crates/reify-kernel-occt/cpp/occt_wrapper.cpp): live end-to-end on the
  op-execute seam. `AffineApply` det<0 is the fallback lowering. PASS.
- `walk-insertion-site` — `walk_placed_realizations` + `ApplyTransform` issue
  point (crates/reify-eval/src/geometry_ops.rs): the single site where the
  improper op composes ahead of the proper world transform. PASS.
- `manifold-rejection` — G6 branch 4: the Manifold adapter stubs mirror
  (crates/reify-kernel-manifold/src/kernel.rs stub arm) — the
  `E_DERIVED_SUB_KERNEL_UNSUPPORTED` rejection is producer-self (A-δ
  delivers it; T14 observes it fire). PASS (producer-self).

## A-ε — OCCT det<0 orientation probe

- `occt-harness` — the OCCT-gated test surface exists
  (crates/reify-kernel-occt tests; `#[ignore = "requires OCCT"]` precedent in
  reify-eval). System OCCT is 7.8 (CLAUDE.md native-deps invariant: reason
  from 7.8 headers, not the reify-deps 7.9 copy). PASS.

## A-ζ — Layer-2 check verbs

- `relation-vocabulary` — `crates/reify-compiler/src/relation_signatures.rs`
  (existing verbs incl. `concentric`; grep-verified) + the
  `crates/reify-eval/src/relate_solve.rs` check path: the registries A-ζ
  extends. PASS.
- `verbs-parse-today` — grammar-fixture:
  `tests/prd-gate/fixtures/adt_relation_verbs.ri` parses (probe 2026-08-26) —
  zero grammar cost confirmed. The vocabulary ENTRY is producer-self (T11-T13
  observe check-mode fire). PASS.

## A-η — rotation builtins

- `builtin-registry` — `crates/reify-compiler/src/builtin_signatures.rs` +
  eval implementations + `docs/reify-stdlib-reference.md`: the cheapest
  extension class (overlay G3 note: builtin additions are "no new syntax"
  leaves). Orientation algebra in crates/reify-stdlib/src/orientation.rs.
  PASS.

## A-θ — GUI derived badge

- `entity-path-tree` — GUI tree/selection is keyed purely on entity-path
  strings (`gui/src/navigation.ts` — derived geometry may legitimately lack a
  source span); debug MCP `store_state`/screenshot channels exist
  (gui/src-tauri/src/debug_server.rs). PASS.

## A-ι — docs-truth bundle

- Same surface as PRD-0 θ (chunks dir, examples_smoke gate, INDEX.md,
  reify-design SKILL.md) — grep-verified there, unchanged. Writable only
  after A-δ/A-ζ land (deps wired; DAG direction correct). PASS.

## A-κ — integration gate

- `observation-channels` — CLI STEP-grep
  (crates/reify-cli/tests/harness_cli/cli_sub_placement_assembly.rs),
  `mesh_stats` (debug MCP), `_RUST_COUPLED_RI_FIXTURES` registration +
  PG-DRIFT: all verified in the PRD-0 manifest, unchanged. PASS.

## A-λ — PRD close

- No capabilities: docs stamp per overlay decompose-close obligations.

## A-μ — Layer-3 capstone [MILESTONE] (standalone task, not a PRD-A decomposition leaf)

- `dep-gating` — `add_dependency` edges + scheduler unmet-deps semantics gate
  dispatchability; `metadata.execution_class="decision"` converts to a
  born-at-L2 human escalation at dispatch (docs/task-authoring.md §4 — routes
  to a HUMAN; this is the one leaf that genuinely requires the design
  authority). PASS.
