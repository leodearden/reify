# review-all program — reify — review-all-reify-20261010 (deliberated 2026-10-12)

Coordination contract for every `/prd` session of this program. Source: `plans/review-all-reify-20261010.md` (rendered) and `plans/review-all-reify-20261010.json` (the record; finding keys, statements, measurements, skeptic notes). Deliberation: one sitting on 2026-10-12, 22 rulings below. Nothing here relitigates a ruling; a `/prd` session that finds the evidence changed cites the ruling and returns the question to the lead.

## Streams

| stream | mode | size/risk | wave | upstream seams | finding keys |
|---|---|---|---|---|---|
| **S11 OCCT crate internals** | agent | M/medium | 1 | none | 10: fk-e6bdef1ef0ae, fk-d9cc4b340a69, fk-9eb90a5a82ff, fk-10c48f52f01c, fk-68d69d7d9ace, fk-d22418138fa2, fk-ba1ea4167dbe, fk-030602c2fd62, fk-baae0532b743, fk-69bd53399b08 |
| **S13 Selector resolution** | agent | M/low | 1 | none | 4: fk-25474a437b00, fk-e984283444b1, fk-bef6f2b4851c, fk-b4af214cc9fe |
| **S14 Solver crates: layering, shared element primitives, one predicate home** | agent | M/low | 1 | none | 14: fk-63218ffb51cd, fk-edde1bcefb9f, fk-bc1e40784fe0, fk-188e6a0adc9c, fk-20c32882531a, fk-8f79315fc7e8, fk-95dd3ef3a20a, fk-c940cb1edad4, fk-824124d1e74e, fk-66ffd82ed984, fk-c497935d4b8a, fk-473bb6240e81, fk-e719c0dc7f0d, fk-813c018bf3cf |
| **S15 Trampoline decode and result build (PVAC)** | agent | M/medium | 1 | none | 15: fk-34754bed8353, fk-b5dc0cd8ec92, fk-340a2cbd265b, fk-cee09de8be7a, fk-f33ff551c66d, fk-a28b1b57ed64, fk-dc44c395ee01, fk-d30cf96fc557, fk-9082d3765668, fk-da538013331f, fk-2ac688334abf, fk-c02d5d2d8515, fk-4ac205f76343, fk-90433ba69658, fk-9bc22225093b |
| **S19 run_all runner and infra-test harness** | agent | L/medium | 1 | none | 17: fk-acca603a9628, fk-9c87c8ca99b5, fk-1417bf006761, fk-53a9afc0d2b9, fk-34a184f51e6f, fk-35b618e9b24d, fk-4229ece8d26d, fk-3cb220f9cc94, fk-a86e30ad9ecb, fk-9c99c0223cb3, fk-2e56a7e3fcb7, fk-1c2e90be3491, fk-6213e3dfdbec, fk-75e28dab2f08, fk-6e371c1b8c6f, fk-37507523c365, fk-c796ce463c73 |
| **S20 Warm-lane scripts: extend lib_lane_lock.sh, one env name, one status oracle** | agent | M/low | 1 | none | 9: fk-60aa699ea8ce, fk-05e33a09a266, fk-6fab601d98c2, fk-26e1e3d2eb67, fk-4b80fe8f7de1, fk-cee9ca6b041a, fk-7dba1d7f3f47, fk-d30433df0ed6, fk-638f0a64b948 |
| **S21 Native-dep table and the OCCT directory decision in lib_gui_launch.sh** | agent | S/low | 1 | none | 3: fk-ce79499430e2, fk-765ff49d2de5, fk-e05e85e98f91 |
| **S22 PRD probe gate: typed outcomes and one path classifier** | agent | M/medium | 1 | none | 7: fk-d75787147427, fk-1922380bc3aa, fk-bf4e8bbb7dda, fk-6a006c36a78d, fk-f34f07d2dde4, fk-21c29ddc104d, fk-0497ab17e729 |
| **S24 reify-audit: typed Finding, shared recognisers, registry-status lane** | spawn | L/medium | 1 | none | 10: fk-33f9891ad16c, fk-0b1d530f0264, fk-ec468bfa673a, fk-c45888cf175b, fk-bdc845122da0, fk-20ce222ff90c, fk-31eb23b6b93f, fk-422bbd7986a6, fk-ba93c26e42d1, fk-d668027f44f2 |
| **S27 Tooling front end** | agent | M/medium | 1 | none | 15: fk-f5264d76e3ad, fk-41819ea934bd, fk-b420a7695f68, fk-3a3ae797f979, fk-1a772d160168, fk-74ce7cce677f, fk-5854202d4aed, fk-d9b1ed83ed62, fk-7b9fad8a5d00, fk-2da836d68189, fk-677d67cb2ac8, fk-bed8cadb429f, fk-3e8bf5666555, fk-10941c98934e, fk-972b7d28a290 |
| **S3 Dependency and invalidation stratum** | agent | M/medium | 1 | none | 9: fk-f0a930933a38, fk-919c3ad5cc92, fk-73145187f8a6, fk-013bfbef8746, fk-9063f57c1daf, fk-f0961047b0a6, fk-8188fcb77d66, fk-7378f0be931f, fk-23c462ee8918 |
| **S6 Kinematics: typed Joint, Relation kind and mechanism record** | spawn | L/medium | 1 | none | 20: fk-4543a3a88865, fk-6a6bbd16da97, fk-337e4f0bcfa9, fk-f584626a8627, fk-14c3de3a675c, fk-02810f753a67, fk-90837877eee7, fk-9b8484939308, fk-241b66aa0262, fk-3cfd0be186c0, fk-49105d89fd7b, fk-691b37115ddf, fk-98f62f3f0562, fk-7bfc9a5014e2, fk-ca97659733e9, fk-45c54d189935, fk-6467cfff64bc, fk-a9b0785fd293, fk-726aff24a191, fk-38811e6f7300 |
| **S7 Compiler module namespace** | agent | L/low | 1 | none | 11: fk-28fab6d836c8, fk-78d6c776ba64, fk-6460b80f5442, fk-120b6255ec1c, fk-a9fb39978c17, fk-eeae7ace09ad, fk-bde20c8c3ed4, fk-fc9a2fe8498b, fk-39797230491b, fk-6432e37b9f24, fk-6875d651562b |
| **S9 Compiler declaration passes: one classification per member, one acceptance relation** | agent | M/medium | 1 | none | 15: fk-0bdf9598b402, fk-196deab564e7, fk-30505ec518b4, fk-ee1be91862ca, fk-27489259525d, fk-60053402aba5, fk-6621fbaa0058, fk-cf6356374fa1, fk-254bc351a8ab, fk-37775e022693, fk-cc676a29f5d9, fk-75d9313c6ee6, fk-cb941b3d058e, fk-76bdd62534fe, fk-ddfdce77efbc |
| **S10 Kernel interface: typed replies, total delegation, one selector** | spawn | L/high | 2 | S11 (ShapeStore, op lowering), S6 (typed kinds in reify-ir) | 21: fk-047cb467bd07, fk-ef5f5da8f729, fk-a01d863c73ba, fk-1d857a1e9750, fk-fdb4e221350a, fk-59c46755d24c, fk-0375628ba585, fk-66c1d2fe9ff9, fk-80c2f82c17d6, fk-e6a266a72f41, fk-107e28de9029, fk-92d313d2986f, fk-a0ce1853f2da, fk-57ffd6bd2af2, fk-08aa3fff3378, fk-7b093573458b, fk-448317051ff9, fk-65bb1ffb5d60, fk-10ec3d2a1b76, fk-0f933e75bf7d, fk-7bfef234d79e |
| **S12 Check surface: structured constraint subject and explicit measurement context** | spawn | M/medium | 2 | S27 (compile_user_module), S9 | 14: fk-e9cceaaac89a, fk-88643a4cc993, fk-c944ff1a913b, fk-d98e178717e6, fk-083c497dc973, fk-72f83d1ef8c0, fk-23f520f22f28, fk-747e27932117, fk-bf1370c79e89, fk-c24f5d7302ed, fk-9f2a530a6921, fk-925d0004d647, fk-e5a129d3cc24, fk-4718cc84e52a |
| **S18 verify.sh: structured plan steps and one role table** | spawn | L/high | 2 | none hard; S19 event log is a sibling, kept separate | 13: fk-507b17f8adc7, fk-21149d55fd41, fk-73c909381788, fk-03d27a868cfd, fk-b072e820e926, fk-fc5ee820d36e, fk-d47473e4d8d8, fk-902a5ad9889e, fk-6ccbfc296ee8, fk-7b476f26ad45, fk-763b633b2775, fk-3988a6996b82, fk-7bc1c1449010 |
| **S25 GUI backend: engine.rs modules, one event sink, EvalQueue for debug writes, shared tool contract** | spawn | L/high | 2 | S24 (typed Finding lane is unrelated) — none hard; shares types.rs with S26 so goes first | 12: fk-05311be442e6, fk-b28eb5276628, fk-4ecb76174487, fk-2dcacd59c79d, fk-995c30a8a446, fk-55213deb2362, fk-e2b409ec7580, fk-58f6f0d58857, fk-c4da71a0014e, fk-7212abff873d, fk-bb84d4272cfd, fk-cc4329a75e3e |
| **S4 Cell-eval transaction closure (INV-EVAL-1/2/3)** | agent | L/medium | 2 | S3 (expr_classify stratum) | 19: fk-32d11c0fd0ab, fk-3ee0f9a51f25, fk-d1ebb866f47b, fk-be356be3cbd5, fk-a6cb97b97c14, fk-0ff49d22cff9, fk-6f316376a4ba, fk-8dd6a16ea574, fk-263246810f8f, fk-de6e6f9e6b14, fk-9915c9230418, fk-82271f54167a, fk-4c92d87f92ce, fk-3ca766d13afa, fk-5534150bb053, fk-d44472dae647, fk-42181a362efb, fk-e8a47806e557, fk-2ec6ef96fa1c |
| **S5 Analysis stratum: compute targets in one crate, contract below the solver** | spawn | L/medium | 2 | S15 (result builder, typed decoders) | 11: fk-6e78ea52b691, fk-8cd376f72fca, fk-250d45d9d1f4, fk-34bddf1fcbce, fk-7d4f2be7b634, fk-d870b2b3bfdf, fk-e6650962065a, fk-a0d2e755b215, fk-952285bb817b, fk-750e0c9f3bd6, fk-cf610e43d544 |
| **S8 Builtin registry migration and one interception classifier** | spawn | L/medium | 2 | S7 (explicit imports; builtin tables out of units.rs) | 23: fk-33801a7c7a36, fk-935b6eca2b50, fk-5a6c2aff4589, fk-ffa4ba4c2d0b, fk-eeebffcc4a83, fk-ea33d4b5d69d, fk-2e03b9202f6d, fk-a494d2285710, fk-d402f7243603, fk-be87290e126e, fk-a72f4510f36b, fk-561a5a490532, fk-7bfe957c7f09, fk-ecc0b4fb9d6b, fk-76e1dc7c434e, fk-7e3c3d10ae46, fk-eca29487b2c9, fk-419ebb996480, fk-81f949615552, fk-4bd71b633033, fk-e187a67f4b32, fk-2845dd9262b4, fk-07c78c4ca710 |
| **S1 Engine state partition** | spawn | L/high | 3 | S4 (cell-eval closure), S3; S5 (analysis crate) for the KernelSet/registration posture | 12: fk-800d61acd5d0, fk-ba15d44904cb, fk-d8fd9ed9be89, fk-cb6c853cb648, fk-e9536a3edbe7, fk-8250a05a9a10, fk-fb8cf3e787b9, fk-3aeb721eaaa6, fk-d2c64e9d3868, fk-4fa7bda0085b, fk-4db43199eda2, fk-06103e5f06eb |
| **S16 elastic_static and modal trampoline factoring; mesh-morph observability** | spawn | L/medium | 3 | S15, S5 | 12: fk-3f6ee7ecd393, fk-9facd9401295, fk-603bbbab0515, fk-cecdff4f1373, fk-7687550ec80f, fk-43921c86c7d3, fk-baa063db2052, fk-12285c864993, fk-d49d3ac15f14, fk-acc10f2161f6, fk-d632489a4c98, fk-e3acedd1c27a |
| **S2 Realization pipeline: one per-template walk, typed executor** | spawn | L/high | 3 | S1 (PerBuildState, EngineConfig) | 5: fk-f7e4716af878, fk-53997e96ef7c, fk-17e2aa81f6d6, fk-98754d80a67e, fk-9e6d548589d8 |
| **S26 GUI frontend: generated wire types, bridge port, schema-validated handlers** | spawn | L/medium | 3 | S25 (types.rs enums, SessionEvents) | 13: fk-2be83576bc8b, fk-252aa009dcdc, fk-559caca373a7, fk-e0f49abf4b34, fk-b2129b18b706, fk-7262d24da57d, fk-c8938e374057, fk-a03c96e5733d, fk-39619f02ae9c, fk-068d91243153, fk-0b62433f5807, fk-b131e38ebad7, fk-5067404e6b75 |
| **S23 Prose rehoming and symbol citations** | agent | L/low | 4 | S24 (PCITE lane) and every code stream that moves prose | 29: fk-465b5eec51e2, fk-4186fb4f7289, fk-cb27e6ba131b, fk-91aaee6b8fde, fk-5f3dcd2ae885, fk-8c68164d0151, fk-25c46b3f4c72, fk-632e4850d7d0, fk-e4b87a3c5f90, fk-b0a7b8818be4, fk-e90cf61da43a, fk-d0569c316d46, fk-7fbe91c20e82, fk-9e30579d2c52, fk-70d435aa0170, fk-3ac9bc99b963, fk-67ca81ff9067, fk-22068b67e813, fk-679840f73698, fk-eb5a9e37eb9f, fk-15e94643506d, fk-2e1e82c1d1c6, fk-e2730bcb8111, fk-a401e42a7b49, fk-420be0954017, fk-863955f49c19, fk-5b6284e261af, fk-a26634db4007, fk-14195f0a11d2 |

No key appears in two streams (the synthesis assigned each canonical once; the digest verified it).

## Waves (lead-sequenced)

- Wave 1 — no upstream seam; agent streams run as one /team, spawn streams as spawned sessions: S7, S3, S15, S19, S20, S21, S22, S27, S9, S13, S14, S11, S24, S6
- Wave 2 — consumes wave-1 seams: S4, S5, S8, S12, S10, S25, S18
- Wave 3 — consumes wave-2 seams: S1, S16, S26, S2
- Wave 4 — burn-down after the code streams have moved files: S23

Within a wave, reify-eval streams land in the order S3 → S4 → S1 → S2; compiler streams S7 → S9 → S8 → S12; analysis S15 → S5 → S16; kernel S11 → S10; GUI S25 → S26; infra S19/S20/S21/S22 in parallel, S18 alone (every file is a verify-pipeline file: full gate per landing).

## Seam ownership (G4)

| shared artefact | owner | consumers |
|---|---|---|
| Engine constructor signature (reify-cli main.rs 9 sites, mcp_context, reify-lsp 4, gui 3, test_runner, 24+ test files via cache_store_mut) | S1 | — |
| scripts/check-compute-trampoline-registration.sh KNOWN_SITES | S1 | S5 |
| docs/invariants.md INV-FEA-1 / INV-BUILD-1 rows | S1 | — |
| RealizationOpsInput/RealizationOutputs (compute-contract consumers) | S2 | — |
| engine_build <-> geometry_ops placed_walk cycle (realize_sub_override_handles passed as closure) | S2 | — |
| DispatchTestState scenarios in engine_build/tests.rs | S2 | — |
| registry_drift_tests.rs pins on geometry_ops predicates | S3 | — |
| INV-EVAL-6 owner (#5023) for propagate_freshness_only: wire or delete is that milestone's decision, not this stream's | S3 | — |
| docs/invariants.md INV-EVAL-1/2/3 status cells (one owner: the stream) | S4 | — |
| cache_divergence.rs consumer of EvalEvent payload | S4 | — |
| INV-EVAL-2 ratchet (invariant candidate 2) | S4 | — |
| crates/reify-compute-contract Cargo dependency on reify-solver-elastic (cargo-tree assertion; must flip before the slab codec can be shared with reify-shell-ext | S5 | — |
| docs/prds/v0_3/compute-node-contract.md (registration contract owner) | S5 | — |
| Home of JointKind/RelationKind (compiler joint_signatures and relation_signatures + conformance also match the names) | S6 | — |
| JointValue flat-storage width contract with sweep::build_snapshot_list and dark-factory-visible .ri warm-start rows | S6 | — |
| #7188 SingleDofKind and #7974 consolidation tasks | S6 | — |
| crates/reify-compiler/src/lib.rs re-export block (one owner for the migration order) | S7 | — |
| reify_compiler::satisfies_trait_bound public surface (consumed by S12's conforms_to_trait deletion) | S7 | — |
| reify-builtins row schema (compiler, eval, LSP, reify-audit RI_KEYWORDS all read it) | S8 | — |
| reify_expr::eval_builtin dispatch contract with reify-stdlib | S8 | — |
| docs/prds/v0_6/builtin-signature-registry.md legs beta..omega | S8 | — |
| TopologyTemplate::all_value_cells() (shared with S27's value-cell consumers) | S9 | — |
| MemberDecl variant additions (reify-ast) and their five dispatch sites | S9 | — |
| GeometryKernel trait (27 methods; 79 implementors incl. MockGeometryKernel) | S10 | — |
| reify-test-support assert_kernel_contract! and MockGeometryKernel (1,306 references) | S10 | — |
| reify-kernel-conformance producer x consumer matrix (INV-GEO-1..4 rows) | S10 | — |
| OcctKernelHandle public surface vs stubs.rs mirror (24/68 and 16/33 today) | S11 | — |
| occt_wrapper.cpp accessor set | S11 | — |
| reify_core::Diagnostic shape (every crate constructs it) | S12 | — |
| Engine::check signature (production callers: CLI heavy and light paths, LSP analysis/diagnostics, GUI preview engine) | S12 | — |
| INV-SF-6 diagnostics-carry-codes row | S12 | — |
| geometry_ops/tests.rs rewrite boundary shared with S2's placed_walk extraction | S13 | — |
| ElementStiffness.data direct indexing (72 sites in 9 files) | S14 | — |
| tilted_q_for_shell_tests fixture reached from 6 modules | S14 | — |
| reify-constraints solver predicate exports consumed by reify-eval engine_eval.rs | S14 | — |
| crates/reify-compiler/stdlib/*.ri structure defs as the SPOT for defaults (reify-audit PVAC/PDROP owners) | S15 | — |
| reify_ir::arg_acceptance (shared with kernel_validation per S10) | S15 | — |
| compute_targets/mod.rs sampled_*_field helpers reached back into by elastic_static (move to a leaf sampled_channels.rs) | S16 | — |
| CantileverAdaptiveProblem private fields read by the trampoline | S16 | — |
| reify-mesh-morph eligibility seam (compose_morph) as the single public entry | S16 | — |
| scripts/verify-pipeline-paths.txt and the config-only fast path (every file here is a verify-pipeline file: full gate per landing) | S18 | — |
| dark-factory consumers of @@TOKEN@@/Summary/FAILED lines and DF_VERIFY_ROLE | S18 | — |
| tests/infra/plan_capture_lib.sh | S18 | — |
| dark-factory's slot-timeout and Summary/FAILED consumers | S19 | — |
| run-all-classification.manifest rows for any new wrapper | S19 | — |
| tests/infra/README.md Python migration policy (new members are Python) | S19 | — |
| dark-factory git_ops.py::_seed_script_supports and _seed_warm_lane (cross-repo lock handshake and flag probe) | S20 | — |
| orchestrator/lane_lifecycle.py and plan_tools.py serialisers (golden-record owner) | S20 | — |
| scripts/lib_lane_lock.sh (one owner for probe, path and acquire) | S20 | — |
| scripts/check-manifold-deps.sh gate arms (INVARIANT: one arm per NativeDep variant, pinned by test_occt_deps_preflight.sh) | S21 | — |
| scripts/lib_gui_launch.sh ordering contract (pin step last) once it also owns the OCCT dir prepend | S21 | — |
| dark-factory merge queue's import of reify_overlap_detector (needs DF's orchestrator package) | S22 | — |
| the Enumerator/Synthesize prompts (schema text in prose) | S22 | — |
| reify-audit line-cite lane: PLINECITE (#6935 → S24 ο #8822; NOT a PCITE extension, S24 ruling R1) — invariant candidate 3's lane; S23 burns down its ledger | S24 (#8822) | S23 |
| docs/invariants.md Pointer column + design-invariants census Gate grammar (S24 PRD §5.7) | S24 (#8821) | every stream that flips a registry status |
| crates/reify-audit/plinecite-baseline.txt + plinecite-baseline-gen --rename | S24 (#8822) | every stream that moves a file under PLINECITE scope |
| docs/invariants.md status cells touched by the false-claim corrections (S24 owns the checker) | S23 | — |
| .claude/skills/audit/references/severity-routing.md and cli-invocation.md (consume Finding fields and exit codes) | S24 | — |
| crates/reify-audit/ptodo-baseline.txt fingerprint format (merge gate) | S24 | — |
| docs/invariants.md and docs/legibility/design-invariants.md row format | S24 | — |
| gui/src-tauri/src/eval_queue.rs SnapshotPublisher (#7854 owner) | S25 | — |
| docs/debug-mcp-contract.md and the sidecar allowlist (mcp__reify-debug__*) | S25 | — |
| reify-mcp ReifyToolContext trait (CLI and Tauri hosts) | S25 | — |
| gui/src-tauri/src/types.rs as the generation source (shared with S25) | S26 | — |
| gui/test/visual/assertions.ts KNOWN_DEBUG_TOOL_NAMES and sidecar system-prompt.ts tool tables | S26 | — |
| reify_compiler compile entry points (compile / compile_with_stdlib / compile_with_stdlib_checked): one owner for the front-end function | S27 | — |
| gui lsp_bridge.rs lane_for_method and gui/src/editor/lspClient.ts (consumers of LspMethod) | S27 | — |

Rule: one stream edits a shared artefact; the others consume its filed leaf ids (held leaves below).

## Promoted invariants (pointers only; the registry is docs/invariants.md + docs/legibility/design-invariants.md)

- INV candidate 1 → owner S24: An invariant registry row's status is derived from an enforcement pointer that resolves, never hand-kept (INV-META-1, existing; f316). — checkable form in the record §synthesis.invariant_candidates[0]; promoted via the four-site lockstep edit by the owning stream. **Filed:** S24 ξ #8821 (reify-audit --pattern PREGISTRY; INV-META-1 → partial: enforced(audit) for registry status; held until S4/S1/S10/S25 record their owning leaf ids on it).
- INV candidate 2 → owner S4: Engine cell-eval contexts are built only through cell_eval_ctx (INV-EVAL-2, existing; mechanism corrected from 'type' to 'type+lint'; f229, f275, p7, p20). — checkable form in the record §synthesis.invariant_candidates[1]; promoted via the four-site lockstep edit by the owning stream.
- INV candidate 3 → owner S24 + S23: Tracked source prose never cites code by file:line; pointers are symbol paths (f95, f109, f122, f34, f44, f19). — checkable form in the record §synthesis.invariant_candidates[2]; promoted via the four-site lockstep edit by the owning stream. **Filed:** S24 ο #8822 as INV-DOC-1 via PLINECITE (ruling R1), not a PCITE extension; S23 burn-down leaves depend on #8822.
- INV candidate 4 → owner S5 + S15: Every Rust-written result structure and every @optimized target agree with their .ri declarations (INV-PD-2 result-fields-populated-or-owned and INV-PD-1, existing; the chartered PVAC/PDROP detectors; — checkable form in the record §synthesis.invariant_candidates[3]; promoted via the four-site lockstep edit by the owning stream.
- INV candidate 5 → owner repo (/prd overlay G-gate): An extracted primitive lands with an adoption ratchet: a shared helper that replaces N inline copies ships with a count test on the inline pattern, so the 3-of-N abandonment shape cannot recur (p20, u — checkable form in the record §synthesis.invariant_candidates[4]; promoted via the four-site lockstep edit by the owning stream.
- INV candidate 6 → owner S10: A delegating GeometryKernel wrapper forwards every trait method (extends INV-GEO-4's shared-contract suite; f210, f84, p14). — checkable form in the record §synthesis.invariant_candidates[5]; promoted via the four-site lockstep edit by the owning stream.
- INV candidate 7 → owner S3 + S7: No intra-crate module strongly-connected component grows; the measured SCCs (reify-eval 24 modules, reify-ir 4, reify-compiler peer cycles) shrink monotonically (f324, f328, f240, f329). — checkable form in the record §synthesis.invariant_candidates[6]; promoted via the four-site lockstep edit by the owning stream.
- INV candidate 8 → owner S12: Constraint diagnostics carry their subject structurally; no consumer parses message prose to recover a constraint id (INV-SF-6 diagnostics-carry-codes and dark-factory INV-2 structured-facts-at-failur — checkable form in the record §synthesis.invariant_candidates[7]; promoted via the four-site lockstep edit by the owning stream.

## Resolved decisions (do not relitigate)

### S1 Engine state partition
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream. Front ends obtain an Engine through ONE constructor taking EngineConfig plus an explicit KernelSet and a Registration posture enum; Engine's 67 fields are partitioned into owned sub-structs by lifetime class (EngineConfig, PerBuildState with reset(surface), EvalArtifacts, PurposeState, Caches, Instrumentation); per-call outputs return on EvalResult/BuildResult/TessellateResult.
- Options considered: one constructor + posture enum; named posture constructors (batch-CLI / interactive-GUI / LSP-analysis); accept as standing
- Consequence that decided it: Every caller is edited once; a new front end cannot forget a posture because posture is data the engine can print and the registration lint can read; a new knob is one EngineConfig field instead of a new constructor and a lint-table row.
- Keys: all 13 S1 finding keys (see report §Candidate streams S1)

### S2 Realization pipeline: one per-template walk, typed executor
- **Ruling** (human, 2026-10-12): Proceed as specified, one spawn stream: SurfacePolicy built from BuildSurface drives one per-template walk shared by the four drivers; execute_realization_ops becomes a free-function executor module with per-op OpOutcome, one function per terminal conversion and one commit/rollback owner; BuildScheduler::LegacyMultiPass retired so the schedule is non-optional; produced_repr write-back through one sink on every surface; project_realization_read_handle gets one home.
- Options considered: proceed as specified; split: retire LegacyMultiPass as a separate first leaf; accept as standing
- Consequence that decided it: One walk and one executor: a new per-realization knob or terminal conversion is one policy field or one function, not four hand-copied drivers and a 2,300-line body; 3 drivers, 8 option gates and 124 legacy-scheduler test pins become deletable.
- Keys: all 5 S2 finding keys

### S4 Cell-eval transaction closure (INV-EVAL-1/2/3)
- **Ruling** (human, 2026-10-12): Proceed as an agent stream. Engine::edit_source is DELETED together with its 15 test files' calls and the twin instrumentation; one warm-edit path on edit_param's phase methods. eval_ctx_with_meta becomes private to cell_eval_ctx.rs and every cell-eval site routes through cell_eval_ctx; CacheStore::record_evaluation* become pub(super) so commit_cell_result is the only value-cell write; EvalEvent gets a typed cache-leg field; every value-scan detector becomes a PostPassDetector with declared per-path applicability; one per-cell elaboration primitive shared by template scope and unfold's instance scope; the dead Engine::cell_eval_ctx and its parity test go.
- Options considered: delete edit_source; keep edit_source rebuilt over shared phase methods; accept as standing
- Consequence that decided it: INV-EVAL-1/2/3 stop being opt-in: a new cell-eval site cannot omit containment or the runtime sink, a new value-cell write cannot skip a leg, and a future warm source edit is built on edit_param's phase methods rather than a second body that drifts.
- Keys: all 21 S4 finding keys

### S5 Analysis stratum: compute targets in one crate, contract below the solver
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream. Compute targets move to a NEW analysis crate (depending on compute-contract, solver-elastic, fdm, shell-extract and stdlib), registered through an injected table like MorphRegistration; a per-target descriptor (typed TargetId, ComputeFn, persistence codec, significance comparator, completion hook, cancellation channel) registered once in ComputeDispatchRegistry, from which PERSISTABLE_TARGETS, persistent lookup/write, the LSP target list and the .ri @optimized parity test derive; reify-compute-contract keeps only the contract types and drops its dependency on reify-solver-elastic; one cancellation channel.
- Options considered: new analysis crate; keep targets in reify-eval behind a module boundary; accept as standing
- Consequence that decided it: Analysis code gets one stratum: reify-eval loses its reify_fdm dependency and 8 of 13 solver-elastic sites, the compute contract no longer sits above the solver, and a new target is one module in one crate instead of a stdlib half plus an eval half.
- Keys: all 11 S5 finding keys

### S6 Kinematics: typed Joint, Relation kind and mechanism record
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream. The Joint and Relation kind enums live in reify-ir (below compiler, constraints, stdlib and eval); a typed Joint enum (one variant per kind, carrying unit axes/ranges) with exhaustive methods and Joint::from_value as the single parser; a typed RelationKind replacing RelationInstance.name string matches; a typed LoopClosure record classified at construction; the Newton-state width computed by one helper (fixes the snapshot warm-start width defect); stdlib's loop_closure_value::JointKind becomes a re-export during migration.
- Options considered: reify-ir home; reify-stdlib loop_closure_value.rs home; accept as standing
- Consequence that decided it: One definition reachable from every consumer, so the compiler's joint_ctor_result_type and conformance tables derive from it; adding a joint kind becomes one enum variant with exhaustive methods instead of ~20 string tables and 26 drift loops.
- Keys: all 20 S6 finding keys

### S7 Compiler module namespace
- **Ruling** (human, 2026-10-12): Proceed as an agent stream: replace the 28 crate-root `pub(crate) use X::*` globs and 25 `use super::*` imports with explicit `use crate::module::{names}`; move the builtin tables out of units.rs into builtin_families.rs; a ResolutionCtx argument replaces the three thread-local resolution scopes; CompilationScope's sub_* maps become one per-sub record behind query methods; Type::children()/any_descendant() in reify-core replaces the three identical walkers.
- Options considered: proceed; accept as standing
- Consequence that decided it: Reading one compiler file names where its callees live; the hidden module cycle becomes visible and lintable; 28 globs, 25 glob imports, 3 thread-locals and 4 shims become deletable; reify-constraints drops its reify-stdlib Cargo edge.
- Keys: all 11 S7 finding keys

### S8 Builtin registry migration and one interception classifier
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream finishing the builtin-signature-registry PRD: one reify-builtins row per builtin carrying static facts (name, arity, arg slots, result type) PLUS a typed intercept-kind / diagnose-hook column consumed by reify-expr (reify-expr gains a reify-builtins dependency); the 22-arm ladder, 30 name slices, geometry_arg_indices, compile_geometry_call_inner, selector maps, LSP completion/hover signatures and the reserved-keyword list derive from the rows.
- Options considered: static facts + typed intercept/diagnose column; static facts only with reify-expr keeping its own classifier enum; accept as standing
- Consequence that decided it: A new builtin is one row; dual_eval and the compiler lint read the same classifier; the pairwise-disjointness test suite that compensated for the missing registry becomes deletable.
- Keys: all 25 S8 finding keys

### S10 Kernel interface: typed replies, total delegation, one selector
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream. Total delegation is enforced by capability sub-traits exposed through Option-returning accessors on GeometryKernel (choice 1); GeometryOp quantity fields are decoded ONCE into typed SI newtypes built only through the eval-layer gate (choice 2); Mesh-consuming trait methods take &ValidatedMesh; GeometryQuery returns typed replies so the JSON tokenizers and wrappers go; one kernel-selection function in dispatcher.rs.
- Options considered: sub-traits via accessors vs flat trait + macro-generated holder/parity test; typed SI newtypes at the eval gate vs Value everywhere through a shared extract macro; accept as standing
- Consequence that decided it: A new capability is a new sub-trait and a wrapper cannot silently lose a method (9 missing forwarders become unrepresentable); one decode point so a new op cannot choose Value vs f64 and kernels no longer need the warn-only tripwire.
- Keys: all 22 S10 finding keys

### S16 elastic_static and modal trampoline factoring; mesh-morph observability
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream (ordered after S15's result builder and S5's crate move): explicit overload entry points instead of Value sniffing; the trampoline split into decode -> typed request, a route enum over (shell/tet x adaptive lane), per-route solvers and one result builder; CantileverSolveRequest replaces the 14-positional-arg internal; modal_ops.rs split along its measured partition; mesh-morph's morph()/stats.rs retired.
- Options considered: proceed; accept as standing
- Consequence that decided it: A new FEA route or option is one enum variant and one solver function; 20 positional-None test calls and 47 private-name test imports go once seams exist.
- Keys: all 12 S16 finding keys

### S19 run_all runner and infra-test harness
- **Ruling** (human, 2026-10-12): Proceed as an agent stream: one _ra_run_member (spawn, scrub, capture, sanitize, rc, fail detail) called by every branch; discovery from classification_discovered_set only; a structured per-member event log (REIFY_RUN_ALL_EVENT_LOG) that tests and the slot-timeout classifier read instead of grep-able log lines, with dark-factory reading the same channel; harness guard engines moved to sourced libs.
- Options considered: proceed; accept as standing
- Consequence that decided it: 3 of 4 branch copies, Sections D-G of test_slot_timeout_marker (3,413 lines), 233 skip-mirror asserts and 89 source-shape checks become deletable; a new infra member cannot leak the slot-timeout token by construction.
- Keys: all 17 S19 finding keys

### S23 Prose rehoming and symbol citations
- **Ruling** (human, 2026-10-12): Proceed as an agent stream, area by area: rewrite the ~700 file:line citations to symbol pointers; move reviewer-answering prose into the PRD/docs section, commit message or task record it belongs to, leaving a one-line pointer; correct the measured false claims (e.g. INV-BUILD-1 status).
- Options considered: proceed in full; citations and false-claim corrections only; accept as standing
- Consequence that decided it: Comments stance restored where seats measured prose as the majority of production files; 334 infra and ~150 crate line cites, 14 plan.json pointers and 136 per-variant derive tests become deletable.
- Keys: all 29 S23 finding keys

### S18 verify.sh: structured plan steps and one role table
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream whose TARGET is all Python: a typed plan (step records with kind, argv fragments, gate path) and the executor both in Python; scripts/verify.sh stays as a one-line exec shim so hooks, dark-factory-orchestrator.yaml and scripts/verify-pipeline-paths.txt keep their contract; the text --print-plan, @@TOKEN@@ lines and Summary/FAILED lines remain byte-stable projections of the typed plan until dark-factory migrates to the JSON form; a plan-parity test runs old and new over every role x scope x profile before a one-commit cutover under the full gate; the 67 infra tests that parse verify.sh's source text are replaced by Python structured-query tests per the infra-test policy; run_all.sh stays out of this stream (S19 stands).
- Options considered: all Python with verify.sh as an exec shim; Python planner first with a bash executor, executor later; bash associative-array records plus JSON-lines --print-plan
- Consequence that decided it: One language and one typed plan for the gate's policy (roles, profiles, scopes, walls, admission); bash's only argument was migration risk, which the parity test and the shim absorb; a hybrid would have been a detour since the executor is the small part.
- Keys: all 13 S18 finding keys
- Note: Leo's framing: verify.sh is far bigger than a bash script should be; the choice is Python planner vs all-Python, and bash brings nothing durable.

### S24 reify-audit: typed Finding, shared recognisers, registry-status lane
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream; --since, TimeWindow, producer_branch and the /audit skill's --since mode are DELETED (sweeps are always whole-corpus; a future window is designed deliberately). Finding gets a typed kind enum, a required located EvidenceRef and a subject enum; fingerprint and sort keys derive from fields; one source-shape recogniser module; fallible GitOps; the registry-status lane (invariant candidate 1).
- Options considered: delete --since and the mode; implement the window at task-metadata load; accept as standing
- Consequence that decided it: No inert flag that silently disables P2's backlog guard; the ptodo-baseline fingerprint and routing stop parsing summary text (a release build today sorts G-allow findings at line 0).
- Keys: all 10 S24 finding keys

### S3 Dependency and invalidation stratum
- **Ruling** (human, 2026-10-12): Proceed as an agent stream: the nine pure predicates move to expr_classify.rs importing only reify_ir/reify_core so deps, invariants, resolve_order and realization_staleness import downward (the calibration skeptic measured the 24-module SCC shrinking to about 5, not dissolving; the stream records the residual cycles). Under #5023, propagate_freshness_only and freshness_walk.rs are DELETED; async completions mark dependents dirty through the single dirty cone, which satisfies INV-EVAL-6.
- Options considered: delete the walk, dirty cone only; wire it under #5023 as one completion transaction; leave to #5023's owner
- Consequence that decided it: One invalidation mechanism and no retained parity obligation; the gating.rs dead code the briefing lists goes with it.
- Keys: all 9 S3 finding keys

### S25 GUI backend: engine.rs modules, one event sink, EvalQueue for debug writes, shared tool contract
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream with the MCP tool clause STRENGTHENED: each reify-mcp write tool has ONE implementation written over the ReifyToolContext host trait (CLI and Tauri hosts implement the trait only); the GUI debug server delegates the five reify_* tool names to that implementation instead of re-implementing them; the dead mcp_tool_call Tauri command path is deleted; reify_set_parameter's value grammar is settled once in the typed request/response pair owned by reify-mcp. The rest as specified: engine.rs split along its measured partition, one reload() and one GuiState builder, the eight emitters collapsed into one SessionEvents sink, debug-server writes through the EvalQueue.
- Options considered: one implementation per tool over the host trait (strengthened); shared typed contract with two host-side dispatchers as the synthesis wrote
- Consequence that decided it: No tool logic exists twice, so the two surfaces cannot diverge on substance again (today they disagree on whether a bare number is accepted); parity tests that transcribe key sets go.
- Keys: all 14 S25 finding keys

### S26 GUI frontend: generated wire types, bridge port, schema-validated handlers
- **Ruling** (human, 2026-10-12): Proceed as a spawn stream with specta + tauri-specta: wire types, the 34 Tauri commands and the events are generated end to end into a committed bindings.ts guarded by a regenerate-and-diff parity test (the GUI test script runs tsc without building Rust); bridge.ts becomes thin adapters over the generated bindings and serves as the typed bridge port App and engineStore take; closed-set fields become Rust enums serialised to string-literal unions; one ordered side-panel entry list; viewStateStore owns tree reactivity; debug handlers validate params once at dispatch against one declared schema per tool.
- Options considered: specta + tauri-specta; ts-rs (types only)
- Consequence that decided it: A renamed command, a changed argument or a new event fails tsc instead of surfacing at runtime; the 11 wholesale vi.mock factories, the source-parsing mock contract and the regex-over-Rust parity helpers become deletable; dependency risk bounded to gui/src-tauri and reversible to ts-rs since both are derive-based.
- Keys: all 13 S26 finding keys

### Uncontested block: S9, S11, S12, S13, S14, S15, S20, S21, S22, S27
- **Ruling** (human, 2026-10-12): All ten approved as specified in the report's §Candidate streams: S9 compiler declaration passes (one LetKind classification per member, member_support table), S11 OCCT crate internals (ShapeStore; execute_with_history owns the op lowering; the split op lowering folded as one medium entry), S12 check surface (structured constraint subject on Diagnostic; no message-prose matchers; CheckEnginePlan/MeasurementContext layered under S27's compile_user_module) — spawn; S13 selector resolution (one compose over SelectorNode), S14 solver crates layering (validated force-density problem, one sign-rule home, shell primitives below assembly), S15 trampoline decode and result build (one typed decoder per stdlib input structure on arg_acceptance), S20 warm-lane scripts (extend the existing lib_lane_lock.sh with lane_lock_path/lane_lock_acquire; one env name; one status oracle), S21 native-dep table (native-deps.toml read by lib.rs and check-manifold-deps.sh; OCCT directory decision in lib_gui_launch.sh), S22 PRD probe gate (typed outcomes, one path classifier), S27 tooling front end (one compile_user_module used by CLI, MCP context, LSP) — agent.
- Options considered: approve all; hold or accept named ones
- Consequence that decided it: Each is one /prd session; seam owners per the G4 table.
- Keys: the streams' finding keys per the report

### Contradictions
- **Ruling** (human, 2026-10-12): The synthesis's 15 self-resolutions are accepted as recorded in the report §Contradictions (one reify-audit PCITE lane owned by S24 instead of four line-cite lints; S20 extends lib_lane_lock.sh; the OCCT directory decision joins lib_gui_launch.sh; the datum-projection table lives in reify-ir under S8; the three check-preparation mechanisms are layered S27 -> S12; f324 and f70 and the four namespace findings re-levelled to medium; p16 unranked and left to #5023 (now ruled: delete); morph() deletable narrowed to one stub-pin test; stdlib codec deletable narrowed to one f64 slab codec and one severity codec pair sequenced after S5; the check() pre-call-protocol count struck; KNOWN_V02_KERNELS derived from KernelId, not deleted; the h14 exoneration narrowed to stays-whole files plus qoi.rs). The two posed questions were ruled: S6 enum home reify-ir; S24 --since deleted.
- Options considered: accept all 15; revisit one
- Consequence that decided it: Each resolution names its owning stream so /prd keeps one owner per layer.
- Keys: per §Contradictions

### Invariant candidates
- **Ruling** (human, 2026-10-12): All eight promoted in checkable form through the four-site lockstep edit, each inside its owning stream: 1 registry status derives from a resolving enforcement pointer (S24 lane; strengthens INV-META-1); 2 cell-eval contexts only via cell_eval_ctx (INV-EVAL-2 mechanism corrected to type+lint, infra ratchet baseline 61 -> 0, S4); 3 no file:line citations in tracked prose (PCITE extension with fingerprint baseline, S24 lane, S23 burn-down); 4 Rust-written result structures and @optimized targets agree with their .ri declarations (INV-PD-1/2, S5/S15); 5 an extracted primitive ships with an adoption ratchet (repo-wide; /prd overlay G-gate: a task saying 'extract X' names the ratchet member); 6 a delegating GeometryKernel wrapper forwards every trait method (S10; extends INV-GEO-4); 7 no intra-crate module SCC grows, measured SCCs shrink (verify-side Tarjan baseline beside assert-crate-dag.sh, S3/S7); 8 constraint diagnostics carry their subject structurally (INV-SF-6 / dark-factory INV-2, S12).
- Options considered: promote each or not
- Consequence that decided it: Eight checkable gates replace intent; the program doc carries pointers per promoted INV id, never a second list.
- Keys: per §Invariant candidates

### Weakened findings ranked in a cell's top three (18, two high)
- **Ruling** (human, 2026-10-12): Kept in their streams at weakened weight as the record shows; each /prd seat re-reads the skeptic note under the freshness clause before building a leaf on one. Leo's note: which kernels get built where (LSP, GUI preview, CLI light path kernel-less today) is subject to change and changes are planned, so a defect obscured by the current kernel-build choices — fk-88643a4cc993 check() reading realization_handles/achieved_repr_tol that only build/tessellate fill — is still serious; S12 treats it as high in substance.
- Options considered: keep at weakened weight with /prd re-verification; drop weakened findings from streams
- Consequence that decided it: No finding disappears at the markdown or program layer; the obscured-by-current-wiring class is named so S12 does not downgrade it.
- Keys: the 18 keys listed in the deliberation brief §F

### Quality-findings contract §2: Rust `Type::method` anchor normalisation
- **Ruling** (human, 2026-10-12): Add the sentence to §2: a method or associated item is written `path::Type.method`, and the normaliser maps Rust's `Type::method` onto it before hashing (dark-factory findings_artefact.py and the orchestration script already do, d1316a47be).
- Options considered: add the sentence; revert the code; defer to the finding_key.py landing
- Consequence that decided it: The ratified text matches the one key implementation; the 15 recovered findings keep their keys.
- Keys: n/a (contract)

### Filing stance: contract §8 step 1 on reify
- **Ruling** (human, 2026-10-12): This run's 41 mechanical filings are ratified (step 1 was vacuous: no reify task carried an x_finding_key before this run). From the next run, step 1 is performed as a paginated get_tasks scan building a key index in the filing seat (a sanctioned API read, never a tasks.db read) until find_tasks_by_metadata lands in fused-memory; that tool is already tracked as dark-factory task #6385 (pending, high: backend query, protocol, interceptor pass-through, MCP tool, docs), so no new task is filed.
- Options considered: ratify + get_tasks-scan interim; ratify, interim left to the next lead; cancel the 41 filings
- Consequence that decided it: Dedup by exact key, including deferred tasks, from the next run on; nothing waits on the tool.
- Keys: n/a (process)

## Shared conventions

- Filing stamps on every task: `metadata.x_finding_key` (list), `metadata.x_finding_run = "review-all-reify-20261010"`, `metadata.source = "review-all"`, `priority` = severity, `metadata.files` file-level; `planning_mode=True` → `add_dependency` → one `commit_planning` per batch. An acceptance is a `cancelled` task with `metadata.x_acceptance_reason` (never `deferred`).
- **Freshness clause**: anchors were verified at `a030ee7528f5e5c7065dbc9871fc6802157db455`; before building a task on one, `git diff a030ee7528..main -- <path>` and re-read the symbol. Weakened findings are re-read with their skeptic note first (ruling: Weakened findings).
- **Dedup before filing** (contract §8, reify stance ruled 2026-10-12): step 1 = a paginated `get_tasks` scan building a key index (never a tasks.db read) until `find_tasks_by_metadata` lands (dark-factory #6385); then `search_tasks` ≥ 0.6 confirmed with `get_task`. The report's `## Mechanical filings` rows for a stream's keys are handed to the seat verbatim.
- **Held leaf**: a leaf that depends on a later-wave stream's contract is filed in planning mode with `metadata.x_seam_wait = {stream, contract}`, wired into its own batch's dependencies and excluded from that batch's `commit_planning`; the owning stream's session adds the dependency, commits it and re-stamps the sidecar.
- **One editor per document**: a `/prd` seat never edits this program doc or another stream's PRD; it returns its FILED row and cross-reference lines, and the lead applies them.
- PRDs live at `docs/prds/v0_6/<slug>.md` with capability manifests per `.claude/skills/prd/project.md`; every leaf cites the INV it establishes where one was promoted above; new infra tests are Python stdlib unittest with a `.sh` wrapper (reify CLAUDE.md).
- **Extract-with-ratchet** (INV candidate 5): a task whose proposal says 'extract X' names the ratchet member counting the superseded inline pattern.
- **Same-diff obligations for every code stream that moves files or symbols** (S24, 2026-10-13): after S24 ο #8822 lands, a moved file under PLINECITE scope runs `plinecite-baseline-gen --rename <old>=<new>` in the same diff; after S24 ξ #8821 lands, a moved or renamed symbol named by a docs/invariants.md Pointer has its pointer updated in the same diff. A leaf that flips an INV status cell records its id in #8821's owner map (S4: INV-EVAL-1/2/3; S1: INV-FEA-1/INV-BUILD-1; S10: INV-GEO-1..4; S25: INV-GUI-2); the lead releases #8821 once all four are recorded.

## Triage briefing

- Which kernels are built where (LSP, GUI preview and the CLI light path are kernel-less today) is planned to change; a defect obscured by the current wiring (e.g. check() reading realization_handles/achieved_repr_tol) is a real defect, not a non-issue.
- S18 lands a Python verify with `scripts/verify.sh` as an exec shim: dark-factory's stdout-token and `--print-plan` text consumers keep working byte-for-byte until they migrate; a parity test over role × scope × profile gates the cutover.
- Instrument debt (recorded, not a stream): `/hotspot-survey` whole and `/review --since none` have never run under the contract on reify; each should run once as its own attended session (~3M tokens, 60–90 min) so the next `/review-all` can refresh them and their trigger chains exist.

## Lead amendments to the synthesis text (recorded, not relitigated)

- S19: the per-member event channel is verify-tree wide and named `REIFY_VERIFY_EVENT_LOG` (heuristic 1), not `REIFY_RUN_ALL_EVENT_LOG`; every slot_emit_timeout caller writes a record; S18's Python verify passes the variable through.
- S3: the predicate homes are three by purpose (expr_classify, realization_inputs, structural_query); the checkable rule is "no import into the measured SCC and only crates below reify-eval", since four of the nine predicates need reify_compiler/reify_expr.
- Key reassignments: fk-030602c2fd62 (OCCT query() JSON replies) S11 → S10 (typed GeometryQuery reply leaf); fk-bb1ba38a6a46 (types.rs ALL arrays, weakened, previously unrouted) → S7 π; fk-c940cb1edad4 (ExecutionMode) → stamped on #8375/#8381 (elastic-static-solver-performance already deletes the axis; no leaf); fk-21c29ddc104d's verify.sh home → S18 (S22 θ keeps the classifier).
- S27: the MCP context stays deleted per #6665 (Leo 2026-08-26); the single compile front end is resolution-unification §3.1 `compile_program` (#5516), which S27 γ consumes — the G4 row "reify_compiler compile entry points" is owned by resolution-unification β, S27 consumes.
- S21/S22: no bash edits to scripts/verify.sh ahead of S18's Python rewrite; S18 carries the two apply_env / decide_scope adaptations as cross-references.
- S15: "the chartered PVAC detector" means #7102 (result-field-vacuity-closure); S15 C4 is INV-PD-2's runtime half.
- INV candidate 7 is INV-ARCH-1 (docs/invariants.md), tool and baseline built by S3 α with reify-compiler excluded until S7 κ; lockstep sites on reify: docs/invariants.md, docs/legibility/design-invariants.md, the program doc pointer, one CLAUDE.md Pointers line.
- Post-S1 follow-up candidate (not filed): typed ValueCellId / InstancePath (S3 OQ-2 b, ~2,066 sites).
- The G4 row "dark-factory merge queue's import of reify_overlap_detector" names a seam that does not exist (DF #1897 fell back to the default detector; no register_for_reify call): a dark-factory task to register it is a lead follow-up; S22 θ keeps importing the shared module.
- Wave-2 assignment: the R4 `else_members` ratchet rows for reify-eval (8 chains) and reify-test-support (1) in `tests/infra/test_compiler_decl_pass_ratchets.py` have no owning leaf (S27 cross-reference) → S4 owns them.

## Resolved decisions — pass 1 open questions (wave 1, ruled by the lead 2026-10-12 under the deliberation rulings; a seat that finds the code contradicts one returns it)

### S3 — docs/prds/v0_6/dependency-invalidation-stratum.md
- OQ-1 → (B): three homes by purpose (expr_classify, realization_inputs, structural_query); the checkable rule is "no import into the measured SCC, and only crates below reify-eval" — lead amendment of the ruling's "imports only reify_ir/reify_core" (impossible at main for 4 of 9).
- OQ-2 → (c): (a) now (InstancePath is the grammar home; ValueCellId.entity stays String); (b) typed ValueCellId recorded in the program doc as a post-S1 follow-up candidate, not filed now.
- OQ-3 → (i) α builds the only tool, baseline and INV-ARCH-1 row; (ii) S7's glob-removal leaf raises the reify-compiler rows as a named one-time exception in its commit (the tool stays simple).
- OQ-4 → lockstep sites on reify: docs/invariants.md row (α), docs/legibility/design-invariants.md entry (G7/review read it), the program doc pointer; CLAUDE.md gets one Pointers line only.
- OQ-5 → keep the {cache, deps} 2-cycle as a recorded residual; no ν.
- OQ-6 → accept the --explain-undef behaviour changes with the pin test.

### S7 — docs/prds/v0_6/compiler-module-namespace.md
- Q1 INV-ARCH-1 tool owner → (a): S3 α builds it with reify-compiler excluded until S7 κ; S7 ξ adds the compiler rows (consistent with the S3 OQ-3 ruling).
- Q2 fk-bb1ba38a6a46 (weakened, unfiled, unrouted) → (a): assign to S7 as π (strum::EnumCount on the 9 geometry-kind enums); program doc stream table amended.
- Q3 fk-a9fb39978c17 reach → (a): ν does the sub_* record only; the disjointness and key-parity pairs are recorded as a follow-up candidate in the PRD, not widened.
- Q4 κ at medium → confirmed (Contradictions ruling).
- Q5 29 globs incl. 4 conformance/ ones → accepted; baseline 29.

### S14 — docs/prds/v0_6/solver-crates-layering.md
- Q1 fk-c940cb1edad4 (ExecutionMode) contradicts committed elastic-static-solver-performance (#8375/#8381 delete the axis) → (A): stamp x_finding_key fk-c940cb1edad4 onto #8375 and #8381 (update_task metadata, list form), disposition filed:8375; no leaf; the ruling's "one ExecutionMode enum" is superseded by that PRD (G6 stale premise) — recorded in the program doc.
- Q2 GeometryError ownership → (A): κ adds BackendUnavailable in wave 1; S10 consumes it (cross-ref to S10).
- Q3 floor↔core cycle → (A): constraints_witness and floor_infeasible_diagnostic both move to solver/core (PRD D17).

### S11 — docs/prds/v0_6/occt-crate-internals.md
- Q1 fk-030602c2fd62 (query() JSON replies) → (a): key moves to S10's typed-reply leaf (producer + eval parsers switch atomically there); S11 ι only moves query() into query.rs; program doc stream table amended (key reassigned S11 → S10).
- Q2 OCCT warm start fk-69bd53399b08 → (a): reduce to the trait no-op (warm_state None, with_warm_state no-op), inventory registration stays, INV-GEO-3 OCCT arm reworded stateless; no consumer exists (heuristic 6).
- Q3 ordering vs #7438 (OpControl) → (a): γ1 and θ land first; decomposition notes on #7438 that the control goes to execute_with_history inside the call closure; edge #7438 → θ.

### S20 — docs/prds/v0_6/warm-lane-scripts-one-lock-one-env-one-oracle.md
- Q1 lane_lock_acquire(mode, wait) → (a) mode = wait mode (0 / N / unlimited); exclusive always; shared stays lane_lock_probe.
- Q2 --mount flags → (a) keep the flags meaning the worktrees dir, add the looks-like-mount warning; no DF flag-day.
- Q3 manifest row → (a) L3 adds the row itself in today's format (overlay requires same-diff registration); S19 owns the format, not the rows.
- Q4 golden record home → (a) canonical in DF regenerated by D5, vendored byte copy in reify with PROVENANCE.md (hermetic under /usr/bin/python3).
- Q5 f133 premise (5 mark sites, DF marks via yaml touch) → (a) accept the two-phase L9/L10 design; L9 ALSO measures (c): if hooks/pre-merge-commit marks immediately before land.sh's --no-ff move, delete land.sh's own pre-mark instead of trapping it.
- Q6 thin --reseed on seed exit 70 → propagate 70 unchanged (a lane-lock refusal is a distinct outcome, never mapped to the generic fault code).

### S21 — docs/prds/v0_6/native-dep-table-and-loader-path.md
- no open questions

### S22 — docs/prds/v0_6/prd-probe-gate-typed-outcomes.md
- R1 reify_overlap_detector unregistered in dark-factory (G4 row names a seam that does not exist) → (a): keep θ importing the shared module; LEAD FOLLOW-UP: file a dark-factory task "register_for_reify with project_id in the merge-queue overlap detection" (DF #1897 fell back to the default detector); program doc G4 row corrected.
- R2 ι and the verify.sh home → (b): ι moves to S18 (consistent with the S21 ruling: no bash edits to verify.sh ahead of its Python rewrite); fk-21c29ddc104d lands on θ minus the verify.sh home, S18's planner imports reify_path_attribution.py; the verify-pipeline-paths.txt row is S18's.
- R3 schema layering → (a) as drafted (α publishes what α decides; pdv publishes its own).
- R4 refinements (a)(b)(c) → confirmed.

### S19 — docs/prds/v0_6/run-all-runner-infra-harness.md
- Q1 event-channel scope/name → (C): the channel is verify-tree wide (every slot_emit_timeout caller writes a record) and is named REIFY_VERIFY_EVENT_LOG for an accurate name (heuristic 1); lead amendment of the synthesis's REIFY_RUN_ALL_EVENT_LOG label, recorded in the program doc; C3 rule 4 holds and η deletes D-G outright; S18's Python verify passes the var through and appends slot_timeout records if it reimplements slot acquisition.
- Q2 τ (verify_scope / semaphore partitions) → (A): ceded to S18; τ is NOT filed (nothing to cancel); S18's PRD carries the two partitions as cross-reference.
- Q3 D7 → (A): h14 partition satellites are behaviour-preserving moves and stay bash; only new assertions are Python.
- Premise correction accepted (fk-3cb220f9cc94: only test_tree_sitter_pipeline.sh runs a private harness).

### S15 — docs/prds/v0_6/trampoline-decode-and-result-build.md
- OQ1 PVAC wording vs result-field-vacuity-closure charter (#7102) → (a): S15 C4 is INV-PD-2's runtime half; #7102 stays the PVAC gate; program doc wording corrected ("the chartered PVAC detector" refers to #7102).
- OQ2 PDROP overlap → (b): S15's decoders land first and #7079 attaches its declaration to them (edge #7079 → β reversed: β → #7079); #7080/#7084 remain their own tasks consuming β (edges β → #7080, β → #7084 added at pass 2) — β does not wait on unstarted tasks.
- OQ3 declared defaults at runtime for PDROP C3 → (a): #7079 derives them from load_stdlib() templates; lead note on #7079; no production defaults table (D1 stands).
- OQ4 user supports on the shell route → (a): S16 owns it (per-route solvers); γ only makes the drop loud; cross-ref to S16.
- OQ5 fk-a28b1b57ed64 wider than recorded → confirmed: δ stays in S15 (fixes the two registry reads); the compiler placeholder unchanged; S1 untouched.

### S9 — docs/prds/v0_6/compiler-declaration-passes.md
- Q1 fk-60053402aba5 (#5499) and fk-6621fbaa0058 (#5425/#5430) → (a)+(b): stamp x_finding_key on #5499 and #5430 (dispositions filed:5499, filed:5430) AND add both as dependencies of π so S9 cannot close before they land; no new leaves.
- Q2 f-number mislabels → confirmed; keys and statements govern.
- Q3 relation_signatures ownership → (a) as written: S6 mints RelationKind; S9 λ owns the RelationSig rows; S8 reads them.
- Q4 β on #5425 → (a) hard edge.
- Q5 purpose-body parse error in reify-syntax inside γ → confirmed in S9 (no other stream owns ts_parser::lower_purpose_members).
- Q6 S12 contract → S9 §5 is the contract; S12 names any extra need at its own decomposition.

### S13 — docs/prds/v0_6/selector-resolution-factoring.md
- Q1 θ owner → (a): drop θ from S13; fk-07c78c4ca710 is S8's and deferred #6010 already moves the helper map; S13 files no held leaf.
- Q2 α₀ (delivered-check path widening in 11 other sidecars/task metadata) → (a): the pass-2 decompose session performs the three writes per check before filing (lead-sanctioned update_task writes on those tasks); not a leaf.
- Q3 h14 partition membership → (a): accept PRD D4/D5 (code wins over the record's "none"; measured 4 cycles).
- Q4 registry_drift_tests.rs path-only edits → (a): sanctioned for S13 β/γ.
- Q5 three first-leaf walks → (b): leave as the PRD does.

### S27 — docs/prds/v0_6/tooling-front-end.md
- OQ1 MCP context vs #6665 (Leo 2026-08-26: delete `reify mcp-server` and mcp_context.rs) → (A): #6665 stands; δ is not filed; the MCP parts of fk-f5264d76e3ad and fk-5854202d4aed close by that deletion; ο lands at the reify-mcp transport level.
- OQ2 compile_user_module vs resolution-unification §3.1 compile_program (#5516 β pending) → (A): compile_program is the one front end; γ moves AnalysisContext and the remaining LSP sites onto it and depends on #5516/#5520; the G4 owner row is re-pointed to resolution-unification β; S12's cross-reference becomes "layered under compile_program / CompiledProgram".
- OQ3 β scope vs #6851 → (A): predicate moves to reify-eval, LSP consumes it, CLI exit policy unchanged (#5403's).
- OQ4 reify.toml → (A): docs only.

## FILED — program status

Wave 1 (pass 2 completed 2026-10-12; every row returned by its seat, applied by the lead). Trigger-chain human gate: reify #8612 (deferred, planning_mode, interim form; `commit_planning` is the human's act once this table is complete for the whole program). Held leaves are released by the owning stream's session (S6: S9 λ #8741, S15 ο #8718; S1: S27 φ; S26: S27 λ). Dark-factory companions: #6853/#6854/#6855 (S20), #6856 (S19 ε).

| stream | PRD | tasks (anchors) | held leaves | notes |
|---|---|---|---|---|
| S3 | docs/prds/v0_6/dependency-invalidation-stratum.md | 8673 α (INV-ARCH-1 tool), 8687 β (predicates below the SCC), 8675 γ, 8691 δ, 8695 ε, 8699 ζ (high, gate-dep), 8677 η, 8701 θ, 8704 ι, 8680 κ, 8708 λ, 8710 μ | none | OQ-1..6 applied; reify-compiler excluded from the SCC baseline until S7 κ, S7 ξ adds the compiler rows; typed ValueCellId is a post-S1 candidate; #5023 and #4958 amended |
| S7 | docs/prds/v0_6/compiler-module-namespace.md | 8613 α, 8615 β, 8617 γ, 8620 δ, 8622 ε, 8623 ζ, 8625 η, 8627 θ, 8631 ι, 8636 κ, 8638 λ, 8643 μ, 8650 ν, 8661 π, 8665 ω (+ adopted 8552, 8551, 8560) | 8657 ξ (waited on S3 α #8673; edge wired; released by the lead's commit_planning) | all medium except ω low; no gate-8612 deps; superseded and cancelled: #7338, #8321, #8006, #7986; μ waits on #6955, #5493 |
| S9 | docs/prds/v0_6/compiler-declaration-passes.md | 8730 α, 8731 β (high, gate), 8732 γ, 8733 δ, 8734 ε, 8735 ζ, 8736 η, 8738 θ, 8739 ι, 8740 κ, 8741 λ, 8742 μ, 8743 ν (all_value_cells, S27 seam), 8744 ξ, 8745 ο, 8746 π | 8741 λ (x_seam_wait S6: RelationKind enum in reify-ir) | fk-60053402aba5 → filed:5499, fk-6621fbaa0058 → filed:5430 (keys stamped; π depends on both), fk-75d9313c6ee6 → filed:8556; β hard-depends on #5425; ξ on S7 8615/8627, ο on S7 8623 |
| S11 | docs/prds/v0_6/occt-crate-internals.md | 8648 α, 8652 γ1, 8659 β, 8666 γ2, 8669 ζ (high), 8672 θ, 8676 δ, 8682 ε, 8684 ι, 8688 ω | none | ζ gate-dep of 8612; #8582 cancelled (subsumed by 8669); fk-030602c2fd62 reassigned to S10; fk-baae0532b743 stamped on #7379 (prerequisite of ε); #7438 and #7688 → 8672 |
| S13 | docs/prds/v0_6/selector-resolution-factoring.md | 8747 α, 8748 β, 8749 γ, 8750 δ, 8751 ε, 8752 ζ, 8753 η, 8754 ι | none | θ dropped (→ S8/#6010); α₀ performed at pass 2 (delivered-check path corrections on #6179 #5779 #5780 #5781 #8401 #5439 #5745 #5747 #8288 + sidecar-only #5623 #5658; six other PRDs' sidecars + .md twins edited: angle-dimension-completion, angle-units-surface-convergence, implicit-solids, placement-relations-belt, units-length-gate-completion, meshing-service); #7381 cancelled as subsumed; α → #8673, β → #8687; no high leaves |
| S14 | docs/prds/v0_6/solver-crates-layering.md | α 8614, β 8616, γ 8618, δ 8619, ε 8621, ζ 8626, η 8628 (high), θ 8629, ι 8630, κ 8632, λ 8637, μ 8640, ν 8645, ξ 8653 (high), ο 8656 (high), π 8658 (high), ρ 8660 (close) | none | gate deps 8628/8653/8656/8658; fk-c940cb1edad4 stamped on #8375/#8381 (filed:8375); #8528 adopted; #8581/#8449 cancelled (subsumed by ξ 8653); out-of-program edges #8375→δ,ζ,ι; #8528→γ; #6655/#7492/#7420→ξ |
| S15 | docs/prds/v0_6/trampoline-decode-and-result-build.md | 8674 α (high, gate), 8679 β, 8683 γ, 8686 δ, 8690 ε, 8693 ζ, 8697 η, 8698 θ, 8702 ι, 8705 κ, 8706 λ, 8711 μ, 8714 ν, 8717 ξ, 8718 ο, 8720 π, 8722 ρ, 8724 σ | 8718 ο (x_seam_wait S6: dynamics/eval.rs MechanismTree and JointKind leaves) | subsumed+cancelled #8524 (→8683), #8559 (→8698); #7079/#7080/#7084 depend on 8679; upstream #6922 → α γ δ η ο, #6941 → α θ; C2 maps UnsupportedKind to the existing FeaLoadKindUnsupported |
| S19 | docs/prds/v0_6/run-all-runner-infra-harness.md | 8678 α, 8681 β (high), 8685 γ (high, REIFY_VERIFY_EVENT_LOG), 8689 δ, 8692 ζ₁, 8694 η (high), 8696 ζ₂, 8700 θ (high), 8703 ι, 8707 κ, 8712 λ, 8716 μ, 8719 μ₂, 8721 ν, 8723 ξ, 8725 ο, 8726 π, 8727 ρ, 8728 σ, 8729 Ω; dark_factory:6856 (ε, high) | 8719 (S20 L1 #8624; edge wired; released by the lead) | adopts #8535/#8536/#8538; #7445 → 8678; η 8694 external_deps dark_factory:6856 (must land and deploy first); τ ceded to S18; gate 8612 ← 8681, 8685, 8694, 8700, dark_factory:6856 |
| S20 | docs/prds/v0_6/warm-lane-scripts-one-lock-one-env-one-oracle.md | 8624 (L1), 8635, 8641, 8647, 8709, 8668, 8541 (adopted L7), 8670, 8671, 8713, 8715 (close); DF companions dark_factory:6853 (D5), 6854 (D1), 6855 (D8) | none | f133 mark sites corrected to 5 (DF yaml main_gate_mark_command repointed in L9, effective at next orchestrator restart); no high leaves so no gate-8612 deps; S19 μ₂ depends on L1 = 8624 |
| S21 | docs/prds/v0_6/native-dep-table-and-loader-path.md | 8634 (α, high), 8646 (β), 8654 (γ), 8663 (ω close) | none | δ dropped per lead ruling (verify.sh share → S18 by cross-reference); #7623 cancelled, subsumed by 8646; #8105 now depends on 8654; gate 8612 depends on 8634 |
| S22 | docs/prds/v0_6/prd-probe-gate-typed-outcomes.md | 8633 α, 8639 β, 8642 γ (high), 8644 δ (high), 8649 ε, 8651 ζ, 8655 η, 8662 θ (high), 8664 κ, 8667 λ | none | gate 8612 ← 8642, 8644, 8662; #8122 and #8178 cancelled as subsumed (by 8642, 8664); 8662 waits on in-flight #7491; ι moved to S18 per R2 |
| S27 | docs/prds/v0_6/tooling-front-end.md | 8755–8765, 8767–8779 (24 leaves; high 8761 β, 8778 γ → gate 8612) | 8776 φ (S1: EngineConfig + KNOWN_SITES entry), 8777 λ (S26: generated TS wire types carrying LspMethod::ALL) | δ not filed (OQ1-A, #6665 stands); γ consumes #5516/#5520; ζ ← #8743, π ← #8742; subsumed and cancelled #8447, #8557, #8336, #7720, #8611; notes on #6665, #6851, #6695, #6805 |
| S24 | docs/prds/v0_6/reify-audit-typed-findings-and-registry-lanes.md | 8806 α, 8807 β (high, gate), 8808 γ1 (high, gate), 8809 γ2 (high, gate), 8810 δ, 8811 ε, 8812 ζ, 8813 η, 8814 θ, 8815 ι, 8816 κ, 8817 λ, 8818 μ, 8819 ν, 8820 π, 8821 ξ, 8822 ο, 8823 ω (close) | 8821 ξ (x_seam_wait S4, S1, S10, S25: their leaf ids owning INV-EVAL-1/2/3, INV-FEA-1/INV-BUILD-1, INV-GEO-1..4, INV-GUI-2; released by the lead) | rulings R1–R5 (Leo 2026-10-13); α #8806 ← in-flight #6935 + #7098; ο #8822 ← #6935; subsumed+cancelled #7066 #8199 → ζ, #8516 → ε, #8200 → κ, #7988 → λ, #8365 → μ, #7740 → δ, #8509 #7302 → ο; INV candidate 1 → ξ (INV-META-1 → partial: enforced(audit) for registry status); INV candidate 3 → ο as INV-DOC-1 via PLINECITE (not PCITE); gate 8612 ← 8807, 8808, 8809; registry-grammar notes appended to #6701 #5101 #8720 #8657 #7102 #7085 #5414 #7098 |

Not yet filed: S6, S10, S12, S18, S25, S4, S5, S8, S1, S16, S2, S26, S23.

## Pass-2 cross-reference lines (wave 1; applied or carried to the named stream's PRD by the lead)

- **S3** → INV candidate 7 → INV-ARCH-1 (α 8673); prd overlay G1 seam row §3.6 retired after 8699; S23 (architecture doc §3.5, freshness-4-variant PRD); S7 ξ (baseline excluded map, after κ and 8673); every cycle-breaking stream lowers its baseline row in the same diff; S8 (expr_classify.rs after 8687); S4 (depends on 8687, 8691, 8695); S25 (set_observed_demand after 8680); S5 (direct compute-contract imports after 8687); optional edge #8338 → 8701 (not added).
- **S7** → S9 (entity/traits/forall/guards leaves depend on 8615, 8627; member_cell_decls.rs); S8 (builtin-table leaves depend on 8622; expr.rs leaves on 8627, 8631; sixth partition; operator_typing.rs after 8620); S3 (8673 keeps reify-compiler excluded; 8657 moves it in); S12 (satisfies_trait_bound stable); S6 (#8560 first); S5 (#8551 first); S27/resolution-unification β (8643 edits one body).
- **S9** → S27 (ν 8743; R4 row in tests/infra/test_compiler_decl_pass_ratchets.py; ParseErrorKind::diagnostic_code from 8742); S12 (contract ids β 8731, γ 8732, μ 8742); S6 session releases 8741; S8 (RelationSig from 8741); S4 (8734 deletes rewrite_port_placeholder; 8735 SubOrigin recognisers); #8597 vs 8740 (no edge; rebase).
- **S11** → S10 PRD (typed-SI decode at op_args.rs depends on 8652; sub-trait accessors over call<R> depend on 8672; typed GeometryQuery reply leaf carries fk-030602c2fd62 and depends on 8684; MockGeometryKernel mirrors ReprPolicy, depends on 8669).
- **S13** → #8288 item 7, #6682 KEEP-UNTOUCHED, S3 (#8401 check scans expr_classify.rs; 8747 lowers the reify-eval baseline row), S2, S4, S8/#6010, S10, S1, S5 (as in pass 1).
- **S14** → elastic-static-solver-performance (#8381 edits qoi.rs after μ 8640 moves its tests); S4 (ν 8645 leaves the eval_ctx_with_meta site); S5 (wave-1 trampoline edits; ShellFrame doc pointer); S16 (AssemblyElement::new after δ 8619); S10 (BackendUnavailable from κ 8632); S3 α baseline (β 8616, π 8658 shrink SCCs); S23 (θ 8629, μ 8640 own that prose).
- **S15** → S6 session releases 8718 (add_dependency on its MechanismTree/JointKind leaves, commit_planning, re-stamp the S15 sidecar); optional edges not wired: #7100 → 8693, #7101 → 8690/8693/8697, #7081 → 8683.
- **S19** → S18 PRD (the two verify-test partitions ceded, after 8694; Python verify passes REIFY_VERIFY_EVENT_LOG through and appends slot_timeout records if it reimplements slot acquisition); S20 (8624 and 8716/8721 touch the same test files, file locks serialize; 8719 depends on 8624).
- **S20** → S19 μ₂ → 8624; G4 row "dark-factory-orchestrator.yaml git.main_gate_mark_command/unmark_command + hooks/main-gate-lib.sh sentinel format | S20"; DF #3074 lib_lane_state.sh SPOT question for the repatriation (noted).
- **S21** → S18 PRD (apply_env from lib_native_ld_path.sh cargo-target class #8654; ratchet exemption 2→0; conditional native-deps.toml row in verify-pipeline-paths.txt); G4 row wording (table-driven dispatch); S23 (CLAUDE.md Native deps sentence + Pointers row current); #8269 edits the OCCT row after #8634.
- **S22** → S18 PRD (decide_scope reads reify_path_attribution.py attribute in one batched call; fixture-list/gui-event-channels arms stay policy; B14 parity; S18 deletes affected-crates-lib.sh::reify_is_inert_path which 8662 keeps as a thin wrapper; S18 adds the module to verify-pipeline-paths.txt); G4 new row "scripts/reify_path_attribution.py | S22 (8662) | S18 planner, reify_overlap_detector"; lead follow-up DF register_for_reify task.
- **S27** → S9/#8743 (R4 rows for reify-eval (8 chains) and reify-test-support (1) have no owning leaf → lead assigns to S4 at wave 2); S1 (wire EngineConfig leaf → 8776 and release; KNOWN_SITES entry moves in 8776's diff); S26 (wire TS-generation leaf → 8777 and release); S25 (engine.rs stdlib literal adopts dev_stdlib_root from 8769 unless #5519 deletes the path); S18 (8776 moves the infra-tests manifest row for main.rs).

Lead follow-ups outside any stream: file a dark-factory task to register reify_overlap_detector with project_id (S22 R1); file a reify task for scripts/lane-task-status.sh reading tasks.db directly (S20); instrument debt: /hotspot-survey whole and /review --since none as their own attended runs.

## Pass-1 cross-reference lines (wave 1, restored 2026-10-13; still binding where a later pass-2 line does not supersede them)

- **S3** → FILED row; INV candidate 7 → INV-ARCH-1 (α); amend #5023 and #4958 descriptions (completion transaction seeds dirty_cone); prd overlay G1 seam row §3.6 retired after ζ; S23 (architecture doc §3.5, freshness-4-variant PRD); S7 (baseline rows); every cycle-breaking stream lowers its baseline row in the same diff (table in the result); S8 (name-set predicates live in expr_classify.rs); S4 (consumes expr_classify/DependencyGraph/dirty_cone); S25 (set_observed_demand); S5 (direct compute-contract imports); next review-all instrument: brace-matched cfg(test) exclusion (second SCC is 5, not 3).
- **S7** → S3 (exclusion until κ), S8 (depends on ε, θ, ι; sixth partition; operator_typing.rs), S9 (depends on β, θ; member_cell_decls.rs), S12 (satisfies_trait_bound stable), S6 (#8560 repoints), S5 (#8551 repoints), S27 (μ edits compile_with_prelude_context_checked_with_config body only); FILED row at medium.
- **S14** → FILED row; elastic-static-solver-performance stamping; S4 (ν deletes engine_eval predicate mirrors; eval_ctx_with_meta site left to S4); S5 (compute_targets trampolines edited in wave 1 by S14; ShellFrame doc pointer); S16 (AssemblyElement::new only); S10 (BackendUnavailable consumed); S3/S7 (INV 7 baseline); S23 (θ/μ prose; S23 keeps the file:line rewrites in 5 files).
- **S11** → S10 PRD (MockGeometryKernel mirrors ζ's ReprPolicy; typed-SI decode replaces extract_length_f64 at op_args.rs → depends on γ1; sub-trait accessors over θ's call<R>; typed GeometryQuery reply leaf carries fk-030602c2fd62 and depends on ι); decomposition notes: #8582 subsumed by ζ; #7379 → ε; #7248 → ζ; #8284 → θ, δ; #7688 optional → θ; #7438 → θ.
- **S20** → FILED row text; seam row "dark-factory-orchestrator.yaml git.main_gate_mark_command/unmark_command + hooks/main-gate-lib.sh sentinel format | S20"; S23 Greek-letter aliases in gc comments; DF repatriation PRD must carry the new libs; LEAD FOLLOW-UP: scripts/lane-task-status.sh reads .taskmaster/tasks/tasks.db directly (forbidden by CLAUDE.md) — no finding covers it; file a mechanical task.
- **S22** → G4 rows (overlap detector correction; new row reify_path_attribution.py | S22 | S18, overlap detector); S18 PRD (decide_scope reads attribute; fixture-list and gui-event-channels arms stay policy); pass 2: #8122 and #8178 cancelled as subsumed (reason names the subsuming leaf; the leaf carries their keys), #7491 → θ.
- **S19** → program doc seam rows for S18/S19 split (S19 = run_all-emitted lines + @@REIFY_SLOT_TIMEOUT@@ family; S18 = verify.sh-emitted lines); S18 PRD (event records, shim passes the var; harness-layout-lib public names; plan_capture_lib untouched; the two partitions ceded); S23 PRD (35 crate headers repointed by θ — do not re-touch); S24 PRD (schema.rs include_str! of tasks_db_schema.sql); S20 PRD (μ₂ depends on lane-lock leaf; ν only gc fixtures); pass-2: add #7445 → α.
- **S15** → G4 rows (stdlib_inputs decoders | S15 | S5, S16; result builders | S15 | S5, S16, #7100/#7101); S5 (move carries stdlib_inputs/builders; INV 4 .ri half is S5's); S16 (typed request from C1 decoders; one result builder = ε; shell-route supports); S10 (registry_free is the constructor the overlay fields extend); S6 (ο ordering); S8 (κ analysis diagnose registration; ξ moves fea::diagnose); S4 (no new EvalContext site); task notes #7079 #7100 #7101 #7081 #5802; #8524 #8559 subsumed by γ/θ.
- **S9** → S27 (ν = all_value_cells; R4 row; ParseErrorKind::diagnostic_code adoption), S7 (ξ/ο deps; D3 module naming), S6 (RelationKind consumed by λ), S8 (reads RelationSig), S4 (ε deletes rewrite_port_placeholder; ζ SubOrigin matches), S23 (skip four prose pieces S9 rehomes); mechanical rows fk-60053402aba5 → filed:5499, fk-6621fbaa0058 → filed:5430, fk-75d9313c6ee6 → filed:8556.
- **S13** → #8288 (nodes_for_faces moves), #6682 KEEP-UNTOUCHED list, #5658 stale delivered check (pre-existing; needs the three-write correction), S2 (placed_walk only), S4 (eval_ctx_with_meta sites relocated; PendingWarmSeedsGuard), S8/#6010, S10 (dispatch_* decoders → query.rs), S1 (lib.rs test mod line), S5 (CarriedTopology), S3 (INV-7 baseline).
- **S27** → G4 row re-point; FILED subsumed #8447 #8557 #8336 #7720 (cancel as subsumed at decompose, keys carried); S12, S9 (ratchet rows), S1 (KNOWN_SITES moves with φ), S26 (LspMethod in generated types), S25 (dev_stdlib_root unless #5519 deletes the path), S18 (infra-tests manifest row moves with φ); task notes #6851 (panic re-verified; predicate home), #6695 (three sites not four), #6805 (add tooling_fe_stdlib_enum.ri to its corpus); stale rustdoc sentence on reify_compiler::compile (fix in γ).

## Cross-reference lines — waves 2–4 and spawn streams (applied by the lead)

- **S24** → Program doc: INV candidate 1 → S24 ξ #8821 (reify-audit --pattern PREGISTRY; INV-META-1 becomes partial: enforced(audit) for registry status, proposed for task INV-cites). INV candidate 3 → S24 ο #8822 (PLINECITE ledger ratchet, new row INV-DOC-1), NOT a PCITE extension (ruling R1). Correct the G4 row "reify-audit PCITE lane scope (manifests today): invariant candidate 3's owner | S23" to name PLINECITE (#6935 → #8822) as the lane; S23 burns down its ledger. · Program doc G4 additions: "docs/invariants.md Pointer column + design-invariants census Gate grammar (PRD §5.7) | S24 (#8821)"; "crates/reify-audit/plinecite-baseline.txt + plinecite-baseline-gen --rename | S24 (#8822)". · S4 session: when its leaves that own the INV-EVAL-1/2/3 status cells are filed, record their ids in #8821's owner map. Those leaves flip the status and remove the pending-owner tag. Note INV-EVAL-2's named symbol `CellEvalCtx` does not exist; the real one is `cell_eval_ctx`. · S1 session: record the leaf ids owning INV-FEA-1 and INV-BUILD-1 on #8821. INV-FEA-1's named `Engine::new_production` does not exist; `register_production_compute_fns` implements it. · S10 session: record the leaf ids owning INV-GEO-1..4 on #8821, and correct INV-GEO-4's inverted prose: the has_occt real arm IS wired in reify-kernel-occt lib.rs (commit ca22b761d8), and the stub arm is instantiated in stubs.rs. · S25 session: record the leaf id owning INV-GUI-2 (partial) on #8821. · Last of S4/S1/S10/S25, or the lead: once all four have recorded their ids, commit_planning #8821 and commit the re-stamped sidecar ξ block. · S23 PRD: its file:line burn-down (including tests/infra, per S19's §9) consumes #8822's widened PLINECITE ledger, so its burn-down leaves depend on #8822. Registry status cells S23 corrects must keep `reify-audit --pattern PREGISTRY` green once #8821 lands. · Every code stream that moves files (S1, S2, S3, S4, S7, S8, S9, S10, S12, S25, S26) has two same-diff obligations. After #8822 lands, a moved file under PLINECITE scope needs `plinecite-baseline-gen --rename <old>=<new>` in the same diff. After #8821 lands, a moved or renamed symbol named by a docs/invariants.md Pointer needs its pointer updated in the same diff. · S19: S24 adds run-all-classification.manifest rows (ζ, ξ, ο) in today's format, and tests/infra/adoption-ratchet.manifest rows (β, ε, ζ, η, ν) using `crates/reify-audit/src/**/*.rs` globs. S19 λ's tests/common/schema.rs edit and ν (#8819) serialise by file lock; no edge was added. · S15: #8720 now carries a details note to write INV-PD-2's enforcement as a `**Enforcement**:` paragraph in the pointer grammar. S7: #8657 is INV-ARCH-1's pending-owner for the reify-compiler half.
