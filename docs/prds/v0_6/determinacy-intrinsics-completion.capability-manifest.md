# Capability Manifest — determinacy-intrinsics-completion

Mechanizes G3 + G6 for each leaf of `docs/prds/v0_6/determinacy-intrinsics-completion.md`. One block per task: `capability → evidence`. Any FAIL binding blocks the batch. Evidence forms per the reify overlay (`.claude/skills/prd/project.md` → Capability Manifest): wired-on-main / anti-orphan, anti-inversion (DAG-direction), field-population (sentinel = `Value::Undef`/`None`), grammar-fixture, numeric-floor.

Reflective substrate is **merged on main** (tasks 2289/2544/4137/4138 done) — the intrinsics desugar onto it; they do not depend on the deferred `purposes-completion` batch.

---

## α — Compiler-sugar determinacy intrinsics + example consumer

| Capability asserted by α's signal | Evidence | Verdict |
|---|---|---|
| Reflective `forall p in subject.params: determined(p)` compiles (the desugar target) | `grep:crates/reify-compiler/src/expr.rs:2330-2348` (`PurposeReflectiveAggregation` placeholder) wired into the purpose-body compile path | **PASS** (wired-on-main) |
| Reflective placeholder materializes at activation | `grep:crates/reify-eval/src/engine_purposes.rs:809-970` (`expand_purpose_reflective_placeholders`); passing test `crates/reify-eval/tests/purpose_activation.rs:1422` (`activate_expands_geometric_params_placeholder_to_populated_list`) | **PASS** |
| `determined(p)` intrinsic resolves + name-faithful semantics (present-but-undef ⇒ false) | `grep:crates/reify-compiler/src/expr.rs:1513-1549`; task 4138 (done) fixed `determined()` undef-state semantics | **PASS** |
| `reify check --purpose <name>=<entity>` activation surface | **on main**: `grep:crates/reify-cli/src/main.rs:206` (`parse_purpose_flag`), `:279` (`--purpose` arg), `:387/:422` (`activate_purpose`/`activate_purpose_with_bindings`) — the flag + activation API have landed (purposes-completion CLI half merged ahead of its batch) | **PASS** (wired-on-main) |
| Intrinsic call shape parses | `grammar-fixture:docs/prds/v0_6/fixtures/determinacy_intrinsics.ri` — `tree-sitter parse --quiet` exit 0, 0 ERROR nodes (verified 2026-06-02) | **PASS** (grammar) |

**Note:** α's leaf signal is the CLI end-to-end (BT4) — `reify check --purpose design_review=<entity> examples/determinacy_intrinsics.ri`, observable today because the `--purpose` flag is already on main — backed by the compile-side golden-equivalence (BT1–BT3). No unmerged-substrate assumption (G3 clean).

---

## β — Realizer achieved-representation-tolerance metric (intermediate → unlocks γ)

| Capability | Evidence | Verdict |
|---|---|---|
| OCCT point-to-shape exact distance primitive | `grep:crates/reify-kernel-occt/src/ffi.rs:776` + `crates/reify-kernel-occt/src/lib.rs:863` (`BRepExtrema_DistShapeShape`, used by `min_clearance`) | **PASS** (wired-on-main) |
| Tessellate call site to hook achieved-tol recording | `grep:crates/reify-eval/src/engine_build.rs:4135` (`src.tessellate(pid, per_stage_tol)`) | **PASS** |
| Achieved-tol storage field/map | **does not exist** — `RealizationNodeData` (`graph.rs:46`) / `Mesh` (`reify-ir/src/geometry.rs:1487`) carry no tolerance field; `TessResult` (`ffi.rs:24`) returns only vertices/indices/normals | **PASS (β is the producer)** — β *builds* this; it is not an assumed-present capability |
| Measured value is non-sentinel (field-population) | β writes a real `f64` (max sampled facet deviation) on the production tessellate path, not `Undef`/`None`; unrealized subject ⇒ `None` (honest absence) | **PASS** — producer writes a real value |

**DAG-direction:** β is upstream of γ (the consumer). No inversion.

---

## γ — `RepresentationWithin` assertion eval + report

| Capability | Evidence | Verdict |
|---|---|---|
| Achieved-tol available at constraint-eval time | producer = **β (upstream)**; C1 ordering invariant pins realization-before-eval (else Indeterminate) | **PASS** (anti-inversion: β upstream) |
| `RepresentationWithin` recognition shape | `grep:crates/reify-eval/src/tolerance_combine.rs:129-211` (`extract_output_tolerance_bound` gates: arg0 `ValueRef:StructureRef`, arg1 `Literal:LENGTH`) — γ reuses the same gates for the assertion arm | **PASS** (wired-on-main) |
| Subject → realized geometry resolution | `grep:crates/reify-eval/src/engine_build.rs:117,418` (`named_steps: HashMap<String, KernelHandle>`) | **PASS** |
| Constraint `Satisfaction` → report path | `grep:crates/reify-ir/src/constraint.rs:40` (`ConstraintResult{satisfaction}`), `crates/reify-eval/src/engine_constraints.rs:29` (`dispatch_constraints`) → `report_eval_output` | **PASS** |
| Budget extractor unbroken (C2 regression) | existing `tolerance_combine.rs` + `tolerance_scope.rs` test suites stay green | **PASS** (regression-locked) |
| **Numeric floor (G6).** The assertion's metric is a *sampled* max facet-chord deviation | `floor`: sampled deviation ≤ true Hausdorff deviation (under-estimate). The signal asserts **"max sampled facet deviation ≤ bound"**, NOT "provably within tolerance everywhere". Demonstrable & non-tautological: a coarse curved subject's sampled deviation ≫ a fine one (BT5), flipping Satisfied↔Violated (BT6/BT7). **Rejected floors:** configured-deflection echo = circular (bound drives deflection); vertex sampling ≈ 0 (vertices on surface). | **PASS** — bound is a comparison on a demonstrable coarse/fine pairing, not an accuracy number ≤ a method floor; the sampled-metric caveat is documented (§8.3) |

---

## δ — §12 doc reconciliation + B+H integration gate

| Capability | Evidence | Verdict |
|---|---|---|
| All of α + γ green | δ depends on α, γ (upstream); its signal is the BT1–BT9 boundary suite | **PASS** (anti-inversion) |
| §12 doc matches shipped code | doc-edit task; verified by the boundary tests it cites | **PASS** |
| §12 doc-ownership (no clobber with #4018) | G4 resolution (§6): δ owns `reify-stdlib-reference.md` §12 in full; #4018 narrowed to spec §9.5 — surfaced for the user to trim #4018, **not** edited by this batch | **PASS with coordination-note** |

---

## Summary

All bindings **PASS** (δ carries one G4 doc-ownership coordination note, not a FAIL). G3 is clean — every assumed capability (reflective machinery, `--purpose` flag, `BRepExtrema` distance, tessellate site, constraint→report path) is verified wired-on-main; the only new substrate (the achieved-tol metric) is built by β as a producer, upstream of its γ consumer. The single G6 hazard — the sampled-deviation metric — is bounded by asserting a *sampled* comparison (demonstrable on a coarse/fine pairing) rather than an everywhere-within-tolerance guarantee, with the under-estimate caveat documented (§8.3). Batch is clear to queue.
