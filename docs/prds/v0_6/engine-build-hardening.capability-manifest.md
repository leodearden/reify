# Capability manifest — engine-build-hardening

Binds each leaf signal's asserted capabilities to executed evidence (G3+G6 mechanized).
Evidence gathered 2026-07-06 against main `bc3771221f`. Line anchors are hints; symbols are authoritative.

**D3-workflow deviation (PRD §9 OQ-5):** the `Workflow` tool is unavailable in this harness; the deterministic core (`scripts/prd-capability-check.py`) was executed directly for the single `.ri`-probeable premise (leaf κ). All other premises are engine-internal (not grammar/check/ir-probeable) and are bound with executed grep / task-status evidence below.

## Leaf ε — version-id discipline gate (signal c)

| Capability | Evidence | Verdict |
|---|---|---|
| Exactly 5 raw allocate-and-bump sites exist to migrate (α/β/γ upstream) | `grep -n next_version_id` executed: `concurrent.rs:172-173`, `engine_eval.rs:2937-2938`, `engine_eval.rs:3818-3819`, `engine_edit.rs:858-859`, `engine_edit.rs:2426-2427`; every site allocates the (snapshot, version) pair in lockstep | PASS |
| The divergent read sites exist (δ upstream) | `engine_build.rs:7993 fn current_eval_version` (private, panics pre-eval); `engine_admin.rs:2211 VersionId(self.next_version_id.saturating_sub(1))` (doc :2197 "or 0 before the first eval") | PASS |
| Non-allocation write needing allowlist | `concurrent.rs:227 self.next_version_id = setup.version.0` (setup-restore assignment; gate allow-comment or moot if concurrent stack deleted) | PASS (bound into D7) |
| Source-scanning integration-test mechanism has in-repo precedent | `crates/reify-eval/tests/no_stale_undef_invariant_gate.rs` (fs-scanning gate test); `scripts/check_event_inventory.sh` wired at `scripts/verify.sh:1354` (name-drift lint precedent) | PASS |
| DAG-direction | producers α/β/γ/δ all upstream of ε (edges ε→β/γ/δ, β/γ/δ→α) | PASS |

## Leaf θ — eviction retired, fixture stays green (signal a)

| Capability | Evidence | Verdict |
|---|---|---|
| The fixture exists and runs on main | `crates/reify-eval/tests/manifold_cross_kernel_real.rs::engine_routes_overlapping_box_union_to_manifold_mesh` over `examples/multi_kernel/manifold_boolean.ri`; OCCT-gated suite in verify | PASS |
| The defensive eviction exists to retire | `engine_build.rs:6294-6295 feature_tag_table.remove(cached_handle.id); topology_attribute_table.remove(cached_handle.id)` inside the 4349 collision-guard block (:6264-6296); trade-off doc names "#4351's KernelHandle re-key" as the retirement condition | PASS |
| Re-key producer is upstream and real | task **#4351** `pending` (deps 3437 done, 4635 done, 4827 pending; committed progress a36777f..9917da4); re-scoped 2026-06-24 to TopologyAttributeTable-only; edge θ→4351 wired | PASS (producer:task-4351, upstream) |
| FeatureTagTable half resolves by deletion, not re-key | task **#4827** `pending` (P2 τB deletes FeatureTag/FeatureTagTable outright, verified write-only-dead); its signal covers the execute_realization_ops zero-behavior-change corpus | PASS (producer:task-4827, upstream via 4351→4827 and η→4827) |
| Eviction-rationale docs to update exist | `reify-ir/src/geometry.rs:4072-4074` (FeatureTagTable::remove — dies with #4827) and `:4633-4635` (TopologyAttributeTable::remove — θ updates) | PASS |
| Anti-inversion | #4351 does not depend on θ; θ→4351 only | PASS |

## Leaf ι — reset_per_build_state (signals: corpus green + CLI pin d + compile-forcing)

| Capability | Evidence | Verdict |
|---|---|---|
| The 4 divergent reset blocks exist | table resets gated at `engine_build.rs:2745-2747` (build_snapshot) / `:3487-3489` (build_with_geometry_output), ungated at `:5120-5126` (tessellate_realizations) / `:10122-10127` (tessellate_snapshot); `realization_handles.clear()` build-surfaces-only (:2645, :3306); `achieved_repr_tol.clear()` tessellate-surfaces-only (:5126, :10127) | PASS |
| Fold-in precedent exists | `Engine::reset_dispatch_tallies` `engine_build.rs:2571-2574`, called at entry points (e.g. :2640) | PASS |
| **Per-surface asymmetry is production-load-bearing** (the naive-union hazard) | `crates/reify-cli/src/main.rs:555-640` — combined-kinds arm: `build()` then `tessellate_realizations()`, comments explicitly state "build() clears+repopulates realization_handles but never touches achieved_repr_tol; tessellate_realizations() is the exact converse". Bound into PRD §4 D5 (surface-parameterized reset), NOT contradicted by the design | PASS (design refined) |
| Must-survive fields exist as claimed | `lib.rs:787 warm_pool`, `:916 realization_cache`, `:982-990 morph_source` (doc: "In-memory only — never persisted"), `:1055-1065 persistent_cache_dir/persistent_hit_count/persistent_miss_count` | PASS |
| Exhaustive-destructure enforcement precedent | exhaustive-struct-literal drift guard (`elastic_result.rs`) cited in `docs/invariants.md` precedents list | PASS |
| CLI pin observable through product read path | `reify check` verdict output (CLI output difference — G2 menu item 1) | PASS |

## Leaf κ — mixed-kernel attribute-selector example (signal b)

| Capability | Evidence | Verdict |
|---|---|---|
| Combined syntax compiles today (grammar + check substrate) | **Executed probe** `scripts/prd-capability-check.py tests/prd-gate/engine-build-hardening-probe-set.json --json` → verdict **PASS** (exit 0, `All constraints satisfied.`); fixture `tests/prd-gate/fixtures/engine_build_hardening_kappa_mixed_kernel_selector.ri`; the `@face`-undef-during-eval warning is the documented eval-path deferral (selectors resolve on build/tessellate — exactly the path κ's test drives) | PASS |
| Cross-kernel real-kernel harness exists | `crates/reify-eval/tests/manifold_cross_kernel_real.rs` (linker-anchor pattern, `reify-kernel-manifold` dev-dep with test-fixtures feature, `Cargo.toml:139`); `examples/multi_kernel/` precedent (`manifold_boolean.ri`, `pragma_override.ri`) | PASS |
| Attribute-resolved selector production path is wired (not test-only) | `examples/ad_hoc_face_selector.ri` documents the production chain: `post_process_ad_hoc_selectors` (engine_build.rs) → `try_eval_ad_hoc_selector` (geometry_ops.rs) → `TopologyAttributeTable` reads, seeded by `seed_primitive_attributes_for_handle`; pinned by `ad_hoc_selector_smoke_tests.rs` | PASS (wired-on-main) |
| Field-population: attributes are real values, not sentinels | selector resolution yields `Value::Frame{origin, basis}` from kernel Centroid/FaceNormal queries on the build path (never fabricated); the eval-path `Undef` is the documented deferral, patched post-process | PASS |
| Per-kernel independent attribute reads (the NEW assertion) | producer = **#4351** re-key (upstream, edge κ→4351); today's collision overwrite (`engine_build.rs:4376`-era anchor, per #4351's RCA) is exactly what the re-key fixes — the assertion is unachievable pre-4351 and κ therefore sits downstream | PASS (producer:task-4351, upstream) |
| "Renders" premise | tessellation output assertable in-test (mesh non-empty), matching the existing real-kernel test idiom; no GUI dependency | PASS |

## Intermediates (capability spot-checks)

- **α/β/γ/δ (allocator family):** all target sites verified present (see ε bindings); `Engine` fields `next_snapshot_id`/`next_version_id` private to reify-eval (`lib.rs:487` area) — pub(crate) helper suffices, no cross-crate exposure needed.
- **ζ (RealizationOpsInput):** signature verified ~24 params (`engine_build.rs:6074-6162`); precedent struct `RealizationOutputs<'a>` (:117-154) with rationale doc; call sites = 4 production (:2826/:3708/:4702/:5761) + `DispatchTestState` (:12104/:12146, internal calls :12186/:12255) + 3 wrapper-bypassing tests (:14182/:14349/:14502) — all inside engine_build.rs (single-file lock). |
- **η (probe extraction):** short-circuit region verified (:~6188-6320 incl. the `} // end is_terminal_realization cache-probe guard` close); collision tests verified moved-scope (:18869-19100); #4827 deletes the FeatureTagTable half (its metadata.files includes engine_build.rs) → edge η→4827.
