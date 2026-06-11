# Capability manifest — realization-read-api

Binds each leaf's asserted capabilities to evidence (mechanizes G3+G6). Verified against main @ `828b95afb0`, 2026-06-10. **Numeric-floor: N/A across the batch** — no signal asserts a numeric accuracy bound; all assertions are structural (counts, types, presence/None, content_hash identity). **Grammar-fixture: N/A** — zero novel `.ri` syntax; every leaf `grammar_confirmed=true`.

| Leaf | Capability asserted by signal | Evidence | Verdict |
|---|---|---|---|
| α | `RealizationReadHandle` exists to extend | grep: `crates/reify-eval/src/engine_compute.rs:112` | PASS (wired) |
| α | content types exist: `SampledField` / `Mesh` / `VolumeMesh` | grep: `reify-ir/src/value.rs:90-126`; `reify-ir/src/geometry.rs:2400` (tessellate→Mesh); `reify-ir/src/geometry.rs:1984` | PASS (wired) |
| α | `content_hash` available per realization | grep: `reify-eval/src/graph.rs` `RealizationNodeData.content_hash` | PASS (wired) |
| β | `ComputeNodeData.realization_inputs: Vec<RealizationNodeId>` | grep: `reify-eval/src/graph.rs:159` | PASS (wired) |
| β | `Value::GeometryHandle.realization_ref` lowering bridge | grep: `reify-ir/src/value.rs:966-970` | PASS (wired) |
| β | `Engine.realization_handles` node→handle map | grep: `reify-eval/src/lib.rs:533` | PASS (wired) |
| β | cache key folds realization content hashes | grep: `reify-eval/src/compute_cache_key.rs:91` + tests `:250,:276,:403,:443` | PASS (wired, pre-existing — NOT β's scope) |
| β | dispatch lowering sites exist (currently pass `&[]`) | grep: `engine_eval.rs:3596`, `:4033`-area, `:4580`-area | PASS (wired) |
| γ | `GeometryKernel::volume_mesh()` projection method | grep: ABSENT from `reify-ir/src/geometry.rs` trait — **γ creates it** | PASS (producer-self) |
| γ | gmsh claims `(Convert{from: Mesh}, VolumeMesh)` | grep: `reify-kernel-gmsh/src/register.rs:99` | PASS (wired) |
| δ | `densify_grid_to_sampled` Voxel→SampledField helper | producer: **task 4421** (pending, dep wired); openvdb densify substrate exists (`reify-kernel-openvdb/src/ingest.rs`) | PASS (producer-upstream) |
| δ | Voxelize dispatcher stage (real BRep→Mesh→Voxel chains) | producer: **task 4422** (pending, dep wired) | PASS (producer-upstream) |
| δ | `cfg(has_openvdb)` degradation gate | grep: `reify-eval/build.rs:59-63` | PASS (wired) |
| δ | empty-value sentinel check: `Some(SampledField)` produced on the production projection path, `None` only as honest degradation | production producer = β/δ projection store (in-batch), asserted in η's two-way suite | PASS (field-population, in-batch producer) |
| ε | `value_inputs[1]` SampledField seam exists to migrate | grep: `shell_extract_compute.rs:346-357` | PASS (wired) |
| ε | `sdf()` accessor content | producer: δ (intra-batch dep) | PASS (producer-upstream) |
| ζ | `build_slab_sdf` exists to remove | grep: `compute_targets/shell_solve.rs` (imported at `engine_eval.rs:3537`) | PASS (wired) |
| ζ | body realization reachable at FEA lowering (G6 branch-3 check: NOT producible from ζ alone) | producer: **task 4091** (pending, dep wired) — ζ explicitly gates on it | PASS (producer-upstream) |
| η | full chain `.ri`→realization→projection→trampoline | producers: γ/δ/ε deps (transitively 4421/4422) | PASS (producer-upstream) |
| η | anti-inversion check: η's e2e output is producible from its OWN dep set (no capability lives in tasks depending on η) | consumers 4091/3429 dep-edge ONTO γ, not the reverse | PASS |

No FAIL bindings. Out-of-batch consumer edges wired at decompose: 4091→γ, 3429→γ.
